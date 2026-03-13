import fs from "fs/promises";
import path from "path";

export interface TerrariumConfig {
  wallet: string;
  models: {
    owner: string | null;
    high: string | null;
    medium: string | null;
    low: string | null;
  };
  owner_cron: string;
  max_concurrent_employees: number;
  budget: {
    downgrade_threshold_usd: number;
    pause_threshold_usd: number;
  };
}

const CONFIG_FILENAME = "terrarium.json";

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
