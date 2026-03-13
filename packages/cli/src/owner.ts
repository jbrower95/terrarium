#!/usr/bin/env node
import { execSync } from "child_process";
import fs from "fs/promises";
import path from "path";
import { readConfig, writeConfig, readModelConfig, readAutoReview, setModelVar, setAutoReview, type ModelConfig } from "../../core/src/config.js";
import { getBudgetStatus, getUsdcBalance } from "../../core/src/budget.js";
import {
  shouldPause,
  shouldDowngrade,
  shouldResume,
  pauseAllTiers,
  downgradeTiers,
  upgradeTiers,
  estimateDailyCost,
  formatModelCatalog,
} from "../../core/src/models.js";
import { appendActivity, readActivityLog, countMergesSinceLastUpdate } from "../../core/src/activity.js";
import { readMail, respondToMail } from "../../core/src/mail.js";
import { appendJournalEntry, readJournalContext } from "../../core/src/journal.js";
import { listOpenPrs, reviewPr, approvePr, requestChanges, mergePr } from "../../core/src/pr.js";
import { listOpenIssues, fileIssue } from "../../core/src/tasks.js";
import { complete } from "../../core/src/inference.js";

function gitRun(cmd: string, cwd: string): void {
  execSync(cmd, { cwd, stdio: "inherit" });
}

function getRepoFromRemote(): { owner: string; name: string } {
  const remoteUrl = execSync("git remote get-url origin", { encoding: "utf-8" }).trim();
  const sshMatch = remoteUrl.match(/git@github\.com:([^/]+)\/(.+?)(?:\.git)?$/);
  if (sshMatch) return { owner: sshMatch[1], name: sshMatch[2] };
  const httpsMatch = remoteUrl.match(/https?:\/\/github\.com\/([^/]+)\/(.+?)(?:\.git)?$/);
  if (httpsMatch) return { owner: httpsMatch[1], name: httpsMatch[2] };
  throw new Error("Could not determine repo owner/name from git remote");
}

async function commitAndPush(repoRoot: string, message: string): Promise<void> {
  try {
    gitRun("git add -A", repoRoot);
    // Only commit if there are staged changes
    const status = execSync("git status --porcelain", { cwd: repoRoot, encoding: "utf-8" }).trim();
    if (!status) return;
    gitRun(`git commit -m ${JSON.stringify(message)}`, repoRoot);
    gitRun("git push", repoRoot);
  } catch (err) {
    console.error("Warning: git commit/push failed:", (err as Error).message);
  }
}

/** Apply model tier changes: update GitHub repo variables for each tier. */
async function applyModelConfig(
  repoOwner: string,
  repoName: string,
  models: ModelConfig,
  repoRoot: string,
  reason: string
): Promise<void> {
  for (const tier of ["owner", "high", "medium", "low"] as Array<keyof ModelConfig>) {
    try {
      setModelVar(repoOwner, repoName, tier, models[tier]);
    } catch (err) {
      console.error(`  Warning: failed to set ${tier} model var:`, (err as Error).message);
    }
  }
  await appendActivity(repoRoot, {
    type: "model_adjusted",
    message: `${reason} → owner=${models.owner ?? "paused"} high=${models.high ?? "paused"} medium=${models.medium ?? "paused"} low=${models.low ?? "paused"}`,
  });
}

async function handleMail(
  repoRoot: string,
  repoOwner: string,
  repoName: string,
  model: string
): Promise<void> {
  const entries = await readMail(repoRoot);
  const unanswered = entries.filter((e) => !e.answered);
  if (unanswered.length === 0) return;

  console.log(`Processing ${unanswered.length} unanswered mail entries...`);

  for (const entry of unanswered) {
    try {
      const prompt = `You are the owner of a software project managed by terrarium.
You received the following message from an automated worker:

Subject: ${entry.subject}
From: ${entry.from}

${entry.body}

Please respond helpfully. If this is a failure report, decide whether to:
- Re-file the issue with higher complexity
- Skip the issue with an explanation
- Provide specific guidance for the next attempt

Keep your response concise and actionable.`;

      const result = await complete({ model, prompt });
      await respondToMail(repoRoot, entry.id, result.text);
      console.log(`  Responded to mail: ${entry.subject}`);
    } catch (err) {
      console.error(`  Failed to respond to mail ${entry.id}:`, err);
    }
  }
}

async function handlePrReviews(
  repoRoot: string,
  repoOwner: string,
  repoName: string,
  model: string,
  autoReviewEnabled: boolean
): Promise<void> {
  if (!autoReviewEnabled) {
    console.log("Auto-review is disabled — skipping PR reviews.");
    return;
  }

  const prs = listOpenPrs(repoOwner, repoName);
  const botPrs = prs.filter((pr) => !pr.isDraft && pr.author === "github-actions[bot]");
  if (botPrs.length === 0) return;

  console.log(`Reviewing ${botPrs.length} open PRs...`);

  for (const pr of botPrs) {
    try {
      const { approved, feedback } = await reviewPr(repoOwner, repoName, pr.number, { model });

      if (approved) {
        approvePr(repoOwner, repoName, pr.number, feedback);
        mergePr(repoOwner, repoName, pr.number);
        await appendActivity(repoRoot, {
          type: "pr_merged",
          pr_number: pr.number,
          model,
          message: `Merged PR #${pr.number}: ${pr.title}`,
        });
        console.log(`  Approved and merged PR #${pr.number}: ${pr.title}`);
      } else {
        requestChanges(repoOwner, repoName, pr.number, feedback);
        await appendActivity(repoRoot, {
          type: "pr_reviewed",
          pr_number: pr.number,
          model,
          message: `Requested changes on PR #${pr.number}`,
        });
        console.log(`  Requested changes on PR #${pr.number}: ${pr.title}`);
      }
    } catch (err) {
      console.error(`  Failed to review PR #${pr.number}:`, err);
    }
  }
}

async function reconcilePlanWithIssues(
  repoRoot: string,
  repoOwner: string,
  repoName: string,
  model: string,
  journalContext: string
): Promise<void> {
  const planPath = path.join(repoRoot, "PLAN.md");
  let planContent: string;
  try {
    planContent = await fs.readFile(planPath, "utf-8");
  } catch {
    console.log("No PLAN.md found, skipping plan reconciliation");
    return;
  }

  const openIssues = listOpenIssues(repoOwner, repoName);
  const openTitles = openIssues.map((i) => i.title.toLowerCase());

  const prompt = `You are the owner of a software project managed by terrarium.

${journalContext ? journalContext + "\n\n" : ""}## Current task
Given the following PLAN.md and list of existing open GitHub issues, identify any tasks in the plan that don't have a corresponding open issue yet.

PLAN.md:
${planContent}

Existing open issues:
${openTitles.length > 0 ? openTitles.map((t, i) => `${i + 1}. ${t}`).join("\n") : "(none)"}

For each missing task, respond with a JSON array:
[
  {
    "title": "Issue title",
    "body": "Detailed description",
    "complexity": "low|medium|high",
    "priority": "low|medium|high"
  }
]

Only include tasks clearly absent from the issue list. Respond with only the JSON array.`;

  try {
    const result = await complete({ model, prompt });
    const newIssues = JSON.parse(result.text.trim()) as Array<{
      title: string;
      body: string;
      complexity: "low" | "medium" | "high";
      priority: "low" | "medium" | "high";
    }>;

    for (const issue of newIssues) {
      const issueNumber = fileIssue(repoOwner, repoName, issue.title, issue.body, issue.complexity, issue.priority);
      await appendActivity(repoRoot, {
        type: "issue_filed",
        issue_number: issueNumber,
        message: `Filed issue #${issueNumber}: ${issue.title}`,
      });
      console.log(`  Filed issue #${issueNumber}: ${issue.title}`);
    }
  } catch (err) {
    console.error("  Failed to reconcile plan with issues:", err);
  }
}

async function postStakeholderUpdate(
  repoRoot: string,
  repoOwner: string,
  repoName: string,
  model: string,
  journalContext: string
): Promise<void> {
  const mergeCount = await countMergesSinceLastUpdate(repoRoot);
  if (mergeCount < 5) return;

  console.log(`Posting stakeholder update (${mergeCount} merges since last update)...`);

  const entries = await readActivityLog(repoRoot);
  const recentEntries = entries.slice(-50);

  const prompt = `You are the owner of a software project. Write a brief stakeholder update.

${journalContext ? journalContext + "\n\n" : ""}Recent activity (last 50 entries):
${recentEntries.map((e) => `- [${e.timestamp}] ${e.type}: ${e.message ?? ""}`).join("\n")}

Write a concise update (3-5 bullet points) covering: what was completed, current status, any blockers.
Format as a GitHub issue body with markdown.`;

  try {
    const result = await complete({ model, prompt });
    fileIssue(
      repoOwner,
      repoName,
      `Terrarium Stakeholder Update — ${new Date().toISOString().split("T")[0]}`,
      result.text,
      "low",
      "low"
    );
    await appendActivity(repoRoot, {
      type: "stakeholder_update",
      message: "Posted stakeholder update",
    });
    console.log("  Posted stakeholder update");
  } catch (err) {
    console.error("  Failed to post stakeholder update:", err);
  }
}

/**
 * Let the owner decide whether to adjust model assignments or auto-review
 * based on budget, using the model catalog as context.
 */
async function reviewModelStrategy(
  repoOwner: string,
  repoName: string,
  repoRoot: string,
  currentModels: ModelConfig,
  balance: number,
  dailyRunRate: number,
  projectedDays: number,
  ownerModel: string
): Promise<void> {
  const estimatedCost = estimateDailyCost(currentModels);
  const catalog = formatModelCatalog();

  const prompt = `You are the owner of a terrarium project. You manage model assignments for automated workers.

## Current state
- Balance: $${balance.toFixed(2)} USDC
- Actual daily run rate (from activity log): $${dailyRunRate.toFixed(2)}/day
- Estimated daily cost at current model config: $${estimatedCost.toFixed(2)}/day
- Projected days remaining: ${Number.isFinite(projectedDays) ? projectedDays.toFixed(1) : "∞"}

## Current model assignments
- owner: ${currentModels.owner ?? "paused"}
- high complexity: ${currentModels.high ?? "paused"}
- medium complexity: ${currentModels.medium ?? "paused"}
- low complexity: ${currentModels.low ?? "paused"}

## Available models
${catalog}

## Task
Decide if model assignments should change. Consider:
1. Are we getting good value? (coding score vs cost)
2. Is the budget sustainable?
3. Should auto-review be enabled (costs ~1 inference call per PR) or disabled to save budget?

Respond ONLY with a JSON object:
{
  "changes": {
    "owner": "<model-id or null to keep current>",
    "high": "<model-id or null to keep current>",
    "medium": "<model-id or null to keep current>",
    "low": "<model-id or null to keep current>"
  },
  "auto_review": true | false | null,
  "reasoning": "<1-2 sentence explanation>"
}

Use null for any tier where no change is needed. Only suggest changes if there's a clear reason.`;

  try {
    const result = await complete({ model: ownerModel, prompt });
    const decision = JSON.parse(result.text.trim()) as {
      changes: Partial<ModelConfig>;
      auto_review: boolean | null;
      reasoning: string;
    };

    console.log(`  Model strategy: ${decision.reasoning}`);

    // Apply any suggested model changes
    let changed = false;
    const updatedModels = { ...currentModels };
    for (const tier of ["owner", "high", "medium", "low"] as Array<keyof ModelConfig>) {
      if (decision.changes[tier] !== null && decision.changes[tier] !== undefined) {
        updatedModels[tier] = decision.changes[tier];
        changed = true;
      }
    }

    if (changed) {
      await applyModelConfig(repoOwner, repoName, updatedModels, repoRoot, `Strategy adjustment: ${decision.reasoning}`);
      console.log("  Updated model assignments.");
    }

    // Apply auto-review toggle
    if (decision.auto_review !== null) {
      setAutoReview(repoOwner, repoName, decision.auto_review);
      await appendActivity(repoRoot, {
        type: "model_adjusted",
        message: `Auto-review ${decision.auto_review ? "enabled" : "disabled"}: ${decision.reasoning}`,
      });
      console.log(`  Auto-review ${decision.auto_review ? "enabled" : "disabled"}.`);
    }
  } catch (err) {
    console.error("  Failed to review model strategy:", err);
  }
}

async function writeJournalEntry(
  repoRoot: string,
  repoOwner: string,
  repoName: string,
  balance: number,
  dailyRunRate: number,
  projectedDays: number,
  models: ModelConfig,
  autoReview: boolean,
  ownerModel: string,
  journalContext: string
): Promise<void> {
  const entries = await readActivityLog(repoRoot);
  const mergesSinceUpdate = await countMergesSinceLastUpdate(repoRoot);
  const recentActivity = entries.slice(-20);

  const prompt = `You are the owner of a software project. Write a brief journal entry for this owner cycle.

${journalContext ? "## Context from previous cycles\n\n" + journalContext + "\n\n" : ""}## This cycle's activity
${recentActivity.map((e) => `- [${e.timestamp}] ${e.type}: ${e.message ?? ""}`).join("\n")}

Write 2-4 sentences summarizing:
- What happened this cycle
- Current project health
- Any concerns or next priorities

Be direct and specific. This will be read by you (the owner) on the next wake to regain context.`;

  try {
    const result = await complete({ model: ownerModel, prompt });
    await appendJournalEntry(repoRoot, {
      timestamp: new Date().toISOString(),
      balance,
      dailyRunRate,
      projectedDays,
      models: models as Record<string, string | null>,
      autoReview,
      mergesSinceLastUpdate: mergesSinceUpdate,
      body: result.text,
    });
    console.log("  Journal entry written.");
  } catch (err) {
    console.error("  Failed to write journal entry:", err);
  }
}

async function main(): Promise<void> {
  const repoRoot = process.cwd();
  console.log(`[terrarium-owner] Starting owner cycle in ${repoRoot}`);

  const config = await readConfig(repoRoot);
  const { owner: repoOwner, name: repoName } = getRepoFromRemote();
  const models = readModelConfig();
  const autoReview = readAutoReview();

  await appendActivity(repoRoot, {
    type: "owner_wake",
    message: `Owner woke in ${repoOwner}/${repoName}`,
  });

  // Regain context from journal
  const journalContext = await readJournalContext(repoRoot);
  if (journalContext) {
    console.log("Loaded journal context from previous cycles.");
  }

  // Budget check
  console.log("Checking budget...");
  const balance = await getUsdcBalance(config.wallet);
  const { dailyRunRate, projectedDaysRemaining } = await getBudgetStatus(repoRoot, config.wallet);

  console.log(
    `  Balance: $${balance.toFixed(2)} USDC | Run rate: $${dailyRunRate.toFixed(2)}/day | Projected: ${Number.isFinite(projectedDaysRemaining) ? projectedDaysRemaining.toFixed(1) + " days" : "N/A"}`
  );
  console.log(
    `  Models: owner=${models.owner ?? "paused"} | high=${models.high ?? "paused"} | medium=${models.medium ?? "paused"} | low=${models.low ?? "paused"}`
  );
  console.log(`  Auto-review: ${autoReview ? "enabled" : "disabled"}`);

  // Hard pause
  if (shouldPause(balance, config.budget.pause_threshold_usd)) {
    console.log("  Balance below pause threshold — pausing all tiers");
    const paused = pauseAllTiers(models);
    await applyModelConfig(repoOwner, repoName, paused, repoRoot, `Paused all tiers (balance: $${balance.toFixed(2)})`);
    setAutoReview(repoOwner, repoName, false);
    await writeJournalEntry(repoRoot, repoOwner, repoName, balance, dailyRunRate, projectedDaysRemaining, paused, false, models.owner ?? "openrouter/qwen/qwen3.5-35b-a3b", journalContext);
    await commitAndPush(repoRoot, "chore(terrarium): owner cycle — paused (balance critical)");
    console.log("Paused. Exiting owner cycle.");
    return;
  }

  // Auto-downgrade if running low
  if (shouldDowngrade(projectedDaysRemaining)) {
    console.log(`  < 3 days projected — downgrading tiers`);
    const downgraded = downgradeTiers(models);
    await applyModelConfig(repoOwner, repoName, downgraded, repoRoot, `Downgraded tiers (${projectedDaysRemaining.toFixed(1)} days remaining)`);
    // Also disable auto-review when downgrading to save budget
    if (autoReview) {
      setAutoReview(repoOwner, repoName, false);
      console.log("  Disabled auto-review to conserve budget.");
    }
  }

  // Resume if balance recovered
  if (shouldResume(balance, config.budget.downgrade_threshold_usd)) {
    const anyPaused = Object.values(models).some((m) => !m);
    if (anyPaused) {
      console.log("  Balance recovered — upgrading tiers");
      const upgraded = upgradeTiers(models);
      await applyModelConfig(repoOwner, repoName, upgraded, repoRoot, `Upgraded tiers (balance recovered: $${balance.toFixed(2)})`);
    }
  }

  // Re-read models after potential adjustments
  const currentModels = readModelConfig();
  const ownerModel = currentModels.owner;
  if (!ownerModel) {
    console.log("Owner model is null (paused). Skipping inference tasks.");
    await commitAndPush(repoRoot, "chore(terrarium): owner cycle — paused");
    return;
  }

  // Let owner reason about model strategy (once per cycle, uses ~1 inference call)
  await reviewModelStrategy(repoOwner, repoName, repoRoot, currentModels, balance, dailyRunRate, projectedDaysRemaining, ownerModel);

  // Re-read auto-review after strategy adjustment
  const currentAutoReview = readAutoReview();

  // Process mail
  await handleMail(repoRoot, repoOwner, repoName, ownerModel);

  // Review PRs (gated on auto-review setting)
  await handlePrReviews(repoRoot, repoOwner, repoName, ownerModel, currentAutoReview);

  // Reconcile PLAN.md with issues (passes journal context for continuity)
  await reconcilePlanWithIssues(repoRoot, repoOwner, repoName, ownerModel, journalContext);

  // Stakeholder update if enough merges
  await postStakeholderUpdate(repoRoot, repoOwner, repoName, ownerModel, journalContext);

  // Write journal entry for this cycle
  await writeJournalEntry(repoRoot, repoOwner, repoName, balance, dailyRunRate, projectedDaysRemaining, currentModels, currentAutoReview, ownerModel, journalContext);

  await commitAndPush(repoRoot, "chore(terrarium): owner cycle updates");
  console.log("[terrarium-owner] Owner cycle complete");
}

main().catch((err) => {
  console.error("[terrarium-owner] Fatal error:", err);
  process.exit(1);
});
