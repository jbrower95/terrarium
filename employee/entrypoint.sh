#!/usr/bin/env bash
set -euo pipefail

# ── Input validation ─────────────────────────────────────────────────
ISSUE="${TERRARIUM_ISSUE:?TERRARIUM_ISSUE is required}"
COMPLEXITY="${TERRARIUM_COMPLEXITY:-medium}"
MODE="${TERRARIUM_MODE:-implement}"

# Model resolution: env var per tier, with defaults
DEFAULT_HIGH="openai/gpt-5.4"
DEFAULT_MEDIUM="openai/gpt-5.4"
DEFAULT_LOW="qwen/qwen3.5-35b-a3b"

case "$COMPLEXITY" in
  high)   MODEL="${TERRARIUM_MODEL_HIGH:-$DEFAULT_HIGH}" ;;
  medium) MODEL="${TERRARIUM_MODEL_MEDIUM:-$DEFAULT_MEDIUM}" ;;
  low)    MODEL="${TERRARIUM_MODEL_LOW:-$DEFAULT_LOW}" ;;
  *)      MODEL="${TERRARIUM_MODEL_MEDIUM:-$DEFAULT_MEDIUM}" ;;
esac

BRANCH="terrarium/issue-${ISSUE}"

# ── Helper: emit artifact JSON ───────────────────────────────────────
emit_artifact() {
  local result_json="$1"
  jq -n \
    --argjson run_id "${GITHUB_RUN_ID:-0}" \
    --arg role "employee" \
    --argjson issue "$ISSUE" \
    --arg model "$MODEL" \
    --argjson input_tokens "${INPUT_TOKENS:-0}" \
    --argjson output_tokens "${OUTPUT_TOKENS:-0}" \
    --argjson cost_usd "${COST_USD:-0}" \
    --argjson result "$result_json" \
    '{run_id: $run_id, role: $role, issue: $issue, model: $model,
      input_tokens: $input_tokens, output_tokens: $output_tokens,
      cost_usd: $cost_usd, result: $result}'
}

emit_stuck() {
  local reason="$1"
  gh issue comment "$ISSUE" --body "Stuck: ${reason}" || true
  gh issue edit "$ISSUE" --add-label "stuck" || true
  gh issue edit "$ISSUE" --remove-label "in-progress" || true
  emit_artifact "{\"type\":\"stuck\",\"issue\":${ISSUE},\"reason\":$(jq -Rn --arg r "$reason" '$r')}"
}

emit_error() {
  local error="$1"
  gh issue edit "$ISSUE" --remove-label "in-progress" || true
  emit_artifact "{\"type\":\"error\",\"error\":$(jq -Rn --arg e "$error" '$e')}"
}

# ── Phase 1: Fetch issue ─────────────────────────────────────────────
echo "::group::Fetching issue #${ISSUE}"
ISSUE_JSON=$(gh issue view "$ISSUE" --json number,title,body,labels,comments)
ISSUE_TITLE=$(echo "$ISSUE_JSON" | jq -r '.title')
ISSUE_BODY=$(echo "$ISSUE_JSON" | jq -r '.body')

# Gather comments
COMMENTS=""
COMMENT_COUNT=$(echo "$ISSUE_JSON" | jq '.comments | length')
if [ "$COMMENT_COUNT" -gt 0 ]; then
  COMMENTS=$(echo "$ISSUE_JSON" | jq -r '.comments[] | "---\n\(.author.login): \(.body)"')
fi
echo "::endgroup::"

# ── Phase 2: Claim the issue ─────────────────────────────────────────
gh issue edit "$ISSUE" --add-label "in-progress" || true
gh issue comment "$ISSUE" --body "🤖 Working on this..." || true

# ── Phase 3: Branch setup ────────────────────────────────────────────
echo "::group::Branch setup"
EXISTING_PR=$(gh pr list --head "$BRANCH" --json number -q '.[0].number // empty' 2>/dev/null || echo "")
REVIEW_CONTEXT=""

if [ -n "$EXISTING_PR" ]; then
  echo "Existing PR #${EXISTING_PR} found, checking out branch"
  git fetch origin "$BRANCH" || true
  git checkout "$BRANCH"
  git pull origin "$BRANCH" --rebase || true
  # Gather review comments
  REVIEW_CONTEXT=$(gh pr view "$EXISTING_PR" --comments 2>/dev/null || echo "")
else
  DEFAULT_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD --short 2>/dev/null | sed 's|origin/||' || echo "main")
  echo "Creating new branch from ${DEFAULT_BRANCH}"
  git checkout "$DEFAULT_BRANCH"
  git pull origin "$DEFAULT_BRANCH" || true
  git checkout -b "$BRANCH"
fi
echo "::endgroup::"

# ── Heal mode: rebase and fix conflicts ───────────────────────────────
if [ "$MODE" = "heal" ]; then
  echo "::group::Heal mode: rebasing $BRANCH onto default branch"
  DEFAULT_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD --short 2>/dev/null | sed 's|origin/||' || echo "main")

  # Attempt a clean rebase first.
  set +e
  git rebase "origin/${DEFAULT_BRANCH}" 2>/tmp/rebase-stderr.log
  REBASE_EXIT=$?
  set -e

  if [ $REBASE_EXIT -eq 0 ]; then
    echo "Clean rebase succeeded"
    git push --force-with-lease
    emit_artifact "{\"type\":\"healed\",\"issue\":${ISSUE},\"method\":\"rebase\"}"
    echo "::endgroup::"
    exit 0
  fi

  echo "Rebase has conflicts, using opencode to resolve..."
  git rebase --abort 2>/dev/null || true

  # Use opencode to resolve the merge: merge main into the branch and fix conflicts.
  cat > opencode.json << 'OCCONF'
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "openrouter": {
      "options": {
        "apiKey": "{env:OPENROUTER_API_KEY}"
      }
    }
  },
  "permission": "allow"
}
OCCONF

  # Merge instead of rebase — leaves conflict markers in files for opencode to fix.
  set +e
  git merge "origin/${DEFAULT_BRANCH}" --no-edit 2>/tmp/merge-stderr.log
  MERGE_EXIT=$?
  set -e

  if [ $MERGE_EXIT -eq 0 ]; then
    echo "Clean merge succeeded"
    git push --force-with-lease
    rm -f opencode.json
    rm -rf .opencode/
    emit_artifact "{\"type\":\"healed\",\"issue\":${ISSUE},\"method\":\"merge\"}"
    echo "::endgroup::"
    exit 0
  fi

  # Get the list of conflicted files.
  CONFLICTED_FILES=$(git diff --name-only --diff-filter=U)
  echo "Conflicted files:"
  echo "$CONFLICTED_FILES"

  HEAL_PROMPT="You are resolving merge conflicts on branch ${BRANCH}.

The branch was merging origin/${DEFAULT_BRANCH} but hit conflicts in these files:
${CONFLICTED_FILES}

Each file has standard git conflict markers (<<<<<<< HEAD, =======, >>>>>>>).

## Instructions
- Open each conflicted file and resolve the conflicts by keeping the correct combination of both sides.
- The branch's changes implement issue #${ISSUE}. Preserve the intent of both sides.
- After resolving, make sure the code compiles and is correct.
- Do NOT add new features or refactor — only resolve the conflicts.
- Mark each file as resolved with 'git add <file>' after fixing it."

  set +e
  opencode run --format json -m "openrouter/${MODEL}" "$HEAL_PROMPT" > /tmp/opencode-output.json 2>/tmp/opencode-stderr.log
  OPENCODE_EXIT=$?
  set -e
  echo "::endgroup::"

  # Parse token/cost data from heal.
  INPUT_TOKENS=$(grep step_finish /tmp/opencode-output.json | jq -s 'map(.part.tokens.input // 0) | add // 0' 2>/dev/null || echo 0)
  OUTPUT_TOKENS=$(grep step_finish /tmp/opencode-output.json | jq -s 'map(.part.tokens.output // 0) | add // 0' 2>/dev/null || echo 0)
  COST_USD=$(grep step_finish /tmp/opencode-output.json | jq -s 'map(.part.cost // 0) | add // 0' 2>/dev/null || echo 0)

  rm -f opencode.json
  rm -rf .opencode/

  if [ $OPENCODE_EXIT -ne 0 ]; then
    echo "opencode failed to resolve conflicts"
    git merge --abort 2>/dev/null || true
    emit_error "heal: opencode failed to resolve conflicts (exit ${OPENCODE_EXIT})"
    exit 0
  fi

  # Check if conflicts are resolved.
  REMAINING=$(git diff --name-only --diff-filter=U 2>/dev/null || echo "")
  if [ -n "$REMAINING" ]; then
    echo "Unresolved conflicts remain: $REMAINING"
    git merge --abort 2>/dev/null || true
    emit_stuck "heal: unresolved conflicts remain in: ${REMAINING}"
    exit 0
  fi

  # Commit and push the merge resolution.
  git add -A -- ':!opencode.json' ':!.opencode/' ':!employee-output.log' ':!artifact.json'
  git commit -m "terrarium: heal merge conflicts for #${ISSUE}" || true
  git push
  emit_artifact "{\"type\":\"healed\",\"issue\":${ISSUE},\"method\":\"merge_resolved\",\"cost_usd\":${COST_USD}}"
  exit 0
fi

# ── Phase 4: Generate opencode config ────────────────────────────────
# Write config; model is passed via -m flag, not in config
cat > opencode.json << 'OCCONF'
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "openrouter": {
      "options": {
        "apiKey": "{env:OPENROUTER_API_KEY}"
      }
    }
  },
  "permission": "allow"
}
OCCONF

# ── Phase 5: Build prompt and run opencode ────────────────────────────
PROMPT="Implement the following GitHub issue.

## Issue #${ISSUE}: ${ISSUE_TITLE}

${ISSUE_BODY}"

if [ -n "$COMMENTS" ]; then
  PROMPT="${PROMPT}

## Issue comments
${COMMENTS}"
fi

if [ -n "$REVIEW_CONTEXT" ]; then
  PROMPT="${PROMPT}

## PR review comments (address these)
${REVIEW_CONTEXT}"
fi

PROMPT="${PROMPT}

## Instructions
- Keep changes minimal and focused on the issue.
- Do NOT refactor, rename, or reorganize unrelated code.
- Do NOT add dependencies unless the issue explicitly requires them.
- Do NOT change function signatures or remove code unrelated to the issue.
- Verify your changes compile/work if possible (e.g. run the build command)."

echo "::group::Running opencode with model ${MODEL}"
set +e
opencode run --format json -m "openrouter/${MODEL}" "$PROMPT" > /tmp/opencode-output.json 2>/tmp/opencode-stderr.log
OPENCODE_EXIT=$?
set -e
echo "::endgroup::"

if [ $OPENCODE_EXIT -ne 0 ]; then
  echo "opencode exited with code ${OPENCODE_EXIT}"
  cat /tmp/opencode-stderr.log >&2 || true
  emit_error "opencode exited with code ${OPENCODE_EXIT}"
  exit 0
fi

# ── Phase 6: Parse token/cost data ────────────────────────────────────
INPUT_TOKENS=$(grep step_finish /tmp/opencode-output.json | jq -s 'map(.part.tokens.input // 0) | add // 0' 2>/dev/null || echo 0)
OUTPUT_TOKENS=$(grep step_finish /tmp/opencode-output.json | jq -s 'map(.part.tokens.output // 0) | add // 0' 2>/dev/null || echo 0)
COST_USD=$(grep step_finish /tmp/opencode-output.json | jq -s 'map(.part.cost // 0) | add // 0' 2>/dev/null || echo 0)

echo "Tokens: input=${INPUT_TOKENS} output=${OUTPUT_TOKENS} cost=\$${COST_USD}"

# ── Phase 7: Check for changes and commit ─────────────────────────────
# Remove opencode config before checking status
rm -f opencode.json
# Also remove any opencode state dirs
rm -rf .opencode/

if git diff --quiet && git diff --cached --quiet && [ -z "$(git ls-files --others --exclude-standard)" ]; then
  echo "No changes detected"
  emit_stuck "opencode made no file changes"
  exit 0
fi

echo "::group::Committing changes"
# Exclude files created by the workflow wrapper
git add -A -- ':!employee-output.log' ':!artifact.json'
git commit -m "terrarium: implement #${ISSUE} - ${ISSUE_TITLE}"
git push -u origin "$BRANCH"
SHA=$(git rev-parse HEAD)
echo "::endgroup::"

# ── Phase 8: Create or update PR ──────────────────────────────────────
if [ -n "$EXISTING_PR" ]; then
  echo "Updated existing PR #${EXISTING_PR}"
  emit_artifact "{\"type\":\"pr_updated\",\"number\":${EXISTING_PR},\"commit_sha\":\"${SHA}\"}"
else
  PR_BODY=$(cat <<PRBODY
Closes #${ISSUE}

**Model:** \`${MODEL}\`
**Commit:** \`${SHA}\`
**Cost:** \$${COST_USD} (${INPUT_TOKENS} in / ${OUTPUT_TOKENS} out)

---
*Automated by terrarium-employee*
PRBODY
)
  PR_URL=$(gh pr create --head "$BRANCH" --title "#${ISSUE}: ${ISSUE_TITLE}" --body "$PR_BODY")
  PR_NUMBER=$(echo "$PR_URL" | grep -oE '[0-9]+$')
  echo "Created PR #${PR_NUMBER}"
  emit_artifact "{\"type\":\"pr\",\"number\":${PR_NUMBER},\"commit_sha\":\"${SHA}\"}"
fi
