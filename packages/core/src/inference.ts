import Anthropic from "@anthropic-ai/sdk";

const DEFAULT_MAX_TOKENS = 8192;

export interface CompleteOptions {
  model: string;
  prompt: string;
  systemPrompt?: string;
  maxTokens?: number;
}

export interface CompleteResult {
  text: string;
  inputTokens: number;
  outputTokens: number;
  costUsd: number;
}

// Approximate cost per 1M tokens (USD) as of 2025
const COST_PER_1M_INPUT: Record<string, number> = {
  "claude-opus-4-6": 15.0,
  "claude-sonnet-4-6": 3.0,
  "claude-haiku-4-5-20251001": 0.25,
};

const COST_PER_1M_OUTPUT: Record<string, number> = {
  "claude-opus-4-6": 75.0,
  "claude-sonnet-4-6": 15.0,
  "claude-haiku-4-5-20251001": 1.25,
};

function estimateCost(
  model: string,
  inputTokens: number,
  outputTokens: number
): number {
  const inputRate = COST_PER_1M_INPUT[model] ?? 3.0;
  const outputRate = COST_PER_1M_OUTPUT[model] ?? 15.0;
  return (inputTokens * inputRate + outputTokens * outputRate) / 1_000_000;
}

export async function complete(options: CompleteOptions): Promise<CompleteResult> {
  const apiKey = process.env.ANTHROPIC_API_KEY;
  if (!apiKey) {
    throw new Error("ANTHROPIC_API_KEY environment variable is not set");
  }

  const client = new Anthropic({ apiKey });

  const messages: Anthropic.MessageParam[] = [
    { role: "user", content: options.prompt },
  ];

  const response = await client.messages.create({
    model: options.model,
    max_tokens: options.maxTokens ?? DEFAULT_MAX_TOKENS,
    system: options.systemPrompt,
    messages,
  });

  const text = response.content
    .filter((block): block is Anthropic.TextBlock => block.type === "text")
    .map((block) => block.text)
    .join("\n");

  const inputTokens = response.usage.input_tokens;
  const outputTokens = response.usage.output_tokens;
  const costUsd = estimateCost(options.model, inputTokens, outputTokens);

  return { text, inputTokens, outputTokens, costUsd };
}
