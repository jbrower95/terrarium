use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub head_ref: String,
    pub diff_stats: String,
    pub head_sha: String,
}

/// Intermediate struct for deserializing `gh pr list --json` output.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
    number: u64,
    title: String,
    author: GhAuthor,
    head_ref_name: String,
    additions: u64,
    deletions: u64,
    head_ref_oid: String,
}

#[derive(Debug, Deserialize)]
struct GhAuthor {
    login: String,
}

impl From<GhPullRequest> for PullRequest {
    fn from(gh: GhPullRequest) -> Self {
        PullRequest {
            number: gh.number,
            title: gh.title,
            author: gh.author.login,
            head_ref: gh.head_ref_name,
            diff_stats: format!("+{} -{}", gh.additions, gh.deletions),
            head_sha: gh.head_ref_oid,
        }
    }
}

/// List all open pull requests.
pub async fn list_open_prs() -> Result<Vec<PullRequest>> {
    let fields = "number,title,author,headRefName,additions,deletions,headRefOid";

    let output = Command::new("gh")
        .args([
            "pr", "list", "--state", "open", "--json", fields, "--limit", "200",
        ])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr list failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let gh_prs: Vec<GhPullRequest> =
        serde_json::from_str(&stdout).context("failed to parse gh pr list output")?;

    Ok(gh_prs.into_iter().map(PullRequest::from).collect())
}

/// Get the diff for a pull request.
pub async fn get_pr_diff(pr: u64) -> Result<String> {
    let pr_str = pr.to_string();
    let output = Command::new("gh")
        .args(["pr", "diff", &pr_str])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr diff failed: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Merge a pull request using the default merge strategy.
pub async fn merge_pr(pr: u64) -> Result<()> {
    let pr_str = pr.to_string();
    let output = Command::new("gh")
        .args(["pr", "merge", &pr_str, "--merge", "--delete-branch"])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr merge failed: {stderr}");
    }

    Ok(())
}

/// Request changes on a pull request with feedback.
pub async fn request_changes(pr: u64, feedback: &str) -> Result<()> {
    let pr_str = pr.to_string();
    let output = Command::new("gh")
        .args([
            "pr",
            "review",
            &pr_str,
            "--request-changes",
            "--body",
            feedback,
        ])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr review --request-changes failed: {stderr}");
    }

    Ok(())
}

/// Create a new pull request. Returns the PR number.
pub async fn create_pr(branch: &str, title: &str, body: &str) -> Result<u64> {
    let output = Command::new("gh")
        .args([
            "pr", "create", "--head", branch, "--title", title, "--body", body,
        ])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr create failed: {stderr}");
    }

    // gh pr create prints a URL like https://github.com/owner/repo/pull/42
    let stdout = String::from_utf8_lossy(&output.stdout);
    let number = stdout
        .trim()
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .context("failed to parse PR number from gh output")?;

    Ok(number)
}
