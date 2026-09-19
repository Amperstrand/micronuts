//! Token format dispatch: V4 (CBOR) is the core path; V3 (JSON) is a
//! read-compat layer for the legacy ecosystem (CDK wallets, older
//! tools, cashu-audit). V3 is parsed here — where `serde_json` lives —
//! and mapped into the V4 internal types; `cashu-core-lite` stays pure
//! no_std with no JSON dependency.

use cashu_core_lite::error::CashuError;
use cashu_core_lite::token::{decode_token, Proof as TokenProof, TokenV4, TokenV4Token};

/// Decode any Cashu token string (V3 or V4) into the internal V4 types.
/// V4 (`cashuB` + base64url CBOR) passes through to the core decoder.
/// V3 (`cashuA` + base64 JSON) is parsed here and mapped.
pub fn decode_token_any(token_str: &str) -> Result<TokenV4, CashuError> {
    if token_str.starts_with("cashuA") {
        decode_v3(token_str)
    } else {
        decode_token(token_str.as_bytes())
            .map_err(|e| CashuError::Protocol(format!("invalid token: {e}")))
    }
}

/// V3 format: `cashuA` + standard base64 of a JSON body:
/// ```json
/// {"token":[{"mint":"...","proofs":[{"id":"...","amount":N,"secret":"...","C":"hex"}]}],
///  "unit":"sat","memo":"..."}
/// ```
fn decode_v3(token_str: &str) -> Result<TokenV4, CashuError> {
    let b64_body = token_str
        .strip_prefix("cashuA")
        .ok_or_else(|| CashuError::Protocol("not a cashuA token".into()))?;

    // V3 uses standard base64 (with padding), not base64url
    use base64::Engine;
    let json_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_body)
        .map_err(|e| CashuError::Protocol(format!("V3 base64 decode: {e}")))?;

    let v3: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|e| CashuError::Protocol(format!("V3 JSON parse: {e}")))?;

    let groups = v3
        .get("token")
        .and_then(|t| t.as_array())
        .ok_or_else(|| CashuError::Protocol("V3: missing token array".into()))?;

    let mut tokens = Vec::new();
    for group in groups {
        let proofs_json = group
            .get("proofs")
            .and_then(|p| p.as_array())
            .ok_or_else(|| CashuError::Protocol("V3: missing proofs".into()))?;

        // Group by keyset_id (V3 proofs each carry their own `id`)
        let mut by_keyset: std::collections::BTreeMap<String, Vec<TokenProof>> =
            std::collections::BTreeMap::new();
        for p in proofs_json {
            let id = p
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CashuError::Protocol("V3: proof missing id".into()))?;
            let amount = p
                .get("amount")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| CashuError::Protocol("V3: proof missing amount".into()))?;
            let secret = p
                .get("secret")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CashuError::Protocol("V3: proof missing secret".into()))?
                .to_string();
            let c_hex = p
                .get("C")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CashuError::Protocol("V3: proof missing C".into()))?;
            let c = hex::decode(c_hex)
                .map_err(|e| CashuError::Protocol(format!("V3: proof C not hex: {e}")))?;

            by_keyset
                .entry(id.to_string())
                .or_default()
                .push(TokenProof {
                    amount,
                    keyset_id: id.to_string(),
                    secret,
                    c,
                    dleq: None, // V3 tokens don't carry DLEQ
                });
        }

        for (keyset_id, proofs) in by_keyset {
            tokens.push(TokenV4Token { keyset_id, proofs });
        }

    }

    let unit = v3
        .get("unit")
        .and_then(|u| u.as_str())
        .unwrap_or("sat")
        .to_string();
    let memo = v3
        .get("memo")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());

    // Take the mint from the first group (all groups in practice share it)
    let mint = groups
        .first()
        .and_then(|g| g.get("mint"))
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    Ok(TokenV4 {
        mint,
        unit,
        memo,
        tokens,
    })
}
