use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use terrarium_core::{
    inference::{self, Message},
    models,
    pr::{self, PullRequest},
    spend::RunArtifact,
    tasks,
};
use tokio::process::Command;

// ── JSON response from the model ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ImplementationPlan {
    files: Vec<FileChange>,
    commit_message: String,
}

#[derive(Debug, Deserialize)]
struct FileChange {
    path: String,
    content: String,
}

// ── Artifact result types ────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum EmployeeResult {
    Pr {
        number: u64,
        commit_sha: String,
    },
    PrUpdated {
        number: u64,
        commit_sha: String,
    },
    Stuck {
        issue: u64,
        reason: String,
    },
    Error {
        error: String,
    },
}

// ── Main ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let issue_number: u64 = env::var("TERRARIUM_ISSUE")
        .context("TERRARIUM_ISSUE not set")?
        .parse()
        .context("TERRARIUM_ISSUE must be a number")?;

    let complexity = env::var("TERRARIUM_COMPLEXITY").unwrap_or_else(|_| "medium".to_string());

    let model = resolve_model(&complexity)?;
    let api_key = env::var("OPENROUTER_API_KEY").context("OPENROUTER_API_KEY not set")?;

    let result = run_employee(issue_number, &complexity, &model, &api_key).await;

    // Build and emit artifact regardless of success/failure.
    let (employee_result, inf_tokens) = match result {
        Ok(r) => r,
        Err(e) => {
            // On error: clean up labels, emit error artifact.
            let _ = tasks::remove_label(issue_number, "in-progress").await;
            let er = EmployeeResult::Error {
                error: format!("{e:#}"),
            };
            emit_artifact(issue_number, &model, 0, 0, 0.0, &er);
            // Still exit 0 so the artifact is the canonical output.
            return Ok(());
        }
    };

    emit_artifact(
        issue_number,
        &model,
        inf_tokens.0,
        inf_tokens.1,
        inf_tokens.2,
        &employee_result,
    );

    Ok(())
}

// ── Core workflow ────────────────────────────────────────────────────

/// Returns (result, (input_tokens, output_tokens, cost_usd)).
async fn run_employee(
    issue_number: u64,
    _complexity: &str,
    model: &str,
    api_key: &str,
) -> Result<(EmployeeResult, (u64, u64, f64))> {
    // 1. Fetch issue details.
    let issue = fetch_issue(issue_number).await?;

    // 2. Claim the issue.
    tasks::add_label(issue_number, "in-progress").await?;
    tasks::add_comment(issue_number, "\u{1f916} Working on this...").await?;

    // 3. Check for existing PR (issue #27).
    let branch_name = format!("terrarium/issue-{issue_number}");
    let existing_pr = find_existing_pr(&branch_name).await?;
    let review_context = match &existing_pr {
        Some(pr) => gather_review_comments(pr.number).await.unwrap_or_default(),
        None => String::new(),
    };

    // 4. Gather repo context.
    let repo_name = detect_repo_name().await?;
    let file_listing = list_repo_files().await?;
    let issue_comments = fetch_issue_comments(issue_number).await?;
    let related_files = read_related_files(&issue.body, &file_listing).await?;

    // 5. Build prompt.
    let messages = build_prompt(
        &repo_name,
        &issue,
        &issue_comments,
        &file_listing,
        &related_files,
        &review_context,
        existing_pr.is_some(),
    );

    // 6. Run inference.
    let inf = inference::infer(model, messages, api_key, true).await?;

    // 7. Parse the response.
    let plan: ImplementationPlan = match serde_json::from_str(&inf.content) {
        Ok(p) => p,
        Err(e) => {
            let reason = format!("Failed to parse model response: {e}");
            handle_stuck(issue_number, &reason).await?;
            return Ok((
                EmployeeResult::Stuck {
                    issue: issue_number,
                    reason,
                },
                (inf.input_tokens, inf.output_tokens, inf.cost_usd),
            ));
        }
    };

    if plan.files.is_empty() {
        let reason = "Model returned no file changes.".to_string();
        handle_stuck(issue_number, &reason).await?;
        return Ok((
            EmployeeResult::Stuck {
                issue: issue_number,
                reason,
            },
            (inf.input_tokens, inf.output_tokens, inf.cost_usd),
        ));
    }

    // 8. Apply changes, commit, push, create/update PR.
    let result = apply_and_push(
        issue_number,
        &branch_name,
        &plan,
        existing_pr.as_ref(),
        model,
        &issue.title,
    )
    .await?;

    Ok((result, (inf.input_tokens, inf.output_tokens, inf.cost_usd)))
}

// ── Issue helpers ────────────────────────────────────────────────────

async fn fetch_issue(number: u64) -> Result<tasks::Issue> {
    let num = number.to_string();
    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &num,
            "--json",
            "number,title,body,labels,milestone,assignees,state",
        ])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue view failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // gh issue view --json returns the same shape as a single element of
    // gh issue list --json, so we parse via an intermediate struct.
    #[derive(Deserialize)]
    struct GhIssue {
        number: u64,
        title: String,
        body: String,
        labels: Vec<GhLabel>,
        milestone: Option<GhMilestone>,
        assignees: Vec<GhAssignee>,
        state: String,
    }
    #[derive(Deserialize)]
    struct GhLabel {
        name: String,
    }
    #[derive(Deserialize)]
    struct GhMilestone {
        title: String,
    }
    #[derive(Deserialize)]
    struct GhAssignee {
        login: String,
    }

    let gh: GhIssue = serde_json::from_str(&stdout).context("failed to parse gh issue view")?;

    Ok(tasks::Issue {
        number: gh.number,
        title: gh.title,
        body: gh.body,
        labels: gh.labels.into_iter().map(|l| l.name).collect(),
        milestone: gh.milestone.map(|m| m.title),
        assignee: gh.assignees.into_iter().next().map(|a| a.login),
        state: gh.state,
    })
}

async fn fetch_issue_comments(number: u64) -> Result<String> {
    let num = number.to_string();
    let output = Command::new("gh")
        .args(["issue", "view", &num, "--comments"])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        // Non-fatal: just return empty.
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ── PR helpers ───────────────────────────────────────────────────────

async fn find_existing_pr(branch: &str) -> Result<Option<PullRequest>> {
    let prs = pr::list_open_prs().await?;
    Ok(prs.into_iter().find(|p| p.head_ref == branch))
}

async fn gather_review_comments(pr_number: u64) -> Result<String> {
    let num = pr_number.to_string();
    let output = Command::new("gh")
        .args(["pr", "view", &num, "--comments"])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ── Repo context helpers ─────────────────────────────────────────────

async fn detect_repo_name() -> Result<String> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"])
        .output()
        .await
        .context("failed to detect repo name")?;

    if !output.status.success() {
        return Ok("unknown/repo".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn list_repo_files() -> Result<String> {
    let output = Command::new("find")
        .args([
            ".",
            "-type",
            "f",
            "-not",
            "-path",
            "./.git/*",
            "-not",
            "-path",
            "./target/*",
            "-not",
            "-path",
            "./node_modules/*",
        ])
        .output()
        .await
        .context("failed to list repo files")?;

    let listing = String::from_utf8_lossy(&output.stdout).to_string();
    // Truncate if too large to fit in context.
    Ok(truncate_string(&listing, 8000))
}

/// Attempt to read files that seem relevant based on the issue body.
async fn read_related_files(issue_body: &str, file_listing: &str) -> Result<String> {
    let mut context = String::new();
    let files: Vec<&str> = file_listing.lines().collect();

    // Find files mentioned in the issue body.
    let mut matched: Vec<&str> = Vec::new();
    for file in &files {
        let file_trimmed = file.trim().trim_start_matches("./");
        if file_trimmed.is_empty() {
            continue;
        }
        if issue_body.contains(file_trimmed) {
            matched.push(file_trimmed);
        }
    }

    // Also include Cargo.toml / package.json at root for build context.
    for sentinel in &["Cargo.toml", "package.json", "README.md"] {
        let path = format!("./{sentinel}");
        if files.iter().any(|f| f.trim() == path || f.trim() == *sentinel)
            && !matched.contains(sentinel)
        {
            matched.push(sentinel);
        }
    }

    // Limit to 10 files to avoid blowing up the context window.
    matched.truncate(10);

    for file in matched {
        let content = tokio::fs::read_to_string(file).await.unwrap_or_default();
        let content = truncate_string(&content, 15000);
        context.push_str(&format!("--- {file} ---\n{content}\n\n"));
    }

    Ok(context)
}

// ── Prompt construction ──────────────────────────────────────────────

fn build_prompt(
    repo_name: &str,
    issue: &tasks::Issue,
    comments: &str,
    file_listing: &str,
    related_files: &str,
    review_context: &str,
    is_update: bool,
) -> Vec<Message> {
    let mut system = format!(
        "You are a software engineer working on {repo_name}. \
         Implement the following issue.\n\n\
         Respond with a JSON object containing:\n\
         - \"files\": an array of objects with \"path\" (relative file path) and \"content\" (full file content)\n\
         - \"commit_message\": a concise commit message\n\n\
         Important:\n\
         - Write complete file contents, not patches or diffs.\n\
         - Only include files that need to be created or modified.\n\
         - Keep changes minimal and focused on the issue.\n\
         - Do NOT refactor, rename, reorganize, or \"improve\" existing code.\n\
         - Do NOT change function signatures, remove functions, or alter code unrelated to the issue.\n\
         - Do NOT add new dependencies unless the issue explicitly requires them.\n\
         - When rewriting a file, preserve all existing code exactly as-is except for the specific lines the issue asks you to change.\n\
         - Do NOT include any text outside the JSON object."
    );

    if is_update {
        system.push_str(
            "\n\nThis is an UPDATE to a previous implementation that received review feedback. \
             Address the review comments below and push improvements.",
        );
    }

    let mut user = format!(
        "## Issue #{}: {}\n\n{}\n",
        issue.number, issue.title, issue.body
    );

    if !comments.is_empty() {
        user.push_str(&format!("\n## Issue comments\n{comments}\n"));
    }

    if !review_context.is_empty() {
        user.push_str(&format!("\n## PR review comments (address these)\n{review_context}\n"));
    }

    user.push_str(&format!("\n## Repository files\n```\n{file_listing}\n```\n"));

    if !related_files.is_empty() {
        user.push_str(&format!("\n## Relevant file contents\n{related_files}\n"));
    }

    vec![
        Message {
            role: "system".to_string(),
            content: system,
        },
        Message {
            role: "user".to_string(),
            content: user,
        },
    ]
}

// ── Git / apply / push ──────────────────────────────────────────────

async fn apply_and_push(
    issue_number: u64,
    branch_name: &str,
    plan: &ImplementationPlan,
    existing_pr: Option<&PullRequest>,
    model: &str,
    issue_title: &str,
) -> Result<EmployeeResult> {
    // Ensure we're on a clean starting point.
    if existing_pr.is_some() {
        // Check out existing branch.
        run_git(&["fetch", "origin", branch_name]).await.ok();
        run_git(&["checkout", branch_name]).await?;
        run_git(&["pull", "origin", branch_name, "--rebase"]).await.ok();
    } else {
        // Make sure we're on the default branch before creating a new one.
        let default = detect_default_branch().await?;
        run_git(&["checkout", &default]).await?;
        run_git(&["pull", "origin", &default]).await.ok();
        run_git(&["checkout", "-b", branch_name]).await?;
    }

    // Write files.
    for file in &plan.files {
        // Ensure parent directory exists.
        if let Some(parent) = std::path::Path::new(&file.path).parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(&file.path, &file.content)
            .await
            .with_context(|| format!("failed to write {}", file.path))?;
    }

    // Stage and commit.
    let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    let mut add_args = vec!["add"];
    add_args.extend(paths.iter());
    run_git(&add_args).await?;

    run_git(&["commit", "-m", &plan.commit_message]).await?;

    // Push.
    run_git(&["push", "-u", "origin", branch_name]).await?;

    // Get commit SHA.
    let sha = get_head_sha().await?;

    // Create or update PR.
    if let Some(pr) = existing_pr {
        Ok(EmployeeResult::PrUpdated {
            number: pr.number,
            commit_sha: sha,
        })
    } else {
        let pr_body = format!(
            "Closes #{issue_number}\n\n\
             **Model:** `{model}`\n\
             **Commit:** `{sha}`\n\n\
             ---\n\
             *Automated by terrarium-employee*"
        );

        let pr_title = format!("#{issue_number}: {issue_title}");
        let pr_number = pr::create_pr(branch_name, &pr_title, &pr_body).await?;

        Ok(EmployeeResult::Pr {
            number: pr_number,
            commit_sha: sha,
        })
    }
}

async fn run_git(args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to spawn git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {stderr}", args.join(" "));
    }

    Ok(())
}

async fn detect_default_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .output()
        .await
        .context("failed to detect default branch")?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Strip "origin/" prefix.
        Ok(branch.strip_prefix("origin/").unwrap_or(&branch).to_string())
    } else {
        // Fallback.
        Ok("main".to_string())
    }
}

async fn get_head_sha() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .context("failed to get HEAD sha")?;

    if !output.status.success() {
        anyhow::bail!("git rev-parse HEAD failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ── Stuck / cleanup helpers ─────────────────────────────────────────

async fn handle_stuck(issue_number: u64, reason: &str) -> Result<()> {
    let comment = format!("Stuck: {reason}");
    tasks::add_comment(issue_number, &comment).await.ok();
    tasks::add_label(issue_number, "stuck").await.ok();
    tasks::remove_label(issue_number, "in-progress").await.ok();
    Ok(())
}

// ── Model resolution ────────────────────────────────────────────────

fn resolve_model(complexity: &str) -> Result<String> {
    let env_key = format!("TERRARIUM_MODEL_{}", complexity.to_uppercase());

    if let Ok(m) = env::var(&env_key) {
        return Ok(m);
    }

    // Fall back to model config defaults.
    let config = models::read_model_config()?;
    let model = match complexity {
        "high" => config.high,
        "medium" => config.medium,
        "low" => config.low,
        _ => config.medium,
    };

    model.context(format!("no model configured for complexity {complexity}"))
}

// ── Artifact emission ───────────────────────────────────────────────

fn emit_artifact(
    issue_number: u64,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    result: &EmployeeResult,
) {
    let artifact = RunArtifact {
        run_id: std::process::id() as u64,
        role: "employee".to_string(),
        issue: Some(issue_number),
        model: model.to_string(),
        input_tokens,
        output_tokens,
        cost_usd,
        result: serde_json::to_value(result).unwrap_or_default(),
    };

    // Artifact goes to stdout as a single JSON line.
    if let Ok(json) = serde_json::to_string(&artifact) {
        println!("{json}");
    }
}

// ── Utilities ────────────────────────────────────────────────────────

fn truncate_string(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        // Find a safe char boundary.
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut truncated = s[..end].to_string();
        truncated.push_str("\n... (truncated)");
        truncated
    }
}
