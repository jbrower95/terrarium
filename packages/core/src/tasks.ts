import { execSync } from "child_process";

export type Complexity = "low" | "medium" | "high";
export type Priority = "low" | "medium" | "high" | "critical";

export interface Issue {
  number: number;
  title: string;
  body: string;
  labels: string[];
  state: "open" | "closed";
  assignees: string[];
}

function gh(args: string): string {
  return execSync(`gh ${args}`, { encoding: "utf-8" }).trim();
}

export function listOpenIssues(
  repoOwner: string,
  repoName: string
): Issue[] {
  const raw = gh(
    `issue list --repo ${repoOwner}/${repoName} --state open --json number,title,body,labels,state,assignees --limit 100`
  );
  const items = JSON.parse(raw) as Array<{
    number: number;
    title: string;
    body: string;
    labels: Array<{ name: string }>;
    state: string;
    assignees: Array<{ login: string }>;
  }>;

  return items.map((item) => ({
    number: item.number,
    title: item.title,
    body: item.body,
    labels: item.labels.map((l) => l.name),
    state: item.state as "open" | "closed",
    assignees: item.assignees.map((a) => a.login),
  }));
}

export function getIssue(
  repoOwner: string,
  repoName: string,
  issueNumber: number
): Issue {
  const raw = gh(
    `issue view ${issueNumber} --repo ${repoOwner}/${repoName} --json number,title,body,labels,state,assignees`
  );
  const item = JSON.parse(raw) as {
    number: number;
    title: string;
    body: string;
    labels: Array<{ name: string }>;
    state: string;
    assignees: Array<{ login: string }>;
  };

  return {
    number: item.number,
    title: item.title,
    body: item.body,
    labels: item.labels.map((l) => l.name),
    state: item.state as "open" | "closed",
    assignees: item.assignees.map((a) => a.login),
  };
}

export function nextTask(
  repoOwner: string,
  repoName: string,
  complexity: Complexity
): Issue | null {
  const issues = listOpenIssues(repoOwner, repoName);

  // Filter: not in-progress, matching complexity label (or no complexity label)
  const available = issues.filter(
    (issue) =>
      !issue.labels.includes("in-progress") &&
      !issue.labels.includes("blocked") &&
      (issue.labels.includes(`complexity:${complexity}`) ||
        !issue.labels.some((l) => l.startsWith("complexity:")))
  );

  if (available.length === 0) return null;

  // Prefer highest priority
  const priorityOrder = ["priority:critical", "priority:high", "priority:medium", "priority:low"];
  for (const p of priorityOrder) {
    const match = available.find((i) => i.labels.includes(p));
    if (match) return match;
  }

  return available[0];
}

export function claimIssue(
  repoOwner: string,
  repoName: string,
  issueNumber: number
): void {
  gh(`issue edit ${issueNumber} --repo ${repoOwner}/${repoName} --add-label "in-progress"`);
  gh(
    `issue comment ${issueNumber} --repo ${repoOwner}/${repoName} --body "Terrarium employee is working on this issue."`
  );
}

export function unclaimIssue(
  repoOwner: string,
  repoName: string,
  issueNumber: number
): void {
  gh(`issue edit ${issueNumber} --repo ${repoOwner}/${repoName} --remove-label "in-progress"`);
}

export function fileIssue(
  repoOwner: string,
  repoName: string,
  title: string,
  body: string,
  complexity: Complexity,
  priority: Priority = "medium"
): number {
  const raw = gh(
    `issue create --repo ${repoOwner}/${repoName} --title ${JSON.stringify(title)} --body ${JSON.stringify(body)} --label "complexity:${complexity},priority:${priority}" --json number`
  );
  const result = JSON.parse(raw) as { number: number };
  return result.number;
}
