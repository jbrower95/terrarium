import fs from "fs/promises";
import path from "path";
import type { InstallAnswers } from "./prompts.js";

/** Shared env block injected into any step that needs model config from repo variables */
const MODEL_VARS_ENV = `\
          TERRARIUM_MODEL_OWNER: \${{ vars.TERRARIUM_MODEL_OWNER }}
          TERRARIUM_MODEL_HIGH: \${{ vars.TERRARIUM_MODEL_HIGH }}
          TERRARIUM_MODEL_MEDIUM: \${{ vars.TERRARIUM_MODEL_MEDIUM }}
          TERRARIUM_MODEL_LOW: \${{ vars.TERRARIUM_MODEL_LOW }}
          TERRARIUM_AUTO_REVIEW: \${{ vars.TERRARIUM_AUTO_REVIEW }}`;

function renderOwnerWorkflow(answers: InstallAnswers): string {
  return `name: Terrarium Owner
on:
  schedule:
    - cron: '${answers.ownerCron}'
  workflow_dispatch: {}

concurrency:
  group: terrarium-owner
  cancel-in-progress: false

jobs:
  owner:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      issues: write
      pull-requests: write
      # Needed to update repo variables (model assignments)
      actions: write
    steps:
      - uses: actions/checkout@v4
        with:
          token: \${{ secrets.GITHUB_TOKEN }}
          # Fetch full history so git push works
          fetch-depth: 0
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
      - run: npm ci
      - name: Run owner cycle
        run: npx terrarium-owner
        env:
          ANTHROPIC_API_KEY: \${{ secrets.ANTHROPIC_API_KEY }}
          GITHUB_TOKEN: \${{ secrets.GITHUB_TOKEN }}
${MODEL_VARS_ENV}
`;
}

function renderEmployeeWorkflow(): string {
  return `name: Terrarium Employee
on:
  workflow_dispatch:
    inputs:
      complexity:
        description: 'Task complexity'
        required: true
        default: 'medium'
        type: choice
        options: [low, medium, high]
      issue_number:
        description: 'Issue number (leave blank to auto-select)'
        required: false

concurrency:
  group: terrarium-issue-\${{ inputs.issue_number || 'auto' }}
  cancel-in-progress: false

jobs:
  employee:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      issues: write
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
        with:
          token: \${{ secrets.GITHUB_TOKEN }}
          fetch-depth: 0
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
      - run: npm ci
      - name: Install Claude Code CLI
        run: npm install -g @anthropic-ai/claude-code
      - name: Run employee task
        run: npx terrarium-employee
        env:
          ANTHROPIC_API_KEY: \${{ secrets.ANTHROPIC_API_KEY }}
          GITHUB_TOKEN: \${{ secrets.GITHUB_TOKEN }}
          TASK_COMPLEXITY: \${{ inputs.complexity }}
          TASK_ISSUE_NUMBER: \${{ inputs.issue_number }}
${MODEL_VARS_ENV}
`;
}

function renderPrReviewWorkflow(): string {
  return `name: Terrarium PR Review
on:
  pull_request:
    types: [opened, synchronize, reopened]

concurrency:
  group: terrarium-pr-review-\${{ github.event.pull_request.number }}
  cancel-in-progress: true

jobs:
  review:
    runs-on: ubuntu-latest
    # Only auto-review PRs opened by the terrarium employee workflow
    if: github.actor == 'github-actions[bot]'
    permissions:
      contents: read
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
      - run: npm ci
      - name: Review PR
        run: npx terrarium-review
        env:
          ANTHROPIC_API_KEY: \${{ secrets.ANTHROPIC_API_KEY }}
          GITHUB_TOKEN: \${{ secrets.GITHUB_TOKEN }}
          PR_NUMBER: \${{ github.event.pull_request.number }}
${MODEL_VARS_ENV}
`;
}

export async function writeWorkflows(
  repoRoot: string,
  answers: InstallAnswers
): Promise<void> {
  const workflowsDir = path.join(repoRoot, ".github", "workflows");
  await fs.mkdir(workflowsDir, { recursive: true });

  const files: Array<{ name: string; content: string }> = [
    { name: "owner.yml", content: renderOwnerWorkflow(answers) },
    { name: "employee.yml", content: renderEmployeeWorkflow() },
    { name: "pr-review.yml", content: renderPrReviewWorkflow() },
  ];

  for (const file of files) {
    const filePath = path.join(workflowsDir, file.name);
    await fs.writeFile(filePath, file.content, "utf-8");
    console.log(`  Wrote ${path.relative(repoRoot, filePath)}`);
  }
}
