import fs from "fs/promises";
import path from "path";
import type { TerrariumConfig } from "./config.js";
import type { ModelConfig } from "./config.js";

const STATUS_START = "<!-- terrarium-status-start -->";
const STATUS_END = "<!-- terrarium-status-end -->";

export interface StatusData {
  walletAddress: string;
  tokenAddress: string | null;
  balanceEth: number;
  balanceUsd: number;
  openRouterCredits: number;
  dailyRunRate: number;
  projectedDays: number;
  models: ModelConfig;
  autoReview: boolean;
  openIssues: number;
  openPrs: number;
  totalMerged: number;
  zoraUrl: string | null;
  basescanUrl: string;
}

function healthColor(projectedDays: number): string {
  if (projectedDays === Infinity || projectedDays > 14) return "brightgreen";
  if (projectedDays > 7) return "green";
  if (projectedDays > 3) return "yellow";
  if (projectedDays > 1) return "orange";
  return "red";
}

function modelBadge(tier: string, model: string | null): string {
  const display = model ? model.split("/").pop()! : "paused";
  const color = model ? "blue" : "lightgrey";
  return `![${tier}](https://img.shields.io/badge/${encodeURIComponent(tier)}-${encodeURIComponent(display)}-${color})`;
}

function shieldsBadge(label: string, value: string, color: string): string {
  return `![${label}](https://img.shields.io/badge/${encodeURIComponent(label)}-${encodeURIComponent(value)}-${color})`;
}

export function renderStatusMarkdown(data: StatusData): string {
  const health = healthColor(data.projectedDays);
  const projectedLabel = Number.isFinite(data.projectedDays)
    ? `${data.projectedDays.toFixed(0)} days`
    : "∞";

  const balanceBadge = shieldsBadge(
    "treasury",
    `$${data.balanceUsd.toFixed(2)}`,
    health
  );
  const runwayBadge = shieldsBadge("runway", projectedLabel, health);
  const runRateBadge = shieldsBadge(
    "burn",
    `$${data.dailyRunRate.toFixed(2)}/day`,
    "informational"
  );
  const creditsBadge = shieldsBadge(
    "credits",
    `$${data.openRouterCredits.toFixed(2)}`,
    data.openRouterCredits > 1 ? "green" : "red"
  );

  const donateButton = data.zoraUrl
    ? `[![Donate](https://img.shields.io/badge/donate-fund_this_project-ff69b4?style=for-the-badge)](${data.zoraUrl})`
    : `[![Donate](https://img.shields.io/badge/donate-send_ETH_on_Base-ff69b4?style=for-the-badge)](${data.basescanUrl})`;

  const lines = [
    STATUS_START,
    "",
    `### 🌱 terrarium`,
    "",
    `${balanceBadge} ${runwayBadge} ${runRateBadge} ${creditsBadge}`,
    "",
    `${modelBadge("owner", data.models.owner)} ${modelBadge("high", data.models.high)} ${modelBadge("medium", data.models.medium)} ${modelBadge("low", data.models.low)}`,
    "",
    `| | |`,
    `|---|---|`,
    `| **Open issues** | ${data.openIssues} |`,
    `| **Open PRs** | ${data.openPrs} |`,
    `| **Merged PRs** | ${data.totalMerged} |`,
    `| **Auto-review** | ${data.autoReview ? "enabled" : "disabled"} |`,
    `| **Wallet** | [\`${data.walletAddress.slice(0, 6)}...${data.walletAddress.slice(-4)}\`](${data.basescanUrl}) |`,
    "",
    donateButton,
    "",
    `<sub>Updated by terrarium owner — ${new Date().toISOString().replace("T", " ").slice(0, 19)} UTC</sub>`,
    "",
    STATUS_END,
  ];

  return lines.join("\n");
}

/**
 * Update the README.md in the repo root, replacing everything between
 * the terrarium status markers. If markers don't exist, prepend the status
 * block at the top of the file (after the first heading).
 */
export async function updateReadmeStatus(
  repoRoot: string,
  data: StatusData
): Promise<void> {
  const readmePath = path.join(repoRoot, "README.md");
  let content: string;
  try {
    content = await fs.readFile(readmePath, "utf-8");
  } catch {
    content = "# Project\n";
  }

  const statusBlock = renderStatusMarkdown(data);

  if (content.includes(STATUS_START) && content.includes(STATUS_END)) {
    // Replace existing block
    const startIdx = content.indexOf(STATUS_START);
    const endIdx = content.indexOf(STATUS_END) + STATUS_END.length;
    content = content.slice(0, startIdx) + statusBlock + content.slice(endIdx);
  } else {
    // Insert after first heading
    const headingMatch = content.match(/^#[^\n]*\n/m);
    if (headingMatch) {
      const insertAt = headingMatch.index! + headingMatch[0].length;
      content =
        content.slice(0, insertAt) +
        "\n" +
        statusBlock +
        "\n" +
        content.slice(insertAt);
    } else {
      content = statusBlock + "\n\n" + content;
    }
  }

  await fs.writeFile(readmePath, content, "utf-8");
}
