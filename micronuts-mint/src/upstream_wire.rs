//! Wire codecs for talking to the upstream Cashu mint over HTTP JSON:
//! NUT-00 blinded-message/proof/signature JSON, keyset parsing, and the
//! pending-output records needed to unblind responses. Pure conversion —
//! no HTTP, no wallet state.

use std::collections::HashMap;

use cashu_core_lite::crypto::blind_message;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut00;
use rand::RngCore;
use serde_json::{json, Value};

use crate::upstream::{json_amount_or, json_str};

/// Upstream keyset needed to blind/unblind reserve outputs.
#[derive(Clone)]
pub(crate) struct UpstreamKeyset {
    pub(crate) id: String,
    pub(crate) input_fee_ppk: u64,
    pub(crate) keys: HashMap<u64, PublicKey>,
}

impl UpstreamKeyset {
    /// Pick the first active keyset matching `unit` from a `/v1/keys`
    /// response body.
    pub(crate) fn from_keys_response(value: &Value, unit: &str) -> Result<Self, CashuError> {
        let keysets = value
            .get("keysets")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                CashuError::Protocol("upstream /v1/keys: no keysets array".to_string())
            })?;
        for keyset in keysets {
            let keyset_unit = keyset.get("unit").and_then(Value::as_str).unwrap_or("");
            let active = keyset
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if keyset_unit != unit || !active {
                continue;
            }
            return Ok(UpstreamKeyset {
                id: json_str("id", &keyset["id"])?,
                input_fee_ppk: json_amount_or("input_fee_ppk", keyset.get("input_fee_ppk"), 0)?,
                keys: parse_keyset_keys(keyset)?,
            });
        }
        Err(CashuError::KeysetNotFound)
    }
}

/// Blinded output we are waiting to unblind (secret + blinder). The
/// signature's amount (imprinted by the mint for blanks) is authoritative.
pub(crate) struct PendingChange {
    pub(crate) secret_hex: String,
    pub(crate) blinder: SecretKey,
}

/// Blind power-of-two outputs for `amounts`, returning the wire JSON and
/// the pending records needed to unblind the response.
pub(crate) fn blind_outputs(
    amounts: &[u64],
    keyset_id: &str,
) -> Result<(Vec<Value>, Vec<PendingChange>), CashuError> {
    let mut outputs = Vec::with_capacity(amounts.len());
    let mut pending = Vec::with_capacity(amounts.len());
    for &amount in amounts {
        // Random 32-byte secret per output — NOT NUT-13 deterministic.
        let mut secret = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret);
        let secret_hex = hex::encode(secret);
        // `blinder: None` draws a fresh random scalar (std feature).
        let blinded = blind_message(secret_hex.as_bytes(), None)
            .map_err(|_| CashuError::Crypto("reserve blind_message failed".to_string()))?;
        outputs.push(json!({
            "amount": amount,
            "B_": hex::encode(blinded.blinded.to_bytes()),
            "id": keyset_id,
        }));
        pending.push(PendingChange {
            secret_hex,
            blinder: blinded.blinder,
        });
    }
    Ok((outputs, pending))
}

pub(crate) fn signatures_array(value: &Value) -> Result<&Vec<Value>, CashuError> {
    value
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or_else(|| CashuError::Protocol("upstream mint: no signatures array".to_string()))
}

/// Wire JSON for a proof (NUT-00: `C` compressed-point hex).
pub(crate) fn proof_json(proof: &nut00::Proof) -> Value {
    json!({
        "amount": proof.amount,
        "id": proof.id,
        "secret": proof.secret,
        "C": hex::encode(proof.c.to_bytes()),
    })
}

/// Parse a hex-encoded compressed curve point.
pub(crate) fn json_point(value: &Value) -> Result<PublicKey, CashuError> {
    let hex_str = json_str("point", value)?;
    let bytes = hex::decode(&hex_str)
        .map_err(|_| CashuError::Protocol("upstream point is not hex".to_string()))?;
    let compressed: [u8; 33] = bytes
        .try_into()
        .map_err(|_| CashuError::Protocol("upstream point is not 33 bytes".to_string()))?;
    PublicKey::from_bytes(&compressed)
        .ok_or_else(|| CashuError::Protocol("upstream point is not a valid key".to_string()))
}

fn parse_keyset_keys(keyset: &Value) -> Result<HashMap<u64, PublicKey>, CashuError> {
    let entries = keyset
        .get("keys")
        .and_then(Value::as_object)
        .ok_or_else(|| CashuError::Protocol("upstream keyset: no keys map".to_string()))?;
    let mut keys = HashMap::with_capacity(entries.len());
    for (amount, pubkey) in entries {
        let parsed: u64 = amount.parse().map_err(|_| {
            CashuError::Protocol(format!("upstream keyset: bad amount key '{amount}'"))
        })?;
        keys.insert(parsed, json_point(pubkey)?);
    }
    Ok(keys)
}
