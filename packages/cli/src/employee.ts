#!/usr/bin/env node
import { execSync, spawnSync } from "child_process";
import { readConfig } from "../../core/src/config.js";
import { appendActivity } from "../../core/src/activity.js";
import { appendMail } from "../../core/src/mail.js";
import {
  nextTask,
  claimIssue,
  unclaimIssue,
  getIssue,
  type Complexity,
} from "../../core/src/tasks.js";
import { createPr } from "../../core/src/pr.js";

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

function buildEmployeePrompt(issue: {
  number: number;
  title: string;
  body: string;
}): string {
  return `You are a software engineer working on a GitHub repository. Your job is to implement the following GitHub issue.

Issue #${issue.number}: ${issue.title}

${issue.body}

Instructions:
1. Create a new git branch named: terrarium/issue-${issue.number}
2. Implement the changes required by this issue
3. Commit your changes with a meaningful commit message referencing issue #${issue.number}
4. Push the branch to origin

Use the following git commands:
- git checkout -b terrarium/issue-${issue.number}
- (make your changes)
- git add -A
- git commit -m "fix: implement issue #${issue.number} — ${issue.title}"
- git push -u origin terrarium/issue-${issue.number}

Important:
- Write clean, well-structured code
- Follow existing code conventions in the repository
- Add tests if the project has a test suite
- Do not modify unrelated files
- If you cannot complete the task, stop and explain why`;
}

function runClaudeCli(prompt: string, model: string): { success: boolean; output: string } {
  const result = spawnSync(
    "claude",
    ["--print", "--model", model, prompt],
    {
      encoding: "utf-8",
      timeout: 10 * 60 * 1000, // 10 minute timeout
      env: { ...process.env },
    }
  );

  if (result.error) {
    return { success: false, output: result.error.message };
  }

  const output = (result.stdout ?? "") + (result.stderr ?? "");
  const success = result.status === 0;

  return { success, output };
}

function branchExists(branchName: string): boolean {
  try {
    execSync(`git rev-parse --verify ${branchName}`, {
      stdio: "pipe",
      encoding: "utf-8",
    });
    return true;
  } catch {
    return false;
  }
}

async function main(): Promise<void> {
  const repoRoot = process.cwd();
  console.log(`[terrarium-employee] Starting task in ${repoRoot}`);

  // config is still read for wallet/budget fields; models come from env vars
  await readConfig(repoRoot);
  const { owner: repoOwner, name: repoName } = getRepoFromRemote();

  const rawComplexity = process.env.TASK_COMPLEXITY ?? "medium";
  const complexity = (["low", "medium", "high"].includes(rawComplexity)
    ? rawComplexity
    : "medium") as Complexity;

  const rawIssueNumber = process.env.TASK_ISSUE_NUMBER;
  let issueNumber: number | null = rawIssueNumber
    ? parseInt(rawIssueNumber, 10)
    : null;

  if (issueNumber !== null && isNaN(issueNumber)) {
    issueNumber = null;
  }

  // Models live in GitHub repo variables, injected as env vars by the workflow
  const MODEL_ENV_VARS: Record<Complexity, string> = {
    low: "TERRARIUM_MODEL_LOW",
    medium: "TERRARIUM_MODEL_MEDIUM",
    high: "TERRARIUM_MODEL_HIGH",
  };
  const model = process.env[MODEL_ENV_VARS[complexity]] ?? null;
  if (!model) {
    console.log(
      `Model for complexity '${complexity}' is null (paused). Exiting.`
    );
    process.exit(0);
  }

  // Auto-select issue if not provided
  if (issueNumber === null) {
    console.log(
      `No issue number provided. Auto-selecting a ${complexity} complexity task...`
    );
    const task = nextTask(repoOwner, repoName, complexity);
    if (!task) {
      console.log("No available tasks found. Exiting.");
      process.exit(0);
    }
    issueNumber = task.number;
    console.log(`  Selected issue #${issueNumber}: ${task.title}`);
  }

  // Fetch issue details
  const issue = getIssue(repoOwner, repoName, issueNumber);
  console.log(`Working on issue #${issue.number}: ${issue.title}`);

  // Claim the issue
  claimIssue(repoOwner, repoName, issueNumber);
  console.log(`  Claimed issue #${issueNumber}`);

  const branchName = `terrarium/issue-${issueNumber}`;

  try {
    // Run claude CLI to implement the issue
    console.log(`  Running Claude (${model}) on issue #${issueNumber}...`);
    const prompt = buildEmployeePrompt(issue);
    const { success, output } = runClaudeCli(prompt, model);

    if (!success) {
      throw new Error(`Claude CLI failed:\n${output}`);
    }

    console.log("  Claude completed implementation");

    // Verify branch was created and pushed
    if (!branchExists(branchName) && !branchExists(`origin/${branchName}`)) {
      // Claude may not have created the branch — try to push whatever we have
      console.log(
        `  Branch ${branchName} not found — checking for uncommitted changes`
      );

      const status = execSync("git status --porcelain", {
        encoding: "utf-8",
      }).trim();

      if (status.length > 0) {
        execSync(`git checkout -b ${branchName}`, { stdio: "pipe" });
        execSync("git add -A", { stdio: "pipe" });
        execSync(
          `git commit -m "fix: implement issue #${issueNumber} — ${issue.title}"`,
          { stdio: "pipe" }
        );
        execSync(`git push -u origin ${branchName}`, { stdio: "pipe" });
      } else {
        throw new Error(
          "No changes were made and no branch was created by Claude"
        );
      }
    }

    // Create PR
    const prBody = `Closes #${issueNumber}\n\n## Summary\n\nThis PR implements the changes requested in issue #${issueNumber}.\n\n${issue.title}\n\n---\n_Implemented by terrarium-employee using ${model}_`;

    const prNumber = createPr(
      repoOwner,
      repoName,
      `fix: ${issue.title} (#${issueNumber})`,
      prBody,
      branchName
    );

    // Remove in-progress label
    unclaimIssue(repoOwner, repoName, issueNumber);

    await appendActivity(repoRoot, {
      type: "submitted",
      issue_number: issueNumber,
      pr_number: prNumber,
      model,
      message: `Submitted PR #${prNumber} for issue #${issueNumber}`,
    });

    console.log(
      `[terrarium-employee] Done. Opened PR #${prNumber} for issue #${issueNumber}`
    );
  } catch (err) {
    const errorMessage = (err as Error).message;
    console.error(`[terrarium-employee] Error:`, errorMessage);

    // Unclaim the issue
    try {
      unclaimIssue(repoOwner, repoName, issueNumber);
    } catch (unclaimErr) {
      console.error("Failed to unclaim issue:", unclaimErr);
    }

    // Append to MAIL.md
    await appendMail(repoRoot, {
      from: "terrarium-employee",
      subject: `Failed to implement issue #${issueNumber}: ${issue.title}`,
      body: `The employee worker failed to complete issue #${issueNumber}.\n\n**Error:**\n\`\`\`\n${errorMessage}\n\`\`\`\n\n**Issue title:** ${issue.title}\n**Complexity:** ${complexity}\n**Model:** ${model}`,
    });

    await appendActivity(repoRoot, {
      type: "mail_sent",
      issue_number: issueNumber,
      model,
      message: `Failed to implement issue #${issueNumber}: ${errorMessage.slice(0, 200)}`,
    });

    process.exit(1);
  }
}

main().catch((err) => {
  console.error("[terrarium-employee] Fatal error:", err);
  process.exit(1);
});
