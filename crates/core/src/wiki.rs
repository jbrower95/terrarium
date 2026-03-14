use anyhow::{Context, Result};
use tokio::process::Command;

/// Clone the wiki repo into a temporary directory, returning the path.
///
/// GitHub wikis live at `https://github.com/{owner}/{repo}.wiki.git`.
/// The `GH_TOKEN` / `GITHUB_TOKEN` env var is used for auth.
async fn clone_wiki(repo_slug: &str) -> Result<tempfile::TempDir> {
    let tmp = tempfile::tempdir().context("failed to create temp dir for wiki")?;

    // Build authenticated URL using the token available in CI.
    let token = std::env::var("GH_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .unwrap_or_default();

    let url = if token.is_empty() {
        format!("https://github.com/{repo_slug}.wiki.git")
    } else {
        format!("https://x-access-token:{token}@github.com/{repo_slug}.wiki.git")
    };

    let output = Command::new("git")
        .args(["clone", "--depth", "1", &url, "."])
        .current_dir(tmp.path())
        .output()
        .await
        .context("failed to spawn git clone for wiki")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone wiki failed: {stderr}");
    }

    // Configure git identity for commits.
    let _ = Command::new("git")
        .args(["config", "user.name", "terrarium[bot]"])
        .current_dir(tmp.path())
        .output()
        .await;
    let _ = Command::new("git")
        .args(["config", "user.email", "terrarium[bot]@users.noreply.github.com"])
        .current_dir(tmp.path())
        .output()
        .await;

    Ok(tmp)
}

/// Write (or overwrite) a wiki page, commit, and push.
///
/// `page_name` is the page slug (e.g. `"Milestone-1"`). The file will be
/// written as `{page_name}.md` in the wiki repo root.
pub async fn update_wiki_page(repo_slug: &str, page_name: &str, content: &str) -> Result<()> {
    let wiki_dir = clone_wiki(repo_slug).await?;
    let page_path = wiki_dir.path().join(format!("{page_name}.md"));

    // Write the page content.
    tokio::fs::write(&page_path, content)
        .await
        .context("failed to write wiki page")?;

    // Stage and commit.
    let add_output = Command::new("git")
        .args(["add", &format!("{page_name}.md")])
        .current_dir(wiki_dir.path())
        .output()
        .await
        .context("failed to git add wiki page")?;

    if !add_output.status.success() {
        let stderr = String::from_utf8_lossy(&add_output.stderr);
        anyhow::bail!("git add wiki page failed: {stderr}");
    }

    // Check if there are changes to commit.
    let diff = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(wiki_dir.path())
        .output()
        .await
        .context("failed to git diff wiki")?;

    if diff.status.success() {
        // No changes — page content is identical.
        return Ok(());
    }

    let commit_output = Command::new("git")
        .args(["commit", "-m", &format!("Update {page_name}")])
        .current_dir(wiki_dir.path())
        .output()
        .await
        .context("failed to git commit wiki")?;

    if !commit_output.status.success() {
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        anyhow::bail!("git commit wiki failed: {stderr}");
    }

    // Push.
    let push_output = Command::new("git")
        .args(["push", "origin", "master"])
        .current_dir(wiki_dir.path())
        .output()
        .await
        .context("failed to git push wiki")?;

    if !push_output.status.success() {
        let stderr = String::from_utf8_lossy(&push_output.stderr);
        anyhow::bail!("git push wiki failed: {stderr}");
    }

    Ok(())
}

/// Render a milestone status page for the wiki.
///
/// Takes milestone info and its issues, produces markdown content.
pub fn render_milestone_page(
    milestone: &crate::milestones::Milestone,
    issues: &[crate::tasks::Issue],
    open_prs: &[crate::pr::PullRequest],
) -> String {
    let total = milestone.open_issues + milestone.closed_issues;
    let pct = if total > 0 {
        (milestone.closed_issues as f64 / total as f64 * 100.0) as u64
    } else {
        0
    };

    let mut out = String::with_capacity(2048);

    out.push_str(&format!("# {}\n\n", milestone.title));

    if !milestone.description.is_empty() {
        out.push_str(&milestone.description);
        out.push_str("\n\n");
    }

    // Progress bar
    out.push_str(&format!(
        "**Progress:** {}/{} issues complete ({}%)\n\n",
        milestone.closed_issues, total, pct
    ));

    // Issues table
    if !issues.is_empty() {
        out.push_str("## Issues\n\n");
        out.push_str("| # | Title | Status | Complexity |\n");
        out.push_str("|---|-------|--------|------------|\n");

        for issue in issues {
            let status = if issue.state == "closed" {
                "done"
            } else if issue.labels.iter().any(|l| l == "stuck") {
                "stuck"
            } else if issue.labels.iter().any(|l| l == "in-progress") {
                "in-progress"
            } else {
                "open"
            };

            let complexity = issue
                .labels
                .iter()
                .find(|l| l.starts_with("complexity:"))
                .map(|l| l.strip_prefix("complexity:").unwrap_or(l))
                .unwrap_or("unset");

            out.push_str(&format!(
                "| #{} | {} | {} | {} |\n",
                issue.number, issue.title, status, complexity
            ));
        }
        out.push('\n');
    }

    // Related PRs
    let related_prs: Vec<_> = open_prs
        .iter()
        .filter(|pr| {
            // Match PRs whose title references any issue in this milestone.
            issues.iter().any(|i| {
                pr.title.contains(&format!("#{}", i.number))
            })
        })
        .collect();

    if !related_prs.is_empty() {
        out.push_str("## Open PRs\n\n");
        for pr in &related_prs {
            out.push_str(&format!(
                "- PR #{}: {} (by {}, {})\n",
                pr.number, pr.title, pr.author, pr.diff_stats
            ));
        }
        out.push('\n');
    }

    // Footer
    out.push_str("---\n*Updated automatically by terrarium-owner*\n");

    out
}
