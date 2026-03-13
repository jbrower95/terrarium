import fs from "fs/promises";
import path from "path";

export type ActivityType =
  | "owner_wake"
  | "model_adjusted"
  | "task_claimed"
  | "submitted"
  | "pr_reviewed"
  | "pr_merged"
  | "issue_filed"
  | "mail_sent"
  | "stakeholder_update";

export interface ActivityEntry {
  timestamp: string;
  type: ActivityType;
  issue_number?: number;
  pr_number?: number;
  model?: string;
  cost_usd?: number;
  message?: string;
  [key: string]: unknown;
}

const ACTIVITY_FILENAME = "activity.jsonl";

export async function appendActivity(
  repoRoot: string,
  entry: Omit<ActivityEntry, "timestamp">
): Promise<void> {
  const activityPath = path.join(repoRoot, ACTIVITY_FILENAME);
  const line =
    JSON.stringify({ timestamp: new Date().toISOString(), ...entry }) + "\n";
  await fs.appendFile(activityPath, line, "utf-8");
}

export async function readActivityLog(
  repoRoot: string
): Promise<ActivityEntry[]> {
  const activityPath = path.join(repoRoot, ACTIVITY_FILENAME);
  try {
    const raw = await fs.readFile(activityPath, "utf-8");
    return raw
      .split("\n")
      .filter((line) => line.trim().length > 0)
      .map((line) => JSON.parse(line) as ActivityEntry);
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw err;
  }
}

export async function countMergesSinceLastUpdate(
  repoRoot: string
): Promise<number> {
  const entries = await readActivityLog(repoRoot);

  const lastUpdateIdx = entries
    .map((e, i) => ({ e, i }))
    .filter(({ e }) => e.type === "stakeholder_update")
    .pop()?.i ?? -1;

  return entries
    .slice(lastUpdateIdx + 1)
    .filter((e) => e.type === "pr_merged").length;
}
