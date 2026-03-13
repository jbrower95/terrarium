use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// An ERC-4337 UserOperation.
#[derive(Debug, Clone, Serialize)]
pub struct UserOperation {
    pub sender: String,
    pub nonce: u64,
    pub call_data: Vec<u8>,
    /// The JWT bytes used as signature for OIDC-based account abstraction.
    pub signature: Vec<u8>,
    pub call_gas_limit: u64,
    pub verification_gas_limit: u64,
    pub pre_verification_gas: u64,
    pub max_fee_per_gas: u64,
    pub max_priority_fee_per_gas: u64,
}

/// Response shape from the GitHub Actions OIDC token endpoint.
#[derive(Deserialize)]
struct OidcTokenResponse {
    value: String,
}

/// Request a GitHub Actions OIDC token.
///
/// Uses the `ACTIONS_ID_TOKEN_REQUEST_URL` and `ACTIONS_ID_TOKEN_REQUEST_TOKEN`
/// environment variables that GitHub injects into Actions runs with
/// `permissions: id-token: write`.
pub async fn request_oidc_token(audience: Option<&str>) -> Result<String> {
    let request_url =
        std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL").context("ACTIONS_ID_TOKEN_REQUEST_URL not set")?;
    let request_token =
        std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN").context("ACTIONS_ID_TOKEN_REQUEST_TOKEN not set")?;

    let mut url = request_url;
    if let Some(aud) = audience {
        url = format!("{}&audience={}", url, aud);
    }

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&request_token)
        .header("Accept", "application/json; api-version=2.0")
        .send()
        .await
        .context("failed to request OIDC token")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OIDC token request failed ({}): {}", status, body);
    }

    let token_resp: OidcTokenResponse = resp
        .json()
        .await
        .context("failed to parse OIDC token response")?;

    Ok(token_resp.value)
}

/// ABI-encode a call to `execute(address,uint256,bytes)`.
///
/// Selector: `0xb61d27f6` (keccak256 of `execute(address,uint256,bytes)`).
fn encode_execute_calldata(to: &str, value: u64, inner_data: &[u8]) -> Vec<u8> {
    let selector: [u8; 4] = [0xb6, 0x1d, 0x27, 0xf6];

    // Pad address to 32 bytes (left-pad with zeros).
    let to_clean = to.strip_prefix("0x").unwrap_or(to);
    let to_bytes = hex::decode(to_clean).unwrap_or_else(|_| vec![0u8; 20]);
    let mut to_word = [0u8; 32];
    let offset = 32 - to_bytes.len().min(32);
    to_word[offset..].copy_from_slice(&to_bytes[..to_bytes.len().min(32)]);

    // Encode value as uint256 (big-endian, 32 bytes).
    let mut value_word = [0u8; 32];
    value_word[24..].copy_from_slice(&value.to_be_bytes());

    // Offset to the dynamic `bytes` parameter (3 * 32 = 96).
    let mut offset_word = [0u8; 32];
    offset_word[31] = 96;

    // Length of the inner data.
    let mut len_word = [0u8; 32];
    let data_len = inner_data.len() as u64;
    len_word[24..].copy_from_slice(&data_len.to_be_bytes());

    // Data padded to 32-byte boundary.
    let padded_len = ((inner_data.len() + 31) / 32) * 32;
    let mut padded_data = vec![0u8; padded_len];
    padded_data[..inner_data.len()].copy_from_slice(inner_data);

    let mut result = Vec::with_capacity(4 + 32 * 4 + padded_len);
    result.extend_from_slice(&selector);
    result.extend_from_slice(&to_word);
    result.extend_from_slice(&value_word);
    result.extend_from_slice(&offset_word);
    result.extend_from_slice(&len_word);
    result.extend_from_slice(&padded_data);
    result
}

/// Build a UserOperation with the OIDC JWT as its signature.
pub async fn build_user_op(
    sender: &str,
    to: &str,
    value: u64,
    calldata: Vec<u8>,
    jwt: &str,
    nonce: u64,
) -> Result<UserOperation> {
    let call_data = encode_execute_calldata(to, value, &calldata);

    Ok(UserOperation {
        sender: sender.to_string(),
        nonce,
        call_data,
        signature: jwt.as_bytes().to_vec(),
        call_gas_limit: 100_000,
        verification_gas_limit: 500_000,
        pre_verification_gas: 50_000,
        max_fee_per_gas: 1_000_000_000,       // 1 gwei
        max_priority_fee_per_gas: 100_000_000, // 0.1 gwei
    })
}

/// Hex-encode bytes with a `0x` prefix.
fn to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// Submit a UserOperation to an ERC-4337 bundler via JSON-RPC.
///
/// Returns the user operation hash.
pub async fn submit_user_op(
    bundler_url: &str,
    entry_point: &str,
    op: &UserOperation,
) -> Result<String> {
    let user_op = serde_json::json!({
        "sender": op.sender,
        "nonce": format!("0x{:x}", op.nonce),
        "callData": to_hex(&op.call_data),
        "signature": to_hex(&op.signature),
        "callGasLimit": format!("0x{:x}", op.call_gas_limit),
        "verificationGasLimit": format!("0x{:x}", op.verification_gas_limit),
        "preVerificationGas": format!("0x{:x}", op.pre_verification_gas),
        "maxFeePerGas": format!("0x{:x}", op.max_fee_per_gas),
        "maxPriorityFeePerGas": format!("0x{:x}", op.max_priority_fee_per_gas),
    });

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_sendUserOperation",
        "params": [user_op, entry_point],
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(bundler_url)
        .json(&body)
        .send()
        .await
        .context("failed to send user operation to bundler")?;

    let status = resp.status();
    let json: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse bundler response")?;

    if let Some(error) = json.get("error") {
        anyhow::bail!("bundler error ({}): {}", status, error);
    }

    let hash = json
        .get("result")
        .and_then(|v| v.as_str())
        .context("missing result in bundler response")?
        .to_string();

    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_execute_produces_correct_selector() {
        let encoded = encode_execute_calldata(
            "0x0000000000000000000000000000000000000001",
            0,
            &[],
        );
        assert_eq!(&encoded[..4], &[0xb6, 0x1d, 0x27, 0xf6]);
    }

    #[test]
    fn encode_execute_address_padding() {
        let encoded = encode_execute_calldata(
            "0x0000000000000000000000000000000000000001",
            0,
            &[],
        );
        // Address should be in the last byte of the first 32-byte word after selector.
        assert_eq!(encoded[4 + 31], 0x01);
        // All preceding bytes should be zero.
        assert!(encoded[4..4 + 31].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_execute_value_encoding() {
        let encoded = encode_execute_calldata(
            "0x0000000000000000000000000000000000000001",
            42,
            &[],
        );
        // Value is in the second 32-byte word, big-endian u64 in last 8 bytes.
        let value_start = 4 + 32;
        assert_eq!(encoded[value_start + 31], 42);
    }

    #[test]
    fn encode_execute_with_inner_data() {
        let inner = vec![0xaa, 0xbb, 0xcc];
        let encoded = encode_execute_calldata(
            "0x0000000000000000000000000000000000000001",
            0,
            &inner,
        );
        // After selector + 3 words (address, value, offset) + length word, data starts.
        let data_start = 4 + 32 * 4;
        assert_eq!(&encoded[data_start..data_start + 3], &[0xaa, 0xbb, 0xcc]);
        // Should be padded to 32 bytes.
        assert_eq!(encoded.len(), data_start + 32);
    }

    #[tokio::test]
    async fn build_user_op_sets_jwt_as_signature() {
        let op = build_user_op(
            "0xsender",
            "0x0000000000000000000000000000000000000001",
            0,
            vec![],
            "my.jwt.token",
            5,
        )
        .await
        .unwrap();

        assert_eq!(op.sender, "0xsender");
        assert_eq!(op.nonce, 5);
        assert_eq!(op.signature, b"my.jwt.token");
        assert!(!op.call_data.is_empty());
    }

    #[test]
    fn to_hex_works() {
        assert_eq!(to_hex(&[0xde, 0xad]), "0xdead");
        assert_eq!(to_hex(&[]), "0x");
    }
}
