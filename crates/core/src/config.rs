use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const CONFIG_FILENAME: &str = "terrarium.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrariumConfig {
    pub wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Read a `terrarium.json` from the given repo root directory.
pub fn read_config(repo_root: &Path) -> Result<TerrariumConfig> {
    let path = repo_root.join(CONFIG_FILENAME);
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config: TerrariumConfig = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config)
}

/// Write a `terrarium.json` to the given repo root directory.
pub fn write_config(repo_root: &Path, config: &TerrariumConfig) -> Result<()> {
    let path = repo_root.join(CONFIG_FILENAME);
    let contents = serde_json::to_string_pretty(config)
        .context("failed to serialize config")?;
    std::fs::write(&path, contents.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn round_trip_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = TerrariumConfig {
            wallet: "0xabc123".to_string(),
            token: Some("tok_secret".to_string()),
        };
        write_config(dir.path(), &config).unwrap();
        let loaded = read_config(dir.path()).unwrap();
        assert_eq!(loaded.wallet, config.wallet);
        assert_eq!(loaded.token, config.token);
    }

    #[test]
    fn round_trip_config_no_token() {
        let dir = tempfile::tempdir().unwrap();
        let config = TerrariumConfig {
            wallet: "0xdef456".to_string(),
            token: None,
        };
        write_config(dir.path(), &config).unwrap();

        // Verify the JSON does not contain "token" key when None
        let raw = fs::read_to_string(dir.path().join("terrarium.json")).unwrap();
        assert!(!raw.contains("token"));

        let loaded = read_config(dir.path()).unwrap();
        assert_eq!(loaded.wallet, "0xdef456");
        assert!(loaded.token.is_none());
    }

    #[test]
    fn read_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_config(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn read_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("terrarium.json"), "not json").unwrap();
        let result = read_config(dir.path());
        assert!(result.is_err());
    }
}
