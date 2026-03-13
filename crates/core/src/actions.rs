use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{milestones, pr, tasks};

/// A single action the owner can take during a cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    FileIssue {
        title: String,
        body: String,
        complexity: String,
        priority: String,
        milestone: String,
    },
    DispatchEmployee {
        issue_number: u64,
        complexity: String,
    },
    SetModel {
        tier: String,
        model: String,
    },
    SetAutoReview {
        enabled: bool,
    },
    MergePr {
        pr_number: u64,
    },
    RequestChanges {
        pr_number: u64,
        feedback: String,
    },
    CommentIssue {
        issue_number: u64,
        body: String,
    },
    TopUp {
        amount_usd: f64,
    },
    CloseMilestone {
        milestone: String,
    },
    StakeholderUpdate {
        body: String,
    },
    Journal {
        body: String,
    },
}

/// The result of executing a single action.
#[derive(Debug, Clone)]
pub struct ActionResult {
    pub action: String,
    pub success: bool,
    pub detail: String,
}

impl ActionResult {
    fn ok(action: &str, detail: impl Into<String>) -> Self {
        ActionResult {
            action: action.to_string(),
            success: true,
            detail: detail.into(),
        }
    }

    fn err(action: &str, detail: impl Into<String>) -> Self {
        ActionResult {
            action: action.to_string(),
            success: false,
            detail: detail.into(),
        }
    }
}

/// Parse a JSON string into a list of owner actions.
///
/// The input should be a JSON array of action objects. If the model wraps
/// the array in a top-level object with an `"actions"` key, that is also
/// accepted.
pub fn parse_actions(json: &str) -> Result<Vec<Action>> {
    let trimmed = json.trim();

    // Try parsing as a direct array first.
    if let Ok(actions) = serde_json::from_str::<Vec<Action>>(trimmed) {
        return Ok(actions);
    }

    // Some models wrap the array in an object: { "actions": [...] }
    #[derive(Deserialize)]
    struct Wrapper {
        actions: Vec<Action>,
    }

    if let Ok(wrapper) = serde_json::from_str::<Wrapper>(trimmed) {
        return Ok(wrapper.actions);
    }

    // Try extracting a JSON array from within markdown code fences.
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            let slice = &trimmed[start..=end];
            if let Ok(actions) = serde_json::from_str::<Vec<Action>>(slice) {
                return Ok(actions);
            }
        }
    }

    anyhow::bail!("failed to parse action list from model response")
}

/// Execute a list of owner actions sequentially, collecting results.
pub async fn execute_actions(actions: &[Action]) -> Vec<ActionResult> {
    let mut results = Vec::with_capacity(actions.len());

    for action in actions {
        let result = execute_one(action).await;
        results.push(result);
    }

    results
}

async fn execute_one(action: &Action) -> ActionResult {
    match action {
        Action::FileIssue {
            title,
            body,
            complexity,
            priority,
            milestone,
        } => {
            let labels: Vec<&str> = vec![
                leak_label(&format!("complexity:{complexity}")),
                leak_label(&format!("priority:{priority}")),
            ];
            match tasks::create_issue(title, body, &labels, Some(milestone)).await {
                Ok(num) => ActionResult::ok(
                    "file_issue",
                    format!("created issue #{num}"),
                ),
                Err(e) => ActionResult::err("file_issue", format!("{e:#}")),
            }
        }

        Action::DispatchEmployee {
            issue_number,
            complexity,
        } => match dispatch_employee_workflow(*issue_number, complexity).await {
            Ok(()) => ActionResult::ok(
                "dispatch_employee",
                format!("dispatched employee for issue #{issue_number}"),
            ),
            Err(e) => ActionResult::err("dispatch_employee", format!("{e:#}")),
        },

        Action::SetModel { tier, model } => {
            let var_name = format!("TERRARIUM_MODEL_{}", tier.to_uppercase());
            match set_gh_variable(&var_name, model).await {
                Ok(()) => ActionResult::ok(
                    "set_model",
                    format!("set {tier} model to {model}"),
                ),
                Err(e) => ActionResult::err("set_model", format!("{e:#}")),
            }
        }

        Action::SetAutoReview { enabled } => {
            let value = if *enabled { "true" } else { "false" };
            match set_gh_variable("TERRARIUM_AUTO_REVIEW", value).await {
                Ok(()) => ActionResult::ok(
                    "set_auto_review",
                    format!("auto-review set to {value}"),
                ),
                Err(e) => ActionResult::err("set_auto_review", format!("{e:#}")),
            }
        }

        Action::MergePr { pr_number } => match pr::merge_pr(*pr_number).await {
            Ok(()) => ActionResult::ok(
                "merge_pr",
                format!("merged PR #{pr_number}"),
            ),
            Err(e) => ActionResult::err("merge_pr", format!("{e:#}")),
        },

        Action::RequestChanges {
            pr_number,
            feedback,
        } => match pr::request_changes(*pr_number, feedback).await {
            Ok(()) => ActionResult::ok(
                "request_changes",
                format!("requested changes on PR #{pr_number}"),
            ),
            Err(e) => ActionResult::err("request_changes", format!("{e:#}")),
        },

        Action::CommentIssue { issue_number, body } => {
            match tasks::add_comment(*issue_number, body).await {
                Ok(()) => ActionResult::ok(
                    "comment_issue",
                    format!("commented on issue #{issue_number}"),
                ),
                Err(e) => ActionResult::err("comment_issue", format!("{e:#}")),
            }
        }

        Action::TopUp { amount_usd } => {
            // TODO: Implement wallet top-up via OIDC wallet transaction.
            // This requires building calldata from budget::build_topup_calldata
            // and signing + broadcasting via the ERC-4337 wallet module.
            ActionResult::ok(
                "top_up",
                format!("top_up ${:.2} -- not yet implemented (wallet module pending)", amount_usd),
            )
        }

        Action::CloseMilestone { milestone } => {
            match close_milestone_by_title(milestone).await {
                Ok(num) => ActionResult::ok(
                    "close_milestone",
                    format!("closed milestone \"{milestone}\" (#{num})"),
                ),
                Err(e) => ActionResult::err("close_milestone", format!("{e:#}")),
            }
        }

        Action::StakeholderUpdate { body } => {
            // Post a stakeholder update as a repo discussion or issue comment.
            // For now, create a special issue labeled "stakeholder-update".
            let labels: Vec<&str> = vec!["stakeholder-update"];
            match tasks::create_issue("Stakeholder Update", body, &labels, None).await {
                Ok(num) => ActionResult::ok(
                    "stakeholder_update",
                    format!("posted stakeholder update as issue #{num}"),
                ),
                Err(e) => ActionResult::err("stakeholder_update", format!("{e:#}")),
            }
        }

        Action::Journal { body } => {
            // The journal entry will be written by the owner binary after
            // collecting all action results. We record the body here so the
            // caller can include it.
            ActionResult::ok(
                "journal",
                format!("journal entry recorded: {}", truncate(body, 80)),
            )
        }
    }
}

/// Dispatch the employee workflow for a given issue via `gh workflow dispatch`.
async fn dispatch_employee_workflow(issue_number: u64, complexity: &str) -> Result<()> {
    let output = Command::new("gh")
        .args([
            "workflow",
            "run",
            "employee.yml",
            "--field",
            &format!("issue_number={issue_number}"),
            "--field",
            &format!("complexity={complexity}"),
        ])
        .output()
        .await
        .context("failed to spawn gh workflow run")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh workflow run failed: {stderr}");
    }

    Ok(())
}

/// Set a GitHub repository variable via `gh variable set`.
async fn set_gh_variable(name: &str, value: &str) -> Result<()> {
    let output = Command::new("gh")
        .args(["variable", "set", name, "--body", value])
        .output()
        .await
        .context("failed to spawn gh variable set")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh variable set failed: {stderr}");
    }

    Ok(())
}

/// Find a milestone by title and close it.
async fn close_milestone_by_title(title: &str) -> Result<u64> {
    let milestones = milestones::list_open_milestones().await?;
    let ms = milestones
        .iter()
        .find(|m| m.title == title)
        .with_context(|| format!("milestone \"{title}\" not found"))?;
    let number = ms.number;
    milestones::close_milestone(number).await?;
    Ok(number)
}

/// Helper: allocate a label string that lives for the duration of the process.
///
/// `tasks::create_issue` takes `&[&str]`, so we need string references that
/// outlive the function scope.  This leaks a small amount of memory per cycle
/// which is acceptable for a short-lived GHA binary.
fn leak_label(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Truncate a string to `max` characters, appending "..." if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_direct_array() {
        let json = r#"[
            { "action": "merge_pr", "pr_number": 12 },
            { "action": "journal", "body": "cycle complete" }
        ]"#;
        let actions = parse_actions(json).unwrap();
        assert_eq!(actions.len(), 2);
        match &actions[0] {
            Action::MergePr { pr_number } => assert_eq!(*pr_number, 12),
            other => panic!("expected MergePr, got {:?}", other),
        }
    }

    #[test]
    fn parse_wrapped_object() {
        let json = r#"{ "actions": [{ "action": "set_auto_review", "enabled": false }] }"#;
        let actions = parse_actions(json).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::SetAutoReview { enabled } => assert!(!enabled),
            other => panic!("expected SetAutoReview, got {:?}", other),
        }
    }

    #[test]
    fn parse_code_fenced() {
        let json = "Here is my response:\n```json\n[\n  { \"action\": \"journal\", \"body\": \"all done\" }\n]\n```\n";
        let actions = parse_actions(json).unwrap();
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn parse_all_action_types() {
        let json = r#"[
            { "action": "file_issue", "title": "t", "body": "b", "complexity": "medium", "priority": "high", "milestone": "v1" },
            { "action": "dispatch_employee", "issue_number": 7, "complexity": "medium" },
            { "action": "set_model", "tier": "low", "model": "qwen/qwen3.5-35b" },
            { "action": "set_auto_review", "enabled": true },
            { "action": "merge_pr", "pr_number": 5 },
            { "action": "request_changes", "pr_number": 6, "feedback": "fix tests" },
            { "action": "comment_issue", "issue_number": 3, "body": "looking good" },
            { "action": "top_up", "amount_usd": 10.0 },
            { "action": "close_milestone", "milestone": "v0.1" },
            { "action": "stakeholder_update", "body": "shipped v0.1" },
            { "action": "journal", "body": "cycle 42 complete" }
        ]"#;
        let actions = parse_actions(json).unwrap();
        assert_eq!(actions.len(), 11);
    }

    #[test]
    fn parse_invalid_json_fails() {
        assert!(parse_actions("not json at all").is_err());
    }

    #[test]
    fn parse_empty_array() {
        let actions = parse_actions("[]").unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn action_result_helpers() {
        let ok = ActionResult::ok("test", "worked");
        assert!(ok.success);
        assert_eq!(ok.action, "test");

        let err = ActionResult::err("test", "failed");
        assert!(!err.success);
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let long = "a".repeat(100);
        let result = truncate(&long, 10);
        assert_eq!(result.len(), 13); // 10 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn action_roundtrip_serde() {
        let action = Action::FileIssue {
            title: "Test".into(),
            body: "Body".into(),
            complexity: "medium".into(),
            priority: "high".into(),
            milestone: "v1".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"file_issue\""));
        let parsed: Action = serde_json::from_str(&json).unwrap();
        match parsed {
            Action::FileIssue { title, .. } => assert_eq!(title, "Test"),
            _ => panic!("wrong variant"),
        }
    }
}
