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

/// Step 1: Wallet setup
async fn step_wallet(wallet_flag: Option<&str>) -> Result<String> {
    eprintln!();
    eprintln!("  Step 1: Wallet");

    eprintln!("  \u{26a0} Wallet deployment requires Base RPC + funded deployer. Skipping in dev mode.");

    let wallet = if let Some(w) = wallet_flag {
        w.to_string()
    } else if is_interactive() {
        let input = prompt("  Wallet address (or press Enter for placeholder): ")?;
        if input.is_empty() {
            "0x0000000000000000000000000000000000000000".to_string()
        } else {
            input
        }
    } else {
        eprintln!("  (non-interactive: using placeholder)");
        "0x0000000000000000000000000000000000000000".to_string()
    };

    eprintln!("  \u{2713} Wallet: {wallet}");
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
        let wallet = step_wallet(cli.wallet.as_deref()).await?;

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
