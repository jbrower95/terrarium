use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

/// A single entry in the static model catalog.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    /// OpenRouter model ID
    pub id: &'static str,
    /// Human-readable display name
    pub name: &'static str,
    /// Relative coding quality score (1-100)
    pub coding_score: u32,
    /// Cost per million input tokens (USD)
    pub cost_input: f64,
    /// Cost per million output tokens (USD)
    pub cost_output: f64,
}

/// Per-tier model selection persisted alongside config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelConfig {
    pub owner: Option<String>,
    pub high: Option<String>,
    pub medium: Option<String>,
    pub low: Option<String>,
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

pub static MODEL_CATALOG: &[ModelEntry] = &[
    ModelEntry {
        id: "moonshotai/kimi-k2.5",
        name: "Kimi K2.5",
        coding_score: 82,
        cost_input: 0.60,
        cost_output: 2.00,
    },
    ModelEntry {
        id: "qwen/qwen3.5-coder-32b",
        name: "Qwen 3.5 Coder 32B",
        coding_score: 72,
        cost_input: 0.20,
        cost_output: 0.60,
    },
    ModelEntry {
        id: "qwen/qwen3.5-35b",
        name: "Qwen 3.5 35B",
        coding_score: 70,
        cost_input: 0.20,
        cost_output: 0.60,
    },
    ModelEntry {
        id: "qwen/qwen3.5-35b-a3b",
        name: "Qwen 3.5 35B-A3B",
        coding_score: 71,
        cost_input: 0.16,
        cost_output: 1.30,
    },
    ModelEntry {
        id: "qwen/qwen3.5-72b",
        name: "Qwen 3.5 72B",
        coding_score: 76,
        cost_input: 0.40,
        cost_output: 1.20,
    },
    ModelEntry {
        id: "anthropic/claude-sonnet-4",
        name: "Claude Sonnet 4",
        coding_score: 88,
        cost_input: 3.00,
        cost_output: 15.00,
    },
    ModelEntry {
        id: "openai/gpt-5.4",
        name: "GPT-5.4",
        coding_score: 92,
        cost_input: 2.50,
        cost_output: 15.00,
    },
    ModelEntry {
        id: "openai/gpt-4.1",
        name: "GPT-4.1",
        coding_score: 85,
        cost_input: 2.00,
        cost_output: 8.00,
    },
    ModelEntry {
        id: "openai/gpt-4.1-mini",
        name: "GPT-4.1 Mini",
        coding_score: 74,
        cost_input: 0.40,
        cost_output: 1.60,
    },
    ModelEntry {
        id: "openai/o4-mini",
        name: "o4-mini",
        coding_score: 80,
        cost_input: 1.10,
        cost_output: 4.40,
    },
    ModelEntry {
        id: "google/gemini-2.5-flash",
        name: "Gemini 2.5 Flash",
        coding_score: 78,
        cost_input: 0.15,
        cost_output: 0.60,
    },
    ModelEntry {
        id: "deepseek/deepseek-r1",
        name: "DeepSeek R1",
        coding_score: 79,
        cost_input: 0.55,
        cost_output: 2.19,
    },
    ModelEntry {
        id: "deepseek/deepseek-v3",
        name: "DeepSeek V3",
        coding_score: 71,
        cost_input: 0.30,
        cost_output: 0.88,
    },
];

const DEFAULT_OWNER: &str = "moonshotai/kimi-k2.5";
const DEFAULT_HIGH: &str = "openai/gpt-5.4";
const DEFAULT_MEDIUM: &str = "openai/gpt-5.4";
const DEFAULT_LOW: &str = "qwen/qwen3.5-35b-a3b";

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Look up a model entry by its OpenRouter ID.
pub fn lookup(id: &str) -> Option<&'static ModelEntry> {
    MODEL_CATALOG.iter().find(|m| m.id == id)
}

/// Read model configuration from environment variables, falling back to
/// compiled defaults.
///
/// Env vars: `TERRARIUM_MODEL_OWNER`, `TERRARIUM_MODEL_HIGH`,
///           `TERRARIUM_MODEL_MEDIUM`, `TERRARIUM_MODEL_LOW`
pub fn read_model_config() -> Result<ModelConfig> {
    let get = |key: &str, default: &str| -> Option<String> {
        Some(env::var(key).unwrap_or_else(|_| default.to_string()))
    };

    Ok(ModelConfig {
        owner: get("TERRARIUM_MODEL_OWNER", DEFAULT_OWNER),
        high: get("TERRARIUM_MODEL_HIGH", DEFAULT_HIGH),
        medium: get("TERRARIUM_MODEL_MEDIUM", DEFAULT_MEDIUM),
        low: get("TERRARIUM_MODEL_LOW", DEFAULT_LOW),
    })
}

/// Rough daily cost estimate assuming a token budget per tier.
///
/// Heuristic budgets (tokens/day):
///   owner  — 500k input, 200k output
///   high   — 1M input, 400k output
///   medium — 2M input, 800k output
///   low    — 4M input, 1.5M output
pub fn estimate_daily_cost(config: &ModelConfig) -> f64 {
    struct TierBudget {
        input_mtok: f64,
        output_mtok: f64,
    }

    let budgets: &[(&Option<String>, TierBudget)] = &[
        (
            &config.owner,
            TierBudget { input_mtok: 0.5, output_mtok: 0.2 },
        ),
        (
            &config.high,
            TierBudget { input_mtok: 1.0, output_mtok: 0.4 },
        ),
        (
            &config.medium,
            TierBudget { input_mtok: 2.0, output_mtok: 0.8 },
        ),
        (
            &config.low,
            TierBudget { input_mtok: 4.0, output_mtok: 1.5 },
        ),
    ];

    budgets
        .iter()
        .map(|(model_id, budget)| {
            let model_id = match model_id {
                Some(id) => id.as_str(),
                None => return 0.0,
            };
            match lookup(model_id) {
                Some(entry) => {
                    entry.cost_input * budget.input_mtok
                        + entry.cost_output * budget.output_mtok
                }
                None => 0.0,
            }
        })
        .sum()
}

/// Render the full model catalog as a markdown table.
pub fn format_model_catalog() -> String {
    let mut out = String::from(
        "| Model | ID | Score | $/MTok In | $/MTok Out |\n\
         |---|---|---:|---:|---:|\n",
    );
    for m in MODEL_CATALOG {
        out.push_str(&format!(
            "| {} | `{}` | {} | {:.2} | {:.2} |\n",
            m.name, m.id, m.coding_score, m.cost_input, m.cost_output,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_entries() {
        assert!(MODEL_CATALOG.len() >= 13);
    }

    #[test]
    fn lookup_known_model() {
        let entry = lookup("moonshotai/kimi-k2.5").unwrap();
        assert_eq!(entry.name, "Kimi K2.5");
    }

    #[test]
    fn lookup_gpt54() {
        let entry = lookup("openai/gpt-5.4").unwrap();
        assert_eq!(entry.name, "GPT-5.4");
    }

    #[test]
    fn lookup_qwen35_a3b() {
        let entry = lookup("qwen/qwen3.5-35b-a3b").unwrap();
        assert_eq!(entry.name, "Qwen 3.5 35B-A3B");
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("nonexistent/model").is_none());
    }

    #[test]
    fn default_model_config() {
        let cfg = read_model_config().unwrap();
        assert_eq!(cfg.owner.as_deref(), Some("moonshotai/kimi-k2.5"));
        assert_eq!(cfg.high.as_deref(), Some("openai/gpt-5.4"));
        assert_eq!(cfg.medium.as_deref(), Some("openai/gpt-5.4"));
        assert_eq!(cfg.low.as_deref(), Some("qwen/qwen3.5-35b-a3b"));
    }

    #[test]
    fn estimate_cost_is_positive() {
        let cfg = read_model_config().unwrap();
        let cost = estimate_daily_cost(&cfg);
        assert!(cost > 0.0, "expected positive cost, got {cost}");
    }

    #[test]
    fn format_catalog_is_markdown_table() {
        let table = format_model_catalog();
        assert!(table.starts_with("| Model"));
        assert!(table.contains("Kimi K2.5"));
        assert!(table.contains("DeepSeek R1"));
    }

    #[test]
    fn estimate_cost_empty_config() {
        let cfg = ModelConfig {
            owner: None,
            high: None,
            medium: None,
            low: None,
        };
        assert_eq!(estimate_daily_cost(&cfg), 0.0);
    }
}
