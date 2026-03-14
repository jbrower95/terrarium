use crate::{
    budget::Balance,
    milestones::Milestone,
    models::{self, ModelConfig},
    pr::PullRequest,
    spend::SpendSummary,
    tasks::Issue,
};

/// All data needed to render the owner prompt in a single inference call.
pub struct OwnerContext {
    pub repo_owner: String,
    pub repo_name: String,
    pub balance: Balance,
    pub openrouter_credits: f64,
    pub projected_days: f64,
    pub spend: SpendSummary,
    pub models: ModelConfig,
    pub auto_review: bool,
    pub milestones: Vec<(Milestone, Vec<Issue>)>,
    pub open_prs: Vec<PullRequest>,
    pub journal_context: String,
}

/// Render the pre-materialized owner prompt from the collected context.
pub fn materialize_owner_context(ctx: &OwnerContext) -> String {
    let mut out = String::with_capacity(8192);

    // Header
    out.push_str(&format!(
        "You are the autonomous owner of {}/{}.\n\n",
        ctx.repo_owner, ctx.repo_name
    ));

    // Budget
    out.push_str("## Budget\n");
    out.push_str(&format!(
        "Treasury: {:.4} ETH (${:.2})\n",
        ctx.balance.eth, ctx.balance.usd
    ));
    out.push_str(&format!(
        "OpenRouter credits: ${:.2}\n",
        ctx.openrouter_credits
    ));
    out.push_str(&format!(
        "Projected runway: {:.0} days\n\n",
        ctx.projected_days
    ));

    // Recent spend (24h)
    out.push_str("## Recent spend (last 24h)\n");
    out.push_str(&format!(
        "- Owner wake cycles: {} invocations, ${:.2}\n",
        ctx.spend.owner_runs_24h, ctx.spend.owner_spend_24h
    ));

    for (issue_num, cost, runs) in &ctx.spend.by_issue {
        out.push_str(&format!(
            "- Issue #{}: {} runs, ${:.2}\n",
            issue_num, runs, cost
        ));
    }

    out.push_str(&format!(
        "- Employee runs: {} invocations, ${:.2}\n",
        ctx.spend.employee_runs_24h, ctx.spend.employee_spend_24h
    ));
    out.push_str(&format!(
        "Total 24h: ${:.2}\n\n",
        ctx.spend.total_spend_24h
    ));

    // 7d spend
    out.push_str("## Spend (last 7d)\n");
    out.push_str(&format!(
        "Total: ${:.2} (${:.2}/day avg)\n\n",
        ctx.spend.total_spend_7d, ctx.spend.daily_avg_7d
    ));

    // Models
    out.push_str("## Models\n");
    render_model_tier(&mut out, "owner", &ctx.models.owner);
    render_model_tier(&mut out, "high", &ctx.models.high);
    render_model_tier(&mut out, "medium", &ctx.models.medium);
    render_model_tier(&mut out, "low", &ctx.models.low);
    out.push_str(&format!(
        "- Auto-review: {}\n\n",
        if ctx.auto_review {
            "enabled"
        } else {
            "disabled"
        }
    ));

    // Milestones
    out.push_str("## Milestones\n");
    if ctx.milestones.is_empty() {
        out.push_str("(No open milestones.)\n\n");
    }
    for (ms, issues) in &ctx.milestones {
        let total = ms.open_issues + ms.closed_issues;
        out.push_str(&format!(
            "### {} ({}/{} remaining)\n",
            ms.title, ms.open_issues, total
        ));
        if !ms.description.is_empty() {
            out.push_str(&ms.description);
            out.push('\n');
        }
        if issues.is_empty() {
            out.push_str("(No issues filed yet -- consider breaking this down.)\n");
        } else {
            out.push_str("  Issues:\n");
            for issue in issues {
                let stuck = issue.labels.iter().any(|l| l == "stuck");
                let in_progress = issue.labels.iter().any(|l| l == "in-progress");
                let complexity = issue
                    .labels
                    .iter()
                    .find(|l| {
                        l.as_str() == "complexity:low"
                            || l.as_str() == "complexity:medium"
                            || l.as_str() == "complexity:high"
                    })
                    .map(|l| l.as_str())
                    .unwrap_or("complexity:unset");

                let mut suffix = String::new();
                if stuck {
                    suffix.push_str(" !! STUCK");
                }
                if in_progress {
                    suffix.push_str(" [in-progress]");
                }
                out.push_str(&format!(
                    "  - #{}: {} [{}]{}\n",
                    issue.number, issue.title, complexity, suffix
                ));
            }
        }
        out.push('\n');
    }

    // Open PRs
    out.push_str(&format!("## Open PRs ({})\n", ctx.open_prs.len()));
    if ctx.open_prs.is_empty() {
        out.push_str("(No open PRs.)\n");
    }
    for pr in &ctx.open_prs {
        let mut health = String::new();
        if pr.mergeable == "CONFLICTING" {
            health.push_str(" !! CONFLICTS");
        }
        if pr.ci_status == "FAILURE" {
            health.push_str(" !! CI FAILING");
        } else if pr.ci_status == "PENDING" {
            health.push_str(" [ci pending]");
        }
        let is_terrarium = pr.head_ref.starts_with("terrarium/");
        if is_terrarium && pr.mergeable == "CONFLICTING" {
            health.push_str(" → use heal_branch");
        }
        out.push_str(&format!(
            "- #{}: {} (by {}, {}{})\n",
            pr.number, pr.title, pr.author, pr.diff_stats, health
        ));
    }
    out.push('\n');

    // Journal context
    out.push_str("## Journal (last 3 entries)\n");
    if ctx.journal_context.is_empty() {
        out.push_str("(No journal entries yet.)\n");
    } else {
        out.push_str(&ctx.journal_context);
    }
    out.push_str("\n\n---\n\n");

    // Action vocabulary
    out.push_str("Decide what to do this cycle. Respond with a JSON array of actions.\n\n");
    out.push_str("Available actions:\n");
    out.push_str(ACTION_VOCABULARY);
    out.push('\n');

    out
}

fn render_model_tier(out: &mut String, tier: &str, model_id: &Option<String>) {
    match model_id {
        Some(id) => {
            let cost_str = match models::lookup(id) {
                Some(entry) => format!(
                    "${:.2}/${:.2} per MTok in/out",
                    entry.cost_input, entry.cost_output
                ),
                None => "unknown model".to_string(),
            };
            out.push_str(&format!("- {tier}: {id} ({cost_str})\n"));
        }
        None => {
            out.push_str(&format!("- {tier}: unset\n"));
        }
    }
}

const ACTION_VOCABULARY: &str = r#"```json
[
  { "action": "file_issue", "title": "...", "body": "...", "complexity": "medium", "priority": "high", "milestone": "v0.1" },
  { "action": "dispatch_employee", "issue_number": 7, "complexity": "medium" },
  { "action": "set_model", "tier": "medium", "model": "openrouter/moonshotai/kimi-k2.5" },
  { "action": "set_auto_review", "enabled": true },
  { "action": "merge_pr", "pr_number": 12 },
  { "action": "request_changes", "pr_number": 13, "feedback": "..." },
  { "action": "comment_issue", "issue_number": 5, "body": "..." },
  { "action": "top_up", "amount_usd": 10 },
  { "action": "close_milestone", "milestone": "v0.1" },
  { "action": "stakeholder_update", "body": "..." },
  { "action": "journal", "body": "..." },
  { "action": "heal_branch", "pr_number": 35 }
]
```

**heal_branch**: Use this to rebase and fix merge conflicts on terrarium-created PRs that show `!! CONFLICTS`. Only use on PRs with branches starting with `terrarium/`."#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Balance;
    use crate::milestones::Milestone;
    use crate::spend::SpendSummary;

    fn sample_context() -> OwnerContext {
        OwnerContext {
            repo_owner: "jbrower95".into(),
            repo_name: "terrarium".into(),
            balance: Balance {
                eth: 0.02,
                usd: 45.20,
            },
            openrouter_credits: 12.50,
            projected_days: 21.0,
            spend: SpendSummary {
                owner_spend_24h: 0.15,
                owner_runs_24h: 1,
                employee_spend_24h: 0.90,
                employee_runs_24h: 3,
                total_spend_24h: 1.05,
                total_spend_7d: 7.35,
                daily_avg_7d: 1.05,
                by_issue: vec![(7, 0.50, 2), (12, 0.40, 1)],
            },
            models: ModelConfig {
                owner: Some("moonshotai/kimi-k2.5".into()),
                high: Some("moonshotai/kimi-k2.5".into()),
                medium: Some("qwen/qwen3.5-35b".into()),
                low: Some("qwen/qwen3.5-35b".into()),
            },
            auto_review: true,
            milestones: vec![
                (
                    Milestone {
                        number: 1,
                        title: "v0.1 -- Initial scaffolding".into(),
                        description: "Set up the project skeleton.".into(),
                        open_issues: 2,
                        closed_issues: 1,
                    },
                    vec![
                        Issue {
                            number: 7,
                            title: "Implement context module".into(),
                            body: String::new(),
                            labels: vec![
                                "complexity:medium".into(),
                                "in-progress".into(),
                            ],
                            milestone: Some("v0.1 -- Initial scaffolding".into()),
                            assignee: None,
                            state: "open".into(),
                        },
                        Issue {
                            number: 8,
                            title: "Fix stuck build".into(),
                            body: String::new(),
                            labels: vec!["complexity:low".into(), "stuck".into()],
                            milestone: Some("v0.1 -- Initial scaffolding".into()),
                            assignee: None,
                            state: "open".into(),
                        },
                    ],
                ),
            ],
            open_prs: vec![PullRequest {
                number: 18,
                title: "Add budget module".into(),
                author: "employee-bot".into(),
                head_ref: "terrarium/issue-5".into(),
                diff_stats: "+120 -30".into(),
                head_sha: "abc123".into(),
                mergeable: "MERGEABLE".into(),
                ci_status: "SUCCESS".into(),
            }],
            journal_context: "## 2026-03-13 14:30:00 UTC\n- Balance: $50.00\nMerged PR #17.\n"
                .into(),
        }
    }

    #[test]
    fn prompt_contains_header() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("autonomous owner of jbrower95/terrarium"));
    }

    #[test]
    fn prompt_contains_budget() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("Treasury: 0.0200 ETH ($45.20)"));
        assert!(prompt.contains("OpenRouter credits: $12.50"));
        assert!(prompt.contains("Projected runway: 21 days"));
    }

    #[test]
    fn prompt_contains_spend() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("Owner wake cycles: 1 invocations, $0.15"));
        assert!(prompt.contains("Issue #7: 2 runs, $0.50"));
        assert!(prompt.contains("Total 24h: $1.05"));
        assert!(prompt.contains("$7.35 ($1.05/day avg)"));
    }

    #[test]
    fn prompt_contains_models() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("owner: moonshotai/kimi-k2.5"));
        assert!(prompt.contains("medium: qwen/qwen3.5-35b"));
        assert!(prompt.contains("Auto-review: enabled"));
    }

    #[test]
    fn prompt_contains_milestones() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("v0.1 -- Initial scaffolding (2/3 remaining)"));
        assert!(prompt.contains("#7: Implement context module [complexity:medium] [in-progress]"));
        assert!(prompt.contains("#8: Fix stuck build [complexity:low] !! STUCK"));
    }

    #[test]
    fn prompt_contains_prs() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("Open PRs (1)"));
        assert!(prompt.contains("#18: Add budget module (by employee-bot, +120 -30)"));
    }

    #[test]
    fn prompt_contains_journal() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("Merged PR #17"));
    }

    #[test]
    fn prompt_contains_action_vocabulary() {
        let prompt = materialize_owner_context(&sample_context());
        assert!(prompt.contains("file_issue"));
        assert!(prompt.contains("dispatch_employee"));
        assert!(prompt.contains("merge_pr"));
        assert!(prompt.contains("Respond with a JSON array of actions"));
    }

    #[test]
    fn prompt_empty_milestones() {
        let mut ctx = sample_context();
        ctx.milestones = vec![];
        let prompt = materialize_owner_context(&ctx);
        assert!(prompt.contains("No open milestones"));
    }

    #[test]
    fn prompt_empty_prs() {
        let mut ctx = sample_context();
        ctx.open_prs = vec![];
        let prompt = materialize_owner_context(&ctx);
        assert!(prompt.contains("Open PRs (0)"));
        assert!(prompt.contains("No open PRs"));
    }

    #[test]
    fn prompt_unset_model() {
        let mut ctx = sample_context();
        ctx.models.low = None;
        let prompt = materialize_owner_context(&ctx);
        assert!(prompt.contains("low: unset"));
    }
}
