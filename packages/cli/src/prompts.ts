import { execSync } from "child_process";
import inquirer from "inquirer";

export interface InstallAnswers {
  repoOwner: string;
  repoName: string;
  walletAddress: string;
  anthropicApiKey: string;
  ownerModel: string;
  highModel: string;
  mediumModel: string;
  lowModel: string;
  ownerCron: string;
  maxConcurrentEmployees: number;
  downgradThreshold: number;
  pauseThreshold: number;
}

function detectRepoFromRemote(): { owner: string; name: string } | null {
  try {
    const remoteUrl = execSync("git remote get-url origin", {
      encoding: "utf-8",
    }).trim();

    // Handle SSH: git@github.com:owner/repo.git
    const sshMatch = remoteUrl.match(/git@github\.com:([^/]+)\/(.+?)(?:\.git)?$/);
    if (sshMatch) {
      return { owner: sshMatch[1], name: sshMatch[2] };
    }

    // Handle HTTPS: https://github.com/owner/repo.git
    const httpsMatch = remoteUrl.match(
      /https?:\/\/github\.com\/([^/]+)\/(.+?)(?:\.git)?$/
    );
    if (httpsMatch) {
      return { owner: httpsMatch[1], name: httpsMatch[2] };
    }
  } catch {
    // Not a git repo or no remote
  }
  return null;
}

export async function runInstallWizard(): Promise<InstallAnswers> {
  const detected = detectRepoFromRemote();

  const answers = await inquirer.prompt<InstallAnswers>([
    {
      type: "input",
      name: "repoOwner",
      message: "GitHub repo owner (user or org):",
      default: detected?.owner ?? "",
      validate: (input: string) =>
        input.trim().length > 0 ? true : "Repo owner is required",
    },
    {
      type: "input",
      name: "repoName",
      message: "GitHub repo name:",
      default: detected?.name ?? "",
      validate: (input: string) =>
        input.trim().length > 0 ? true : "Repo name is required",
    },
    {
      type: "input",
      name: "walletAddress",
      message: "Base wallet address (tip jar, read-only):",
      validate: (input: string) => {
        if (!/^0x[0-9a-fA-F]{40}$/.test(input.trim())) {
          return "Please enter a valid Ethereum/Base address (0x...)";
        }
        return true;
      },
    },
    {
      type: "password",
      name: "anthropicApiKey",
      message: "Anthropic API key (will be stored as GitHub Secret, never saved locally):",
      mask: "*",
      validate: (input: string) =>
        input.trim().length > 0 ? true : "Anthropic API key is required",
    },
    {
      type: "input",
      name: "ownerModel",
      message: "Owner model:",
      default: "claude-opus-4-6",
    },
    {
      type: "input",
      name: "highModel",
      message: "High complexity model:",
      default: "claude-opus-4-6",
    },
    {
      type: "input",
      name: "mediumModel",
      message: "Medium complexity model:",
      default: "claude-sonnet-4-6",
    },
    {
      type: "input",
      name: "lowModel",
      message: "Low complexity model:",
      default: "claude-haiku-4-5-20251001",
    },
    {
      type: "input",
      name: "ownerCron",
      message: "Owner cron schedule (default: every 30 minutes):",
      default: "*/30 * * * *",
      validate: (input: string) => {
        const parts = input.trim().split(/\s+/);
        if (parts.length !== 5) {
          return "Please enter a valid cron expression (5 fields)";
        }
        return true;
      },
    },
    {
      type: "number",
      name: "maxConcurrentEmployees",
      message: "Max concurrent employees:",
      default: 3,
      validate: (input: number) =>
        input >= 1 && input <= 20 ? true : "Must be between 1 and 20",
    },
    {
      type: "number",
      name: "downgradThreshold",
      message: "Downgrade threshold (USD balance):",
      default: 5.0,
      validate: (input: number) =>
        input > 0 ? true : "Must be greater than 0",
    },
    {
      type: "number",
      name: "pauseThreshold",
      message: "Pause threshold (USD balance):",
      default: 1.0,
      validate: (input: number) =>
        input > 0 ? true : "Must be greater than 0",
    },
  ]);

  return answers;
}
