# terrarium — PLAN

A self-running open source project framework.

Donate to fund a project → an autonomous AI owner manages inference budget, files issues,
reviews PRs, and ships features. Fully on GitHub Actions. Treasury is an on-chain smart wallet
that only the owner workflow can control — no human holds the keys.

---

## User Experience

### Getting started

```bash
cd my-project
npx terrarium --install
```

The installer:

1. **Wallet setup** — deploys an OIDC-gated ERC-4337 smart wallet on Base, deterministic from repo identity (CREATE2)
2. **Token setup** — deploys a Zora bonding curve donation token, treasury as payoutRecipient
3. **Secrets** — sets `OPENROUTER_API_KEY` as GHA secret, model tiers + wallet/token addresses as GHA variables
4. **Security hardening** — enables branch protection on `main`, restricts direct pushes, requires PR reviews, sets CODEOWNERS for `.github/workflows/`
5. **Funding walkthrough** — guides user through buying the first tokens to seed the treasury; shows wallet address + Zora link for funding
6. **First milestone** — prompts user to describe their first milestone; creates it as a GitHub Milestone via `gh api`

Then the installer prints:

```
✓ Wallet deployed: 0xABC...
✓ Token deployed: 0xDEF...

Donation link:    https://zora.co/coin/base/0xDEF...
Treasury badge:   ![Treasury](https://img.shields.io/badge/...)
Donate badge:     [![Donate](https://img.shields.io/badge/...)](https://zora.co/...)

Add these to your README, or the owner will add them automatically on first run.

Your first milestone: "v0.1 — Initial scaffolding"
  The owner will read this milestone and file issues for it on the next cycle.

Push your repo and the owner will wake up within 30 minutes.
```

### After install

The user:
- Creates/edits **GitHub Milestones** to steer the project (via GitHub UI or `gh` CLI)
- Shares the donation link to fund the project
- Watches the repo — the owner files issues, employees open PRs, the owner merges them
- The README shows live treasury balance, model tiers, and a donate button

That's it. No PLAN.md to maintain. No config files to edit. Milestones are the interface.

---

## How It Works

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

---

## Architecture

### Identity & Treasury

The owner's wallet is an **ERC-4337 account-abstraction smart wallet** on Base,
where transaction authorization is a **GitHub Actions OIDC JWT** — not a private key.

**GitHub OIDC**: every GHA run can request a short-lived JWT signed by GitHub's
RSA key. The JWT contains claims: `repository`, `workflow`, `ref`, `sha`,
`repository_visibility`, etc. Anyone can verify it against GitHub's public JWKS.
No one can forge it outside of a real GHA run.

**The wallet's `validateUserOp`**:
1. Decodes the JWT from the UserOperation signature field
2. Verifies RSA signature against GitHub's cached public key (OpenZeppelin RSA lib + EIP-198 modexp precompile)
3. Checks claims:
   - `iss == "https://token.actions.githubusercontent.com"`
   - `repository == "<owner>/<repo>"`
   - `workflow == "owner.yml"`
   - `ref == "refs/heads/main"`
   - `repository_visibility == "public"` (rejects if repo goes private)
   - `exp > block.timestamp`
4. Checks destination constraints (only allowed contracts: OpenRouter top-up, Uniswap, USDC/WETH)
5. Checks daily spend cap (immutable in contract)

**No private key exists.** The wallet is controlled by a cryptographic proof
that specific code is running in a specific public repo on GitHub's infrastructure.

### Token & Funding

Each terrarium project gets a **Zora bonding curve token** on Base.

- Created via Zora factory (`0x777777751622c0d3258f214F9DF38E35BF45baF3`)
- The OIDC wallet is set as `payoutRecipient` and in the `owners` array
- Bonding curve: mint on buy, burn on sell, ETH reserve
- 1% trading fee: 50% to project treasury, 20% to Zora, 30% to referrers
- Graduates to Uniswap V3 at ~$69k market cap, LP locked forever

**Tokens are donations.** Buying the token funds the project. The token is proof
of support — an on-chain receipt. Token holders receive no revenue, no governance
rights, no dividends. Trading fees flow to the project treasury, not to holders.

### Inference & Budget

**OpenRouter** is the inference provider. Model assignments live in GitHub repo
variables (`TERRARIUM_MODEL_OWNER`, `_HIGH`, `_MEDIUM`, `_LOW`), not in code.

**Budget flow:**
1. Treasury holds ETH from bonding curve fees
2. Owner swaps ETH → USDC on Base
3. Owner calls OpenRouter crypto payments API — returns calldata
4. Owner signs + broadcasts the top-up tx via the OIDC wallet
5. OpenRouter credits land, employees use them for inference

**Employee isolation:** Employees only have the `OPENROUTER_API_KEY` (GHA
environment secret). They cannot top up, cannot access the wallet, cannot
access the token. Worst case if the key leaks: someone burns remaining credits.

### Milestones as the Interface

The owner reads **GitHub Milestones** to understand what the project should build.
Each milestone has a title and description. The owner:
- Breaks milestones into concrete issues (with complexity/priority labels)
- Assigns issues to milestones
- Tracks milestone completion (% of issues closed)
- Moves to the next milestone when the current one is complete

Users steer the project by creating, editing, and reordering milestones in the GitHub UI.
No files to maintain. No commits needed to change direction.

### Owner Memory

**JOURNAL.md** — appended by the owner at the end of each wake cycle. Contains:
budget snapshot, model assignments, decisions made, project health assessment.
On the next wake, the owner reads the last 5 journal entries to regain context.

### README Status

The owner updates the target repo's README on each cycle with a live status block:
shields.io badges for treasury balance, runway, burn rate, and OpenRouter credits.
Model tier badges. Issue/PR counts. A donate button linking to the Zora bonding curve.

---

## Owner Cycle: Single Inference Call

The owner makes **one inference call per cycle**. Everything the owner needs to
decide is pre-computed and materialized into the prompt before inference runs.

### Pre-materialized context

```
You are the autonomous owner of {{repoOwner}}/{{repoName}}.

## Budget
Treasury: {{balanceEth}} ETH (${{balanceUsd}})
OpenRouter credits: ${{openRouterCredits}}
Projected runway: {{projectedDays}} days

## Recent spend (last 24h)
- Owner wake cycles: {{ownerInvocations}} invocations, ${{ownerSpend}}
- Issue #{{n}} ({{model}}): {{runs}} runs, ${{cost}}
- ...
Total 24h: ${{totalSpend24h}}

## Spend (last 7d)
- Owner: {{ownerRuns7d}} invocations, ${{ownerSpend7d}}
- Employees: {{employeeRuns7d}} runs, ${{employeeSpend7d}}
Total: ${{totalSpend7d}} (${{dailyAvg7d}}/day avg)

## Models
- owner: {{ownerModel}} (${{ownerCost}}/MTok)
- high: {{highModel}} (${{highCost}}/MTok)
- medium: {{mediumModel}} (${{mediumCost}}/MTok)
- low: {{lowModel}} (${{lowCost}}/MTok)
- Auto-review: {{autoReview}}

## Milestones
{{#each milestones}}
### {{title}} ({{openIssues}}/{{totalIssues}} remaining)
{{description}}
  Issues:
  {{#each issues}}
  - #{{number}}: {{title}} [{{complexity}}] {{#if stuck}}⚠ STUCK{{/if}} {{#if inProgress}}🔄{{/if}}
  {{/each}}
{{/each}}

## Unfiled milestone work
{{#each milestonesWithoutFullCoverage}}
### {{title}}
{{description}}
(No issues filed yet — consider breaking this down.)
{{/each}}

## Open PRs ({{count}})
{{#each prs}}
- #{{number}}: {{title}} (by {{model}}, {{diffStats}})
{{/each}}

## Journal (last 3 entries)
{{journalContext}}

---

Decide what to do this cycle. Respond with a JSON array of actions.
```

### Action vocabulary

```json
[
  { "action": "file_issue", "title": "...", "body": "...", "complexity": "medium", "priority": "high", "milestone": "v0.1" },
  { "action": "dispatch_employee", "issue_number": 7, "complexity": "medium" },
  { "action": "set_model", "tier": "medium", "model": "openrouter/moonshotai/kimi-k2.5" },
  { "action": "set_auto_review", "enabled": true },
  { "action": "merge_pr", "pr_number": 12 },
  { "action": "request_changes", "pr_number": 13, "feedback": "..." },
  { "action": "comment_issue", "issue_number": 5, "body": "..." },
  { "action": "top_up", "amount_usd": 10 },
  { "action": "close_milestone", "milestone": "v0.1" },
  { "action": "stakeholder_update", "body": "..." },
  { "action": "journal", "body": "..." }
]
```

---

## Spend Tracking

Spend data is tracked as **GHA run artifacts**. Each run uploads a JSON artifact:

### Employee run artifact

```json
{
  "run_id": 12345,
  "issue": 12,
  "model": "openrouter/moonshotai/kimi-k2.5",
  "input_tokens": 48000,
  "output_tokens": 12000,
  "cost_usd": 0.90,
  "result": { "type": "pr", "number": 18, "commit_sha": "abc123" }
}
```

### Result types

```json
{ "type": "pr", "number": 18, "commit_sha": "abc123" }
{ "type": "pr_updated", "number": 18, "commit_sha": "def456" }
{ "type": "stuck", "issue": 14, "reason": "needs API credentials not available" }
{ "type": "error", "error": "timed out" }
```

### Owner run artifact

```json
{
  "run_id": 12348,
  "role": "owner",
  "model": "openrouter/moonshotai/kimi-k2.5",
  "input_tokens": 12000,
  "output_tokens": 3000,
  "cost_usd": 0.15,
  "result": { "type": "owner_cycle", "actions": ["filed #19", "merged #18", "set medium=qwen3.5"] }
}
```

Pre-materialization downloads recent artifacts via `gh api` to build the spend breakdown.

---

## Repo Structure

```
terrarium/
  Cargo.toml                    # workspace root
  contracts/                    # Solidity (Foundry)
    src/
      TerrariumWallet.sol       # ERC-4337 account with JWT validation
      JwtValidator.sol          # RSA signature verification + claim parsing
      JwksRegistry.sol          # GitHub OIDC public key cache
      SpendPolicy.sol           # daily cap + destination allowlist (immutable)
    test/
    script/
      DeployWallet.s.sol        # CREATE2 deploy
  crates/
    core/                       # terrarium-core lib crate
      src/
        lib.rs
        config.rs               # terrarium.json R/W
        budget.rs               # wallet balance, OpenRouter credits, top-up calldata
        spend.rs                # download GHA run artifacts, aggregate spend
        context.rs              # pre-materialize owner prompt
        models.rs               # model catalog, tier logic, cost estimation
        inference.rs            # OpenRouter API client
        tasks.rs                # GitHub issues via gh CLI
        pr.rs                   # PR ops via gh CLI
        milestones.rs           # GitHub milestones via gh API
        journal.rs              # JOURNAL.md R/W
        status.rs               # README status block renderer
        wallet.rs               # ERC-4337 UserOp construction, OIDC JWT attachment
        token.rs                # Zora factory interaction
        actions.rs              # parse + execute owner action list
    owner/                      # terrarium-owner bin crate
      src/main.rs               # pre-materialize → infer → execute actions → artifact → README → journal
    employee/                   # terrarium-employee bin crate
      src/main.rs               # claim → infer → PR/stuck/error → artifact
    cli/                        # terrarium CLI (installer)
      src/main.rs               # npx terrarium --install (or cargo install)
  actions/
    owner/action.yml            # composite GHA: downloads + runs terrarium-owner
    employee/action.yml         # composite GHA: downloads + runs terrarium-employee
  templates/
    .github/workflows/
      terrarium.yml.hbs         # the one workflow file users add
```

---

## Config: `terrarium.json`

Minimal. Checked into the target repo by the installer.

```json
{
  "wallet": "0xABC...",
  "token": "0xDEF..."
}
```

Everything else lives in GitHub:
- Model assignments: repo variables (`TERRARIUM_MODEL_OWNER`, `_HIGH`, `_MEDIUM`, `_LOW`)
- Auto-review toggle: `TERRARIUM_AUTO_REVIEW` repo variable
- Secrets: `OPENROUTER_API_KEY` (GHA environment secret)
- Project direction: GitHub Milestones
- Budget thresholds: repo variables or hardcoded in contract (spend policy)

---

## Smart Wallet Contract

### `TerrariumWallet.sol`

ERC-4337 `BaseAccount`. No ECDSA signer. Authorization is a GitHub OIDC JWT.

```
validateUserOp(userOp, userOpHash, missingAccountFunds):
  1. abi.decode(userOp.signature) → (header, payload, rsaSig)
  2. RSA.pkcs1Sha256(sha256(header.payload), rsaSig, e, n)
  3. parse claims from payload
  4. require iss == GITHUB_OIDC_ISSUER
  5. require keccak256(repository) == REPO_HASH
  6. require keccak256(workflow) == WORKFLOW_HASH
  7. require keccak256(ref) == REF_HASH
  8. require keccak256(repository_visibility) == keccak256("public")
  9. require exp > block.timestamp
  10. SpendPolicy.check(userOp.callData)
  11. return _packValidationData(false, exp, iat)
```

### `JwksRegistry.sol`

Stores GitHub's RSA public keys. Permissionless update: anyone can submit a new
key by providing a JWT signed with the current key. Self-bootstrapping rotation.

### `SpendPolicy.sol`

Immutable constraints set at deploy:
- `MAX_DAILY_SPEND`: e.g., $200 worth of ETH/USDC
- `ALLOWED_DESTINATIONS`: OpenRouter Coinbase Commerce address, Uniswap router, WETH/USDC
- Constructor args, frozen forever. No admin.

---

## GitHub Actions

### What the user adds (one file)

```yaml
# .github/workflows/terrarium.yml
name: Terrarium
on:
  schedule:
    - cron: '*/30 * * * *'
  workflow_dispatch: {}
jobs:
  owner:
    uses: jbrower95/terrarium/.github/workflows/owner.yml@main
    permissions:
      id-token: write
      contents: write
      issues: write
      pull-requests: write
      actions: write
    secrets: inherit
```

### What the owner bootstraps on first run

- `employee.yml` workflow (committed to the repo)
- `review.yml` workflow (committed to the repo)
- Initial JOURNAL.md
- README status block
- Files initial issues from the first milestone

### Owner workflow

1. Request OIDC token
2. Pre-materialize context (budget, spend, milestones, issues, PRs, journal)
3. Top up OpenRouter if credits below threshold (OIDC wallet tx)
4. One inference call → action list
5. Execute actions (file issues, dispatch employees, merge PRs, set model vars, etc.)
6. Upload run artifact
7. Update README status
8. Commit JOURNAL.md + README.md

### Employee workflow

1. Read model from `TERRARIUM_MODEL_<COMPLEXITY>` env var
2. Claim issue (label + comment)
3. Run inference → implement
4. On success: push branch, create PR (body includes model + commit sha)
5. On stuck: comment on issue, add `stuck` label
6. On error: unclaim issue
7. Upload run artifact

---

## Security

### Installer hardening

`npx terrarium --install` configures:
- Branch protection on `main` (require PR, require review, no force push)
- CODEOWNERS: `.github/workflows/` requires repo admin review
- Restrict who can push to `main`
- Restrict who can dismiss PR reviews

### Runtime security

```
Layer                  What's protected                    How
───────────────────────────────────────────────────────────────────────────────
Smart contract         Funds can't go to wrong address     Immutable destination allowlist + daily cap
OIDC binding           Only owner.yml can sign txs         JWT claim verification (repo, workflow, ref, visibility)
Repo visibility        Code must be auditable              Contract rejects txs if repo is private
Branch protection      Workflow can't be silently changed  Requires PR + review for .github/workflows/
OpenRouter key         Employees can't overspend           Credits capped by owner's top-up rate
Employee isolation     No financial access                 Only has OpenRouter key, can't top up or access wallet
```

---

## `npx terrarium --install`

```
$ npx terrarium --install

  🌱 terrarium installer

  Detected repo: jbrower95/my-project (public)

  Step 1: Wallet
  Deploying OIDC-gated wallet on Base...
  ✓ Wallet: 0xABC...def

  Step 2: Token
  Deploying donation token on Zora...
  ✓ Token: 0xDEF...789
  ✓ Bonding curve active

  Step 3: Secrets
  OpenRouter API key: ****
  ✓ OPENROUTER_API_KEY set
  ✓ Model variables set (kimi-k2.5 owner/high, qwen3.5 medium/low)
  ✓ Wallet + token addresses set

  Step 4: Security
  ✓ Branch protection enabled on main
  ✓ CODEOWNERS: .github/workflows/ requires admin review
  ✓ Direct pushes to main restricted

  Step 5: Fund your project
  To seed the treasury, buy your project's token:
  → https://zora.co/coin/base/0xDEF...789

  Or send ETH directly to the wallet:
  → 0xABC...def (Base network)

  Step 6: First milestone
  What's the first thing your project should build?
  > Set up a REST API with user authentication

  ✓ Created milestone: "v0.1 — REST API with user auth"

  ──────────────────────────────────────────────────

  Done! Add this to your README:

  [![Donate](https://img.shields.io/badge/donate-fund_this_project-ff69b4?style=for-the-badge)](https://zora.co/coin/base/0xDEF...789)
  ![Treasury](https://img.shields.io/badge/treasury-$0.00-lightgrey)

  The owner will update these badges automatically on each cycle.

  Push your repo. The owner will wake up on the next cron cycle,
  read your milestone, file issues, and start building.
```

---

## Task Lifecycle

```
SUPPORTER                          OWNER (cron)                         EMPLOYEE (dispatch)
────────────────────────────────────────────────────────────────────────────────────────────

buys $PROJECT token on Base
  → ETH enters bonding curve
  → 50% trading fee → treasury

                                   [owner wakes]
                                   pre-materialize:
                                     wallet balance, credits, spend,
                                     milestones, issues, PRs, journal
                                   top up OpenRouter if needed
                                   ↓
                                   ONE inference call → action list:
                                     file issue #7 into milestone "v0.1"
                                     dispatch employee for #7
                                     merge PR #5
                                     journal: "shipped #5, filed #7"
                                   ↓
                                   execute actions
                                   upload run artifact
                                   update README status

                                                                        [dispatched by owner]
                                                                        claim issue #7
                                                                        run inference → implement
                                                                        push branch, create PR
                                                                        upload artifact:
                                                                          { cost: $0.90, result: { type: "pr", number: 18 } }

                                   [owner wakes]
                                   sees PR #18 in context
                                   → merge_pr #18
                                   → close_milestone "v0.1" (all issues done)
                                   → stakeholder_update "shipped v0.1"

USER creates new milestone "v0.2 — Payment integration"
  → owner reads it next cycle
  → files issues, dispatches employees
  → cycle continues
```

---

## Workstreams

### W1 — Smart Wallet Contract (Solidity/Foundry)
- `TerrariumWallet.sol`, `JwtValidator.sol`, `JwksRegistry.sol`, `SpendPolicy.sol`
- Deploy script, forge tests with mock JWTs
- **Blocked by:** nothing
- **Blocks:** W5 (install)

### W2 — Core Crate (Rust)
- `budget.rs`, `spend.rs`, `context.rs`, `models.rs`, `inference.rs`
- `tasks.rs`, `pr.rs`, `milestones.rs`, `journal.rs`, `status.rs`
- `wallet.rs`, `token.rs`, `actions.rs`
- **Blocked by:** nothing (can mock wallet/token until W1 lands)
- **Blocks:** W3, W4

### W3 — Owner Binary (Rust)
- Pre-materialize → infer → execute action list → artifact → README → journal
- Single inference call pattern
- **Blocked by:** W2 (core)

### W4 — Employee Binary (Rust)
- Claim → infer → PR/stuck/error → artifact
- **Blocked by:** W2 (core)

### W5 — Installer CLI
- Wallet deploy, token deploy, secrets, security hardening, funding walkthrough, first milestone
- Can be Rust (same workspace) or keep as npx (wider reach for onboarding)
- **Blocked by:** W1 (wallet), W2 (token interaction)

### W6 — GitHub Actions
- `actions/owner/action.yml`, `actions/employee/action.yml`
- Binary distribution (GitHub Releases, downloaded by composite action)
- **Blocked by:** W3, W4

---

## Principles

- **No human holds the keys.** The wallet is an OIDC-gated smart contract. There is no private key.
- **Code is the authority.** The owner's behavior is open-source, auditable, pinned to a specific commit.
- **Repo must be public.** The contract rejects transactions if `repository_visibility != "public"`.
- **Immutable financial constraints.** Daily spend caps and allowed destinations are frozen at deploy.
- **Employees are credit-bounded.** They use OpenRouter credits and have no financial access.
- **Tokens are donations.** No revenue, governance, or dividends to holders. Trading fees fund the project.
- **The owner is autonomous.** It decides model tiers, budget allocation, task priority, and when to top up.
- **Everything is traceable.** Every on-chain tx links to a GHA run_id and commit sha.
- **One inference call per owner cycle.** All context is pre-materialized. The owner decides, the system executes.
- **Milestones are the interface.** Users steer the project via GitHub Milestones. No config files to maintain.
- **Spend lives in GHA artifacts.** No in-repo logging. History survives rebases and force pushes.
