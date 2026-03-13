#!/usr/bin/env node
import { execSync } from "child_process";
import fs from "fs/promises";
import path from "path";
import { readConfig, writeConfig } from "../../core/src/config.js";
import {
  getBudgetStatus,
  getUsdcBalance,
} from "../../core/src/budget.js";
import {
  shouldPause,
  shouldDowngrade,
  shouldResume,
  pauseAllTiers,
  downgradeTiers,
  upgradeTiers,
} from "../../core/src/models.js";
import { appendActivity, readActivityLog, countMergesSinceLastUpdate } from "../../core/src/activity.js";
import { readMail, respondToMail } from "../../core/src/mail.js";
import {
  listOpenPrs,
  reviewPr,
  approvePr,
  requestChanges,
  mergePr,
} from "../../core/src/pr.js";
import { listOpenIssues, fileIssue } from "../../core/src/tasks.js";
import { complete } from "../../core/src/inference.js";

function gitRun(cmd: string, cwd: string): void {
  execSync(cmd, { cwd, stdio: "inherit" });
}

function getRepoFromRemote(): { owner: string; name: string } {
  const remoteUrl = execSync("git remote get-url origin", {
    encoding: "utf-8",
  }).trim();

  const sshMatch = remoteUrl.match(/git@github\.com:([^/]+)\/(.+?)(?:\.git)?$/);
  if (sshMatch) return { owner: sshMatch[1], name: sshMatch[2] };

  const httpsMatch = remoteUrl.match(
    /https?:\/\/github\.com\/([^/]+)\/(.+?)(?:\.git)?$/
  );
  if (httpsMatch) return { owner: httpsMatch[1], name: httpsMatch[2] };

  throw new Error("Could not determine repo owner/name from git remote");
}

async function commitAndPush(repoRoot: string, message: string): Promise<void> {
  try {
    gitRun("git add -A", repoRoot);
    gitRun(`git commit -m ${JSON.stringify(message)}`, repoRoot);
    gitRun("git push", repoRoot);
  } catch (err) {
    console.error("Warning: git commit/push failed:", (err as Error).message);
  }
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
  model: string
): Promise<void> {
  const prs = listOpenPrs(repoOwner, repoName);
  const botPrs = prs.filter(
    (pr) => !pr.isDraft && pr.author === "github-actions[bot]"
  );

  if (botPrs.length === 0) return;

  console.log(`Reviewing ${botPrs.length} open PRs...`);

  for (const pr of botPrs) {
    try {
      const { approved, feedback } = await reviewPr(
        repoOwner,
        repoName,
        pr.number,
        { model }
      );

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
  model: string
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

  const prompt = `You are a project manager. Given the following PLAN.md and list of existing open GitHub issues,
identify any tasks in the plan that don't have a corresponding open issue yet.

PLAN.md:
${planContent}

Existing open issues:
${openTitles.map((t, i) => `${i + 1}. ${t}`).join("\n")}

For each missing task, respond with a JSON array of objects:
[
  {
    "title": "Issue title",
    "body": "Detailed description of the task",
    "complexity": "low|medium|high",
    "priority": "low|medium|high|critical"
  }
]

Only include tasks that clearly don't have an existing issue. If unsure, skip it.
Only respond with the JSON array, no other text. If no new issues are needed, respond with [].`;

  try {
    const result = await complete({ model, prompt });
    const newIssues = JSON.parse(result.text.trim()) as Array<{
      title: string;
      body: string;
      complexity: "low" | "medium" | "high";
      priority: "low" | "medium" | "high" | "critical";
    }>;

    for (const issue of newIssues) {
      const issueNumber = fileIssue(
        repoOwner,
        repoName,
        issue.title,
        issue.body,
        issue.complexity,
        issue.priority
      );
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
  model: string
): Promise<void> {
  const mergeCount = await countMergesSinceLastUpdate(repoRoot);

  if (mergeCount < 5) return;

  console.log(
    `Posting stakeholder update (${mergeCount} merges since last update)...`
  );

  const entries = await readActivityLog(repoRoot);
  const recentEntries = entries.slice(-50);

  const prompt = `You are the owner of a software project. Write a brief stakeholder update summarizing recent activity.

Recent activity (last 50 entries):
${recentEntries.map((e) => `- [${e.timestamp}] ${e.type}: ${e.message ?? ""}`).join("\n")}

Write a concise stakeholder update (3-5 bullet points) summarizing:
- What was completed
- Current project status
- Any blockers or concerns

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

async function main(): Promise<void> {
  const repoRoot = process.cwd();
  console.log(`[terrarium-owner] Starting owner cycle in ${repoRoot}`);

  const config = await readConfig(repoRoot);
  const { owner: repoOwner, name: repoName } = getRepoFromRemote();

  // Record wake
  await appendActivity(repoRoot, {
    type: "owner_wake",
    message: `Owner woke in ${repoOwner}/${repoName}`,
  });

  // Budget check
  console.log("Checking budget...");
  const balance = await getUsdcBalance(config.wallet);
  const { dailyRunRate, projectedDaysRemaining } = await getBudgetStatus(
    repoRoot,
    config.wallet
  );

  console.log(
    `  Balance: $${balance.toFixed(2)} USDC | Run rate: $${dailyRunRate.toFixed(2)}/day | Projected: ${Number.isFinite(projectedDaysRemaining) ? projectedDaysRemaining.toFixed(1) + " days" : "N/A"}`
  );

  // Handle pause
  if (shouldPause(config, balance)) {
    console.log("  Balance below pause threshold — pausing all tiers");
    config.models = pauseAllTiers(config);
    await writeConfig(repoRoot, config);
    await appendActivity(repoRoot, {
      type: "model_adjusted",
      message: `Paused all model tiers (balance: $${balance.toFixed(2)})`,
    });
    await commitAndPush(repoRoot, "chore(terrarium): pause all tiers — balance critical");
    console.log("Paused. Exiting owner cycle.");
    return;
  }

  // Handle downgrade
  if (shouldDowngrade(config, projectedDaysRemaining)) {
    console.log(`  Projected days remaining (${projectedDaysRemaining.toFixed(1)}) < 3 — downgrading tiers`);
    config.models = downgradeTiers(config);
    await writeConfig(repoRoot, config);
    await appendActivity(repoRoot, {
      type: "model_adjusted",
      message: `Downgraded model tiers (projected days: ${projectedDaysRemaining.toFixed(1)})`,
    });
    await commitAndPush(repoRoot, "chore(terrarium): downgrade model tiers — budget low");
  }

  // Handle resume
  if (shouldResume(config, balance)) {
    console.log("  Balance recovered — upgrading tiers");
    config.models = upgradeTiers(config);
    await writeConfig(repoRoot, config);
    await appendActivity(repoRoot, {
      type: "model_adjusted",
      message: `Upgraded model tiers (balance recovered: $${balance.toFixed(2)})`,
    });
    await commitAndPush(repoRoot, "chore(terrarium): upgrade model tiers — balance recovered");
  }

  const ownerModel = config.models.owner;
  if (!ownerModel) {
    console.log("Owner model is null (paused). Exiting.");
    return;
  }

  // Process mail
  await handleMail(repoRoot, repoOwner, repoName, ownerModel);

  // Review PRs
  await handlePrReviews(repoRoot, repoOwner, repoName, ownerModel);

  // Reconcile plan with issues
  await reconcilePlanWithIssues(repoRoot, repoOwner, repoName, ownerModel);

  // Post stakeholder update if needed
  await postStakeholderUpdate(repoRoot, repoOwner, repoName, ownerModel);

  // Commit any remaining changes
  await commitAndPush(repoRoot, "chore(terrarium): owner cycle updates");

  console.log("[terrarium-owner] Owner cycle complete");
}

main().catch((err) => {
  console.error("[terrarium-owner] Fatal error:", err);
  process.exit(1);
});
