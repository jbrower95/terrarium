use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use chrono::Utc;

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
    /// Repository slug (e.g. "owner/repo") for SVG card URL in markdown.
    pub repo_slug: Option<String>,
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

    // SVG card reference (if repo_slug is available).
    if let Some(ref slug) = data.repo_slug {
        lines.push(format!(
            "![Status](https://raw.githubusercontent.com/{slug}/status/terrarium-status.svg)"
        ));
        lines.push(String::new());
    }

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

// ── SVG status card ──────────────────────────────────────────────────

/// Render a self-contained SVG status card (~400x300px) for the project.
pub fn render_status_svg(data: &StatusData) -> String {
    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();

    // Color-code treasury balance.
    let treasury_color = if data.balance_usd > 50.0 {
        "#4ade80" // green
    } else if data.balance_usd > 10.0 {
        "#facc15" // yellow
    } else {
        "#f87171" // red
    };

    // Runway display.
    let runway_text = if data.projected_days.is_infinite() || data.projected_days > 9999.0 {
        "\u{221e}".to_string() // infinity symbol
    } else {
        format!("{} days", data.projected_days as u64)
    };

    let runway_color = if data.projected_days > 14.0 || data.projected_days.is_infinite() {
        "#4ade80"
    } else if data.projected_days > 3.0 {
        "#facc15"
    } else {
        "#f87171"
    };

    // Model tier short names (strip provider prefix).
    let short_model = |m: &Option<String>| -> String {
        match m {
            Some(s) => s.rsplit('/').next().unwrap_or(s).to_string(),
            None => "unset".to_string(),
        }
    };

    let owner_model = short_model(&data.models.owner);
    let high_model = short_model(&data.models.high);
    let medium_model = short_model(&data.models.medium);
    let low_model = short_model(&data.models.low);

    let auto_review_text = if data.auto_review { "ON" } else { "OFF" };
    let auto_review_color = if data.auto_review { "#4ade80" } else { "#94a3b8" };

    // XML-escape helper for model names that might contain special chars.
    let esc = |s: &str| -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="310" viewBox="0 0 400 310">
  <defs>
    <style>
      .bg {{ fill: #1a1a2e; rx: 12; ry: 12; }}
      .title {{ font-family: -apple-system, 'Segoe UI', sans-serif; font-size: 18px; font-weight: 700; fill: #e2e8f0; }}
      .label {{ font-family: -apple-system, 'Segoe UI', sans-serif; font-size: 11px; fill: #94a3b8; text-transform: uppercase; letter-spacing: 0.05em; }}
      .value {{ font-family: 'SF Mono', 'Fira Code', 'Courier New', monospace; font-size: 14px; fill: #f1f5f9; }}
      .value-lg {{ font-family: 'SF Mono', 'Fira Code', 'Courier New', monospace; font-size: 22px; font-weight: 700; }}
      .model-label {{ font-family: -apple-system, 'Segoe UI', sans-serif; font-size: 10px; fill: #64748b; }}
      .model-value {{ font-family: 'SF Mono', 'Fira Code', 'Courier New', monospace; font-size: 11px; fill: #cbd5e1; }}
      .footer {{ font-family: -apple-system, 'Segoe UI', sans-serif; font-size: 9px; fill: #475569; }}
      .divider {{ stroke: #2d2d4a; stroke-width: 1; }}
      .badge-bg {{ rx: 4; ry: 4; }}
    </style>
  </defs>

  <!-- Background -->
  <rect class="bg" x="0" y="0" width="400" height="310" rx="12" ry="12"/>

  <!-- Header -->
  <text class="title" x="20" y="32">{plant} terrarium</text>

  <!-- Divider -->
  <line class="divider" x1="20" y1="44" x2="380" y2="44"/>

  <!-- Treasury (large) -->
  <text class="label" x="20" y="66">Treasury</text>
  <text class="value-lg" x="20" y="92" fill="{treasury_color}">${treasury:.2}</text>

  <!-- Runway + Burn rate + Credits (row) -->
  <text class="label" x="200" y="66">Runway</text>
  <text class="value" x="200" y="82" fill="{runway_color}">{runway_text}</text>

  <text class="label" x="300" y="66">Burn</text>
  <text class="value" x="300" y="82" fill="#94a3b8">${burn:.2}/d</text>

  <text class="label" x="200" y="100">Credits</text>
  <text class="value" x="200" y="116" fill="#c4b5fd">${credits:.2}</text>

  <!-- Divider -->
  <line class="divider" x1="20" y1="128" x2="380" y2="128"/>

  <!-- Model tiers -->
  <text class="label" x="20" y="148">Models</text>

  <text class="model-label" x="20" y="166">owner</text>
  <text class="model-value" x="70" y="166">{owner_model}</text>

  <text class="model-label" x="200" y="166">high</text>
  <text class="model-value" x="240" y="166">{high_model}</text>

  <text class="model-label" x="20" y="184">medium</text>
  <text class="model-value" x="70" y="184">{medium_model}</text>

  <text class="model-label" x="200" y="184">low</text>
  <text class="model-value" x="240" y="184">{low_model}</text>

  <!-- Divider -->
  <line class="divider" x1="20" y1="196" x2="380" y2="196"/>

  <!-- Activity stats -->
  <text class="label" x="20" y="216">Activity</text>

  <!-- Issues badge -->
  <rect class="badge-bg" x="20" y="224" width="70" height="22" fill="#1e3a5f"/>
  <text class="value" x="55" y="239" text-anchor="middle" font-size="11">{open_issues} issues</text>

  <!-- PRs badge -->
  <rect class="badge-bg" x="100" y="224" width="70" height="22" fill="#1e3a5f"/>
  <text class="value" x="135" y="239" text-anchor="middle" font-size="11">{open_prs} PRs</text>

  <!-- Merged badge -->
  <rect class="badge-bg" x="180" y="224" width="80" height="22" fill="#2d1b4e"/>
  <text class="value" x="220" y="239" text-anchor="middle" font-size="11">{merged} merged</text>

  <!-- Auto-review badge -->
  <rect class="badge-bg" x="270" y="224" width="100" height="22" fill="#1a2e1a"/>
  <text class="value" x="320" y="239" text-anchor="middle" font-size="11" fill="{auto_review_color}">review: {auto_review_text}</text>

  <!-- Divider -->
  <line class="divider" x1="20" y1="258" x2="380" y2="258"/>

  <!-- Footer -->
  <text class="footer" x="20" y="276">Updated: {updated}</text>
  <text class="footer" x="380" y="276" text-anchor="end">github.com/terrarium</text>
</svg>"##,
        plant = "&#x1F331;",
        treasury_color = treasury_color,
        treasury = data.balance_usd,
        runway_color = runway_color,
        runway_text = esc(&runway_text),
        burn = data.daily_run_rate,
        credits = data.openrouter_credits,
        owner_model = esc(&owner_model),
        high_model = esc(&high_model),
        medium_model = esc(&medium_model),
        low_model = esc(&low_model),
        open_issues = data.open_issues,
        open_prs = data.open_prs,
        merged = data.total_merged,
        auto_review_color = auto_review_color,
        auto_review_text = auto_review_text,
        updated = now,
    )
}

/// Publish the SVG status card to the `status` orphan branch via the GitHub
/// Contents API (using `gh api`). This avoids any local git branch gymnastics.
///
/// `repo_slug` should be "owner/repo" (e.g. from `GITHUB_REPOSITORY`).
pub async fn publish_status_svg(svg: &str, repo_slug: &str) -> Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
    let path = "terrarium-status.svg";
    let api_url = format!("/repos/{repo_slug}/contents/{path}");

    // First, try to get the current file SHA (needed for updates).
    let sha = get_file_sha(&api_url).await.ok();

    // Build the JSON payload.
    let mut payload = serde_json::json!({
        "message": "Update status",
        "content": encoded,
        "branch": "status",
    });

    if let Some(sha) = sha {
        payload["sha"] = serde_json::Value::String(sha);
    }

    // Write payload to a temp file (avoids stdin piping complexity).
    let tmp = std::env::temp_dir().join("terrarium-status-payload.json");
    tokio::fs::write(&tmp, serde_json::to_string(&payload)?)
        .await
        .context("failed to write temp payload")?;

    let output = tokio::process::Command::new("gh")
        .args(["api", &api_url, "--method", "PUT", "--input", tmp.to_str().unwrap()])
        .output()
        .await
        .context("failed to invoke gh api for publish_status_svg")?;

    // Clean up temp file (best-effort).
    let _ = tokio::fs::remove_file(&tmp).await;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If the branch doesn't exist yet, create it via the git API.
        if stderr.contains("Reference does not exist")
            || stderr.contains("Branch not found")
            || stderr.contains("No commit found for the ref")
        {
            create_orphan_branch_and_push(svg, repo_slug).await?;
            return Ok(());
        }
        anyhow::bail!("gh api PUT failed: {stderr}");
    }

    Ok(())
}

/// Get the SHA of an existing file on the status branch via gh api.
async fn get_file_sha(api_url: &str) -> Result<String> {
    use tokio::process::Command;

    let output = Command::new("gh")
        .args(["api", &format!("{api_url}?ref=status"), "--jq", ".sha"])
        .output()
        .await
        .context("failed to get file SHA")?;

    if !output.status.success() {
        anyhow::bail!("file not found on status branch");
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        anyhow::bail!("empty SHA");
    }
    Ok(sha)
}

/// Create the orphan `status` branch and push the SVG via the GitHub API.
/// Uses the git trees/commits API to create the branch without a local checkout.
async fn create_orphan_branch_and_push(svg: &str, repo_slug: &str) -> Result<()> {
    use tokio::process::Command;

    let encoded = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());

    // Step 1: Create a blob with the SVG content.
    let blob_output = Command::new("gh")
        .args([
            "api",
            &format!("/repos/{repo_slug}/git/blobs"),
            "--method", "POST",
            "-f", &format!("content={encoded}"),
            "-f", "encoding=base64",
            "--jq", ".sha",
        ])
        .output()
        .await
        .context("failed to create blob")?;

    if !blob_output.status.success() {
        let stderr = String::from_utf8_lossy(&blob_output.stderr);
        anyhow::bail!("create blob failed: {stderr}");
    }
    let blob_sha = String::from_utf8_lossy(&blob_output.stdout).trim().to_string();

    // Step 2: Create a tree with the blob (via temp file for --input).
    let tree_payload = serde_json::json!({
        "tree": [{
            "path": "terrarium-status.svg",
            "mode": "100644",
            "type": "blob",
            "sha": blob_sha,
        }]
    });
    let tmp_tree = std::env::temp_dir().join("terrarium-tree-payload.json");
    tokio::fs::write(&tmp_tree, serde_json::to_string(&tree_payload)?)
        .await
        .context("failed to write tree payload")?;

    let tree_output = Command::new("gh")
        .args([
            "api",
            &format!("/repos/{repo_slug}/git/trees"),
            "--method", "POST",
            "--input", tmp_tree.to_str().unwrap(),
            "--jq", ".sha",
        ])
        .output()
        .await
        .context("failed to create tree")?;

    let _ = tokio::fs::remove_file(&tmp_tree).await;

    if !tree_output.status.success() {
        let stderr = String::from_utf8_lossy(&tree_output.stderr);
        anyhow::bail!("create tree failed: {stderr}");
    }
    let tree_sha = String::from_utf8_lossy(&tree_output.stdout).trim().to_string();

    // Step 3: Create an orphan commit (no parents) via temp file.
    let commit_payload = serde_json::json!({
        "message": "Update status",
        "tree": tree_sha,
        "parents": []
    });
    let tmp_commit = std::env::temp_dir().join("terrarium-commit-payload.json");
    tokio::fs::write(&tmp_commit, serde_json::to_string(&commit_payload)?)
        .await
        .context("failed to write commit payload")?;

    let commit_output = Command::new("gh")
        .args([
            "api",
            &format!("/repos/{repo_slug}/git/commits"),
            "--method", "POST",
            "--input", tmp_commit.to_str().unwrap(),
            "--jq", ".sha",
        ])
        .output()
        .await
        .context("failed to create commit")?;

    let _ = tokio::fs::remove_file(&tmp_commit).await;

    if !commit_output.status.success() {
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        anyhow::bail!("create commit failed: {stderr}");
    }
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout).trim().to_string();

    // Step 4: Create or update the ref.
    let ref_output = Command::new("gh")
        .args([
            "api",
            &format!("/repos/{repo_slug}/git/refs"),
            "--method", "POST",
            "-f", "ref=refs/heads/status",
            "-f", &format!("sha={commit_sha}"),
        ])
        .output()
        .await
        .context("failed to create ref")?;

    if !ref_output.status.success() {
        let stderr = String::from_utf8_lossy(&ref_output.stderr);
        // Ref might already exist, try updating it.
        if stderr.contains("Reference already exists") {
            let update_output = Command::new("gh")
                .args([
                    "api",
                    &format!("/repos/{repo_slug}/git/refs/heads/status"),
                    "--method", "PATCH",
                    "-f", &format!("sha={commit_sha}"),
                    "-F", "force=true",
                ])
                .output()
                .await
                .context("failed to update ref")?;

            if !update_output.status.success() {
                let stderr2 = String::from_utf8_lossy(&update_output.stderr);
                anyhow::bail!("update ref failed: {stderr2}");
            }
        } else {
            anyhow::bail!("create ref failed: {stderr}");
        }
    }

    Ok(())
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
                high: Some("gpt-5.4".into()),
                medium: Some("gpt-5.4".into()),
                low: Some("qwen3.5-a3b".into()),
            },
            auto_review: true,
            open_issues: 5,
            open_prs: 2,
            total_merged: 12,
            zora_url: None,
            basescan_url: "https://basescan.org/address/0xABCD".into(),
            repo_slug: Some("test-owner/terrarium".into()),
        }
    }

    #[test]
    fn test_render_svg_contains_key_data() {
        let svg = render_status_svg(&sample_data());
        assert!(svg.contains("<svg"), "should be valid SVG");
        assert!(svg.contains("terrarium"), "should contain title");
        assert!(svg.contains("45.20"), "should contain treasury balance");
        assert!(svg.contains("21 days"), "should contain runway");
        assert!(svg.contains("2.10"), "should contain burn rate");
        assert!(svg.contains("12.50"), "should contain credits");
        assert!(svg.contains("kimi-k2.5"), "should contain owner model");
        assert!(svg.contains("gpt-5.4"), "should contain high model");
        assert!(svg.contains("5 issues"), "should contain issue count");
        assert!(svg.contains("2 PRs"), "should contain PR count");
        assert!(svg.contains("12 merged"), "should contain merged count");
        assert!(svg.contains("review: ON"), "should contain auto-review status");
        assert!(svg.contains("Updated:"), "should contain timestamp");
    }

    #[test]
    fn test_render_svg_treasury_colors() {
        // Green for >$50
        let mut data = sample_data();
        data.balance_usd = 100.0;
        let svg = render_status_svg(&data);
        assert!(svg.contains("#4ade80"), "treasury >50 should be green");

        // Yellow for >$10
        data.balance_usd = 25.0;
        let svg = render_status_svg(&data);
        assert!(svg.contains("#facc15"), "treasury >10 should be yellow");

        // Red for <=10
        data.balance_usd = 5.0;
        let svg = render_status_svg(&data);
        assert!(svg.contains("#f87171"), "treasury <=10 should be red");
    }

    #[test]
    fn test_render_svg_infinite_runway() {
        let mut data = sample_data();
        data.projected_days = f64::INFINITY;
        let svg = render_status_svg(&data);
        assert!(svg.contains("\u{221e}"), "should show infinity symbol");
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
