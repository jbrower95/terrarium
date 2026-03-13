import fs from "fs/promises";
import path from "path";
import { execSync } from "child_process";

export interface TerrariumConfig {
  wallet: string;
  owner_cron: string;
  max_concurrent_employees: number;
  budget: {
    downgrade_threshold_usd: number;
    pause_threshold_usd: number;
  };
}

/** Model assignments — stored as GitHub repo variables, read from env at runtime. */
export interface ModelConfig {
  owner: string | null;
  high: string | null;
  medium: string | null;
  low: string | null;
}

const CONFIG_FILENAME = "terrarium.json";

// GitHub repo variable names for each tier
export const MODEL_VAR_NAMES: Record<keyof ModelConfig, string> = {
  owner: "TERRARIUM_MODEL_OWNER",
  high: "TERRARIUM_MODEL_HIGH",
  medium: "TERRARIUM_MODEL_MEDIUM",
  low: "TERRARIUM_MODEL_LOW",
};

export const AUTO_REVIEW_VAR = "TERRARIUM_AUTO_REVIEW";

export async function readConfig(repoRoot: string): Promise<TerrariumConfig> {
  const configPath = path.join(repoRoot, CONFIG_FILENAME);
  const raw = await fs.readFile(configPath, "utf-8");
  return JSON.parse(raw) as TerrariumConfig;
}

export async function writeConfig(
  repoRoot: string,
  config: TerrariumConfig
): Promise<void> {
  const configPath = path.join(repoRoot, CONFIG_FILENAME);
  await fs.writeFile(configPath, JSON.stringify(config, null, 2) + "\n", "utf-8");
}

/**
 * Read model config from environment variables.
 * These are set as GitHub repo variables and injected into workflows via vars.*
 * A null value means that tier is paused (no model assigned).
 */
export function readModelConfig(): ModelConfig {
  return {
    owner: process.env[MODEL_VAR_NAMES.owner] ?? null,
    high: process.env[MODEL_VAR_NAMES.high] ?? null,
    medium: process.env[MODEL_VAR_NAMES.medium] ?? null,
    low: process.env[MODEL_VAR_NAMES.low] ?? null,
  };
}

/** Read auto-review setting from env var. Defaults to true. */
export function readAutoReview(): boolean {
  const val = process.env[AUTO_REVIEW_VAR];
  if (val === undefined) return true;
  return val.toLowerCase() !== "false" && val !== "0";
}

/**
 * Update a GitHub repo variable via `gh variable set`.
 * Used by the owner to adjust model tiers without committing terrarium.json.
 */
export function setRepoVariable(
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

export function setModelVar(
  repoOwner: string,
  repoName: string,
  tier: keyof ModelConfig,
  model: string | null
): void {
  setRepoVariable(repoOwner, repoName, MODEL_VAR_NAMES[tier], model ?? "");
}

export function setAutoReview(
  repoOwner: string,
  repoName: string,
  enabled: boolean
): void {
  setRepoVariable(repoOwner, repoName, AUTO_REVIEW_VAR, enabled ? "true" : "false");
}
