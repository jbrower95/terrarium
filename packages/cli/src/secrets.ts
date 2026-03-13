import { execSync } from "child_process";
import type { InstallAnswers } from "./prompts.js";

export async function setGithubSecret(
  repoOwner: string,
  repoName: string,
  name: string,
  value: string
): Promise<void> {
  execSync(
    `gh secret set ${name} --repo ${repoOwner}/${repoName} --body ${JSON.stringify(value)}`,
    { stdio: "pipe" }
  );
}

export async function setInstallSecrets(answers: InstallAnswers): Promise<void> {
  console.log("Setting GitHub Secrets...");

  await setGithubSecret(
    answers.repoOwner,
    answers.repoName,
    "ANTHROPIC_API_KEY",
    answers.anthropicApiKey
  );
  console.log("  ANTHROPIC_API_KEY set");

  await setGithubSecret(
    answers.repoOwner,
    answers.repoName,
    "TERRARIUM_WALLET_ADDRESS",
    answers.walletAddress
  );
  console.log("  TERRARIUM_WALLET_ADDRESS set");
}
