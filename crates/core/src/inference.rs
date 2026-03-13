use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── Public types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct InferenceResult {
    pub content: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

// ── OpenRouter request / response shapes ───────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Serialize)]
struct ResponseFormat {
    r#type: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[serde(default)]
    total_cost: Option<f64>,
}

// ── Core inference function ────────────────────────────────────────────

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// Call the OpenRouter chat-completion API and return the assistant reply
/// together with token-usage metadata.
///
/// When `json_mode` is true the request includes
/// `response_format: { type: "json_object" }` so the model is
/// constrained to produce valid JSON.
pub async fn infer(
    model: &str,
    messages: Vec<Message>,
    api_key: &str,
    json_mode: bool,
) -> Result<InferenceResult> {
    let response_format = if json_mode {
        Some(ResponseFormat {
            r#type: "json_object".to_string(),
        })
    } else {
        None
    };

    let body = ChatRequest {
        model: model.to_string(),
        messages,
        response_format,
    };

    let client = reqwest::Client::new();
    let res = client
        .post(OPENROUTER_CHAT_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header(
            "HTTP-Referer",
            "https://github.com/jbrower95/terrarium",
        )
        .json(&body)
        .send()
        .await
        .context("failed to call OpenRouter chat completions")?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter returned {status}: {text}");
    }

    let chat: ChatResponse = res
        .json()
        .await
        .context("failed to parse OpenRouter response")?;

    let content = chat
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();

    let (input_tokens, output_tokens, cost_usd) = match chat.usage {
        Some(u) => {
            let cost = u.total_cost.unwrap_or(0.0);
            (u.prompt_tokens, u.completion_tokens, cost)
        }
        None => (0, 0, 0.0),
    };

    Ok(InferenceResult {
        content,
        input_tokens,
        output_tokens,
        cost_usd,
    })
}
