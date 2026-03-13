# terrarium

A self-running project framework. Point it at a GitHub repo, fund a Base USDC wallet, and it will autonomously file issues, write code, open PRs, and review them — scaling model quality to your available budget.

---

## Quickstart

```bash
cd your-repo
npx terrarium --install
```

The install wizard will ask for:

1. Your GitHub repo (auto-detected from `git remote`)
2. A Base wallet address for the tip jar (read-only; never signs transactions)
3. Your Anthropic API key (written directly to GitHub Secrets, never stored locally)
4. Initial model tiers (sensible defaults provided)
5. Owner cron schedule (default: every 30 minutes)
6. Max concurrent employees (default: 3)

It then writes `terrarium.json`, the three GHA workflows, and a stub `PLAN.md` to your repo, sets the required GitHub Secrets, and commits everything.

---

## How it works

Edit `PLAN.md` in your repo. The owner reads it on every wake and files GitHub issues for any backlog item not already tracked. Employees pick up issues, write code, push branches, and open PRs. The owner reviews and merges approved PRs, then loops.

### Budget and model tiers

`terrarium.json` maps four task tiers to model names:

```json
{
  "models": {
    "owner":  "claude-opus-4-6",
    "high":   "claude-opus-4-6",
    "medium": "claude-sonnet-4-6",
    "low":    "claude-haiku-4-5-20251001"
  }
}
```

On each owner wake, terrarium reads the USDC balance from the Base RPC and computes a daily run-rate from `activity.jsonl`. When projected runway drops below 3 days, models downgrade one tier. When the balance drops below `pause_threshold_usd`, all new tasks halt. When you top up the wallet and the balance rises above `downgrade_threshold_usd`, models resume and upgrade automatically.

The owner commits any tier changes back to `terrarium.json`. No secrets are ever stored in the repo.

---

## GitHub Actions workflows

Three workflows are installed into `.github/workflows/`:

### `owner.yml` — scheduled cron (default: every 30 min)

The owner daemon. On each wake it:

- Checks the USDC balance and adjusts model tiers if needed
- Reads `PLAN.md` and files any missing GitHub issues
- Reads `MAIL.md` and responds to messages from stuck employees
- Reviews open PRs (auto-merges if approved, requests changes if not)
- Posts a stakeholder summary issue when enough PRs have merged

### `employee.yml` — manual dispatch (or triggered by owner)

Runs a single task. Inputs: `complexity` (low / medium / high) and an optional `issue_number`. If no issue number is given, the employee auto-selects the highest-priority available issue. Uses `TERRARIUM_GH_TOKEN` (a PAT) so pushes and PRs appear under a non-Actions identity.

### `pr-review.yml` — triggered on PR open/sync

Runs automatically when a PR is opened or updated by `github-actions[bot]`. Runs an inference review and either approves (adding `auto-approved`) or requests changes (adding `needs-changes`).

---

## Tip jar

Send USDC on Base to your configured wallet address to fund inference. The more you put in, the smarter the models and the faster the cadence. When funds run low, terrarium degrades gracefully rather than failing hard.

Required GitHub Secrets (set automatically by `npx terrarium --install`):

- `ANTHROPIC_API_KEY` — inference
- `TERRARIUM_GH_TOKEN` — a GitHub PAT with repo scope (for employee branch pushes and PRs)

Optional:
- `BASE_RPC_URL` — Base RPC endpoint for balance reads (defaults to a public endpoint)
