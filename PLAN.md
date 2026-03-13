# terrarium — PLAN

Terrarium is a self-running project framework funded by a tip jar.
It is bigco, rehosted from local daemons to GitHub Actions, and funded by a Base USDC wallet.

Put money in → models get smarter, tasks get done faster.
Money runs out → models downgrade, task cadence slows.

---

## Core Differences from bigco

| bigco | terrarium |
|---|---|
| Owner daemon: long-running local process | Owner: scheduled GitHub Actions cron |
| Employee daemons: local processes polling GitHub | Employees: manually- or owner-triggered GHA workflows |
| Model config: `staff.json` | Model config: `terrarium.json` + GitHub Secrets |
| No budget awareness | Budget: USDC balance on Base, computed run-rate |
| `hardlink` lock files | GHA concurrency groups for mutex |
| Install: clone repo, run binary | Install: `npx terrarium --install` |

---

## Package Structure

```
terrarium/
  packages/
    cli/                   # npx terrarium --install
      src/
        index.ts           # entry point — install wizard
        prompts.ts         # onboarding questions (repo, wallet, models, cron)
        secrets.ts         # gh secret set via GitHub API
        workflows.ts       # render + push .github/workflows/
        config.ts          # write terrarium.json to target repo
    core/                  # shared logic, imported by GHA steps
      src/
        budget.ts          # read USDC balance from Base, compute run-rate
        models.ts          # model tier map, upgrade/downgrade logic
        tasks.ts           # GitHub issues: list, claim, submit, feedback
        pr.ts              # PR create, review, merge
        inference.ts       # thin wrapper: POST to inference API, never exposes key
        config.ts          # terrarium.json R/W
        activity.ts        # activity log (appended to activity.jsonl)
        mail.ts            # MAIL.md append + read
  templates/
    .github/
      workflows/
        owner.yml.hbs      # cron: owner wake cycle
        employee.yml.hbs   # manual dispatch: single task run
        pr-review.yml.hbs  # triggered on PR open: automated review
    terrarium.json.hbs     # initial config template
    PLAN.md.hbs            # initial PLAN.md stub for target project
```

---

## Config: `terrarium.json`

Checked into the target repo. Contains no secrets.

```json
{
  "wallet": "0xABC...",
  "models": {
    "owner":    "claude-opus-4-6",
    "high":     "claude-opus-4-6",
    "medium":   "claude-sonnet-4-6",
    "low":      "claude-haiku-4-5-20251001"
  },
  "owner_cron": "*/30 * * * *",
  "max_concurrent_employees": 3,
  "budget": {
    "downgrade_threshold_usd": 5.00,
    "pause_threshold_usd": 1.00
  }
}
```

GitHub Secrets (never in config):
- `ANTHROPIC_API_KEY` — inference
- `BASE_WALLET_PRIVATE_KEY` — read-only; used only to check balance, never to sign transactions

---

## GitHub Actions Workflows

### `owner.yml` — Scheduled cron (default: every 30 min)

Triggered by: schedule cron (configurable in `terrarium.json`)

Steps:
1. **Budget check** — read USDC balance from Base RPC; compute daily run-rate from `activity.jsonl`
2. **Model adjustment** — if balance < `downgrade_threshold`, downgrade tiers one step; if balance < `pause_threshold`, set all models to `null` (halt new tasks); write back to `terrarium.json`, commit
3. **Cron self-adjustment** — if budget is tight, lengthen `owner_cron`; commit
4. **Audit** — detect stale in-progress issues (PR open > 24h with no activity); re-open to backlog
5. **Plan → Issues** — read `PLAN.md`, reconcile against open GitHub issues, file any missing ones with appropriate complexity/priority labels
6. **Mail** — read `MAIL.md`, respond inline, bump stuck tasks
7. **PR review** — for each open PR: run inference review; if adequate, merge; if not, comment + add `needs-changes` label
8. **Stakeholder update** — if there are >= N merged PRs since last update, post a GitHub issue summarizing recent activity

### `employee.yml` — Manual dispatch (or triggered by owner)

Inputs:
- `complexity` (low / medium / high) — determines model tier
- `issue_number` (optional) — if blank, auto-selects highest-priority available issue

Steps:
1. **Claim** — comment on issue + add `in-progress` label (GHA concurrency group prevents double-claim)
2. **Infer** — run Claude on the issue using the model for the given complexity tier; Claude writes code
3. **Submit** — `git push` branch, `gh pr create`; remove `in-progress` label
4. **On failure** — append to `MAIL.md`; remove `in-progress` label; re-open issue

GHA concurrency:
```yaml
concurrency:
  group: terrarium-issue-${{ inputs.issue_number }}
  cancel-in-progress: false
```

### `pr-review.yml` — Triggered on PR open/sync

Steps:
1. Skip if PR has `draft` label or was opened by a human (non-`github-actions[bot]` author needs human review)
2. Run inference review with `medium` model
3. If review passes: approve + add `auto-approved` label
4. If review fails: request changes + add `needs-changes` label

---

## Budget & Model Logic

```
Run-rate (USD/day) = sum of inference costs in activity.jsonl over last 7 days / 7

Tiers (example):
  owner:  opus   → sonnet  → haiku  → null
  high:   opus   → sonnet  → haiku  → null
  medium: sonnet → haiku   → haiku  → null
  low:    haiku  → haiku   → null   → null

Downgrade trigger: projected days remaining < 3
Pause trigger:     balance < $1
Resume trigger:    balance > $5 (re-read wallet on each owner wake)
```

Owner writes tier changes to `terrarium.json` and commits them.
Employees read model name from `terrarium.json` at task start.

---

## `npx terrarium --install`

Interactive wizard that runs in any existing GitHub repo.

Prompts:
1. GitHub repo (detected from `git remote`)
2. Base wallet address (for tip jar; read-only)
3. Anthropic API key (written to GitHub Secrets, never stored locally)
4. Initial model tiers (defaults shown)
5. Owner cron schedule (default: every 30 min)
6. Max concurrent employees (default: 3)

Actions:
1. Write `terrarium.json` to repo root
2. Write `.github/workflows/owner.yml`, `employee.yml`, `pr-review.yml`
3. Write stub `PLAN.md` (if not present)
4. Set GitHub Secrets via API: `ANTHROPIC_API_KEY`, `BASE_RPC_URL`
5. Commit + push all files
6. Print tip jar address + instructions

---

## Task Lifecycle

```
OWNER (cron)                         EMPLOYEE (dispatch)
────────────────────────────────────────────────────────────

[owner wakes]
  check balance → ok
  plan → file issue #42
    labels: complexity:medium, priority:high

                                     [owner triggers employee.yml]
                                     or [human dispatches manually]
                                       inputs: complexity=medium

                                     claim issue #42
                                       add in-progress label
                                       concurrency group locks

                                     run claude on issue #42
                                       model: terrarium.json["medium"]

                                     ┌─ happy path ──────────────┐
                                     │ push branch               │
                                     │ gh pr create              │
                                     │ remove in-progress label  │
                                     └───────────────────────────┘

                                     ┌─ stuck path ───────────────┐
                                     │ append MAIL.md             │
                                     │ remove in-progress label   │
                                     └────────────────────────────┘

[owner wakes]
  read MAIL.md → respond
    bump complexity if needed
  review open PRs
    auto-merge if approved
  check balance → low
    downgrade medium: sonnet → haiku
    commit terrarium.json
```

---

## Implementation Phases

### Phase 1 — `packages/core`
1. `config.ts` — read/write `terrarium.json`
2. `budget.ts` — Base RPC balance read, run-rate from activity log
3. `models.ts` — tier map, downgrade/upgrade logic
4. `inference.ts` — thin wrapper around Anthropic SDK (key from env, never exported)
5. `tasks.ts` — GitHub issues: list open, claim, unclaim, next-task selection
6. `pr.ts` — create PR, review (inference), merge
7. `mail.ts` — append/read MAIL.md
8. `activity.ts` — append/read activity.jsonl

### Phase 2 — GHA workflow templates
9. `owner.yml.hbs` — full owner cron logic as GHA steps
10. `employee.yml.hbs` — single task dispatch
11. `pr-review.yml.hbs` — PR auto-review

### Phase 3 — `packages/cli` (npx terrarium)
12. `prompts.ts` — onboarding questions
13. `secrets.ts` — GitHub Secrets API
14. `workflows.ts` — render templates, push to repo
15. `index.ts` — wire everything together

### Phase 4 — Polish
16. Stakeholder update post (owner step 8)
17. GHA usage check (owner: skip wake if minutes quota low)
18. Balance top-up detection (resume from paused state on refill)
19. `terrarium status` CLI command — show balance, run-rate, open tasks, active employees

---

## Principles

- **Employees never see the inference key.** `inference.ts` reads `ANTHROPIC_API_KEY` from env (injected by GHA), exposes only a `complete(prompt, model)` function.
- **Owner never sees the inference key.** Owner configures model names in `terrarium.json`; actual key lives only in GHA Secrets.
- **Wallet is read-only.** Only balance queries hit the Base RPC. No signing, no sends.
- **All config is in the repo.** `terrarium.json` is checked in. State is in GitHub (issues, labels, PRs, activity.jsonl).
- **GHA concurrency = mutex.** No hardlink tricks needed; GHA concurrency groups prevent double-claiming.
- **Self-modifying config is normal.** Owner commits model tier changes and cron adjustments. This is expected behavior, not a bug.
