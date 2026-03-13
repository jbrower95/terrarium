import { execSync } from "child_process";
import type { InstallAnswers } from "./prompts.js";

export function setGithubSecret(
  repoOwner: string,
  repoName: string,
  name: string,
  value: string
): void {
  execSync(
    `gh secret set ${name} --repo ${repoOwner}/${repoName} --body ${JSON.stringify(value)}`,
    { stdio: "pipe" }
  );
}

export function setGithubVariable(
  repoOwner: string,
  repoName: string,
  name: string,
  value: string
): void {
  execSync(
    `gh variable set ${name} --repo ${repoOwner}/${repoName} --body ${JSON.stringify(value)}`,
    { stdio: "pipe" }
  );
}

export async function setInstallSecrets(answers: InstallAnswers): Promise<void> {
  console.log("Setting GitHub Secrets...");

  setGithubSecret(answers.repoOwner, answers.repoName, "ANTHROPIC_API_KEY", answers.anthropicApiKey);
  console.log("  ✓ ANTHROPIC_API_KEY (secret)");

  // Wallet address is not sensitive — store as a variable so workflows can read it freely
  setGithubVariable(answers.repoOwner, answers.repoName, "TERRARIUM_WALLET_ADDRESS", answers.walletAddress);
  console.log("  ✓ TERRARIUM_WALLET_ADDRESS (variable)");

  // Model assignments — stored as repo variables, not secrets
  // Owner updates these at runtime via `gh variable set` to adjust model strategy
  console.log("Setting model variables...");

  const modelVars: Array<[string, string]> = [
    ["TERRARIUM_MODEL_OWNER", answers.ownerModel],
    ["TERRARIUM_MODEL_HIGH", answers.highModel],
    ["TERRARIUM_MODEL_MEDIUM", answers.mediumModel],
    ["TERRARIUM_MODEL_LOW", answers.lowModel],
    ["TERRARIUM_AUTO_REVIEW", "true"],
  ];

  for (const [name, value] of modelVars) {
    setGithubVariable(answers.repoOwner, answers.repoName, name, value);
    console.log(`  ✓ ${name}=${value} (variable)`);
  }
}
