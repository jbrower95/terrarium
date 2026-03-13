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
        config.ts            # terrarium.json R/W (wallet, cron, budget thresholds)
        budget.ts            # OpenRouter balance check, crypto top-up calldata
        models.ts            # model catalog, tier upgrade/downgrade, cost estimation
        inference.ts         # thin wrapper: OpenRouter API, key from env
        tasks.ts             # GitHub issues via gh CLI: list, claim, next-task, file
        pr.ts                # PR create, review (inference), merge via gh CLI
        mail.ts              # MAIL.md append/read/respond
        activity.ts          # activity.jsonl append/read
        journal.ts           # JOURNAL.md append/read for owner memory
        wallet.ts            # construct ERC-4337 UserOps, attach OIDC JWT as signature
        token.ts             # Zora factory interaction: deploy token, read bonding curve state
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
        owner.ts             # terrarium-owner entry point (GHA cron)
        employee.ts          # terrarium-employee entry point (GHA dispatch)
        review.ts            # terrarium-review entry point (GHA PR trigger)
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
  actions: write    # update repo variables
```

Steps:
1. Checkout repo (full depth)
2. Request OIDC token (`audience: https://terrarium.xyz` or contract address)
3. **Budget check** — read wallet ETH/USDC balance on Base; read OpenRouter credit balance
4. **Top-up if needed** — construct UserOp to swap ETH→USDC and call OpenRouter crypto payments API; sign with OIDC JWT; submit via bundler (Pimlico)
5. **Model strategy** — read model catalog, compare costs vs. budget, adjust tier variables via `gh variable set`; toggle auto-review based on run rate
6. **Journal read** — load last 5 JOURNAL.md entries for context
7. **Mail** — read MAIL.md, respond to unanswered entries via inference
8. **PR review** — if auto-review enabled, review open bot PRs; merge or request changes
9. **Plan → Issues** — reconcile PLAN.md against open issues, file missing ones
10. **Stakeholder update** — if >= 5 merges since last update, post summary issue
11. **Journal write** — append cycle entry to JOURNAL.md
12. Commit + push any file changes (JOURNAL.md, MAIL.md)

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
4. Claim issue (label + comment)
5. Run `claude --print` with issue context, using assigned model
6. Push branch, create PR (body includes model name)
7. On failure: unclaim, append MAIL.md

Concurrency: `terrarium-issue-${{ inputs.issue_number || 'auto' }}`

### `pr-review.yml` — PR trigger

Only fires for PRs by `github-actions[bot]`. Runs inference review with medium model.
Approves + merges or requests changes.

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
                                   OIDC token → sign UserOp
                                   check balance → $42 ETH
                                   swap ETH → USDC → top up OpenRouter
                                   read JOURNAL.md → regain context
                                   read PLAN.md → file issue #7
                                     labels: complexity:medium, priority:high
                                   set TERRARIUM_MODEL_MEDIUM=kimi-k2.5
                                     (budget allows upgrade)
                                   write JOURNAL.md → "upgraded medium tier, filed #7"

                                                                        [owner dispatches employee.yml]
                                                                        model = vars.TERRARIUM_MODEL_MEDIUM
                                                                        claim issue #7
                                                                        run claude → implement
                                                                        push branch, create PR
                                                                          PR body: "Model: kimi-k2.5"

                                   [owner wakes]
                                   review PR → approved → merge
                                   stakeholder update: "shipped #7"
                                   write JOURNAL.md

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

### Phase 2 — Wallet Client (`packages/core`)
7. `wallet.ts` — construct UserOperations, attach OIDC JWT, submit to bundler
8. `token.ts` — Zora factory: deploy token, read price/supply, sell tokens
9. `budget.ts` — rewrite: check wallet balance on Base, OpenRouter balance, construct top-up tx

### Phase 3 — Install Flow (`packages/cli`)
10. `deploy-wallet.ts` — call CREATE2 deploy from installer
11. `deploy-token.ts` — call Zora factory from installer
12. Update `prompts.ts`, `secrets.ts`, `workflows.ts` for new architecture

### Phase 4 — Core (already built, needs updates)
13. Update `owner.ts` — OIDC token request, UserOp signing, top-up flow
14. Update `employee.ts` — reads model from env var (already done)
15. Update `workflows.ts` — add `id-token: write` permission to owner

### Phase 5 — Polish
16. `terrarium status` CLI — show wallet balance, token price, open tasks, model tiers
17. Gas estimation for OIDC verification (optimize claim parsing)
18. Fallback: if OIDC verification gas is too high, ZK-JWT alternative path

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
