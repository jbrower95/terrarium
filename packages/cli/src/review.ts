#!/usr/bin/env node
import { execSync } from "child_process";
import { readConfig } from "../../core/src/config.js";
import { appendActivity } from "../../core/src/activity.js";
import {
  reviewPr,
  approvePr,
  requestChanges,
  mergePr,
} from "../../core/src/pr.js";

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

async function main(): Promise<void> {
  const repoRoot = process.cwd();
  console.log(`[terrarium-review] Starting PR review in ${repoRoot}`);

  const rawPrNumber = process.env.PR_NUMBER;
  if (!rawPrNumber) {
    console.error("PR_NUMBER environment variable is not set");
    process.exit(1);
  }

  const prNumber = parseInt(rawPrNumber, 10);
  if (isNaN(prNumber)) {
    console.error(`Invalid PR_NUMBER: ${rawPrNumber}`);
    process.exit(1);
  }

  const config = await readConfig(repoRoot);
  const { owner: repoOwner, name: repoName } = getRepoFromRemote();

  const model = config.models.medium;
  if (!model) {
    console.log("Medium model is null (paused). Skipping review.");
    process.exit(0);
  }

  console.log(`  Reviewing PR #${prNumber} with model ${model}...`);

  try {
    const { approved, feedback } = await reviewPr(repoOwner, repoName, prNumber, {
      model,
    });

    if (approved) {
      approvePr(repoOwner, repoName, prNumber, feedback);
      await appendActivity(repoRoot, {
        type: "pr_reviewed",
        pr_number: prNumber,
        model,
        message: `Approved PR #${prNumber}`,
      });
      console.log(`  Approved PR #${prNumber}`);
    } else {
      requestChanges(repoOwner, repoName, prNumber, feedback);
      await appendActivity(repoRoot, {
        type: "pr_reviewed",
        pr_number: prNumber,
        model,
        message: `Requested changes on PR #${prNumber}`,
      });
      console.log(`  Requested changes on PR #${prNumber}`);
    }

    console.log(`[terrarium-review] Done`);
  } catch (err) {
    console.error("[terrarium-review] Error reviewing PR:", err);
    process.exit(1);
  }
}

main().catch((err) => {
  console.error("[terrarium-review] Fatal error:", err);
  process.exit(1);
});
