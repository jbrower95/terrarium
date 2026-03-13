use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

use terrarium_core::{
    actions::{self, ActionResult},
    budget::{self, Balance},
    context::{self, OwnerContext},
    inference::{self, InferenceResult, Message},
    journal::{self, JournalEntry},
    milestones,
    models::{self, ModelConfig},
    pr,
    spend,
    status::{self, StatusData},
    tasks,
};

// ── Run artifact output ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct RunArtifact {
    run_id: u64,
    role: String,
    issue: Option<u64>,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    result: serde_json::Value,
}

// ── Constants ────────────────────────────────────────────────────────

const CREDIT_THRESHOLD_USD: f64 = 5.0;
const DEFAULT_TOPUP_USD: f64 = 10.0;

// ── Main ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let result = run().await;
    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("owner: fatal error: {e:#}");

            // Always output an artifact even on failure (issue #25).
            let artifact = RunArtifact {
                run_id: github_run_id(),
                role: "owner".into(),
                issue: None,
                model: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
                result: serde_json::json!({
                    "status": "error",
                    "error": format!("{e:#}"),
                }),
            };
            let _ = serde_json::to_writer(std::io::stdout(), &artifact);
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<()> {
    let repo_root = repo_root()?;

    // ── Step 1: Read wallet + token from env vars (repo variables) ───
    let wallet = env::var("TERRARIUM_WALLET").unwrap_or_default();
    let token_addr = env::var("TERRARIUM_TOKEN").ok();

    // ── Step 2: Read model config from env vars ──────────────────────
    let model_cfg = models::read_model_config().context("step 2: read model config")?;

    // ── First-run detection (issue #24) ──────────────────────────────
    let journal_path = repo_root.join("JOURNAL.md");
    let first_run = !journal_path.exists();

    if first_run {
        let repo_slug = repo_slug();
        return handle_first_run(&repo_root, &wallet, &model_cfg, &repo_slug).await;
    }

    // ── Step 3: Get wallet balance ───────────────────────────────────
    let rpc_url = env::var("BASE_RPC_URL").unwrap_or_default();
    let balance = match budget::get_wallet_balance(&wallet, &rpc_url).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("warning: wallet balance fetch failed: {e:#}");
            Balance { eth: 0.0, usd: 0.0 }
        }
    };

    // ── Step 4: Get OpenRouter credits ───────────────────────────────
    let api_key = env::var("OPENROUTER_API_KEY").context("OPENROUTER_API_KEY not set")?;
    let openrouter_credits = match budget::get_openrouter_credits(&api_key).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("warning: OpenRouter credits fetch failed: {e:#}");
            0.0
        }
    };

    // ── Step 5: Download recent artifacts and aggregate spend ────────
    let repo_slug = repo_slug();
    let artifacts_7d = match spend::download_recent_artifacts(&repo_slug, 168).await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("warning: artifact download failed: {e:#}");
            vec![]
        }
    };
    let spend_summary = spend::aggregate_spend(&artifacts_7d);

    // ── Step 6: List open milestones + their issues ──────────────────
    let milestones_with_issues = match list_milestones_with_issues().await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("warning: milestone listing failed: {e:#}");
            vec![]
        }
    };

    // ── Step 7: List open PRs ────────────────────────────────────────
    let open_prs = match pr::list_open_prs().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("warning: PR listing failed: {e:#}");
            vec![]
        }
    };

    // ── Step 8: Read journal context (last 5 entries) ────────────────
    let journal_context = journal::read_journal_context(&repo_root, 5)
        .unwrap_or_default();

    // ── Step 9: Calculate runway ─────────────────────────────────────
    let daily_rate = if spend_summary.daily_avg_7d > 0.0 {
        spend_summary.daily_avg_7d
    } else {
        models::estimate_daily_cost(&model_cfg)
    };
    let projected_days = budget::calculate_runway(openrouter_credits + balance.usd, daily_rate);

    // ── Step 10: Check credits below threshold → attempt top-up ──────
    if openrouter_credits < CREDIT_THRESHOLD_USD {
        eprintln!(
            "warning: OpenRouter credits (${:.2}) below threshold (${:.2}), attempting top-up",
            openrouter_credits, CREDIT_THRESHOLD_USD
        );
        if let Err(e) = attempt_topup(&wallet, DEFAULT_TOPUP_USD).await {
            eprintln!("warning: top-up failed: {e:#}");
        }
    }

    // ── Step 11: Build OwnerContext, materialize prompt ───────────────
    let auto_review = env::var("TERRARIUM_AUTO_REVIEW")
        .map(|v| v == "true")
        .unwrap_or(false);

    let (repo_owner, repo_name) = parse_repo_slug(&repo_slug);

    let owner_ctx = OwnerContext {
        repo_owner: repo_owner.clone(),
        repo_name: repo_name.clone(),
        balance: balance.clone(),
        openrouter_credits,
        projected_days,
        spend: spend_summary.clone(),
        models: context_model_config(&model_cfg),
        auto_review,
        milestones: milestones_with_issues,
        open_prs: open_prs.clone(),
        journal_context,
    };

    let prompt = context::materialize_owner_context(&owner_ctx);

    // ── Step 12: Single inference call with JSON mode ─────────────────
    let owner_model = model_cfg
        .owner
        .as_deref()
        .unwrap_or("moonshotai/kimi-k2.5");

    let messages = vec![
        Message {
            role: "system".into(),
            content: "You are the autonomous owner of a software project. Respond with a JSON array of actions.".into(),
        },
        Message {
            role: "user".into(),
            content: prompt,
        },
    ];

    let inference_result = match inference::infer(owner_model, messages, &api_key, true).await {
        Ok(r) => r,
        Err(e) => {
            // Issue #25: inference failure → output error artifact, log to journal, exit.
            eprintln!("error: inference failed: {e:#}");

            let error_entry = JournalEntry {
                timestamp: Utc::now(),
                balance_usd: openrouter_credits + balance.usd,
                daily_run_rate: daily_rate,
                projected_days,
                models: format_model_summary(&model_cfg),
                auto_review,
                body: format!("Inference failed: {e:#}"),
            };
            let _ = journal::append_journal_entry(&repo_root, &error_entry).await;
            let _ = git_commit_journal_and_readme(&repo_root).await;

            let artifact = RunArtifact {
                run_id: github_run_id(),
                role: "owner".into(),
                issue: None,
                model: owner_model.to_string(),
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
                result: serde_json::json!({
                    "status": "error",
                    "error": format!("inference failed: {e:#}"),
                }),
            };
            println!("{}", serde_json::to_string(&artifact)?);
            return Ok(());
        }
    };

    // ── Step 13: Parse action list ───────────────────────────────────
    let parsed_actions = match actions::parse_actions(&inference_result.content) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "warning: failed to parse actions from model response: {e:#}\nRaw response: {}",
                &inference_result.content
            );
            vec![]
        }
    };

    // ── Step 14: Execute actions ─────────────────────────────────────
    let action_results = actions::execute_actions(&parsed_actions).await;

    // Log individual action failures (issue #25).
    for ar in &action_results {
        if !ar.success {
            eprintln!("warning: action '{}' failed: {}", ar.action, ar.detail);
        }
    }

    // ── Step 15: Build StatusData, update README ─────────────────────
    let open_issues = tasks::list_open_issues(None).await.map(|v| v.len() as u64).unwrap_or(0);

    let status_data = StatusData {
        wallet_address: wallet.clone(),
        token_address: token_addr.clone(),
        balance_eth: balance.eth,
        balance_usd: balance.usd,
        openrouter_credits,
        daily_run_rate: daily_rate,
        projected_days,
        models: status_model_config(&model_cfg),
        auto_review,
        open_issues,
        open_prs: open_prs.len() as u64,
        total_merged: 0, // TODO: track merged count
        zora_url: None,
        basescan_url: format!("https://basescan.org/address/{}", wallet),
        repo_slug: Some(repo_slug.clone()),
    };

    if let Err(e) = status::update_readme_status(&repo_root, &status_data).await {
        eprintln!("warning: failed to update README status: {e:#}");
    }

    // ── Step 15b: Render and publish SVG status card ─────────────────
    let svg = status::render_status_svg(&status_data);
    if let Err(e) = status::publish_status_svg(&svg, &repo_slug).await {
        eprintln!("warning: failed to publish SVG status card: {e:#}");
    }

    // ── Step 16: Build JournalEntry, append to journal ───────────────
    let journal_body = build_journal_body(&inference_result, &action_results);
    let journal_entry = JournalEntry {
        timestamp: Utc::now(),
        balance_usd: openrouter_credits + balance.usd,
        daily_run_rate: daily_rate,
        projected_days,
        models: format_model_summary(&model_cfg),
        auto_review,
        body: journal_body,
    };

    if let Err(e) = journal::append_journal_entry(&repo_root, &journal_entry).await {
        eprintln!("warning: failed to append journal entry: {e:#}");
    }

    // ── Step 17: Commit JOURNAL.md + README.md ───────────────────────
    if let Err(e) = git_commit_journal_and_readme(&repo_root).await {
        eprintln!("warning: git commit failed: {e:#}");
    }

    // ── Step 18: Upload run artifact as JSON to stdout ────────────────
    let artifact = RunArtifact {
        run_id: github_run_id(),
        role: "owner".into(),
        issue: None,
        model: owner_model.to_string(),
        input_tokens: inference_result.input_tokens,
        output_tokens: inference_result.output_tokens,
        cost_usd: inference_result.cost_usd,
        result: serde_json::json!({
            "status": "ok",
            "actions_taken": action_results.len(),
            "actions_succeeded": action_results.iter().filter(|a| a.success).count(),
            "actions_failed": action_results.iter().filter(|a| !a.success).count(),
        }),
    };

    println!("{}", serde_json::to_string(&artifact)?);

    Ok(())
}

// ── First-run handler (issue #24) ────────────────────────────────────

async fn handle_first_run(
    repo_root: &PathBuf,
    wallet: &str,
    model_cfg: &ModelConfig,
    repo_slug: &str,
) -> Result<()> {
    eprintln!("first run detected: initializing JOURNAL.md and README status block");

    let now = Utc::now();
    let auto_review = env::var("TERRARIUM_AUTO_REVIEW")
        .map(|v| v == "true")
        .unwrap_or(false);

    // Create initial journal entry.
    let entry = JournalEntry {
        timestamp: now,
        balance_usd: 0.0,
        daily_run_rate: 0.0,
        projected_days: f64::INFINITY,
        models: format_model_summary(model_cfg),
        auto_review,
        body: "First run: initialized JOURNAL.md and README status block.".into(),
    };

    journal::append_journal_entry(repo_root, &entry)
        .await
        .context("failed to create JOURNAL.md on first run")?;

    // Add README status block.
    let status_data = StatusData {
        wallet_address: wallet.to_string(),
        token_address: None,
        balance_eth: 0.0,
        balance_usd: 0.0,
        openrouter_credits: 0.0,
        daily_run_rate: 0.0,
        projected_days: f64::INFINITY,
        models: status_model_config(model_cfg),
        auto_review,
        open_issues: 0,
        open_prs: 0,
        total_merged: 0,
        zora_url: None,
        basescan_url: format!("https://basescan.org/address/{}", wallet),
        repo_slug: Some(repo_slug.to_string()),
    };

    status::update_readme_status(repo_root, &status_data)
        .await
        .context("failed to update README on first run")?;

    // Publish SVG status card on first run too.
    let svg = status::render_status_svg(&status_data);
    if let Err(e) = status::publish_status_svg(&svg, repo_slug).await {
        eprintln!("warning: failed to publish SVG status card on first run: {e:#}");
    }

    // Commit the initial files.
    git_commit_journal_and_readme(repo_root).await?;

    // Output first-run artifact.
    let artifact = RunArtifact {
        run_id: github_run_id(),
        role: "owner".into(),
        issue: None,
        model: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: 0.0,
        result: serde_json::json!({
            "status": "first_run",
            "message": "Initialized JOURNAL.md and README status block",
        }),
    };

    println!("{}", serde_json::to_string(&artifact)?);

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Determine the repo root directory.
fn repo_root() -> Result<PathBuf> {
    if let Ok(workspace) = env::var("GITHUB_WORKSPACE") {
        return Ok(PathBuf::from(workspace));
    }
    // Fall back to current directory.
    env::current_dir().context("failed to determine repo root")
}

/// Get the GitHub run ID from the environment.
fn github_run_id() -> u64 {
    env::var("GITHUB_RUN_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Get the repository slug (owner/repo) from the environment.
fn repo_slug() -> String {
    env::var("GITHUB_REPOSITORY").unwrap_or_else(|_| "unknown/unknown".to_string())
}

/// Parse "owner/repo" into (owner, repo).
fn parse_repo_slug(slug: &str) -> (String, String) {
    let parts: Vec<&str> = slug.splitn(2, '/').collect();
    if parts.len() == 2 {
        (parts[0].to_string(), parts[1].to_string())
    } else {
        (slug.to_string(), String::new())
    }
}

/// List all open milestones with their associated issues.
async fn list_milestones_with_issues(
) -> Result<Vec<(terrarium_core::milestones::Milestone, Vec<terrarium_core::tasks::Issue>)>> {
    let ms_list = milestones::list_open_milestones().await?;
    let mut result = Vec::new();
    for ms in ms_list {
        let issues = match milestones::get_milestone_issues(ms.number).await {
            Ok(i) => i,
            Err(e) => {
                eprintln!(
                    "warning: failed to get issues for milestone '{}': {e:#}",
                    ms.title
                );
                vec![]
            }
        };
        result.push((ms, issues));
    }
    Ok(result)
}

/// Convert the models::ModelConfig to context::OwnerContext's expected format
/// (which uses its own ModelConfig from context module -- but looking at the code,
/// OwnerContext uses models::ModelConfig directly).
fn context_model_config(cfg: &ModelConfig) -> ModelConfig {
    cfg.clone()
}

/// Convert models::ModelConfig to status::ModelConfig (different type).
fn status_model_config(cfg: &ModelConfig) -> status::ModelConfig {
    status::ModelConfig {
        owner: cfg.owner.clone(),
        high: cfg.high.clone(),
        medium: cfg.medium.clone(),
        low: cfg.low.clone(),
    }
}

/// Format model config as a summary string for journal entries.
fn format_model_summary(cfg: &ModelConfig) -> String {
    let fmt = |tier: &str, val: &Option<String>| -> String {
        match val {
            Some(m) => {
                // Strip the provider prefix for brevity.
                let short = m.rsplit('/').next().unwrap_or(m);
                format!("{tier}:{short}")
            }
            None => format!("{tier}:unset"),
        }
    };

    [
        fmt("owner", &cfg.owner),
        fmt("high", &cfg.high),
        fmt("medium", &cfg.medium),
        fmt("low", &cfg.low),
    ]
    .join(", ")
}

/// Build the journal body text from inference result and action results.
fn build_journal_body(
    inference: &InferenceResult,
    action_results: &[ActionResult],
) -> String {
    let mut body = String::new();

    // Summarize inference cost.
    body.push_str(&format!(
        "Inference: {} input / {} output tokens, ${:.4}\n",
        inference.input_tokens, inference.output_tokens, inference.cost_usd
    ));

    // Summarize actions.
    if action_results.is_empty() {
        body.push_str("No actions taken.\n");
    } else {
        let succeeded = action_results.iter().filter(|a| a.success).count();
        let failed = action_results.iter().filter(|a| !a.success).count();
        body.push_str(&format!(
            "Actions: {} total, {} succeeded, {} failed\n",
            action_results.len(),
            succeeded,
            failed
        ));
        for ar in action_results {
            let icon = if ar.success { "ok" } else { "FAIL" };
            body.push_str(&format!("  - [{}] {}: {}\n", icon, ar.action, ar.detail));
        }
    }

    body
}

/// Attempt an automatic top-up of OpenRouter credits via the OIDC wallet.
async fn attempt_topup(wallet: &str, amount_usd: f64) -> Result<()> {
    use terrarium_core::{budget, wallet as wallet_mod};

    // Get the top-up calldata from OpenRouter.
    let calldata = budget::build_topup_calldata(amount_usd)
        .await
        .context("failed to build top-up calldata")?;

    // Request an OIDC token from GitHub Actions.
    let jwt = wallet_mod::request_oidc_token(None)
        .await
        .context("failed to request OIDC token for top-up")?;

    // Build and submit the UserOperation.
    let openrouter_address = "0x0000000000000000000000000000000000000000"; // placeholder
    let user_op = wallet_mod::build_user_op(wallet, openrouter_address, 0, calldata, &jwt, 0)
        .await
        .context("failed to build user op for top-up")?;

    let bundler_url =
        env::var("BUNDLER_URL").unwrap_or_else(|_| "https://api.pimlico.io/v2/8453/rpc".into());
    let entry_point = "0x0000000071727De22E5E9d8BAf0edAc6f37da032"; // v0.7

    let op_hash = wallet_mod::submit_user_op(&bundler_url, entry_point, &user_op)
        .await
        .context("failed to submit top-up user op")?;

    eprintln!("top-up submitted: op_hash={op_hash}");

    Ok(())
}

/// Commit JOURNAL.md and README.md via git.
async fn git_commit_journal_and_readme(repo_root: &PathBuf) -> Result<()> {
    use tokio::process::Command;

    // git add JOURNAL.md README.md
    let add_output = Command::new("git")
        .args(["add", "JOURNAL.md", "README.md"])
        .current_dir(repo_root)
        .output()
        .await
        .context("failed to spawn git add")?;

    if !add_output.status.success() {
        let stderr = String::from_utf8_lossy(&add_output.stderr);
        anyhow::bail!("git add failed: {stderr}");
    }

    // Check if there are staged changes.
    let diff_output = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(repo_root)
        .output()
        .await
        .context("failed to spawn git diff")?;

    if diff_output.status.success() {
        // No staged changes, nothing to commit.
        eprintln!("no changes to commit");
        return Ok(());
    }

    // git commit
    let commit_output = Command::new("git")
        .args([
            "commit",
            "-m",
            "chore(owner): update journal and status [skip ci]",
        ])
        .current_dir(repo_root)
        .output()
        .await
        .context("failed to spawn git commit")?;

    if !commit_output.status.success() {
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        anyhow::bail!("git commit failed: {stderr}");
    }

    Ok(())
}

