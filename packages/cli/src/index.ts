#!/usr/bin/env node
import { execSync } from "child_process";
import path from "path";
import { runInstallWizard } from "./prompts.js";
import { setInstallSecrets } from "./secrets.js";
import { writeWorkflows } from "./workflows.js";
import { writeInitialConfig } from "./config-writer.js";
import { writeStubPlan } from "./plan-writer.js";

function printBanner(): void {
  console.log(`
  ████████╗███████╗██████╗ ██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗
     ██╔══╝██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║
     ██║   █████╗  ██████╔╝██████╔╝███████║██████╔╝██║██║   ██║██╔████╔██║
     ██║   ██╔══╝  ██╔══██╗██╔══██╗██╔══██║██╔══██╗██║██║   ██║██║╚██╔╝██║
     ██║   ███████╗██║  ██║██║  ██║██║  ██║██║  ██║██║╚██████╔╝██║ ╚═╝ ██║
     ╚═╝   ╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝ ╚═════╝ ╚═╝     ╚═╝

  Self-running projects powered by GitHub Actions + Claude
  `);
}

function gitRun(cmd: string, cwd: string): void {
  execSync(cmd, { cwd, stdio: "inherit" });
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);

  if (!args.includes("--install")) {
    console.log("Usage: npx terrarium --install");
    console.log("");
    console.log("  --install    Add terrarium to the current GitHub repo");
    process.exit(0);
  }

  printBanner();

  const repoRoot = process.cwd();
  console.log(`Installing terrarium into: ${repoRoot}\n`);

  // Run onboarding wizard
  const answers = await runInstallWizard();
  console.log("");

  // Write config files
  console.log("Writing configuration files...");
  await writeInitialConfig(repoRoot, answers);
  await writeWorkflows(repoRoot, answers);
  await writeStubPlan(repoRoot, answers.repoName);
  console.log("");

  // Set GitHub Secrets
  await setInstallSecrets(answers);
  console.log("");

  // Commit and push
  console.log("Committing and pushing to GitHub...");
  try {
    gitRun("git add terrarium.json .github/workflows/ PLAN.md", repoRoot);
    gitRun(
      `git commit -m "chore: add terrarium configuration and workflows"`,
      repoRoot
    );
    gitRun("git push", repoRoot);
    console.log("  Committed and pushed\n");
  } catch (err) {
    console.error("  Warning: git commit/push failed:", (err as Error).message);
    console.error("  You may need to commit and push manually.\n");
  }

  // Print success
  const repoUrl = `https://github.com/${answers.repoOwner}/${answers.repoName}`;
  const actionsUrl = `${repoUrl}/actions`;
  const walletUrl = `https://basescan.org/address/${answers.walletAddress}`;

  console.log("terrarium is installed!\n");
  console.log("  Tip jar (Base USDC):");
  console.log(`    ${answers.walletAddress}`);
  console.log(`    ${walletUrl}\n`);
  console.log("  Next steps:");
  console.log(`    1. Fund your tip jar with USDC on Base to start workers`);
  console.log(`    2. Add tasks to PLAN.md and commit`);
  console.log(`    3. The owner will wake every: ${answers.ownerCron}`);
  console.log(`       and file issues from your PLAN.md automatically`);
  console.log(`    4. Dispatch an employee manually at:`);
  console.log(`       ${actionsUrl}`);
  console.log(`    5. Watch progress at:`);
  console.log(`       ${repoUrl}/issues\n`);
  console.log("  Budget thresholds:");
  console.log(`    Downgrade models below: $${answers.downgradThreshold} USDC`);
  console.log(`    Pause all workers below: $${answers.pauseThreshold} USDC\n`);
}

main().catch((err) => {
  console.error("Installation failed:", err);
  process.exit(1);
});
