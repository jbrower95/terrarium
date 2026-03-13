import fs from "fs/promises";
import path from "path";

const JOURNAL_FILENAME = "JOURNAL.md";
const ENTRY_SEPARATOR = "\n---\n";
// How many recent entries to surface as context on wake
const CONTEXT_ENTRY_COUNT = 5;

export interface JournalEntry {
  timestamp: string;
  balance: number;
  dailyRunRate: number;
  projectedDays: number;
  models: Record<string, string | null>;
  autoReview: boolean;
  mergesSinceLastUpdate: number;
  body: string;
}

function formatEntry(entry: JournalEntry): string {
  return [
    `## Owner Update — ${entry.timestamp}`,
    ``,
    `**Budget:** $${entry.balance.toFixed(2)} USDC | Run rate: $${entry.dailyRunRate.toFixed(2)}/day | ~${Number.isFinite(entry.projectedDays) ? entry.projectedDays.toFixed(1) : "∞"} days remaining`,
    `**Models:** owner=\`${entry.models.owner ?? "paused"}\` | high=\`${entry.models.high ?? "paused"}\` | medium=\`${entry.models.medium ?? "paused"}\` | low=\`${entry.models.low ?? "paused"}\``,
    `**Auto-review:** ${entry.autoReview ? "enabled" : "disabled"} | **Merges since last stakeholder post:** ${entry.mergesSinceLastUpdate}`,
    ``,
    entry.body,
  ].join("\n");
}

export async function appendJournalEntry(
  repoRoot: string,
  entry: JournalEntry
): Promise<void> {
  const journalPath = path.join(repoRoot, JOURNAL_FILENAME);
  const formatted = formatEntry(entry) + ENTRY_SEPARATOR;

  let existing = "";
  try {
    existing = await fs.readFile(journalPath, "utf-8");
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") throw err;
    // First entry — add a header
    existing = `# Terrarium Owner Journal\n\nThis file is written by the terrarium owner on each wake cycle.\nIt serves as persistent memory across runs.\n${ENTRY_SEPARATOR}`;
  }

  await fs.writeFile(journalPath, existing + formatted, "utf-8");
}

/**
 * Read the last N journal entries as a context string for the owner's reasoning prompt.
 */
export async function readJournalContext(
  repoRoot: string,
  count = CONTEXT_ENTRY_COUNT
): Promise<string> {
  const journalPath = path.join(repoRoot, JOURNAL_FILENAME);
  let raw: string;
  try {
    raw = await fs.readFile(journalPath, "utf-8");
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") return "";
    throw err;
  }

  // Split on the separator, filter empty/header chunks, take last N
  const entries = raw
    .split(ENTRY_SEPARATOR)
    .map((s) => s.trim())
    .filter((s) => s.startsWith("## Owner Update"));

  const recent = entries.slice(-count);
  if (recent.length === 0) return "";

  return `## Previous owner journal entries (most recent ${recent.length})\n\n${recent.join("\n\n---\n\n")}`;
}
