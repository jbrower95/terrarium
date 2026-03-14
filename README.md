<p align="center">
  <img src="logo.png" width="200" alt="terrarium" />
</p>

<h1 align="center">terrarium</h1>

<p align="center">
  <strong>Autonomous open source, powered by <a href="https://www.x402.org/">x402</a> + GitHub + <a href="https://openrouter.ai/">OpenRouter</a></strong>
</p>

---

Donate to fund a project. An autonomous AI owner manages inference budget, files issues, reviews PRs, and ships features. Fully on GitHub Actions. Treasury is an on-chain smart wallet that only the owner workflow can control — no human holds the keys.

## How it works

```
Supporter donates (buys $PROJECT token on Base via Zora bonding curve)
  → ETH enters bonding curve reserve
  → Trading fees (50%) flow to treasury wallet
  → Treasury is an ERC-4337 smart wallet, OIDC-gated to owner.yml
  → Owner wakes on cron, checks balance
  → Swaps ETH → USDC, tops up OpenRouter credits
  → Owner reads open GitHub Milestones, files issues to fill them
  → Employees (GHA workflows) pick up issues, run inference, push PRs
  → Owner reviews PRs, merges, posts stakeholder updates
  → Project ships features → more supporters → more donations → cycle continues
```

## Getting started

```bash
cd my-project
npx terrarium --install
```

The installer:

1. **Wallet** — deploys an OIDC-gated ERC-4337 smart wallet on Base
2. **Token** — deploys a Zora bonding curve donation token
3. **Secrets** — sets OpenRouter API key + model tier variables
4. **Security** — enables branch protection, CODEOWNERS for workflows
5. **Funding** — guides you through seeding the treasury
6. **First milestone** — creates your first GitHub Milestone

Then push your repo. The owner wakes up on the next cron cycle, reads your milestone, files issues, and starts building.

## Architecture

### Identity & Treasury

The owner's wallet is an **ERC-4337 smart wallet** on Base where transaction authorization is a **GitHub Actions OIDC JWT** — not a private key. No private key exists. The wallet is controlled by cryptographic proof that specific code is running in a specific public repo on GitHub's infrastructure.

### Token & Funding

Each terrarium project gets a **Zora bonding curve token**. Buying the token funds the project. Tokens are donations — holders receive no revenue, governance, or dividends. Trading fees flow to the project treasury.

### Inference & Budget

**OpenRouter** is the inference provider. Model assignments live in GitHub repo variables. The owner autonomously manages model tiers based on available budget — upgrading when flush, downgrading when lean.

### Milestones as the Interface

Users steer the project by creating GitHub Milestones. The owner reads them, breaks them into issues, dispatches employees, and tracks progress. No config files to maintain. No commits needed to change direction.

## Project structure

```
terrarium/
  contracts/                   Solidity (Foundry) — OIDC-gated ERC-4337 wallet
  crates/
    core/                      terrarium-core Rust library
    owner/                     Owner binary (single inference call per cycle)
    employee/                  Employee binary (claim → implement → PR)
    cli/                       Installer CLI
  .github/workflows/
    owner.yml                  Reusable owner workflow
    release.yml                Binary release pipeline
  templates/
    employee.yml               Employee workflow template
    review.yml                 PR review workflow template
```

## Principles

- **No human holds the keys.** The wallet is OIDC-gated. There is no private key.
- **Code is the authority.** The owner's behavior is open-source, auditable, pinned to a specific commit.
- **Repo must be public.** The contract rejects transactions if the repo goes private.
- **Immutable financial constraints.** Daily spend caps and allowed destinations are frozen at deploy.
- **Tokens are donations.** No revenue, governance, or dividends to holders.
- **One inference call per owner cycle.** All context is pre-materialized. The owner decides, the system executes.
- **Milestones are the interface.** Users steer the project via GitHub Milestones.
- **Everything is traceable.** Every on-chain tx links to a GHA run_id and commit sha.

## License

MIT

---

*Terrarium is an independent project and is not affiliated with, endorsed by, or formally associated with x402, GitHub, OpenRouter, Zora, or any other referenced projects or services.*
