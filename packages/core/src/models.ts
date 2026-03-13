import { TerrariumConfig } from "./config.js";

export type ModelTier = "owner" | "high" | "medium" | "low";

// Downgrade chains: each model steps down to the next
const DOWNGRADE_CHAIN: Record<string, string | null> = {
  "claude-opus-4-6": "claude-sonnet-4-6",
  "claude-sonnet-4-6": "claude-haiku-4-5-20251001",
  "claude-haiku-4-5-20251001": null,
};

// Upgrade chains: each model steps up to the next
const UPGRADE_CHAIN: Record<string, string | null> = {
  "claude-haiku-4-5-20251001": "claude-sonnet-4-6",
  "claude-sonnet-4-6": "claude-opus-4-6",
  "claude-opus-4-6": null,
};

// Default model tiers (initial/max state)
const DEFAULT_MODELS: Record<ModelTier, string> = {
  owner: "claude-opus-4-6",
  high: "claude-opus-4-6",
  medium: "claude-sonnet-4-6",
  low: "claude-haiku-4-5-20251001",
};

export function shouldPause(config: TerrariumConfig, balance: number): boolean {
  return balance < config.budget.pause_threshold_usd;
}

export function shouldDowngrade(
  config: TerrariumConfig,
  projectedDaysRemaining: number
): boolean {
  return (
    projectedDaysRemaining < 3 &&
    !Object.values(config.models).every((m) => m === null)
  );
}

export function shouldResume(config: TerrariumConfig, balance: number): boolean {
  // Resume if any tier is null (paused) and balance recovered above downgrade threshold
  const anyPaused = Object.values(config.models).some((m) => m === null);
  return anyPaused && balance > config.budget.downgrade_threshold_usd;
}

export function pauseAllTiers(
  config: TerrariumConfig
): TerrariumConfig["models"] {
  return {
    owner: null,
    high: null,
    medium: null,
    low: null,
  };
}

export function downgradeTiers(
  config: TerrariumConfig
): TerrariumConfig["models"] {
  const updated = { ...config.models };
  for (const tier of ["owner", "high", "medium", "low"] as ModelTier[]) {
    const current = updated[tier];
    if (current !== null) {
      updated[tier] = DOWNGRADE_CHAIN[current] ?? null;
    }
  }
  return updated;
}

export function upgradeTiers(
  config: TerrariumConfig
): TerrariumConfig["models"] {
  const updated = { ...config.models };
  for (const tier of ["owner", "high", "medium", "low"] as ModelTier[]) {
    const current = updated[tier];
    if (current === null) {
      // Restore to default if fully paused
      updated[tier] = DEFAULT_MODELS[tier];
    } else {
      const upgraded = UPGRADE_CHAIN[current];
      if (upgraded !== null) {
        updated[tier] = upgraded;
      }
    }
  }
  return updated;
}

export { DEFAULT_MODELS };
