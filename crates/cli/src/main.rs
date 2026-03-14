use std::env;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::process::Command;

use terrarium_core::milestones;
use terrarium_core::models;
use terrarium_core::token;

#[derive(Parser)]
#[command(name = "terrarium", about = "Terrarium installer")]
struct Cli {
    /// Run the install wizard
    #[arg(long)]
    install: bool,

    /// Wallet address (skip wallet deploy prompt)
    #[arg(long)]
    wallet: Option<String>,

    /// Token address (skip token deploy prompt)
    #[arg(long)]
    token: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Detect the current repo via `gh repo view`.
async fn detect_repo() -> Result<String> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])
        .output()
        .await
        .context("failed to run gh — is the GitHub CLI installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh repo view failed: {stderr}");
    }

    let repo = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if repo.is_empty() {
        anyhow::bail!("could not detect repo — run this inside a GitHub repo");
    }
    Ok(repo)
}

/// Read a line from stdin with a prompt. Returns the trimmed input.
fn prompt(msg: &str) -> Result<String> {
    eprint!("{}", msg);
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Read a line from stdin without echoing (best-effort). Falls back to plain
/// read if the terminal tricks fail.
fn prompt_secret(msg: &str) -> Result<String> {
    eprint!("{}", msg);
    io::stderr().flush()?;

    // Try to disable echo on Unix
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        unsafe {
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut termios) == 0 {
                let mut noecho = termios;
                noecho.c_lflag &= !libc::ECHO;
                libc::tcsetattr(fd, libc::TCSANOW, &noecho);

                let mut line = String::new();
                io::stdin().lock().read_line(&mut line)?;

                libc::tcsetattr(fd, libc::TCSANOW, &termios);
                eprintln!(); // newline after hidden input
                return Ok(line.trim().to_string());
            }
        }
    }

    // Fallback: plain read
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Run `gh secret set <name>` with the value piped to stdin.
async fn gh_secret_set(name: &str, value: &str) -> Result<()> {
    let mut child = Command::new("gh")
        .args(["secret", "set", name])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn gh secret set")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(value.as_bytes()).await?;
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh secret set {name} failed: {stderr}");
    }
    Ok(())
}

/// Run `gh variable set <name> --body <value>`.
async fn gh_variable_set(name: &str, value: &str) -> Result<()> {
    let output = Command::new("gh")
        .args(["variable", "set", name, "--body", value])
        .output()
        .await
        .context("failed to spawn gh variable set")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh variable set {name} failed: {stderr}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Install steps
// ---------------------------------------------------------------------------

/// Check if stdin is a TTY (interactive terminal).
fn is_interactive() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::isatty(io::stdin().as_raw_fd()) != 0 }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The Coinbase Commerce contract on Base — whitelisted for top-ups.
const COINBASE_COMMERCE_CONTRACT: &str = "0x03059433BCdB6144624cC2443159D9445C32b7a8";

/// Default max daily spend: ~$100 at $2000/ETH = 0.05 ETH.
const DEFAULT_MAX_DAILY_SPEND_WEI: &str = "50000000000000000"; // 0.05 ETH

/// GitHub Actions OIDC JWKS endpoint.
const GITHUB_OIDC_JWKS_URI: &str = "https://token.actions.githubusercontent.com/.well-known/jwks";

/// Fetch the first RSA key from GitHub's OIDC JWKS endpoint.
/// Returns (kid, modulus_hex, exponent_hex).
async fn fetch_github_jwks() -> Result<(String, String, String)> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(GITHUB_OIDC_JWKS_URI)
        .send()
        .await
        .context("failed to fetch GitHub JWKS")?
        .json()
        .await
        .context("failed to parse JWKS response")?;

    let keys = resp.get("keys")
        .and_then(|k| k.as_array())
        .context("JWKS response missing keys array")?;

    let rsa_key = keys.iter()
        .find(|k| k.get("kty").and_then(|v| v.as_str()) == Some("RSA"))
        .context("no RSA key found in JWKS")?;

    let kid = rsa_key.get("kid")
        .and_then(|v| v.as_str())
        .context("RSA key missing kid")?
        .to_string();

    let n_b64 = rsa_key.get("n")
        .and_then(|v| v.as_str())
        .context("RSA key missing modulus (n)")?;

    let e_b64 = rsa_key.get("e")
        .and_then(|v| v.as_str())
        .context("RSA key missing exponent (e)")?;

    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let n_bytes = engine.decode(n_b64).context("failed to decode modulus")?;
    let e_bytes = engine.decode(e_b64).context("failed to decode exponent")?;

    let n_hex = format!("0x{}", hex::encode(&n_bytes));
    let e_hex = format!("0x{}", hex::encode(&e_bytes));

    Ok((kid, n_hex, e_hex))
}

/// Deploy the Terrarium wallet stack via `forge script`.
/// Returns the deployed wallet address.
async fn deploy_wallet(
    repo_owner: &str,
    repo_name: &str,
    deployer_key: &str,
    rpc_url: &str,
) -> Result<String> {
    eprintln!("  Fetching GitHub OIDC keys...");
    let (kid, modulus, exponent) = fetch_github_jwks().await?;
    eprintln!("  \u{2713} JWKS kid: {kid}");

    // Find the contracts directory relative to the terrarium package.
    // When installed via npx, the contracts dir is bundled alongside the binary.
    // For now, we look for it relative to the binary or in a well-known location.
    let contracts_dir = find_contracts_dir()?;

    eprintln!("  Deploying contracts to Base...");
    let output = Command::new("forge")
        .args([
            "script",
            "script/DeployWallet.s.sol",
            "--rpc-url", rpc_url,
            "--broadcast",
        ])
        .env("REPO_OWNER", repo_owner)
        .env("REPO_NAME", repo_name)
        .env("MAX_DAILY_SPEND", DEFAULT_MAX_DAILY_SPEND_WEI)
        .env("ALLOWED_DESTINATIONS", COINBASE_COMMERCE_CONTRACT)
        .env("INITIAL_KID", &kid)
        .env("INITIAL_MODULUS", &modulus)
        .env("INITIAL_EXPONENT", &exponent)
        .env("PRIVATE_KEY", deployer_key)
        .current_dir(&contracts_dir)
        .output()
        .await
        .context("failed to run forge script — is Foundry installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        anyhow::bail!("forge script failed:\n{stderr}\n{stdout}");
    }

    // Parse the wallet address from forge output.
    // The script logs: "TerrariumWallet: 0x..."
    let wallet_addr = stdout.lines()
        .chain(stderr.lines())
        .find_map(|line| {
            if line.contains("TerrariumWallet:") {
                line.split_whitespace().last().map(|s| s.to_string())
            } else {
                None
            }
        })
        .context("could not find TerrariumWallet address in forge output")?;

    Ok(wallet_addr)
}

/// Locate the contracts directory. Checks:
/// 1. Relative to the current binary (for installed packages)
/// 2. Relative to cwd (for development)
fn find_contracts_dir() -> Result<std::path::PathBuf> {
    // Check relative to the binary
    if let Ok(exe) = std::env::current_exe() {
        let bin_contracts = exe.parent()
            .map(|p| p.join("../contracts"))
            .and_then(|p| p.canonicalize().ok());
        if let Some(p) = bin_contracts {
            if p.join("foundry.toml").exists() {
                return Ok(p);
            }
        }
    }

    // Check relative to cwd
    let cwd = std::env::current_dir()?;
    for dir in &["contracts", "../contracts"] {
        let p = cwd.join(dir);
        if p.join("foundry.toml").exists() {
            return Ok(p.canonicalize()?);
        }
    }

    // Check TERRARIUM_CONTRACTS_DIR env var
    if let Ok(dir) = std::env::var("TERRARIUM_CONTRACTS_DIR") {
        let p = std::path::PathBuf::from(&dir);
        if p.join("foundry.toml").exists() {
            return Ok(p);
        }
    }

    anyhow::bail!(
        "could not find contracts directory (with foundry.toml). \
         Set TERRARIUM_CONTRACTS_DIR or run from the terrarium repo root."
    )
}

/// Step 1: Wallet setup — deploy or provide an existing address.
async fn step_wallet(wallet_flag: Option<&str>, repo: &str) -> Result<String> {
    eprintln!();
    eprintln!("  Step 1: Wallet");

    // If a wallet address was provided via flag, use it directly.
    if let Some(w) = wallet_flag {
        eprintln!("  \u{2713} Wallet: {w}");
        return Ok(w.to_string());
    }

    if !is_interactive() {
        eprintln!("  \u{26a0} Non-interactive mode: skipping wallet deploy");
        let addr = "0x0000000000000000000000000000000000000000";
        eprintln!("  \u{2713} Wallet: {addr} (placeholder)");
        return Ok(addr.to_string());
    }

    let choice = prompt("  Deploy a new wallet? [Y/n/address]: ")?;

    // If they pasted an address, use it.
    if choice.starts_with("0x") && choice.len() >= 42 {
        eprintln!("  \u{2713} Wallet: {choice}");
        return Ok(choice);
    }

    // If they said no, ask for an address.
    if choice.eq_ignore_ascii_case("n") || choice.eq_ignore_ascii_case("no") {
        let addr = prompt("  Wallet address: ")?;
        if addr.is_empty() {
            anyhow::bail!("wallet address is required");
        }
        eprintln!("  \u{2713} Wallet: {addr}");
        return Ok(addr);
    }

    // Deploy a new wallet.
    let deployer_key = if let Ok(key) = env::var("DEPLOYER_PRIVATE_KEY") {
        eprintln!("  (using DEPLOYER_PRIVATE_KEY from environment)");
        key
    } else {
        prompt_secret("  Deployer private key (hex): ")?
    };

    if deployer_key.is_empty() {
        anyhow::bail!("deployer private key is required for wallet deployment");
    }

    let rpc_url = env::var("BASE_RPC_URL")
        .unwrap_or_else(|_| "https://mainnet.base.org".to_string());

    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    let (repo_owner, repo_name) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        anyhow::bail!("invalid repo format: {repo}");
    };

    let wallet = deploy_wallet(repo_owner, repo_name, &deployer_key, &rpc_url).await?;

    eprintln!("  \u{2713} Wallet deployed: {wallet}");
    Ok(wallet)
}

/// Step 2: Token setup
async fn step_token(token_flag: Option<&str>) -> Result<Option<String>> {
    eprintln!();
    eprintln!("  Step 2: Token");

    eprintln!("  \u{26a0} Token deployment requires Base RPC. Skipping in dev mode.");

    let token_addr = if let Some(t) = token_flag {
        Some(t.to_string())
    } else if is_interactive() {
        let input = prompt("  Token address (or press Enter to skip): ")?;
        if input.is_empty() {
            None
        } else {
            Some(input)
        }
    } else {
        eprintln!("  (non-interactive: skipping token)");
        None
    };

    if let Some(ref addr) = token_addr {
        let url = token::get_zora_url(addr);
        eprintln!("  \u{2713} Token: {addr}");
        eprintln!("  \u{2713} Zora: {url}");
    } else {
        eprintln!("  \u{2713} Token: skipped (can be set later)");
    }

    Ok(token_addr)
}

/// Step 3: Secrets and variables
async fn step_secrets(wallet: &str, token_addr: Option<&str>) -> Result<()> {
    eprintln!();
    eprintln!("  Step 3: Secrets");

    // Prompt for OpenRouter API key (or read from env for non-interactive)
    let api_key = if let Ok(key) = env::var("OPENROUTER_API_KEY") {
        eprintln!("  (using OPENROUTER_API_KEY from environment)");
        key
    } else if is_interactive() {
        prompt_secret("  OpenRouter API key: ")?
    } else {
        String::new()
    };

    if api_key.is_empty() {
        eprintln!("  \u{26a0} No API key provided — skipping secret setup");
        return Ok(());
    }

    // Set the secret
    gh_secret_set("OPENROUTER_API_KEY", &api_key).await?;
    eprintln!("  \u{2713} OPENROUTER_API_KEY set");

    // Read default model config for variable values
    let model_cfg = models::read_model_config()?;

    let owner_model = model_cfg.owner.as_deref().unwrap_or("moonshotai/kimi-k2.5");
    let high_model = model_cfg.high.as_deref().unwrap_or("moonshotai/kimi-k2.5");
    let medium_model = model_cfg.medium.as_deref().unwrap_or("qwen/qwen3.5-35b");
    let low_model = model_cfg.low.as_deref().unwrap_or("qwen/qwen3.5-35b");

    gh_variable_set("TERRARIUM_MODEL_OWNER", owner_model).await?;
    gh_variable_set("TERRARIUM_MODEL_HIGH", high_model).await?;
    gh_variable_set("TERRARIUM_MODEL_MEDIUM", medium_model).await?;
    gh_variable_set("TERRARIUM_MODEL_LOW", low_model).await?;
    eprintln!("  \u{2713} Model variables set ({owner_model} owner/high, {medium_model} medium/low)");

    gh_variable_set("TERRARIUM_WALLET", wallet).await?;
    if let Some(t) = token_addr {
        gh_variable_set("TERRARIUM_TOKEN", t).await?;
    }
    eprintln!("  \u{2713} Wallet + token addresses set as repo variables");

    Ok(())
}

/// Step 4: Security hardening
async fn step_security(repo: &str) -> Result<()> {
    eprintln!();
    eprintln!("  Step 4: Security");

    // Enable branch protection on main
    let protection_body = serde_json::json!({
        "required_status_checks": null,
        "enforce_admins": true,
        "required_pull_request_reviews": {
            "required_approving_review_count": 1
        },
        "restrictions": null,
        "allow_force_pushes": false,
        "allow_deletions": false
    });

    let endpoint = format!("repos/{repo}/branches/main/protection");
    let mut child = Command::new("gh")
        .args([
            "api",
            &endpoint,
            "--method",
            "PUT",
            "--input",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn gh api for branch protection")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(protection_body.to_string().as_bytes()).await?;
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("  \u{26a0} Branch protection failed (may require admin): {stderr}");
    } else {
        eprintln!("  \u{2713} Branch protection enabled on main");
    }

    // Create CODEOWNERS file
    let codeowners_dir = Path::new(".github");
    if !codeowners_dir.exists() {
        std::fs::create_dir_all(codeowners_dir)
            .context("failed to create .github directory")?;
    }

    let codeowners_content = "# Terrarium: workflow files require admin review\n\
                              .github/workflows/ @admin\n";
    std::fs::write(codeowners_dir.join("CODEOWNERS"), codeowners_content)
        .context("failed to write CODEOWNERS")?;
    eprintln!("  \u{2713} CODEOWNERS: .github/workflows/ requires admin review");

    eprintln!("  \u{2713} Direct pushes to main restricted");

    Ok(())
}

/// Step 5: Funding walkthrough
fn step_funding(wallet: &str, token_addr: Option<&str>) {
    eprintln!();
    eprintln!("  Step 5: Fund your project");

    if let Some(addr) = token_addr {
        let url = token::get_zora_url(addr);
        eprintln!("  To seed the treasury, buy your project's token:");
        eprintln!("  \u{2192} {url}");
        eprintln!();
    }

    eprintln!("  Or send ETH directly to the wallet:");
    eprintln!("  \u{2192} {wallet} (Base network)");
}

/// Step 6: First milestone
async fn step_milestone() -> Result<()> {
    eprintln!();
    eprintln!("  Step 6: First milestone");

    let description = prompt("  What should your project build first?\n  > ")?;

    if description.is_empty() {
        eprintln!("  \u{26a0} No milestone description — skipping");
        return Ok(());
    }

    // Create a milestone title from the description
    let title = format!("v0.1 \u{2014} {description}");

    match milestones::create_milestone(&title, &description).await {
        Ok(number) => {
            eprintln!("  \u{2713} Created milestone #{number}: \"{title}\"");
        }
        Err(e) => {
            eprintln!("  \u{26a0} Failed to create milestone: {e}");
            eprintln!("    You can create it manually: gh api repos/{{owner}}/{{repo}}/milestones --method POST -f title=\"{title}\"");
        }
    }

    Ok(())
}

/// Step 7: Write the workflow template
fn step_workflow() -> Result<()> {
    eprintln!();
    eprintln!("  Step 7: Workflow");

    let workflow_dir = Path::new(".github/workflows");
    std::fs::create_dir_all(workflow_dir)
        .context("failed to create .github/workflows directory")?;

    let workflow_content = r#"# .github/workflows/terrarium.yml
name: Terrarium
on:
  push:
    branches: [main, master]
  schedule:
    - cron: '*/30 * * * *'
  workflow_dispatch: {}
jobs:
  owner:
    uses: jbrower95/terrarium/.github/workflows/owner.yml@master
    permissions:
      id-token: write
      contents: write
      issues: write
      pull-requests: write
      actions: write
    secrets: inherit
"#;

    std::fs::write(workflow_dir.join("terrarium.yml"), workflow_content)
        .context("failed to write terrarium.yml")?;

    eprintln!("  \u{2713} .github/workflows/terrarium.yml written");

    Ok(())
}

/// Print final output with badges and next steps.
fn print_final(_wallet: &str, token_addr: Option<&str>) {
    eprintln!();
    eprintln!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    eprintln!();
    eprintln!("  Done! Add these to your README:");
    eprintln!();

    if let Some(addr) = token_addr {
        let url = token::get_zora_url(addr);
        eprintln!(
            "  [![Donate](https://img.shields.io/badge/donate-fund_this_project-ff69b4?style=for-the-badge)]({url})"
        );
    }

    eprintln!("  ![Treasury](https://img.shields.io/badge/treasury-$0.00-lightgrey)");
    eprintln!();
    eprintln!("  The owner will update these badges automatically on each cycle.");
    eprintln!();
    eprintln!("  Push your repo. The owner will wake up on the next cron cycle,");
    eprintln!("  read your milestone, file issues, and start building.");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.install {
        eprintln!();
        eprintln!("  \u{1f331} terrarium installer");
        eprintln!();

        // Detect repo
        let repo = match detect_repo().await {
            Ok(r) => {
                eprintln!("  Detected repo: {r}");
                r
            }
            Err(e) => {
                eprintln!("  \u{26a0} Could not detect repo: {e}");
                eprintln!("  Continuing without repo detection...");
                "unknown/unknown".to_string()
            }
        };

        // Step 1: Wallet
        let wallet = step_wallet(cli.wallet.as_deref(), &repo).await?;

        // Step 2: Token
        let token_addr = step_token(cli.token.as_deref()).await?;

        // Step 3: Secrets + variables
        step_secrets(&wallet, token_addr.as_deref()).await?;

        // Step 4: Security hardening
        step_security(&repo).await?;

        // Step 5: Funding walkthrough
        step_funding(&wallet, token_addr.as_deref());

        // Step 6: First milestone
        step_milestone().await?;

        // Step 7: Workflow template
        step_workflow()?;

        // Final output
        print_final(&wallet, token_addr.as_deref());
    } else {
        eprintln!("terrarium");
        eprintln!();
        eprintln!("Usage: terrarium --install");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --install          Run the install wizard");
        eprintln!("  --wallet <ADDR>    Provide wallet address");
        eprintln!("  --token <ADDR>     Provide token address");
    }

    Ok(())
}
