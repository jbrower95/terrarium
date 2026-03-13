use anyhow::{Context, Result};
use serde::Deserialize;

/// The Zora bonding-curve token factory address on Base.
const ZORA_FACTORY: &str = "0x777777751622c0d3258f214F9DF38E35BF45baF3";

/// Information about a deployed Zora bonding-curve token.
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub total_supply: String,
    pub market_cap_eth: f64,
}

/// JSON-RPC response envelope.
#[derive(Deserialize)]
struct JsonRpcResponse {
    result: Option<String>,
    error: Option<serde_json::Value>,
}

/// Send a raw JSON-RPC `eth_call` and return the hex result.
async fn eth_call(rpc_url: &str, to: &str, data: &str) -> Result<String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [{ "to": to, "data": data }, "latest"],
    });

    let client = reqwest::Client::new();
    let resp: JsonRpcResponse = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .context("eth_call request failed")?
        .json()
        .await
        .context("failed to parse eth_call response")?;

    if let Some(err) = resp.error {
        anyhow::bail!("eth_call error: {}", err);
    }

    resp.result.context("missing result in eth_call response")
}

/// Send a raw JSON-RPC `eth_sendTransaction` call and return the tx hash.
async fn eth_send_transaction(rpc_url: &str, to: &str, data: &str) -> Result<String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_sendTransaction",
        "params": [{ "to": to, "data": data }],
    });

    let client = reqwest::Client::new();
    let resp: JsonRpcResponse = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .context("eth_sendTransaction request failed")?
        .json()
        .await
        .context("failed to parse eth_sendTransaction response")?;

    if let Some(err) = resp.error {
        anyhow::bail!("eth_sendTransaction error: {}", err);
    }

    resp.result.context("missing result in eth_sendTransaction response")
}

/// Hex-encode a string as an ABI-encoded `string` (length word + padded data).
fn abi_encode_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let padded_len = ((bytes.len() + 31) / 32) * 32;

    let mut encoded = vec![0u8; 32 + padded_len];
    // Length word.
    let len = bytes.len() as u64;
    encoded[24..32].copy_from_slice(&len.to_be_bytes());
    // Data.
    encoded[32..32 + bytes.len()].copy_from_slice(bytes);
    encoded
}

/// Pad a 20-byte address to a 32-byte ABI word.
fn abi_encode_address(addr: &str) -> [u8; 32] {
    let clean = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(clean).unwrap_or_else(|_| vec![0u8; 20]);
    let mut word = [0u8; 32];
    let offset = 32 - bytes.len().min(32);
    word[offset..].copy_from_slice(&bytes[..bytes.len().min(32)]);
    word
}

/// Deploy a Zora bonding-curve token via the factory contract on Base.
///
/// Constructs calldata for the factory's `deploy` function and submits
/// an `eth_sendTransaction` JSON-RPC call. Returns the transaction hash.
///
/// Note: In production this would typically be routed through an ERC-4337
/// UserOperation (see `wallet.rs`). This function is for direct RPC usage.
pub async fn deploy_token(
    name: &str,
    symbol: &str,
    payout_recipient: &str,
    rpc_url: &str,
) -> Result<String> {
    // Build ABI-encoded calldata for the factory's deploy function.
    // Selector: 0x6d3a4b8e — placeholder for the factory's deploy(string,string,address).
    let selector: [u8; 4] = [0x6d, 0x3a, 0x4b, 0x8e];

    let recipient_word = abi_encode_address(payout_recipient);

    let name_encoded = abi_encode_string(name);
    let symbol_encoded = abi_encode_string(symbol);

    // Layout: selector | offset_name | offset_symbol | recipient | name_data | symbol_data
    // Fixed params occupy 3 words (96 bytes).
    let name_offset: u64 = 96;
    let symbol_offset: u64 = name_offset + name_encoded.len() as u64;

    let mut calldata = Vec::new();
    calldata.extend_from_slice(&selector);

    // Offset to name string.
    let mut word = [0u8; 32];
    word[24..].copy_from_slice(&name_offset.to_be_bytes());
    calldata.extend_from_slice(&word);

    // Offset to symbol string.
    let mut word = [0u8; 32];
    word[24..].copy_from_slice(&symbol_offset.to_be_bytes());
    calldata.extend_from_slice(&word);

    // Payout recipient address.
    calldata.extend_from_slice(&recipient_word);

    // Name string data.
    calldata.extend_from_slice(&name_encoded);

    // Symbol string data.
    calldata.extend_from_slice(&symbol_encoded);

    let data_hex = format!("0x{}", hex::encode(&calldata));
    let tx_hash = eth_send_transaction(rpc_url, ZORA_FACTORY, &data_hex).await?;

    Ok(tx_hash)
}

/// Decode a 32-byte hex word as a U256 and return it as a decimal string.
fn decode_uint256(hex_word: &str) -> String {
    let clean = hex_word.strip_prefix("0x").unwrap_or(hex_word);
    let trimmed = clean.trim_start_matches('0');
    if trimmed.is_empty() {
        return "0".to_string();
    }
    u128::from_str_radix(trimmed, 16)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| format!("0x{}", clean))
}

/// Decode an ABI-encoded string return value.
fn decode_abi_string(hex_data: &str) -> Option<String> {
    let clean = hex_data.strip_prefix("0x").unwrap_or(hex_data);
    let bytes = hex::decode(clean).ok()?;
    if bytes.len() < 64 {
        return None;
    }
    // First 32 bytes: offset to string data. Second 32 bytes: length.
    let len_bytes = &bytes[32..64];
    let len = u64::from_be_bytes(len_bytes[24..32].try_into().ok()?) as usize;
    if bytes.len() < 64 + len {
        return None;
    }
    String::from_utf8(bytes[64..64 + len].to_vec()).ok()
}

/// Query on-chain token info: name, symbol, and totalSupply.
pub async fn get_token_info(token_address: &str, rpc_url: &str) -> Result<TokenInfo> {
    // name() selector: 0x06fdde03
    let name_data = eth_call(rpc_url, token_address, "0x06fdde03").await?;
    // symbol() selector: 0x95d89b41
    let symbol_data = eth_call(rpc_url, token_address, "0x95d89b41").await?;
    // totalSupply() selector: 0x18160ddd
    let supply_data = eth_call(rpc_url, token_address, "0x18160ddd").await?;

    let name = decode_abi_string(&name_data).unwrap_or_default();
    let symbol = decode_abi_string(&symbol_data).unwrap_or_default();
    let total_supply = decode_uint256(&supply_data);

    Ok(TokenInfo {
        address: token_address.to_string(),
        name,
        symbol,
        total_supply,
        market_cap_eth: 0.0, // Would require a price oracle or DEX query.
    })
}

/// Get the Zora UI URL for a token on Base.
pub fn get_zora_url(token_address: &str) -> String {
    format!("https://zora.co/coin/base/{}", token_address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zora_url_format() {
        let url = get_zora_url("0xabcdef1234567890abcdef1234567890abcdef12");
        assert_eq!(
            url,
            "https://zora.co/coin/base/0xabcdef1234567890abcdef1234567890abcdef12"
        );
    }

    #[test]
    fn abi_encode_address_pads_correctly() {
        let word = abi_encode_address("0x0000000000000000000000000000000000000001");
        assert_eq!(word[31], 0x01);
        assert!(word[..31].iter().all(|&b| b == 0));
    }

    #[test]
    fn abi_encode_string_roundtrip() {
        let encoded = abi_encode_string("hello");
        // Length should be 5.
        assert_eq!(encoded[31], 5);
        // Data starts at byte 32.
        assert_eq!(&encoded[32..37], b"hello");
    }

    #[test]
    fn decode_uint256_zero() {
        assert_eq!(
            decode_uint256("0x0000000000000000000000000000000000000000000000000000000000000000"),
            "0"
        );
    }

    #[test]
    fn decode_uint256_nonzero() {
        assert_eq!(
            decode_uint256("0x000000000000000000000000000000000000000000000000000000000000002a"),
            "42"
        );
    }

    #[test]
    fn decode_abi_string_works() {
        // Encode "hello" as ABI string: offset(32) + length(5) + "hello" padded.
        let mut data = vec![0u8; 96];
        data[31] = 32; // offset
        data[63] = 5;  // length
        data[64..69].copy_from_slice(b"hello");
        let hex_str = format!("0x{}", hex::encode(&data));
        assert_eq!(decode_abi_string(&hex_str), Some("hello".to_string()));
    }

    #[test]
    fn decode_abi_string_empty() {
        assert_eq!(decode_abi_string("0x"), None);
    }

    #[test]
    fn zora_factory_address_is_correct() {
        assert_eq!(ZORA_FACTORY, "0x777777751622c0d3258f214F9DF38E35BF45baF3");
    }
}
