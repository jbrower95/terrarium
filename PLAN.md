# terrarium — PLAN

A self-running open source project framework.

Donate to fund a project → an autonomous AI owner manages inference budget, files issues,
reviews PRs, and ships features. Fully on GitHub Actions. Treasury is an on-chain smart wallet
that only the owner workflow can control — no human holds the keys.

---

## How It Works

```
Supporter donates (buys $PROJECT token on Base via Zora bonding curve)
  → ETH enters bonding curve reserve
  → Trading fees (50%) flow to treasury wallet
  → Treasury is an ERC-4337 smart wallet, OIDC-gated to owner.yml
  → Owner wakes on cron, checks balance
  → Owner sells ETH for USDC, tops up OpenRouter credits via crypto payments API
  → Owner reads PLAN.md, files issues, assigns model tiers based on budget
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
4. Checks destination constraints (only allowed contracts: OpenRouter top-up, bonding curve, USDC/WETH)
5. Checks daily spend cap (immutable in contract)

**No private key exists.** The wallet is controlled by a cryptographic proof
that specific code is running in a specific public repo on GitHub's infrastructure.

**What this proves:**
- The repo maintainer cannot move funds (there is no key to hold)
- Funds can only move when the open-source owner.yml workflow runs
- The repo must be public at the time of every transaction
- Every tx links to a specific GHA run_id and commit sha
- The contract's spend caps and destination locks are immutable

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
The bonding curve is a permissionless donation interface with a refund option.

### Inference & Budget

**OpenRouter** is the inference provider. Model assignments live in GitHub repo
variables (`TERRARIUM_MODEL_OWNER`, `_HIGH`, `_MEDIUM`, `_LOW`), not in code.

**Budget flow:**
1. Treasury holds ETH from bonding curve fees
2. Owner swaps ETH → USDC on Base (via Uniswap or similar)
3. Owner calls OpenRouter crypto payments API (`POST /api/v1/credits/coinbase`)
   with `{ amount, sender: wallet, chain_id: "8453" }` — returns calldata
4. Owner signs + broadcasts the top-up tx via the OIDC wallet
5. OpenRouter credits land, employees use them for inference

**Model catalog:** Owner has access to a catalog of models with coding benchmarks
and token costs. Each wake cycle, the owner reasons about model strategy —
adjusting tiers based on budget, run rate, and projected runway.

**Employee isolation:** Employees only have the `OPENROUTER_API_KEY` (GHA
environment secret). They cannot top up, cannot access the wallet, cannot
access the token. Worst case if the key leaks: someone burns remaining credits.

### Owner Memory

**JOURNAL.md** — appended by the owner at the end of each wake cycle. Contains:
budget snapshot, model assignments, decisions made, project health assessment.
On the next wake, the owner reads the last 5 journal entries to regain context.
This is the owner's persistent memory across runs.

### README Status

The owner updates the target repo's README on each cycle with a live status block:
shields.io badges for treasury balance, runway, burn rate, and OpenRouter credits.
Model tier badges. Issue/PR counts. A donate button linking to the Zora bonding curve.
Inserted between `<!-- terrarium-status-start -->` / `<!-- terrarium-status-end -->` markers.

---

## Owner Cycle: Single Inference Call

The owner makes **one inference call per cycle**. Everything the owner needs to
decide is pre-computed and materialized into the prompt before inference runs.
The owner's job is to make decisions, not gather information.

### Pre-materialized context (computed before inference)

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
- PR reviews: {{reviewRuns7d}} invocations, ${{reviewSpend7d}}
Total: ${{totalSpend7d}} (${{dailyAvg7d}}/day avg)

## Models
- owner: {{ownerModel}} (${{ownerCost}}/MTok)
- high: {{highModel}} (${{highCost}}/MTok)
- medium: {{mediumModel}} (${{mediumCost}}/MTok)
- low: {{lowModel}} (${{lowCost}}/MTok)
- Auto-review: {{autoReview}}

## Open issues ({{count}})
{{#each issues}}
- #{{number}}: {{title}} [{{complexity}}] [{{priority}}] {{#if stuck}}⚠ STUCK{{/if}}
{{/each}}

## Open PRs ({{count}})
{{#each prs}}
- #{{number}}: {{title}} (by {{model}}, {{diffStats}})
{{/each}}

## Journal (last 3 entries)
{{journalContext}}

## PLAN.md
{{planContent}}

---

Decide what to do this cycle. Respond with a JSON array of actions.
```

### Action vocabulary

The owner responds with a JSON array. Each action is executed mechanically:

```json
[
  { "action": "file_issue", "title": "...", "body": "...", "complexity": "medium", "priority": "high" },
  { "action": "dispatch_employee", "issue_number": 7, "complexity": "medium" },
  { "action": "set_model", "tier": "medium", "model": "openrouter/moonshotai/kimi-k2.5" },
  { "action": "set_auto_review", "enabled": true },
  { "action": "merge_pr", "pr_number": 12 },
  { "action": "request_changes", "pr_number": 13, "feedback": "..." },
  { "action": "comment_issue", "issue_number": 5, "body": "..." },
  { "action": "top_up", "amount_usd": 10 },
  { "action": "stakeholder_update", "body": "..." },
  { "action": "journal", "body": "Upgraded medium tier. Filed #7. Budget healthy." }
]
```

This replaces the previous design (5+ separate inference calls for mail, PR review,
plan reconciliation, model strategy, stakeholder updates, journal). One call.
Cheaper, faster, and the owner makes better decisions because it sees everything at once.

---

## Spend Tracking

Spend data is tracked as **GHA run artifacts**, not in-repo files. Each employee
and owner run uploads a JSON artifact at completion:

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

### Pre-materialization

The owner's pre-materialization step calls `gh run list` + downloads recent
artifacts to build the spend breakdown. No files committed to the repo for
logging — it's all in GitHub's infrastructure. Spend history survives branch
resets, rebases, and force pushes.

---

## Package Structure

```
terrarium/
  packages/
    contracts/               # Solidity: OIDC-gated ERC-4337 wallet
      src/
        TerrariumWallet.sol  # ERC-4337 account with JWT validation
        JwtValidator.sol     # RSA signature verification + claim parsing
        JwksRegistry.sol     # Cached GitHub OIDC public keys, permissionless update
        SpendPolicy.sol      # Daily caps, destination allowlist (immutable)
      test/
      script/
        DeployWallet.s.sol   # Deterministic CREATE2 deploy
    core/                    # TypeScript: shared logic for GHA steps
      src/
        config.ts            # terrarium.json R/W (wallet, token, cron, budget thresholds)
        budget.ts            # wallet balance, OpenRouter credits, top-up calldata
        models.ts            # model catalog, tier upgrade/downgrade, cost estimation
        inference.ts         # thin wrapper: OpenRouter API, key from env
        tasks.ts             # GitHub issues via gh CLI: list, claim, next-task, file
        pr.ts                # PR create, review (inference), merge via gh CLI
        spend.ts             # download GHA run artifacts, aggregate spend by issue/role/model
        journal.ts           # JOURNAL.md append/read for owner memory
        status.ts            # render README status block with shields.io badges
        wallet.ts            # construct ERC-4337 UserOps, attach OIDC JWT as signature
        token.ts             # Zora factory interaction: deploy token, read bonding curve state
        context.ts           # pre-materialize full owner prompt from all data sources
    cli/                     # npx terrarium --install
      src/
        index.ts             # entry point — install wizard
        prompts.ts           # onboarding questions
        secrets.ts           # gh secret set + gh variable set
        workflows.ts         # render GHA workflow YAMLs
        config-writer.ts     # write terrarium.json
        plan-writer.ts       # write stub PLAN.md
        deploy-wallet.ts     # deploy OIDC wallet via CREATE2
        deploy-token.ts      # deploy Zora bonding curve token
        owner.ts             # terrarium-owner: pre-materialize → infer → execute actions
        employee.ts          # terrarium-employee: claim → infer → PR/stuck/error → upload artifact
        review.ts            # terrarium-review: infer → approve/request changes
  templates/
    .github/workflows/
      owner.yml.hbs
      employee.yml.hbs
      pr-review.yml.hbs
    terrarium.json.hbs
    PLAN.md.hbs
```

---

## Config: `terrarium.json`

Checked into the target repo. Contains no secrets, no model assignments.

```json
{
  "wallet": "0xABC...",
  "token": "0xDEF...",
  "owner_cron": "*/30 * * * *",
  "max_concurrent_employees": 3,
  "budget": {
    "downgrade_threshold_usd": 5.00,
    "pause_threshold_usd": 1.00
  }
}
```

Model assignments: GitHub repo variables (`TERRARIUM_MODEL_OWNER`, `_HIGH`, `_MEDIUM`, `_LOW`).
Auto-review toggle: `TERRARIUM_AUTO_REVIEW` repo variable.
Secrets: `OPENROUTER_API_KEY` (GHA environment secret, employee + owner).

---

## Smart Wallet Contract

### `TerrariumWallet.sol`

ERC-4337 `BaseAccount` implementation. No ECDSA signer. Authorization is a GitHub OIDC JWT.

```
validateUserOp(userOp, userOpHash, missingAccountFunds):
  1. abi.decode(userOp.signature) → (header, payload, rsaSig)
  2. RSA.pkcs1Sha256(sha256(header.payload), rsaSig, e, n)  // OZ RSA lib
  3. parse claims from payload
  4. require iss == GITHUB_OIDC_ISSUER
  5. require keccak256(repository) == REPO_HASH
  6. require keccak256(workflow) == WORKFLOW_HASH
  7. require keccak256(ref) == REF_HASH
  8. require keccak256(repository_visibility) == keccak256("public")
  9. require exp > block.timestamp
  10. SpendPolicy.check(userOp.callData)  // destination + daily cap
  11. return _packValidationData(false, exp, iat)
```

### `JwksRegistry.sol`

Stores GitHub's RSA public keys (modulus + exponent). Permissionless update:
anyone can submit a new key by providing a JWT signed with the old key that
attests to the new key. Self-bootstrapping rotation — no admin, no oracle.

### `SpendPolicy.sol`

Immutable constraints set at deploy:
- `MAX_DAILY_SPEND`: e.g., $200 worth of ETH/USDC
- `ALLOWED_DESTINATIONS`: OpenRouter Coinbase Commerce address, Uniswap router, WETH/USDC contracts
- No admin function to change these — they are constructor args, frozen forever

---

## GitHub Actions Workflows

### `owner.yml` — Scheduled cron

```yaml
permissions:
  id-token: write   # OIDC token request
  contents: write
  issues: write
  pull-requests: write
  actions: write    # update repo variables + read run artifacts
```

Steps:
1. Checkout repo (full depth)
2. Request OIDC token
3. **Pre-materialize** — gather all context (wallet balance, OpenRouter credits, spend from GHA artifacts, open issues, open PRs, journal, PLAN.md) into a single prompt
4. **Top-up if needed** — if OpenRouter credits below threshold, construct + sign + submit top-up tx via OIDC wallet before inference
5. **Infer** — one call to the owner model with the pre-materialized prompt
6. **Execute** — iterate the returned action list: file issues, dispatch employees, merge PRs, set model vars, etc.
7. **Upload artifact** — run metadata (tokens, cost, actions taken)
8. **Update README** — refresh status badges
9. **Write JOURNAL.md** — the journal action from the owner's response
10. Commit + push (JOURNAL.md, README.md)

### `employee.yml` — Manual dispatch

```yaml
permissions:
  contents: write
  issues: write
  pull-requests: write
```

Inputs: `complexity` (choice), `issue_number` (optional)

Steps:
1. Checkout
2. Read model from `TERRARIUM_MODEL_<COMPLEXITY>` env var (injected from `vars.*`)
3. Auto-select issue if none provided (highest priority at or below complexity)
4. Claim issue (add `in-progress` label + comment)
5. Run inference with issue context, using assigned model
6. On success: push branch, create PR (body includes model used + commit sha)
7. On stuck: comment on issue with explanation, add `stuck` label
8. On error: unclaim issue
9. Upload run artifact (tokens, cost, result type)

Concurrency: `terrarium-issue-${{ inputs.issue_number || 'auto' }}`

### `pr-review.yml` — PR trigger

Only fires for PRs by `github-actions[bot]`. Runs inference review with medium model.
Approves or requests changes. Uploads run artifact.

---

## Security Model

```
Layer                  What's protected                    How
───────────────────────────────────────────────────────────────────────────────
Smart contract         Funds can't go to wrong address     Immutable destination allowlist + daily cap
OIDC binding           Only owner.yml can sign txs         JWT claim verification (repo, workflow, ref, visibility)
Repo visibility        Code must be auditable              Contract rejects txs if repo is private
OpenRouter key         Employees can't overspend           Credits capped by owner's top-up rate
Employee isolation     No financial access                 Only has OpenRouter key, can't top up or access wallet
```

Worst case at each compromise level:

| Compromised | Blast radius |
|---|---|
| Employee GHA run | Burns remaining OpenRouter credits (≤ last top-up amount) |
| OpenRouter key leaks | Same — owner rotates key next cycle |
| Owner GHA fully compromised | Contract limits: ≤ daily cap, only to allowed destinations |
| Repo goes private | Wallet freezes — contract rejects all txs |
| Repo admin changes owner.yml | Public git diff visible; OIDC sha claim pins the commit |

---

## `npx terrarium --install`

Interactive wizard, run in any existing GitHub repo.

1. Detect repo from `git remote`
2. Prompt: owner cron, max employees, budget thresholds
3. Prompt: initial model tiers (defaults: kimi-k2.5 owner/high, qwen3.5 medium/low)
4. Prompt: OpenRouter API key
5. **Deploy OIDC wallet** — deterministic CREATE2 deploy on Base, parameterized by repo identity
6. **Deploy Zora token** — call factory with wallet as `payoutRecipient` + `owners`
7. Write `terrarium.json` (wallet address, token address, cron, budget)
8. Write `.github/workflows/` (owner, employee, pr-review)
9. Write stub `PLAN.md` if missing
10. Set GHA secrets: `OPENROUTER_API_KEY`
11. Set GHA variables: model tiers, auto-review, wallet address
12. Commit + push
13. Print: token address, donation link, wallet address

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
                                     issues, PRs, journal, PLAN.md
                                   top up OpenRouter if needed (OIDC wallet)
                                   ↓
                                   ONE inference call → action list:
                                     file issue #7 [medium, high]
                                     dispatch employee for #7
                                     merge PR #5
                                     set medium=kimi-k2.5
                                     journal: "shipped #5, filed #7"
                                   ↓
                                   execute actions mechanically
                                   upload run artifact
                                   update README status
                                   commit JOURNAL.md

                                                                        [dispatched by owner]
                                                                        model = vars.TERRARIUM_MODEL_MEDIUM
                                                                        claim issue #7
                                                                        run inference → implement
                                                                        push branch, create PR
                                                                          PR body: "Model: kimi-k2.5"
                                                                        upload run artifact:
                                                                          { cost: $0.90, result: { type: "pr", number: 18 } }

                                   [owner wakes]
                                   sees PR #18 in pre-materialized context
                                   → action: merge_pr #18
                                   → action: stakeholder_update "shipped #7"
                                   → action: journal "merged #18"

buys more $PROJECT
  → more ETH → treasury
  → owner has more budget
  → upgrades to better models
  → faster, higher quality output
```

---

## Implementation Phases

### Phase 1 — Smart Wallet Contract (`packages/contracts`)
1. `TerrariumWallet.sol` — ERC-4337 BaseAccount with OIDC JWT validation
2. `JwtValidator.sol` — RSA signature verification, JWT claim parsing
3. `JwksRegistry.sol` — GitHub public key cache with permissionless rotation
4. `SpendPolicy.sol` — daily cap + destination allowlist
5. Deploy script — deterministic CREATE2 parameterized by repo identity
6. Tests — forge tests with mock JWTs

### Phase 2 — Wallet Client + Spend Tracking (`packages/core`)
7. `wallet.ts` — construct UserOperations, attach OIDC JWT, submit to bundler
8. `token.ts` — Zora factory: deploy token, read price/supply
9. `budget.ts` — wallet balance on Base, OpenRouter credits + top-up calldata
10. `spend.ts` — download GHA run artifacts via `gh api`, aggregate by issue/role/model
11. `context.ts` — pre-materialize full owner prompt from all data sources

### Phase 3 — Install Flow (`packages/cli`)
12. `deploy-wallet.ts` — CREATE2 deploy from installer
13. `deploy-token.ts` — Zora factory from installer
14. Update `prompts.ts`, `secrets.ts`, `workflows.ts` for new architecture

### Phase 4 — Owner + Employee Rewrite
15. `owner.ts` — pre-materialize → single inference call → execute action list → upload artifact → update README → write journal
16. `employee.ts` — claim → infer → result (PR/stuck/error) → upload artifact
17. `review.ts` — infer → approve/request changes → upload artifact

### Phase 5 — Polish
18. `terrarium status` CLI — show wallet balance, token price, open tasks, model tiers, recent spend
19. Gas estimation for OIDC verification
20. Fallback: ZK-JWT alternative if gas is too high

---

## Principles

- **No human holds the keys.** The wallet is an OIDC-gated smart contract. There is no private key.
- **Code is the authority.** The owner's behavior is determined by open-source, auditable workflow code pinned to a specific commit.
- **Repo must be public.** The contract rejects transactions if `repository_visibility != "public"`.
- **Immutable financial constraints.** Daily spend caps and allowed destinations are set at deploy and cannot be changed.
- **Employees are credit-bounded.** They use OpenRouter credits and have no financial access.
- **Tokens are donations.** Buying the project's token funds development. Holders receive no revenue, governance, or dividends. Trading fees flow to the treasury.
- **The owner is autonomous.** It decides model tiers, budget allocation, task priority, and when to top up — within the contract's constraints.
- **Everything is traceable.** Every on-chain tx links to a GHA run_id and commit sha. Every code change is a public git commit.
- **One inference call per owner cycle.** All context is pre-materialized. The owner decides, the system executes.
- **Spend is tracked in GHA artifacts.** No in-repo logging files. Spend history lives in GitHub's infrastructure, aggregated on demand.
