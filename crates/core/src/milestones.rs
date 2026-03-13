use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::tasks::Issue;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub number: u64,
    pub title: String,
    pub description: String,
    pub open_issues: u64,
    pub closed_issues: u64,
}

/// Intermediate struct for deserializing `gh api milestones` output.
#[derive(Debug, Deserialize)]
struct GhMilestone {
    number: u64,
    title: String,
    description: Option<String>,
    open_issues: u64,
    closed_issues: u64,
}

impl From<GhMilestone> for Milestone {
    fn from(gh: GhMilestone) -> Self {
        Milestone {
            number: gh.number,
            title: gh.title,
            description: gh.description.unwrap_or_default(),
            open_issues: gh.open_issues,
            closed_issues: gh.closed_issues,
        }
    }
}

/// List all open milestones for the current repository.
pub async fn list_open_milestones() -> Result<Vec<Milestone>> {
    let output = Command::new("gh")
        .args([
            "api",
            "repos/{owner}/{repo}/milestones",
            "--jq",
            ".",
        ])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api milestones failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let gh_milestones: Vec<GhMilestone> =
        serde_json::from_str(&stdout).context("failed to parse milestones response")?;

    Ok(gh_milestones.into_iter().map(Milestone::from).collect())
}

/// Create a new milestone. Returns the milestone number.
pub async fn create_milestone(title: &str, description: &str) -> Result<u64> {
    let body = serde_json::json!({
        "title": title,
        "description": description,
    });

    let mut child = Command::new("gh")
        .args([
            "api",
            "repos/{owner}/{repo}/milestones",
            "--method",
            "POST",
            "--input",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn gh")?;

    // Write the JSON body to stdin.
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(body.to_string().as_bytes())
            .await
            .context("failed to write to gh stdin")?;
        // Drop stdin to close it so gh can proceed.
    }

    let output = child
        .wait_with_output()
        .await
        .context("failed to wait on gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api milestones POST failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let created: GhMilestone =
        serde_json::from_str(&stdout).context("failed to parse created milestone")?;

    Ok(created.number)
}

/// Close a milestone by number.
pub async fn close_milestone(number: u64) -> Result<()> {
    let body = serde_json::json!({ "state": "closed" });
    let endpoint = format!("repos/{{owner}}/{{repo}}/milestones/{number}");

    let mut child = Command::new("gh")
        .args(["api", &endpoint, "--method", "PATCH", "--input", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn gh")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(body.to_string().as_bytes())
            .await
            .context("failed to write to gh stdin")?;
    }

    let output = child
        .wait_with_output()
        .await
        .context("failed to wait on gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api milestones PATCH failed: {stderr}");
    }

    Ok(())
}

/// Get all issues associated with a milestone (by milestone number).
/// Resolves the milestone title first, then uses `gh issue list --milestone`.
pub async fn get_milestone_issues(number: u64) -> Result<Vec<Issue>> {
    // First, get the milestone title from its number.
    let endpoint = format!("repos/{{owner}}/{{repo}}/milestones/{number}");
    let output = Command::new("gh")
        .args(["api", &endpoint, "--jq", ".title"])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api milestone GET failed: {stderr}");
    }

    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if title.is_empty() {
        anyhow::bail!("milestone {number} not found or has empty title");
    }

    // Now list issues with that milestone title (includes both open and closed).
    let fields = "number,title,body,labels,milestone,assignees,state";
    let output = Command::new("gh")
        .args([
            "issue",
            "list",
            "--milestone",
            &title,
            "--state",
            "all",
            "--json",
            fields,
            "--limit",
            "200",
        ])
        .output()
        .await
        .context("failed to spawn gh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue list --milestone failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Reuse the GhIssue type from tasks — but it's private there, so we
    // define a local equivalent for deserialization.
    #[derive(Debug, Deserialize)]
    struct GhLabel {
        name: String,
    }
    #[derive(Debug, Deserialize)]
    struct GhMs {
        title: String,
    }
    #[derive(Debug, Deserialize)]
    struct GhAssignee {
        login: String,
    }
    #[derive(Debug, Deserialize)]
    struct GhIssue {
        number: u64,
        title: String,
        body: String,
        labels: Vec<GhLabel>,
        milestone: Option<GhMs>,
        assignees: Vec<GhAssignee>,
        state: String,
    }

    let gh_issues: Vec<GhIssue> =
        serde_json::from_str(&stdout).context("failed to parse gh issue list output")?;

    let issues = gh_issues
        .into_iter()
        .map(|gh| Issue {
            number: gh.number,
            title: gh.title,
            body: gh.body,
            labels: gh.labels.into_iter().map(|l| l.name).collect(),
            milestone: gh.milestone.map(|m| m.title),
            assignee: gh.assignees.into_iter().next().map(|a| a.login),
            state: gh.state,
        })
        .collect();

    Ok(issues)
}
