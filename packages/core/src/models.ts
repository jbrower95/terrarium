import type { ModelConfig } from "./config.js";

export type ModelTier = "owner" | "high" | "medium" | "low";

export interface ModelEntry {
  id: string;
  name: string;
  provider: string;
  /** MMLU or coding benchmark score (0–100), null if unknown */
  codingScore: number | null;
  /** Input cost in USD per million tokens */
  inputCostPerMTok: number;
  /** Output cost in USD per million tokens */
  outputCostPerMTok: number;
  /** Rough capability tier for display */
  tier: "frontier" | "strong" | "fast" | "budget";
}

/**
 * Known model catalog with benchmarks and pricing.
 * Used by the owner to reason about model selection vs. cost.
 */
export const MODEL_CATALOG: ModelEntry[] = [
  // Kimi
  {
    id: "openrouter/moonshotai/kimi-k2.5",
    name: "Kimi K2.5",
    provider: "Moonshot AI (via OpenRouter)",
    codingScore: 87,
    inputCostPerMTok: 0.15,
    outputCostPerMTok: 2.50,
    tier: "frontier",
  },
  // Qwen
  {
    id: "openrouter/qwen/qwen3.5-35b-a3b",
    name: "Qwen 3.5 35B (A3B)",
    provider: "Alibaba (via OpenRouter)",
    codingScore: 72,
    inputCostPerMTok: 0.07,
    outputCostPerMTok: 0.28,
    tier: "strong",
  },
  {
    id: "openrouter/qwen/qwen3.5-7b",
    name: "Qwen 3.5 7B",
    provider: "Alibaba (via OpenRouter)",
    codingScore: 60,
    inputCostPerMTok: 0.03,
    outputCostPerMTok: 0.10,
    tier: "budget",
  },
  // Claude
  {
    id: "claude-opus-4-6",
    name: "Claude Opus 4.6",
    provider: "Anthropic",
    codingScore: 92,
    inputCostPerMTok: 15.0,
    outputCostPerMTok: 75.0,
    tier: "frontier",
  },
  {
    id: "claude-sonnet-4-6",
    name: "Claude Sonnet 4.6",
    provider: "Anthropic",
    codingScore: 84,
    inputCostPerMTok: 3.0,
    outputCostPerMTok: 15.0,
    tier: "strong",
  },
  {
    id: "claude-haiku-4-5-20251001",
    name: "Claude Haiku 4.5",
    provider: "Anthropic",
    codingScore: 68,
    inputCostPerMTok: 0.25,
    outputCostPerMTok: 1.25,
    tier: "fast",
  },
  // GPT / o-series
  {
    id: "openai/gpt-4o",
    name: "GPT-4o",
    provider: "OpenAI",
    codingScore: 85,
    inputCostPerMTok: 2.5,
    outputCostPerMTok: 10.0,
    tier: "strong",
  },
  {
    id: "openai/o3-mini",
    name: "o3-mini",
    provider: "OpenAI",
    codingScore: 88,
    inputCostPerMTok: 1.1,
    outputCostPerMTok: 4.4,
    tier: "strong",
  },
  {
    id: "openai/gpt-4o-mini",
    name: "GPT-4o Mini",
    provider: "OpenAI",
    codingScore: 62,
    inputCostPerMTok: 0.15,
    outputCostPerMTok: 0.60,
    tier: "budget",
  },
  // Gemini
  {
    id: "openrouter/google/gemini-2.0-flash",
    name: "Gemini 2.0 Flash",
    provider: "Google (via OpenRouter)",
    codingScore: 75,
    inputCostPerMTok: 0.10,
    outputCostPerMTok: 0.40,
    tier: "fast",
  },
  // DeepSeek
  {
    id: "openrouter/deepseek/deepseek-r1",
    name: "DeepSeek R1",
    provider: "DeepSeek (via OpenRouter)",
    codingScore: 89,
    inputCostPerMTok: 0.55,
    outputCostPerMTok: 2.19,
    tier: "frontier",
  },
];

/** Default model tiers. Owner and high use kimi for quality; low/medium use qwen for cost. */
export const DEFAULT_MODELS: Record<ModelTier, string> = {
  owner: "openrouter/moonshotai/kimi-k2.5",
  high: "openrouter/moonshotai/kimi-k2.5",
  medium: "openrouter/qwen/qwen3.5-35b-a3b",
  low: "openrouter/qwen/qwen3.5-35b-a3b",
};

/**
 * Ordered downgrade chains: when budget is tight, step each tier down.
 * null = paused (no inference).
 */
const DOWNGRADE_CHAIN: Record<string, string | null> = {
  "openrouter/moonshotai/kimi-k2.5": "openrouter/qwen/qwen3.5-35b-a3b",
  "openrouter/qwen/qwen3.5-35b-a3b": "openrouter/qwen/qwen3.5-7b",
  "openrouter/qwen/qwen3.5-7b": null,
  "claude-opus-4-6": "claude-sonnet-4-6",
  "claude-sonnet-4-6": "claude-haiku-4-5-20251001",
  "claude-haiku-4-5-20251001": null,
  "openai/gpt-4o": "openai/gpt-4o-mini",
  "openai/o3-mini": "openai/gpt-4o-mini",
  "openai/gpt-4o-mini": null,
  "openrouter/google/gemini-2.0-flash": null,
  "openrouter/deepseek/deepseek-r1": "openrouter/qwen/qwen3.5-35b-a3b",
};

const UPGRADE_CHAIN: Record<string, string | null> = {
  "openrouter/qwen/qwen3.5-7b": "openrouter/qwen/qwen3.5-35b-a3b",
  "openrouter/qwen/qwen3.5-35b-a3b": "openrouter/moonshotai/kimi-k2.5",
  "openrouter/moonshotai/kimi-k2.5": null,
  "claude-haiku-4-5-20251001": "claude-sonnet-4-6",
  "claude-sonnet-4-6": "claude-opus-4-6",
  "claude-opus-4-6": null,
  "openai/gpt-4o-mini": "openai/gpt-4o",
  "openai/gpt-4o": null,
};

export function shouldPause(balance: number, threshold: number): boolean {
  return balance < threshold;
}

export function shouldDowngrade(projectedDaysRemaining: number): boolean {
  return projectedDaysRemaining < 3;
}

export function shouldResume(balance: number, downgradeThreshold: number): boolean {
  return balance > downgradeThreshold;
}

/**
 * Estimate daily cost for a given model config based on typical task volume.
 * Rough heuristic: assume ~50k tokens/task, 5 tasks/day.
 */
export function estimateDailyCost(models: ModelConfig): number {
  const TOKENS_PER_TASK = 50_000;
  const TASKS_PER_DAY = 5;
  let total = 0;
  for (const tier of ["owner", "high", "medium", "low"] as ModelTier[]) {
    const modelId = models[tier];
    if (!modelId) continue;
    const entry = MODEL_CATALOG.find((m) => m.id === modelId);
    if (!entry) continue;
    // Owner runs once per cron tick, not per task
    const count = tier === "owner" ? 1 : TASKS_PER_DAY;
    total += ((entry.inputCostPerMTok + entry.outputCostPerMTok) / 2) * (TOKENS_PER_TASK / 1_000_000) * count;
  }
  return total;
}

export function downgradeTiers(models: ModelConfig): ModelConfig {
  const updated = { ...models };
  for (const tier of ["owner", "high", "medium", "low"] as ModelTier[]) {
    const current = updated[tier];
    if (current !== null && current !== undefined && current !== "") {
      updated[tier] = DOWNGRADE_CHAIN[current] ?? null;
    }
  }
  return updated;
}

export function upgradeTiers(models: ModelConfig): ModelConfig {
  const updated = { ...models };
  for (const tier of ["owner", "high", "medium", "low"] as ModelTier[]) {
    const current = updated[tier];
    if (!current) {
      updated[tier] = DEFAULT_MODELS[tier];
    } else {
      updated[tier] = UPGRADE_CHAIN[current] ?? current;
    }
  }
  return updated;
}

export function pauseAllTiers(_models: ModelConfig): ModelConfig {
  return { owner: null, high: null, medium: null, low: null };
}

/** Format the model catalog as a markdown table for the owner's reasoning prompt. */
export function formatModelCatalog(): string {
  const header = "| Model | Provider | Coding Score | Input $/MTok | Output $/MTok | Tier |\n|---|---|---|---|---|---|";
  const rows = MODEL_CATALOG.map(
    (m) =>
      `| ${m.name} (\`${m.id}\`) | ${m.provider} | ${m.codingScore ?? "N/A"} | $${m.inputCostPerMTok} | $${m.outputCostPerMTok} | ${m.tier} |`
  );
  return [header, ...rows].join("\n");
}
