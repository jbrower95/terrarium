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
const OPENROUTER_AUTH_KEY_URL: &str = "https://openrouter.ai/api/v1/auth/key";
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

// ── OpenRouter auth/key response ──────────────────────────────────────

#[derive(Deserialize)]
struct AuthKeyResponse {
    #[serde(default)]
    data: Option<AuthKeyData>,
}

#[derive(Deserialize)]
struct AuthKeyData {
    #[serde(default)]
    limit: Option<f64>,
    #[serde(default)]
    usage: f64,
    #[serde(default)]
    limit_remaining: Option<f64>,
}

// ── OpenRouter coinbase top-up response ────────────────────────────────
// See: https://openrouter.ai/docs/guides/guides/crypto-api

#[derive(Deserialize)]
struct CoinbaseResponse {
    #[serde(default)]
    data: Option<CoinbaseChargeData>,
}

#[derive(Deserialize)]
struct CoinbaseChargeData {
    web3_data: Option<Web3Data>,
}

#[derive(Deserialize)]
struct Web3Data {
    transfer_intent: Option<TransferIntentWrapper>,
}

#[derive(Deserialize)]
struct TransferIntentWrapper {
    metadata: TransferMetadata,
    call_data: TransferCallData,
}

#[derive(Deserialize)]
struct TransferMetadata {
    contract_address: String,
}

#[derive(Deserialize)]
struct TransferCallData {
    recipient_amount: String,
    deadline: String,
    recipient: String,
    recipient_currency: String,
    refund_destination: String,
    fee_amount: String,
    id: String,
    operator: String,
    signature: String,
    prefix: String,
}

/// The result of building top-up calldata: includes the Coinbase commerce
/// contract address, ETH value to send, and ABI-encoded calldata for
/// `swapAndTransferUniswapV3Native`.
pub struct TopUpCalldata {
    /// The Coinbase commerce contract address on Base.
    pub to: String,
    /// The ETH value to send (in wei). Includes a small buffer for swap slippage.
    pub value: u128,
    /// ABI-encoded calldata for `swapAndTransferUniswapV3Native(TransferIntent, uint24)`.
    pub data: Vec<u8>,
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
///
/// Uses the `/api/v1/auth/key` endpoint which returns usage and limit info.
/// If `limit_remaining` is available, use that directly. Otherwise compute
/// from `limit - usage`. Keys without a limit return usage as a negative
/// balance (pay-as-you-go).
pub async fn get_openrouter_credits(api_key: &str) -> Result<f64> {
    let client = reqwest::Client::new();
    let res = client
        .get(OPENROUTER_AUTH_KEY_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .context("failed to fetch OpenRouter auth/key")?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter auth/key returned {status}: {text}");
    }

    let auth: AuthKeyResponse = res
        .json()
        .await
        .context("failed to parse OpenRouter auth/key response")?;

    match auth.data {
        Some(d) => {
            if let Some(remaining) = d.limit_remaining {
                Ok(remaining)
            } else if let Some(limit) = d.limit {
                Ok(limit - d.usage)
            } else {
                // No limit set — report usage as negative (pay-as-you-go)
                Ok(-d.usage)
            }
        }
        None => Ok(0.0),
    }
}

/// Request a credit purchase charge from OpenRouter and build the ABI-encoded
/// calldata for `swapAndTransferUniswapV3Native` on the Coinbase commerce contract.
///
/// `sender` is the wallet address that will send the transaction.
/// `api_key` is the OpenRouter API key (needed for auth).
///
/// Returns the contract address, ETH value, and encoded calldata ready to be
/// wrapped in the wallet's `execute(address,uint256,bytes)`.
pub async fn build_topup_calldata(
    amount_usd: f64,
    sender: &str,
    api_key: &str,
) -> Result<TopUpCalldata> {
    let client = reqwest::Client::new();
    let res = client
        .post(OPENROUTER_COINBASE_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "amount": amount_usd,
            "sender": sender,
            "chain_id": 8453,
        }))
        .send()
        .await
        .context("failed to fetch top-up charge from OpenRouter")?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter coinbase endpoint returned {status}: {text}");
    }

    let body: CoinbaseResponse = res
        .json()
        .await
        .context("failed to parse coinbase charge response")?;

    let charge = body.data.context("coinbase response missing data")?;
    let web3 = charge.web3_data.context("coinbase response missing web3_data")?;
    let intent = web3.transfer_intent.context("coinbase response missing transfer_intent")?;

    let contract_address = intent.metadata.contract_address;
    let cd = &intent.call_data;

    // Parse the deadline — it may be an ISO timestamp or a unix timestamp string.
    let deadline_secs: u64 = if let Ok(ts) = cd.deadline.parse::<u64>() {
        ts
    } else if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&cd.deadline) {
        dt.timestamp() as u64
    } else {
        anyhow::bail!("failed to parse deadline: {}", cd.deadline);
    };

    // Parse numeric fields.
    let recipient_amount = parse_uint256(&cd.recipient_amount)?;
    let fee_amount = parse_uint256(&cd.fee_amount)?;

    // ABI-encode swapAndTransferUniswapV3Native(TransferIntent, uint24).
    let calldata = encode_swap_and_transfer(
        recipient_amount,
        deadline_secs,
        &cd.recipient,
        &cd.recipient_currency,
        &cd.refund_destination,
        fee_amount,
        &cd.id,
        &cd.operator,
        &cd.signature,
        &cd.prefix,
        500, // poolFeesTier — lowest tier, recommended for ETH
    )?;

    // ETH value: recipient_amount + fee_amount + 10% buffer for swap slippage.
    // Excess is refunded to sender by the contract.
    let total = recipient_amount + fee_amount;
    let value = total + total / 10;

    Ok(TopUpCalldata {
        to: contract_address,
        value,
        data: calldata,
    })
}

/// Parse a decimal or hex string into a u128.
fn parse_uint256(s: &str) -> Result<u128> {
    if let Some(hex) = s.strip_prefix("0x") {
        u128::from_str_radix(hex, 16).context("failed to parse hex uint256")
    } else {
        s.parse::<u128>().context("failed to parse decimal uint256")
    }
}

/// Pad a hex address string to a 32-byte ABI word.
fn pad_address(addr: &str) -> Result<[u8; 32]> {
    let clean = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(clean).context("invalid hex address")?;
    let mut word = [0u8; 32];
    let offset = 32 - bytes.len().min(32);
    word[offset..].copy_from_slice(&bytes[..bytes.len().min(32)]);
    Ok(word)
}

/// Encode a u128 as a 32-byte big-endian ABI word.
fn encode_u256(v: u128) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&v.to_be_bytes());
    word
}

/// Encode a u64 as a 32-byte big-endian ABI word.
fn encode_u256_from_u64(v: u64) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[24..].copy_from_slice(&v.to_be_bytes());
    word
}

/// ABI-encode `swapAndTransferUniswapV3Native(TransferIntent, uint24)`.
///
/// Selector: keccak256("swapAndTransferUniswapV3Native((uint256,uint256,address,address,address,uint256,bytes16,address,bytes,bytes),uint24)")
/// = 0x101f2acd (first 4 bytes)
fn encode_swap_and_transfer(
    recipient_amount: u128,
    deadline: u64,
    recipient: &str,
    recipient_currency: &str,
    refund_destination: &str,
    fee_amount: u128,
    id: &str,          // bytes16 hex
    operator: &str,
    signature: &str,   // hex bytes
    prefix: &str,      // hex bytes
    pool_fees_tier: u32,
) -> Result<Vec<u8>> {
    // Function selector for swapAndTransferUniswapV3Native.
    // cast sig "swapAndTransferUniswapV3Native((uint256,uint256,address,address,address,uint256,bytes16,address,bytes,bytes),uint24)"
    let selector: [u8; 4] = [0x8b, 0xf1, 0x22, 0xda];

    // The function has 2 params: a dynamic tuple and a uint24.
    // Head section: offset to tuple (word 0), poolFeesTier (word 1).
    let mut buf = Vec::with_capacity(1024);
    buf.extend_from_slice(&selector);

    // Word 0: offset to tuple data = 64 (2 * 32).
    buf.extend_from_slice(&encode_u256_from_u64(64));
    // Word 1: poolFeesTier.
    buf.extend_from_slice(&encode_u256_from_u64(pool_fees_tier as u64));

    // Now encode the TransferIntent tuple.
    // It has 8 static fields + 2 dynamic fields (signature, prefix).
    // The tuple head is 10 words (one per field), with offsets for the dynamic ones.
    // Static fields size = 10 * 32 = 320 bytes.

    // Parse the dynamic fields.
    let sig_clean = signature.strip_prefix("0x").unwrap_or(signature);
    let sig_bytes = hex::decode(sig_clean).context("invalid signature hex")?;
    let prefix_clean = prefix.strip_prefix("0x").unwrap_or(prefix);
    let prefix_bytes = hex::decode(prefix_clean).context("invalid prefix hex")?;

    // Parse bytes16 id.
    let id_clean = id.strip_prefix("0x").unwrap_or(id);
    let id_bytes = hex::decode(id_clean).context("invalid id hex")?;
    let mut id_word = [0u8; 32];
    // bytes16 is left-aligned in the 32-byte word.
    let id_len = id_bytes.len().min(16);
    id_word[..id_len].copy_from_slice(&id_bytes[..id_len]);

    // Tuple head: 10 words. Dynamic field offsets are relative to the start of the tuple.
    let tuple_head_size: u64 = 10 * 32;

    // signature data starts right after the head.
    let sig_offset = tuple_head_size;
    // prefix data starts after signature (length word + padded data).
    let sig_padded_len = ((sig_bytes.len() + 31) / 32) * 32;
    let prefix_offset = sig_offset + 32 + sig_padded_len as u64;

    // Field 0: recipientAmount (uint256)
    buf.extend_from_slice(&encode_u256(recipient_amount));
    // Field 1: deadline (uint256)
    buf.extend_from_slice(&encode_u256_from_u64(deadline));
    // Field 2: recipient (address)
    buf.extend_from_slice(&pad_address(recipient)?);
    // Field 3: recipientCurrency (address)
    buf.extend_from_slice(&pad_address(recipient_currency)?);
    // Field 4: refundDestination (address)
    buf.extend_from_slice(&pad_address(refund_destination)?);
    // Field 5: feeAmount (uint256)
    buf.extend_from_slice(&encode_u256(fee_amount));
    // Field 6: id (bytes16)
    buf.extend_from_slice(&id_word);
    // Field 7: operator (address)
    buf.extend_from_slice(&pad_address(operator)?);
    // Field 8: offset to signature bytes
    buf.extend_from_slice(&encode_u256_from_u64(sig_offset));
    // Field 9: offset to prefix bytes
    buf.extend_from_slice(&encode_u256_from_u64(prefix_offset));

    // Dynamic field: signature
    buf.extend_from_slice(&encode_u256_from_u64(sig_bytes.len() as u64));
    let mut sig_padded = vec![0u8; sig_padded_len];
    sig_padded[..sig_bytes.len()].copy_from_slice(&sig_bytes);
    buf.extend_from_slice(&sig_padded);

    // Dynamic field: prefix
    let prefix_padded_len = ((prefix_bytes.len() + 31) / 32) * 32;
    buf.extend_from_slice(&encode_u256_from_u64(prefix_bytes.len() as u64));
    let mut prefix_padded = vec![0u8; prefix_padded_len];
    prefix_padded[..prefix_bytes.len()].copy_from_slice(&prefix_bytes);
    buf.extend_from_slice(&prefix_padded);

    Ok(buf)
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
