import fs from "fs/promises";
import path from "path";
import type { InstallAnswers } from "./prompts.js";
import type { TerrariumConfig } from "../../core/src/config.js";

export function buildInitialConfig(answers: InstallAnswers): TerrariumConfig {
  return {
    wallet: answers.walletAddress,
    models: {
      owner: answers.ownerModel,
      high: answers.highModel,
      medium: answers.mediumModel,
      low: answers.lowModel,
    },
    owner_cron: answers.ownerCron,
    max_concurrent_employees: answers.maxConcurrentEmployees,
    budget: {
      downgrade_threshold_usd: answers.downgradThreshold,
      pause_threshold_usd: answers.pauseThreshold,
    },
  };
}

export async function writeInitialConfig(
  repoRoot: string,
  answers: InstallAnswers
): Promise<void> {
  const config = buildInitialConfig(answers);
  const configPath = path.join(repoRoot, "terrarium.json");
  await fs.writeFile(configPath, JSON.stringify(config, null, 2) + "\n", "utf-8");
  console.log(`  Wrote terrarium.json`);
}
