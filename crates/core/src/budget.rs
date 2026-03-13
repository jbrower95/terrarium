use anyhow::{Context, Result};
use serde::Deserialize;

// ── Public types ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Balance {
    pub eth: f64,
    pub usd: f64,
}

// ── Constants ──────────────────────────────────────────────────────────

const DEFAULT_BASE_RPC: &str = "https://mainnet.base.org";
const OPENROUTER_CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
const OPENROUTER_COINBASE_URL: &str = "https://openrouter.ai/api/v1/credits/coinbase";

/// Rough ETH/USD price used when a live feed is unavailable.
/// Good enough for runway estimates; replace with an oracle later.
const FALLBACK_ETH_USD: f64 = 3_000.0;

// ── JSON-RPC helpers ───────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: serde_json::Value,
    id: u64,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    result: Option<String>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

// ── OpenRouter credits response ────────────────────────────────────────

#[derive(Deserialize)]
struct CreditsResponse {
    #[serde(default)]
    data: Option<CreditsData>,
}

#[derive(Deserialize)]
struct CreditsData {
    #[serde(default)]
    total_credits: f64,
    #[serde(default)]
    total_usage: f64,
}

// ── OpenRouter coinbase top-up response ────────────────────────────────

#[derive(Deserialize)]
struct CoinbaseResponse {
    #[serde(default)]
    data: Option<CoinbaseData>,
}

#[derive(Deserialize)]
struct CoinbaseData {
    #[serde(default)]
    calldata: Option<String>,
}

// ── Public API ─────────────────────────────────────────────────────────

/// Fetch the ETH balance of `wallet` via an `eth_getBalance` JSON-RPC call.
///
/// `rpc_url` defaults to Base mainnet (`https://mainnet.base.org`) when an
/// empty string is passed.
pub async fn get_wallet_balance(wallet: &str, rpc_url: &str) -> Result<Balance> {
    let rpc = if rpc_url.is_empty() {
        DEFAULT_BASE_RPC
    } else {
        rpc_url
    };

    let body = JsonRpcRequest {
        jsonrpc: "2.0",
        method: "eth_getBalance",
        params: serde_json::json!([wallet, "latest"]),
        id: 1,
    };

    let client = reqwest::Client::new();
    let res = client
        .post(rpc)
        .json(&body)
        .send()
        .await
        .context("eth_getBalance RPC call failed")?;

    let rpc_res: JsonRpcResponse = res
        .json()
        .await
        .context("failed to parse eth_getBalance response")?;

    if let Some(err) = rpc_res.error {
        anyhow::bail!("eth_getBalance RPC error: {err}");
    }

    let hex = rpc_res
        .result
        .context("eth_getBalance returned null result")?;

    let wei = u128::from_str_radix(hex.trim_start_matches("0x"), 16)
        .context("failed to parse balance hex")?;

    let eth = wei as f64 / 1e18;
    let usd = eth * FALLBACK_ETH_USD;

    Ok(Balance { eth, usd })
}

/// Return the remaining OpenRouter credit balance in USD.
pub async fn get_openrouter_credits(api_key: &str) -> Result<f64> {
    let client = reqwest::Client::new();
    let res = client
        .get(OPENROUTER_CREDITS_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .context("failed to fetch OpenRouter credits")?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter credits returned {status}: {text}");
    }

    let credits: CreditsResponse = res
        .json()
        .await
        .context("failed to parse OpenRouter credits response")?;

    match credits.data {
        Some(d) => Ok(d.total_credits - d.total_usage),
        None => Ok(0.0),
    }
}

/// Ask OpenRouter for the calldata needed to top up credits on Base
/// (chain 8453) via the Coinbase on-chain flow.
pub async fn build_topup_calldata(amount_usd: f64) -> Result<Vec<u8>> {
    let client = reqwest::Client::new();
    let res = client
        .post(OPENROUTER_COINBASE_URL)
        .json(&serde_json::json!({
            "chain_id": "8453",
            "amount_usd": amount_usd,
        }))
        .send()
        .await
        .context("failed to fetch top-up calldata from OpenRouter")?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter coinbase endpoint returned {status}: {text}");
    }

    let body: CoinbaseResponse = res
        .json()
        .await
        .context("failed to parse coinbase calldata response")?;

    let hex = body
        .data
        .and_then(|d| d.calldata)
        .context("coinbase response did not contain calldata")?;

    let hex_clean = hex.trim_start_matches("0x");
    let bytes = hex::decode(hex_clean).context("calldata is not valid hex")?;

    Ok(bytes)
}

/// Calculate how many days the remaining balance will last at the given
/// daily spend rate.  Returns `f64::INFINITY` when `daily_rate` is zero
/// or negative.
pub fn calculate_runway(balance_usd: f64, daily_rate: f64) -> f64 {
    if daily_rate <= 0.0 {
        return f64::INFINITY;
    }
    balance_usd / daily_rate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runway_normal() {
        let days = calculate_runway(100.0, 10.0);
        assert!((days - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn runway_zero_rate() {
        let days = calculate_runway(50.0, 0.0);
        assert!(days.is_infinite());
    }

    #[test]
    fn runway_negative_rate() {
        let days = calculate_runway(50.0, -1.0);
        assert!(days.is_infinite());
    }
}
