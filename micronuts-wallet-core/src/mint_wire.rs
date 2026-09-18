//! Transport-neutral Cashu wire protocol: one `MintClient`
//! implementation over any JSON transport (ureq on native, sync XHR in
//! the browser). Routes, request bodies, response parsing, field
//! strictness, and error wording live here exactly once; each platform
//! supplies only `get_json`/`post_json`. The wire contract mirrors
//! `micronuts-audit-adapter/src/main.rs` (`parse_*` and `*_to_json`):
//! `B_`, `C_`, `C`, `Ys`, `dleq.e/s`, keys keyed by amount-as-string.
//!
//! Field strictness: fields the NUT specs mark required (or that this
//! wallet's logic consumes) are strict — a missing or ill-typed field is
//! an error, never a silent default. Mint-specific accounting extras
//! (`amount_paid`, `updated_at`, …) default when absent so the client
//! also works against minimal foreign mints.

use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut12::BlindSignatureDleq;
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::transport::MintClient;
use serde_json::{json, Value};

/// What a platform must provide: one blocking JSON round-trip per call.
pub trait JsonTransport {
    fn get_json(&self, path: &str) -> Result<Value, CashuError>;
    fn post_json(&self, path: &str, body: &Value) -> Result<Value, CashuError>;
}

/// A Cashu mint client over any [`JsonTransport`]. `Clone` requires the
/// transport to be `Clone` (both are cheap handle clones).
#[derive(Clone)]
pub struct WireMintClient<T: JsonTransport> {
    base: String,
    transport: T,
}

impl<T: JsonTransport> WireMintClient<T> {
    /// Build a client for `base_url`; trailing slashes trimmed.
    pub fn with_transport(base_url: &str, transport: T) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            transport,
        }
    }

    /// The mint base URL this client talks to.
    pub fn base_url(&self) -> &str {
        &self.base
    }
}

impl<T: JsonTransport> WireMintClient<T> {
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
}

impl<T: JsonTransport> MintClient for WireMintClient<T> {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        let body = self.transport.get_json(&self.url("/v1/info"))?;
        parse_mint_info(&body)
    }

    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        let body = self.transport.get_json(&self.url("/v1/keys"))?;
        let keysets = body
            .get("keysets")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol("keys response: missing keysets array"))?;
        let mut parsed = Vec::with_capacity(keysets.len());
        for keyset in keysets {
            parsed.push(parse_keyset(keyset)?);
        }
        Ok(nut01::KeysResponse { keysets: parsed })
    }

    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        let body = self.transport.get_json(&self.url("/v1/keysets"))?;
        let keysets = body
            .get("keysets")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol("keysets response: missing keysets array"))?;
        let mut parsed = Vec::with_capacity(keysets.len());
        for ks in keysets {
            parsed.push(nut02::KeysetInfo {
                id: str_field(ks, "id", "keyset info")?,
                unit: str_field(ks, "unit", "keyset info")?,
                active: bool_field(ks, "active", "keyset info")?,
                input_fee_ppk: u64_field(ks, "input_fee_ppk", "keyset info")?,
            });
        }
        Ok(nut02::KeysetsResponse { keysets: parsed })
    }

    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        let mut body = json!({ "amount": request.amount, "unit": request.unit });
        if let Some(pubkey) = &request.pubkey {
            body["pubkey"] = json!(pubkey);
        }
        let response = self
            .transport
            .post_json(&self.url("/v1/mint/quote/bolt11"), &body)?;
        parse_mint_quote_response(&response)
    }

    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        let path = format!("/v1/mint/quote/bolt11/{}", urlencode(quote_id));
        let response = self.transport.get_json(&self.url(&path))?;
        parse_mint_quote_response(&response)
    }

    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        let mut body = json!({
            "quote": request.quote,
            "outputs": request.outputs.iter().map(blinded_message_to_json).collect::<Vec<_>>(),
        });
        if let Some(signature) = &request.signature {
            body["signature"] = json!(signature);
        }
        let response = self
            .transport
            .post_json(&self.url("/v1/mint/bolt11"), &body)?;
        Ok(nut04::MintResponse {
            signatures: parse_signatures(&response)?,
        })
    }

    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        let body = json!({ "request": request.request, "unit": request.unit });
        let response = self
            .transport
            .post_json(&self.url("/v1/melt/quote/bolt11"), &body)?;
        parse_melt_quote_response(&response)
    }

    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        let path = format!("/v1/melt/quote/bolt11/{}", urlencode(quote_id));
        let response = self.transport.get_json(&self.url(&path))?;
        parse_melt_quote_response(&response)
    }

    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        let mut body = json!({
            "quote": request.quote,
            "inputs": request.inputs.iter().map(proof_to_json).collect::<Vec<_>>(),
        });
        if let Some(outputs) = &request.outputs {
            body["outputs"] = json!(outputs
                .iter()
                .map(blinded_message_to_json)
                .collect::<Vec<_>>());
        }
        let response = self
            .transport
            .post_json(&self.url("/v1/melt/bolt11"), &body)?;
        parse_melt_response(&response)
    }

    fn post_swap(
        &mut self,
        request: nut03::SwapRequest,
    ) -> Result<nut03::SwapResponse, CashuError> {
        let body = json!({
            "inputs": request.inputs.iter().map(proof_to_json).collect::<Vec<_>>(),
            "outputs": request.outputs.iter().map(blinded_message_to_json).collect::<Vec<_>>(),
        });
        let response = self.transport.post_json(&self.url("/v1/swap"), &body)?;
        Ok(nut03::SwapResponse {
            signatures: parse_signatures(&response)?,
        })
    }

    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        let ys: Vec<String> = request
            .ys
            .iter()
            .map(|y| hex::encode(y.to_bytes()))
            .collect();
        let body = json!({ "Ys": ys });
        let response = self
            .transport
            .post_json(&self.url("/v1/checkstate"), &body)?;
        let states = response
            .get("states")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol("checkstate response: missing states array"))?;
        let mut parsed = Vec::with_capacity(states.len());
        for state in states {
            parsed.push(nut07::ProofState {
                y: point_field(state, "Y", "proof state")?,
                state: str_field(state, "state", "proof state")?,
                witness: opt_str_field(state, "witness"),
            });
        }
        Ok(nut07::CheckStateResponse { states: parsed })
    }

    fn post_restore(
        &mut self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        // NUT-09 spec: flat parallel arrays — outputs[i] (B_ hex) pairs with
        // signatures[i]. The adapter emits exactly this shape.
        let outputs: Vec<String> = request
            .outputs
            .iter()
            .map(|y| hex::encode(y.to_bytes()))
            .collect();
        let body = json!({ "outputs": outputs });
        let response = self.transport.post_json(&self.url("/v1/restore"), &body)?;
        let ys = response
            .get("outputs")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol("restore response: missing outputs array"))?;
        let signatures = parse_signatures(&response)?;
        if ys.len() != signatures.len() {
            return Err(protocol(
                "restore response: outputs/signatures length mismatch",
            ));
        }
        let mut parsed = Vec::with_capacity(ys.len());
        for (y, signature) in ys.iter().zip(signatures) {
            let y_hex = y
                .as_str()
                .ok_or_else(|| protocol("restore response: outputs entries must be hex strings"))?;
            parsed.push(nut09::RestoreOutput {
                y: point_from_hex(y_hex)?,
                signature,
            });
        }
        Ok(nut09::RestoreResponse { outputs: parsed })
    }
}

// ---------------------------------------------------------------------------
// JSON → lite type parsers (strict on spec-required fields)
// ---------------------------------------------------------------------------

fn protocol(detail: impl Into<String>) -> CashuError {
    CashuError::Protocol(detail.into())
}

/// First 200 chars of a response body, for parse-error diagnostics.
fn truncate_body(value: &Value) -> String {
    let text = value.to_string();
    text.chars().take(200).collect()
}

fn u64_field(value: &Value, field: &str, ctx: &str) -> Result<u64, CashuError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| protocol(format!("{ctx}: missing or invalid `{field}`")))
}

fn str_field(value: &Value, field: &str, ctx: &str) -> Result<String, CashuError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| protocol(format!("{ctx}: missing or invalid `{field}`")))
}

fn bool_field(value: &Value, field: &str, ctx: &str) -> Result<bool, CashuError> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| protocol(format!("{ctx}: missing or invalid `{field}`")))
}

fn opt_str_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

fn point_from_hex(s: &str) -> Result<PublicKey, CashuError> {
    let bytes = hex::decode(s).map_err(|_| protocol("point field: not valid hex"))?;
    let arr: [u8; 33] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| protocol("point field: expected 33 bytes (66 hex chars)"))?;
    PublicKey::from_bytes(&arr)
        .ok_or_else(|| protocol("point field: not a valid compressed secp256k1 point"))
}

fn point_field(value: &Value, field: &str, ctx: &str) -> Result<PublicKey, CashuError> {
    let s = str_field(value, field, ctx)?;
    point_from_hex(&s).map_err(|_| protocol(format!("{ctx}: invalid `{field}` point")))
}

/// Parse a keyset: `{"id", "unit", "keys": {"<amount>": "<pubkey hex>"}}`.
/// Keys arrive sorted by amount ascending.
fn parse_keyset(value: &Value) -> Result<nut01::KeySet, CashuError> {
    let id = str_field(value, "id", "keyset")?;
    let unit = str_field(value, "unit", "keyset")?;
    let keys_map = value
        .get("keys")
        .and_then(Value::as_object)
        .ok_or_else(|| protocol("keyset: missing keys map"))?;
    let mut keys = Vec::with_capacity(keys_map.len());
    for (amount_str, pubkey) in keys_map {
        let amount: u64 = amount_str
            .parse()
            .map_err(|_| protocol("keyset: non-numeric amount key"))?;
        let pubkey_hex = pubkey
            .as_str()
            .ok_or_else(|| protocol("keyset: pubkey values must be hex strings"))?;
        keys.push(nut01::KeyPair {
            amount,
            pubkey: point_from_hex(pubkey_hex)
                .map_err(|_| protocol("keyset: invalid keyset pubkey point"))?,
        });
    }
    keys.sort_by_key(|kp| kp.amount);
    Ok(nut01::KeySet { id, unit, keys })
}

fn parse_signatures(value: &Value) -> Result<Vec<nut00::BlindSignature>, CashuError> {
    let signatures = value
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            protocol(format!(
                "signatures response: missing signatures array (body: {})",
                truncate_body(value)
            ))
        })?;
    let mut parsed = Vec::with_capacity(signatures.len());
    for sig in signatures {
        parsed.push(parse_signature_entry(sig)?);
    }
    Ok(parsed)
}

fn parse_signature_entry(sig: &Value) -> Result<nut00::BlindSignature, CashuError> {
    let dleq = match sig.get("dleq") {
        None | Some(Value::Null) => None,
        Some(dleq) => Some(BlindSignatureDleq {
            e: scalar_field(dleq, "e")?,
            s: scalar_field(dleq, "s")?,
        }),
    };
    Ok(nut00::BlindSignature {
        amount: u64_field(sig, "amount", "blind signature")?,
        id: str_field(sig, "id", "blind signature")?,
        c: point_field(sig, "C_", "blind signature")?,
        dleq,
    })
}

fn scalar_field(value: &Value, field: &str) -> Result<SecretKey, CashuError> {
    let s = str_field(value, field, "dleq")?;
    let bytes = hex::decode(&s).map_err(|_| protocol("dleq: scalar not valid hex"))?;
    SecretKey::from_slice(&bytes).map_err(|_| protocol("dleq: invalid scalar"))
}

fn parse_mint_quote_response(value: &Value) -> Result<nut04::MintQuoteResponse, CashuError> {
    let state = str_field(value, "state", "mint quote")?;
    let paid = value
        .get("paid")
        .and_then(Value::as_bool)
        .unwrap_or(matches!(state.as_str(), "PAID" | "ISSUED"));
    Ok(nut04::MintQuoteResponse {
        quote: str_field(value, "quote", "mint quote")?,
        request: str_field(value, "request", "mint quote")?,
        paid,
        state,
        expiry: value
            .get("expiry")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        amount: value
            .get("amount")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        unit: value
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or("sat")
            .to_string(),
        amount_paid: value
            .get("amount_paid")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        amount_issued: value
            .get("amount_issued")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        updated_at: value.get("updated_at").and_then(Value::as_u64).unwrap_or(0),
        method: value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("bolt11")
            .to_string(),
        pubkey: opt_str_field(value, "pubkey"),
    })
}

fn parse_melt_quote_response(value: &Value) -> Result<nut05::MeltQuoteResponse, CashuError> {
    let state = str_field(value, "state", "melt quote")?;
    let paid = value
        .get("paid")
        .and_then(Value::as_bool)
        .unwrap_or(state == "PAID");
    Ok(nut05::MeltQuoteResponse {
        quote: str_field(value, "quote", "melt quote")?,
        amount: u64_field(value, "amount", "melt quote")?,
        fee_reserve: u64_field(value, "fee_reserve", "melt quote")?,
        paid,
        state,
        expiry: value
            .get("expiry")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        request: opt_str_field(value, "request").unwrap_or_default(),
        unit: value
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or("sat")
            .to_string(),
        method: value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("bolt11")
            .to_string(),
    })
}

fn parse_melt_response(value: &Value) -> Result<nut05::MeltResponse, CashuError> {
    let change = match value.get("change") {
        None | Some(Value::Null) => None,
        Some(Value::Array(entries)) if entries.is_empty() => None,
        Some(Value::Array(entries)) => {
            let mut parsed = Vec::with_capacity(entries.len());
            for entry in entries {
                parsed.push(parse_signature_entry(entry)?);
            }
            Some(parsed)
        }
        Some(_) => return Err(protocol("melt response: change must be an array")),
    };
    Ok(nut05::MeltResponse {
        paid: bool_field(value, "paid", "melt response")?,
        state: str_field(value, "state", "melt response")?,
        payment_preimage: opt_str_field(value, "payment_preimage"),
        change,
        quote: opt_str_field(value, "quote").unwrap_or_default(),
        amount: value.get("amount").and_then(Value::as_u64).unwrap_or(0),
        fee_reserve: value
            .get("fee_reserve")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        unit: value
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or("sat")
            .to_string(),
        expiry: value.get("expiry").and_then(Value::as_u64).unwrap_or(0),
        request: opt_str_field(value, "request").unwrap_or_default(),
        method: value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("bolt11")
            .to_string(),
    })
}

/// `/v1/info` is display-only metadata; parse the fields we render and
/// default the rest, so foreign mints with richer `nuts` maps still load.
fn parse_mint_info(value: &Value) -> Result<nut06::MintInfo, CashuError> {
    let contact = value
        .get("contact")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|c| nut06::ContactInfo {
                    method: opt_str_field(c, "method").unwrap_or_default(),
                    info: opt_str_field(c, "info").unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut nuts: Vec<(String, nut06::NutSettings)> = Vec::new();
    if let Some(nuts_map) = value.get("nuts").and_then(Value::as_object) {
        for (nut_id, settings) in nuts_map {
            nuts.push((nut_id.clone(), parse_nut_settings(settings)));
        }
        nuts.sort_by(|a, b| natural_nut_order(&a.0, &b.0));
    }

    Ok(nut06::MintInfo {
        name: str_field(value, "name", "mint info")?,
        pubkey: opt_str_field(value, "pubkey").unwrap_or_default(),
        version: opt_str_field(value, "version").unwrap_or_default(),
        description: opt_str_field(value, "description").unwrap_or_default(),
        contact,
        nuts,
    })
}

fn parse_nut_settings(value: &Value) -> nut06::NutSettings {
    let mut settings = nut06::NutSettings::default();
    if let Some(methods) = value.get("methods").and_then(Value::as_array) {
        // NUT-29 advertises methods as plain strings; NUT-04/05 as objects.
        for method in methods {
            match method {
                Value::String(name) => settings.methods.push(nut06::PaymentMethod {
                    method: name.clone(),
                    unit: String::from("sat"),
                }),
                Value::Object(_) => {
                    settings.methods.push(nut06::PaymentMethod {
                        method: opt_str_field(method, "method").unwrap_or_default(),
                        unit: opt_str_field(method, "unit").unwrap_or_default(),
                    });
                }
                _ => {}
            }
        }
    }
    settings.supported = value.get("supported").and_then(Value::as_bool);
    settings.ttl = value.get("ttl").and_then(Value::as_u64);
    settings.max_batch_size = value.get("max_batch_size").and_then(Value::as_u64);
    if let Some(endpoints) = value.get("cached_endpoints").and_then(Value::as_array) {
        for endpoint in endpoints {
            settings
                .cached_endpoints
                .push(nut19_cached_endpoint(endpoint));
        }
    }
    settings
}

fn nut19_cached_endpoint(value: &Value) -> cashu_core_lite::nuts::nut19::CachedEndpoint {
    cashu_core_lite::nuts::nut19::CachedEndpoint {
        method: opt_str_field(value, "method").unwrap_or_default(),
        path: opt_str_field(value, "path").unwrap_or_default(),
    }
}

/// Numeric sort for NUT ids ("2" < "10" < "29").
fn natural_nut_order(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => a.cmp(b),
    }
}

// ---------------------------------------------------------------------------
// lite type → JSON serializers
// ---------------------------------------------------------------------------

fn blinded_message_to_json(message: &nut00::BlindedMessage) -> Value {
    json!({
        "amount": message.amount,
        "id": message.id,
        "B_": hex::encode(message.b.to_bytes()),
    })
}

fn proof_to_json(proof: &nut00::Proof) -> Value {
    let mut value = json!({
        "amount": proof.amount,
        "id": proof.id,
        "secret": proof.secret,
        "C": hex::encode(proof.c.to_bytes()),
    });
    if let Some(witness) = &proof.witness {
        value["witness"] = json!(witness);
    }
    value
}

/// Minimal percent-encoding for path segments (quote ids are UUID-ish, but
/// stay safe against `/` or spaces).
fn urlencode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
