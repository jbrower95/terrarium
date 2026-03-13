use std::path::Path;

use anyhow::{Context, Result};

/// Simple model configuration with four tier slots.
#[derive(Debug, Clone, Default)]
pub struct ModelConfig {
    pub owner: Option<String>,
    pub high: Option<String>,
    pub medium: Option<String>,
    pub low: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StatusData {
    pub wallet_address: String,
    pub token_address: Option<String>,
    pub balance_eth: f64,
    pub balance_usd: f64,
    pub openrouter_credits: f64,
    pub daily_run_rate: f64,
    pub projected_days: f64,
    pub models: ModelConfig,
    pub auto_review: bool,
    pub open_issues: u64,
    pub open_prs: u64,
    pub total_merged: u64,
    pub zora_url: Option<String>,
    pub basescan_url: String,
}

const STATUS_START: &str = "<!-- terrarium-status-start -->";
const STATUS_END: &str = "<!-- terrarium-status-end -->";

/// Build a shields.io badge in markdown.
fn badge(label: &str, message: &str, color: &str) -> String {
    let label_enc = urlencoding(label);
    let msg_enc = urlencoding(message);
    format!(
        "![{label}](https://img.shields.io/badge/{label_enc}-{msg_enc}-{color})",
    )
}

/// Minimal percent-encoding for shields.io URL path segments.
fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('-', "--")
        .replace('$', "%24")
        .replace('/', "%2F")
}

/// Render the full status block as markdown (without the sentinel comments).
pub fn render_status_markdown(data: &StatusData) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Badges row
    let treasury_badge = badge(
        "treasury",
        &format!("${:.2}", data.balance_usd),
        if data.balance_usd > 20.0 {
            "brightgreen"
        } else if data.balance_usd > 5.0 {
            "yellow"
        } else {
            "red"
        },
    );

    let runway_badge = badge(
        "runway",
        &format!("{} days", data.projected_days as u64),
        if data.projected_days > 14.0 {
            "brightgreen"
        } else if data.projected_days > 3.0 {
            "yellow"
        } else {
            "red"
        },
    );

    let burn_badge = badge(
        "burn rate",
        &format!("${:.2}/day", data.daily_run_rate),
        "blue",
    );

    let credits_badge = badge(
        "credits",
        &format!("${:.2}", data.openrouter_credits),
        "purple",
    );

    lines.push(format!(
        "{treasury_badge} {runway_badge} {burn_badge} {credits_badge}"
    ));
    lines.push(String::new());

    // Model tiers
    let model_label = |tier: &str, val: &Option<String>| -> String {
        match val {
            Some(m) => badge(tier, m, "informational"),
            None => badge(tier, "unset", "lightgrey"),
        }
    };

    lines.push(format!(
        "{} {} {} {}",
        model_label("owner", &data.models.owner),
        model_label("high", &data.models.high),
        model_label("medium", &data.models.medium),
        model_label("low", &data.models.low),
    ));
    lines.push(String::new());

    // Details
    lines.push(format!(
        "| Wallet | `{}` | [Basescan]({}) |",
        data.wallet_address, data.basescan_url
    ));
    lines.push("|--------|-------|------------|".into());

    if let Some(ref token) = data.token_address {
        lines.push(format!("| Token | `{token}` | |"));
    }

    if let Some(ref url) = data.zora_url {
        lines.push(format!("| Zora | [link]({url}) | |"));
    }

    lines.push(format!(
        "| Auto-review | {} | |",
        if data.auto_review {
            "enabled"
        } else {
            "disabled"
        }
    ));

    lines.push(format!(
        "| Issues | {} open | {} PRs open, {} merged |",
        data.open_issues, data.open_prs, data.total_merged
    ));

    lines.join("\n")
}

/// Update README.md in the repo root by replacing the status block between
/// the sentinel markers. If the markers are missing, the block is inserted
/// after the first heading.
pub async fn update_readme_status(repo_root: &Path, data: &StatusData) -> Result<()> {
    let readme_path = repo_root.join("README.md");

    let content = if readme_path.exists() {
        tokio::fs::read_to_string(&readme_path)
            .await
            .context("failed to read README.md")?
    } else {
        "# Terrarium\n".to_string()
    };

    let status_block = format!(
        "{STATUS_START}\n{}\n{STATUS_END}",
        render_status_markdown(data)
    );

    let new_content = if let (Some(start_idx), Some(end_idx)) = (
        content.find(STATUS_START),
        content.find(STATUS_END),
    ) {
        // Replace existing block.
        let before = &content[..start_idx];
        let after = &content[end_idx + STATUS_END.len()..];
        format!("{before}{status_block}{after}")
    } else {
        // Insert after first heading.
        insert_after_first_heading(&content, &status_block)
    };

    tokio::fs::write(&readme_path, new_content)
        .await
        .context("failed to write README.md")?;

    Ok(())
}

/// Insert `block` after the first markdown heading line.
fn insert_after_first_heading(content: &str, block: &str) -> String {
    // Find the end of the first line that starts with '#'.
    for (i, line) in content.lines().enumerate() {
        if line.starts_with('#') {
            let byte_offset = content
                .lines()
                .take(i + 1)
                .map(|l| l.len() + 1) // +1 for newline
                .sum::<usize>();
            let (before, after) = content.split_at(byte_offset.min(content.len()));
            return format!("{before}\n{block}\n{after}");
        }
    }

    // No heading found, prepend.
    format!("{block}\n\n{content}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data() -> StatusData {
        StatusData {
            wallet_address: "0xABCD...1234".into(),
            token_address: None,
            balance_eth: 0.02,
            balance_usd: 45.20,
            openrouter_credits: 12.50,
            daily_run_rate: 2.10,
            projected_days: 21.0,
            models: ModelConfig {
                owner: Some("kimi-k2.5".into()),
                high: Some("kimi-k2.5".into()),
                medium: Some("qwen3.5".into()),
                low: Some("qwen3.5".into()),
            },
            auto_review: true,
            open_issues: 5,
            open_prs: 2,
            total_merged: 12,
            zora_url: None,
            basescan_url: "https://basescan.org/address/0xABCD".into(),
        }
    }

    #[test]
    fn test_render_badges() {
        let md = render_status_markdown(&sample_data());
        assert!(md.contains("img.shields.io/badge/treasury"));
        assert!(md.contains("img.shields.io/badge/runway"));
        assert!(md.contains("img.shields.io/badge/burn%20rate"));
        assert!(md.contains("img.shields.io/badge/credits"));
        assert!(md.contains("Auto-review | enabled"));
    }

    #[tokio::test]
    async fn test_update_readme_with_markers() {
        let dir = tempfile::tempdir().unwrap();
        let readme = dir.path().join("README.md");
        let initial = format!(
            "# My Project\n\nSome intro text.\n\n{STATUS_START}\nold status\n{STATUS_END}\n\nMore text.\n"
        );
        tokio::fs::write(&readme, initial.as_bytes()).await.unwrap();

        update_readme_status(dir.path(), &sample_data())
            .await
            .unwrap();

        let result: String = tokio::fs::read_to_string(&readme).await.unwrap();
        assert!(result.contains("img.shields.io/badge/treasury"));
        assert!(!result.contains("old status"));
        assert!(result.contains("More text."));
    }

    #[tokio::test]
    async fn test_update_readme_insert_after_heading() {
        let dir = tempfile::tempdir().unwrap();
        let readme = dir.path().join("README.md");
        tokio::fs::write(&readme, b"# My Project\n\nSome text.\n")
            .await
            .unwrap();

        update_readme_status(dir.path(), &sample_data())
            .await
            .unwrap();

        let result: String = tokio::fs::read_to_string(&readme).await.unwrap();
        assert!(result.starts_with("# My Project\n"));
        assert!(result.contains(STATUS_START));
        assert!(result.contains(STATUS_END));
        assert!(result.contains("Some text."));
    }
}
