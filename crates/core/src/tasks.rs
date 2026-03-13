use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub milestone: Option<String>,
    pub assignee: Option<String>,
    pub state: String,
}

/// Intermediate struct for deserializing `gh issue list --json` output.
#[derive(Debug, Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    body: String,
    labels: Vec<GhLabel>,
    milestone: Option<GhMilestone>,
    assignees: Vec<GhAssignee>,
    state: String,
}

#[derive(Debug, Deserialize)]
struct GhLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GhMilestone {
    title: String,
}

#[derive(Debug, Deserialize)]
struct GhAssignee {
    login: String,
}

impl From<GhIssue> for Issue {
    fn from(gh: GhIssue) -> Self {
        Issue {
            number: gh.number,
            title: gh.title,
            body: gh.body,
            labels: gh.labels.into_iter().map(|l| l.name).collect(),
            milestone: gh.milestone.map(|m| m.title),
            assignee: gh.assignees.into_iter().next().map(|a| a.login),
            state: gh.state,
        }
    }
}

/// Create a new GitHub issue. Returns the issue number.
pub async fn create_issue(
    title: &str,
    body: &str,
    labels: &[&str],
    milestone: Option<&str>,
) -> Result<u64> {
    let mut args = vec![
        "issue".to_string(),
        "create".to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body".to_string(),
        body.to_string(),
    ];

    if !labels.is_empty() {
        args.push("--label".to_string());
        args.push(labels.join(","));
    }

    if let Some(ms) = milestone {
        args.push("--milestone".to_string());
        args.push(ms.to_string());
    }

    let output = Command::new("gh")
        .args(&args)
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue create failed: {stderr}");
    }

    // gh issue create prints a URL like https://github.com/owner/repo/issues/42
    let stdout = String::from_utf8_lossy(&output.stdout);
    let number = stdout
        .trim()
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .context("failed to parse issue number from gh output")?;

    Ok(number)
}

/// List open issues, optionally filtered by milestone title.
pub async fn list_open_issues(milestone: Option<&str>) -> Result<Vec<Issue>> {
    let fields = "number,title,body,labels,milestone,assignees,state";

    let mut args = vec![
        "issue",
        "list",
        "--state",
        "open",
        "--json",
        fields,
        "--limit",
        "200",
    ];

    let ms_string;
    if let Some(ms) = milestone {
        ms_string = ms.to_string();
        args.push("--milestone");
        args.push(&ms_string);
    }

    let output = Command::new("gh")
        .args(&args)
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue list failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let gh_issues: Vec<GhIssue> =
        serde_json::from_str(&stdout).context("failed to parse gh issue list output")?;

    Ok(gh_issues.into_iter().map(Issue::from).collect())
}

/// Add a label to an issue.
pub async fn add_label(issue: u64, label: &str) -> Result<()> {
    let issue_str = issue.to_string();
    let output = Command::new("gh")
        .args(["issue", "edit", &issue_str, "--add-label", label])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue edit --add-label failed: {stderr}");
    }

    Ok(())
}

/// Remove a label from an issue.
pub async fn remove_label(issue: u64, label: &str) -> Result<()> {
    let issue_str = issue.to_string();
    let output = Command::new("gh")
        .args(["issue", "edit", &issue_str, "--remove-label", label])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue edit --remove-label failed: {stderr}");
    }

    Ok(())
}

/// Add a comment to an issue.
pub async fn add_comment(issue: u64, body: &str) -> Result<()> {
    let issue_str = issue.to_string();
    let output = Command::new("gh")
        .args(["issue", "comment", &issue_str, "--body", body])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue comment failed: {stderr}");
    }

    Ok(())
}

/// Close an issue.
pub async fn close_issue(issue: u64) -> Result<()> {
    let issue_str = issue.to_string();
    let output = Command::new("gh")
        .args(["issue", "close", &issue_str])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue close failed: {stderr}");
    }

    Ok(())
}
