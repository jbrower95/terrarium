import { execSync } from "child_process";
import { complete } from "./inference.js";

export interface PullRequest {
  number: number;
  title: string;
  body: string;
  headRefName: string;
  baseRefName: string;
  author: string;
  labels: string[];
  isDraft: boolean;
  mergeable: string;
}

function gh(args: string): string {
  return execSync(`gh ${args}`, { encoding: "utf-8" }).trim();
}

export function listOpenPrs(repoOwner: string, repoName: string): PullRequest[] {
  const raw = gh(
    `pr list --repo ${repoOwner}/${repoName} --state open --json number,title,body,headRefName,baseRefName,author,labels,isDraft,mergeable --limit 50`
  );
  const items = JSON.parse(raw) as Array<{
    number: number;
    title: string;
    body: string;
    headRefName: string;
    baseRefName: string;
    author: { login: string };
    labels: Array<{ name: string }>;
    isDraft: boolean;
    mergeable: string;
  }>;

  return items.map((item) => ({
    number: item.number,
    title: item.title,
    body: item.body,
    headRefName: item.headRefName,
    baseRefName: item.baseRefName,
    author: item.author.login,
    labels: item.labels.map((l) => l.name),
    isDraft: item.isDraft,
    mergeable: item.mergeable,
  }));
}

export function getPrDiff(
  repoOwner: string,
  repoName: string,
  prNumber: number
): string {
  return gh(`pr diff ${prNumber} --repo ${repoOwner}/${repoName}`);
}

export async function reviewPr(
  repoOwner: string,
  repoName: string,
  prNumber: number,
  options: { model: string }
): Promise<{ approved: boolean; feedback: string }> {
  const pr = JSON.parse(
    gh(
      `pr view ${prNumber} --repo ${repoOwner}/${repoName} --json number,title,body,headRefName`
    )
  ) as { number: number; title: string; body: string; headRefName: string };

  let diff = "";
  try {
    diff = getPrDiff(repoOwner, repoName, prNumber);
  } catch {
    diff = "(diff unavailable)";
  }

  const prompt = `You are a code reviewer. Review the following pull request and determine if it should be approved or needs changes.

PR Title: ${pr.title}
PR Body: ${pr.body}

Diff:
\`\`\`
${diff.slice(0, 20000)}
\`\`\`

Respond with a JSON object:
{
  "approved": true|false,
  "feedback": "Your review feedback here"
}

Only respond with the JSON object, no other text.`;

  const result = await complete({ model: options.model, prompt });

  try {
    const parsed = JSON.parse(result.text.trim()) as {
      approved: boolean;
      feedback: string;
    };
    return parsed;
  } catch {
    // If parsing fails, default to requesting changes
    return { approved: false, feedback: result.text };
  }
}

export function approvePr(
  repoOwner: string,
  repoName: string,
  prNumber: number,
  feedback: string
): void {
  gh(
    `pr review ${prNumber} --repo ${repoOwner}/${repoName} --approve --body ${JSON.stringify(feedback)}`
  );
  gh(
    `pr edit ${prNumber} --repo ${repoOwner}/${repoName} --add-label "auto-approved"`
  );
}

export function requestChanges(
  repoOwner: string,
  repoName: string,
  prNumber: number,
  feedback: string
): void {
  gh(
    `pr review ${prNumber} --repo ${repoOwner}/${repoName} --request-changes --body ${JSON.stringify(feedback)}`
  );
  gh(
    `pr edit ${prNumber} --repo ${repoOwner}/${repoName} --add-label "needs-changes"`
  );
}

export function mergePr(
  repoOwner: string,
  repoName: string,
  prNumber: number
): void {
  gh(
    `pr merge ${prNumber} --repo ${repoOwner}/${repoName} --squash --auto`
  );
}

export function createPr(
  repoOwner: string,
  repoName: string,
  title: string,
  body: string,
  headBranch: string,
  baseBranch = "main"
): number {
  const raw = gh(
    `pr create --repo ${repoOwner}/${repoName} --title ${JSON.stringify(title)} --body ${JSON.stringify(body)} --head ${headBranch} --base ${baseBranch} --json number`
  );
  const result = JSON.parse(raw) as { number: number };
  return result.number;
}
