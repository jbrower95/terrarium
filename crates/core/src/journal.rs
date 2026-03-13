use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub timestamp: DateTime<Utc>,
    pub balance_usd: f64,
    pub daily_run_rate: f64,
    pub projected_days: f64,
    pub models: String,
    pub auto_review: bool,
    pub body: String,
}

/// Render a single journal entry as a markdown section.
fn render_entry(entry: &JournalEntry) -> String {
    let auto_review_label = if entry.auto_review {
        "enabled"
    } else {
        "disabled"
    };

    format!(
        "## {timestamp}\n\
         - Balance: ${balance:.2} | Burn: ${burn:.2}/day | Runway: {runway} days\n\
         - Models: {models}\n\
         - Auto-review: {auto_review}\n\
         \n\
         {body}\n",
        timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
        balance = entry.balance_usd,
        burn = entry.daily_run_rate,
        runway = entry.projected_days as u64,
        models = entry.models,
        auto_review = auto_review_label,
        body = entry.body.trim(),
    )
}

/// Append a journal entry to JOURNAL.md in the given repo root.
///
/// Creates the file with a top-level heading if it does not exist.
pub async fn append_journal_entry(repo_root: &Path, entry: &JournalEntry) -> Result<()> {
    let journal_path = repo_root.join("JOURNAL.md");
    let rendered = render_entry(entry);

    let existing = if journal_path.exists() {
        tokio::fs::read_to_string(&journal_path)
            .await
            .context("failed to read JOURNAL.md")?
    } else {
        String::new()
    };

    let new_content = if existing.is_empty() {
        format!("# Journal\n\n{rendered}")
    } else {
        format!("{existing}\n{rendered}")
    };

    tokio::fs::write(&journal_path, new_content)
        .await
        .context("failed to write JOURNAL.md")?;

    Ok(())
}

/// Read the last `n` journal entries from JOURNAL.md and return them as a string.
///
/// Entries are delimited by `## ` headings. If the file does not exist or has
/// fewer than `n` entries, all available entries are returned.
pub fn read_journal_context(repo_root: &Path, n: usize) -> Result<String> {
    let journal_path = repo_root.join("JOURNAL.md");

    if !journal_path.exists() {
        return Ok(String::new());
    }

    let content =
        std::fs::read_to_string(&journal_path).context("failed to read JOURNAL.md")?;

    // Split on entry headings (## ) while keeping the delimiter.
    let mut entries: Vec<&str> = Vec::new();
    let mut start = None;

    for (i, _) in content.match_indices("\n## ") {
        if let Some(s) = start {
            entries.push(&content[s..i]);
        }
        // +1 to skip the leading newline so each chunk starts with "## "
        start = Some(i + 1);
    }

    // Handle the very first entry which may start at offset 0 (after the top heading)
    // or push the last accumulated chunk.
    if let Some(s) = start {
        entries.push(&content[s..]);
    } else {
        // Check if the file starts with "## " (no preceding newline).
        if let Some(pos) = content.find("## ") {
            entries.push(&content[pos..]);
        }
    }

    // If the first entry was found via `find("## ")` and there were also `\n## ` matches,
    // the first entry from `find` is not yet in the vec. Re-scan properly:
    if entries.is_empty() {
        return Ok(String::new());
    }

    // Collect a clean list by re-splitting on the heading pattern.
    let sections: Vec<&str> = split_on_headings(&content);

    let tail = if sections.len() > n {
        &sections[sections.len() - n..]
    } else {
        &sections
    };

    Ok(tail.join("\n"))
}

/// Split content into sections by `## ` headings, returning each section
/// (including its heading line) as a separate string slice.
fn split_on_headings(content: &str) -> Vec<&str> {
    let mut sections: Vec<&str> = Vec::new();
    let mut last_start: Option<usize> = None;

    for (i, line) in content.lines().enumerate() {
        if line.starts_with("## ") {
            let byte_offset = line_byte_offset(content, i);
            if let Some(prev) = last_start {
                sections.push(content[prev..byte_offset].trim_end());
            }
            last_start = Some(byte_offset);
        }
    }

    if let Some(prev) = last_start {
        sections.push(content[prev..].trim_end());
    }

    sections
}

/// Compute the byte offset of the `line_num`-th line (0-indexed) in `content`.
fn line_byte_offset(content: &str, line_num: usize) -> usize {
    let mut offset = 0;
    for (i, line) in content.lines().enumerate() {
        if i == line_num {
            return offset;
        }
        offset += line.len() + 1; // +1 for '\n'
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_entry() -> JournalEntry {
        JournalEntry {
            timestamp: Utc.with_ymd_and_hms(2026, 3, 13, 14, 30, 0).unwrap(),
            balance_usd: 45.20,
            daily_run_rate: 2.10,
            projected_days: 21.0,
            models: "owner:kimi-k2.5, high:kimi-k2.5, medium:qwen3.5, low:qwen3.5".into(),
            auto_review: true,
            body: "Merged PR #18, filed issues #19-#21 for milestone v0.2".into(),
        }
    }

    #[test]
    fn test_render_entry() {
        let entry = sample_entry();
        let rendered = render_entry(&entry);
        assert!(rendered.contains("## 2026-03-13 14:30:00 UTC"));
        assert!(rendered.contains("Balance: $45.20"));
        assert!(rendered.contains("Burn: $2.10/day"));
        assert!(rendered.contains("Runway: 21 days"));
        assert!(rendered.contains("Auto-review: enabled"));
        assert!(rendered.contains("Merged PR #18"));
    }

    #[tokio::test]
    async fn test_append_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let entry = sample_entry();

        append_journal_entry(dir.path(), &entry).await.unwrap();

        let content = std::fs::read_to_string(dir.path().join("JOURNAL.md")).unwrap();
        assert!(content.starts_with("# Journal"));
        assert!(content.contains("## 2026-03-13 14:30:00 UTC"));

        // Append a second entry.
        let mut entry2 = sample_entry();
        entry2.balance_usd = 40.00;
        entry2.body = "Second entry".into();
        append_journal_entry(dir.path(), &entry2).await.unwrap();

        // Read last 1.
        let last_one = read_journal_context(dir.path(), 1).unwrap();
        assert!(last_one.contains("Second entry"));
        assert!(!last_one.contains("Merged PR #18"));

        // Read last 5 (more than available).
        let all = read_journal_context(dir.path(), 5).unwrap();
        assert!(all.contains("Merged PR #18"));
        assert!(all.contains("Second entry"));
    }

    #[test]
    fn test_read_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_journal_context(dir.path(), 5).unwrap();
        assert!(result.is_empty());
    }
}
