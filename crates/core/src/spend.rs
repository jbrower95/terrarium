use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunArtifact {
    pub run_id: u64,
    pub role: String,
    pub issue: Option<u64>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct SpendSummary {
    pub owner_spend_24h: f64,
    pub owner_runs_24h: u64,
    pub employee_spend_24h: f64,
    pub employee_runs_24h: u64,
    pub total_spend_24h: f64,
    pub total_spend_7d: f64,
    pub daily_avg_7d: f64,
    pub by_issue: Vec<(u64, f64, u64)>,
}

/// Download recent workflow run artifacts from a GitHub repository using `gh api`.
///
/// `repo` should be in `owner/name` format (e.g. "user/terrarium").
/// `hours` controls how far back to look.
///
/// This shells out to `gh api` and expects the `gh` CLI to be installed and
/// authenticated. Each artifact is expected to contain a JSON file matching
/// the `RunArtifact` schema.
pub async fn download_recent_artifacts(repo: &str, hours: u64) -> Result<Vec<RunArtifact>> {
    use chrono::Utc;
    use std::process::Stdio;

    let cutoff = Utc::now() - chrono::Duration::hours(hours as i64);
    let cutoff_str = cutoff.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // List recent workflow runs created after the cutoff.
    let runs_output = tokio::process::Command::new("gh")
        .args([
            "api",
            &format!(
                "repos/{repo}/actions/runs?created=>={cutoff_str}&per_page=100"
            ),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("failed to invoke gh api for workflow runs")?;

    if !runs_output.status.success() {
        let stderr = String::from_utf8_lossy(&runs_output.stderr);
        anyhow::bail!("gh api runs failed: {stderr}");
    }

    let runs_json: serde_json::Value = serde_json::from_slice(&runs_output.stdout)
        .context("failed to parse workflow runs response")?;

    let run_ids: Vec<u64> = runs_json
        .get("workflow_runs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r.get("id").and_then(|id| id.as_u64()))
                .collect()
        })
        .unwrap_or_default();

    let mut artifacts: Vec<RunArtifact> = Vec::new();

    for run_id in &run_ids {
        let arts_output = tokio::process::Command::new("gh")
            .args([
                "api",
                &format!("repos/{repo}/actions/runs/{run_id}/artifacts"),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("failed to list artifacts for run")?;

        if !arts_output.status.success() {
            continue;
        }

        let arts_json: serde_json::Value =
            match serde_json::from_slice(&arts_output.stdout) {
                Ok(v) => v,
                Err(_) => continue,
            };

        let art_list = match arts_json.get("artifacts").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => continue,
        };

        for art in art_list {
            let Some(download_url) = art
                .get("archive_download_url")
                .and_then(|v| v.as_str())
            else {
                continue;
            };

            // Download the artifact zip via gh api (handles auth).
            let dl_output = tokio::process::Command::new("gh")
                .args(["api", download_url, "--method", "GET"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await;

            let dl_output = match dl_output {
                Ok(o) if o.status.success() => o,
                _ => continue,
            };

            // Artifact is a zip containing JSON files; try to parse the raw
            // output directly in case gh follows the redirect and gives us the
            // zip bytes.  For simplicity, attempt JSON parse first (works when
            // the artifact is a single JSON file and gh streams it raw).
            if let Ok(artifact) =
                serde_json::from_slice::<RunArtifact>(&dl_output.stdout)
            {
                artifacts.push(artifact);
            } else if let Ok(batch) =
                serde_json::from_slice::<Vec<RunArtifact>>(&dl_output.stdout)
            {
                artifacts.extend(batch);
            }
            // If neither parse works, skip silently (might be a zip we can't
            // handle without an unzip step -- future improvement).
        }
    }

    Ok(artifacts)
}

/// Aggregate a set of run artifacts into a spend summary.
///
/// All artifacts are assumed to be within the relevant time window already.
/// The `_24h` fields include all provided artifacts; `_7d` fields also
/// include all artifacts (the caller should pass 7-day data for accurate 7d
/// numbers).
pub fn aggregate_spend(artifacts: &[RunArtifact]) -> SpendSummary {
    let mut summary = SpendSummary::default();

    // Track per-issue aggregation: issue_number -> (cost, runs)
    let mut issue_map: std::collections::HashMap<u64, (f64, u64)> =
        std::collections::HashMap::new();

    for art in artifacts {
        match art.role.as_str() {
            "owner" => {
                summary.owner_spend_24h += art.cost_usd;
                summary.owner_runs_24h += 1;
            }
            _ => {
                summary.employee_spend_24h += art.cost_usd;
                summary.employee_runs_24h += 1;
            }
        }

        summary.total_spend_24h += art.cost_usd;
        summary.total_spend_7d += art.cost_usd;

        if let Some(issue) = art.issue {
            let entry = issue_map.entry(issue).or_insert((0.0, 0));
            entry.0 += art.cost_usd;
            entry.1 += 1;
        }
    }

    summary.daily_avg_7d = if summary.total_spend_7d > 0.0 {
        summary.total_spend_7d / 7.0
    } else {
        0.0
    };

    summary.by_issue = issue_map
        .into_iter()
        .map(|(issue, (cost, runs))| (issue, cost, runs))
        .collect();

    // Sort by cost descending.
    summary
        .by_issue
        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_artifact(role: &str, issue: Option<u64>, cost: f64) -> RunArtifact {
        RunArtifact {
            run_id: 1,
            role: role.into(),
            issue,
            model: "test-model".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cost_usd: cost,
            result: serde_json::json!({"status": "ok"}),
        }
    }

    #[test]
    fn test_aggregate_empty() {
        let summary = aggregate_spend(&[]);
        assert_eq!(summary.total_spend_24h, 0.0);
        assert_eq!(summary.owner_runs_24h, 0);
        assert_eq!(summary.employee_runs_24h, 0);
    }

    #[test]
    fn test_aggregate_basic() {
        let arts = vec![
            make_artifact("owner", Some(1), 0.50),
            make_artifact("owner", Some(1), 0.25),
            make_artifact("employee", Some(2), 1.00),
            make_artifact("employee", None, 0.10),
        ];

        let summary = aggregate_spend(&arts);

        assert_eq!(summary.owner_runs_24h, 2);
        assert_eq!(summary.employee_runs_24h, 2);
        assert!((summary.owner_spend_24h - 0.75).abs() < 1e-9);
        assert!((summary.employee_spend_24h - 1.10).abs() < 1e-9);
        assert!((summary.total_spend_24h - 1.85).abs() < 1e-9);

        // by_issue should have issue 1 and 2 (sorted by cost desc).
        assert_eq!(summary.by_issue.len(), 2);
        assert_eq!(summary.by_issue[0].0, 2); // issue #2 costs $1.00
        assert_eq!(summary.by_issue[1].0, 1); // issue #1 costs $0.75
    }
}
