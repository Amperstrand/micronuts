//! HTTP-to-CBOR-RPC bridge for the Micronuts demo mint.
//!
//! Spawns `micronuts-mint`'s `mint_server` binary as a subprocess and exposes
//! the Cashu REST endpoints (NUT-01 through NUT-09) by translating each HTTP
//! request into a CBOR `MintRpcRequest` frame, writing it as one hex line to
//! the subprocess stdin, reading one hex CBOR `MintRpcResponse` line back from
//! stdout, decoding it, and returning the payload as JSON that matches the
//! Cashu HTTP field-name spec (`B_`, `C_`, `C`, `Ys`, `Y`, etc.).
//!
//! The framing protocol is documented in `docs/MINT-WALLET-DEMO.md`.
//!
//! Run from the workspace root so `target/debug/mint_server` resolves:
//!   cargo run -p micronuts-audit-adapter
//!
//! Override defaults via env:
//!   MICRONUTS_ADAPTER_PORT=4000     Listen port (default 3030)
//!   MICRONUTS_MINT_BIN=/path/mint   Override subprocess binary path

// Handlers and parse helpers return axum `Response` in Err so `?` works
// directly; the large-err footprint is an accepted simplicity trade for
// this standalone adapter.
#![allow(clippy::result_large_err)]

use std::env;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::rpc::{
    decode_rpc_response, encode_rpc_request, MeltQuoteLookupRequest, MintQuoteLookupRequest,
    MintRpcMethod, MintRpcPayload, MintRpcRequest, MintRpcResult,
};
use cashu_core_lite::{keypair::PublicKey, rpc::MintRpcResponse};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

const DEFAULT_PORT: u16 = 3030;
const DEFAULT_MINT_BIN: &str = "target/debug/mint_server";

/// Wrapped subprocess holding owned pipes behind a mutex so handlers serialize
/// all access. The id counter pairs request/response frames over the line
/// protocol. The `Child` handle is held for lifecycle only (kill on drop) and
/// is never read directly after spawn.
struct MintProcess {
    /// Held only for lifecycle — `kill_on_drop(true)` reaps the subprocess when
    /// the adapter exits. Never read after `spawn`.
    #[allow(dead_code)]
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u32,
}

impl MintProcess {
    fn spawn(bin_path: &str) -> std::io::Result<Self> {
        let mut command = tokio::process::Command::new(bin_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!(
                    "micronuts-audit-adapter: failed to spawn mint_server at '{bin_path}': {err}"
                );
                eprintln!(
                    "  hint: build it first with `cargo build -p micronuts-mint --bin mint_server`"
                );
                eprintln!(
                    "       or override the path with MICRONUTS_MINT_BIN=/abs/path/to/mint_server"
                );
                return Err(err);
            }
        };

        let stdin = child
            .stdin
            .take()
            .expect("mint_server stdin pipe was not captured");
        let stdout = child
            .stdout
            .take()
            .expect("mint_server stdout pipe was not captured");

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    /// Send one RPC frame and read one matched response frame.
    /// Returns the raw RPC response envelope so the caller can apply
    /// domain-specific error→HTTP mapping. Transport/framing failures are
    /// returned as a string for a generic 503 body.
    async fn call_raw(&mut self, method: MintRpcMethod) -> Result<MintRpcResponse, String> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let request = MintRpcRequest { id, method };
        let request_bytes = encode_rpc_request(&request)
            .map_err(|e| format!("failed to encode rpc request: {e}"))?;
        let request_hex = hex::encode(&request_bytes);

        self.stdin
            .write_all(request_hex.as_bytes())
            .await
            .map_err(|e| format!("failed to write to mint_server stdin: {e}"))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("failed to write newline to mint_server stdin: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("failed to flush mint_server stdin: {e}"))?;

        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .await
            .map_err(|e| format!("failed to read mint_server stdout: {e}"))?;
        if read == 0 {
            return Err("mint_server stdout closed (subprocess likely exited)".to_string());
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Err("mint_server returned an empty response line".to_string());
        }

        let response_bytes =
            hex::decode(trimmed).map_err(|e| format!("failed to decode hex rpc response: {e}"))?;
        let response = decode_rpc_response(&response_bytes)
            .map_err(|e| format!("failed to decode rpc response: {e}"))?;

        if response.id != id {
            return Err(format!(
                "rpc response id mismatch: requested {id}, got {}",
                response.id
            ));
        }

        Ok(response)
    }
}

/// Adapter-wide shared state: the long-lived mint subprocess plus a mutex that
/// serializes concurrent HTTP handlers through the single stdin/stdout pair.
#[derive(Clone)]
struct AdapterState {
    mint: Arc<Mutex<MintProcess>>,
}

impl AdapterState {
    /// Issue an RPC call, returning the successful `MintRpcResult`.
    ///
    /// Failures are mapped to an HTTP [`Response`]:
    /// - Transport/framing errors → 503 `MINT_UNAVAILABLE`.
    /// - Service-level `CashuError` payloads → mapped via [`cashu_error_to_response`].
    async fn call_mint(&self, method: MintRpcMethod) -> Result<MintRpcResult, Response> {
        let mut mint = self.mint.lock().await;
        match mint.call_raw(method).await {
            Ok(response) => match response.payload {
                MintRpcPayload::Success(result) => Ok(result),
                MintRpcPayload::Error(err) => Err(cashu_error_to_response(&err)),
            },
            Err(message) => {
                let body = Json(json!({
                    "code": "MINT_UNAVAILABLE",
                    "error": message,
                }));
                Err((StatusCode::SERVICE_UNAVAILABLE, body).into_response())
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = env::var("MICRONUTS_ADAPTER_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let mint_bin = env::var("MICRONUTS_MINT_BIN").unwrap_or_else(|_| DEFAULT_MINT_BIN.to_string());

    let mint = MintProcess::spawn(&mint_bin)?;
    let state = AdapterState {
        mint: Arc::new(Mutex::new(mint)),
    };

    let app = Router::new()
        // NUT-06
        .route("/v1/info", get(get_info))
        // NUT-01
        .route("/v1/keys", get(get_keys))
        .route("/v1/keys/{keyset_id}", get(get_keys_for_id))
        // NUT-02
        .route("/v1/keysets", get(get_keysets))
        // NUT-04
        .route("/v1/mint/quote/bolt11", post(post_mint_quote))
        .route("/v1/mint/quote/bolt11/{quote}", get(get_mint_quote))
        .route("/v1/mint/bolt11", post(post_mint))
        // NUT-03
        .route("/v1/swap", post(post_swap))
        // NUT-05
        .route("/v1/melt/quote/bolt11", post(post_melt_quote))
        .route("/v1/melt/quote/bolt11/{quote}", get(get_melt_quote))
        .route("/v1/melt/bolt11", post(post_melt))
        // NUT-07
        .route("/v1/checkstate", post(post_checkstate))
        // NUT-09
        .route("/v1/restore", post(post_restore))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    eprintln!("micronuts-audit-adapter: listening on http://{addr} (mint_server: {mint_bin})");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /v1/info — NUT-06 mint info.
async fn get_info(State(state): State<AdapterState>) -> Response {
    match state.call_mint(MintRpcMethod::GetInfo).await {
        Ok(MintRpcResult::GetInfo(info)) => {
            (StatusCode::OK, Json(mint_info_to_json(&info))).into_response()
        }
        Ok(other) => unexpected_result_response("GetInfo", &other),
        Err(resp) => resp,
    }
}

/// GET /v1/keys — NUT-01: all active keysets.
async fn get_keys(State(state): State<AdapterState>) -> Response {
    get_keys_inner(state, None).await
}

/// GET /v1/keys/{keyset_id} — NUT-01: filtered to one keyset.
async fn get_keys_for_id(
    State(state): State<AdapterState>,
    Path(keyset_id): Path<String>,
) -> Response {
    get_keys_inner(state, Some(keyset_id)).await
}

/// Shared NUT-01 body — returns 404 when the requested keyset id is unknown.
async fn get_keys_inner(state: AdapterState, keyset_id: Option<String>) -> Response {
    let result = state.call_mint(MintRpcMethod::GetKeys).await;
    match result {
        Ok(MintRpcResult::GetKeys(keys)) => {
            let filtered = match keyset_id.as_deref() {
                Some(id) if !id.is_empty() => {
                    let matching: Vec<_> =
                        keys.keysets.into_iter().filter(|k| k.id == id).collect();
                    if matching.is_empty() {
                        return cashu_error_to_response(&CashuError::KeysetNotFound);
                    }
                    matching
                }
                _ => keys.keysets,
            };
            let body =
                json!({ "keysets": filtered.iter().map(keyset_to_json).collect::<Vec<_>>() });
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(other) => unexpected_result_response("GetKeys", &other),
        Err(resp) => resp,
    }
}

/// GET /v1/keysets — NUT-02.
async fn get_keysets(State(state): State<AdapterState>) -> Response {
    match state.call_mint(MintRpcMethod::GetKeysets).await {
        Ok(MintRpcResult::GetKeysets(resp)) => {
            let body = json!({
                "keysets": resp.keysets.iter().map(keyset_info_to_json).collect::<Vec<_>>()
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(other) => unexpected_result_response("GetKeysets", &other),
        Err(resp) => resp,
    }
}

/// POST /v1/mint/quote/bolt11 — NUT-04 quote creation.
async fn post_mint_quote(State(state): State<AdapterState>, Json(body): Json<Value>) -> Response {
    let request = match parse_mint_quote_request(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match state.call_mint(MintRpcMethod::MintQuote(request)).await {
        Ok(MintRpcResult::MintQuote(resp)) => {
            (StatusCode::OK, Json(mint_quote_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("MintQuote", &other),
        Err(resp) => resp,
    }
}

/// GET /v1/mint/quote/bolt11/{quote} — NUT-04 quote lookup.
async fn get_mint_quote(State(state): State<AdapterState>, Path(quote): Path<String>) -> Response {
    let lookup = MintQuoteLookupRequest { quote };
    match state.call_mint(MintRpcMethod::GetMintQuote(lookup)).await {
        Ok(MintRpcResult::GetMintQuote(resp)) => {
            (StatusCode::OK, Json(mint_quote_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("GetMintQuote", &other),
        Err(resp) => resp,
    }
}

/// POST /v1/mint/bolt11 — NUT-04 mint blinded outputs.
async fn post_mint(State(state): State<AdapterState>, Json(body): Json<Value>) -> Response {
    let request = match parse_mint_request(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match state.call_mint(MintRpcMethod::Mint(request)).await {
        Ok(MintRpcResult::Mint(resp)) => {
            (StatusCode::OK, Json(mint_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("Mint", &other),
        Err(resp) => resp,
    }
}

/// POST /v1/swap — NUT-03 swap proofs for new outputs.
async fn post_swap(State(state): State<AdapterState>, Json(body): Json<Value>) -> Response {
    let request = match parse_swap_request(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match state.call_mint(MintRpcMethod::Swap(request)).await {
        Ok(MintRpcResult::Swap(resp)) => {
            (StatusCode::OK, Json(swap_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("Swap", &other),
        Err(resp) => resp,
    }
}

/// POST /v1/melt/quote/bolt11 — NUT-05 melt quote creation.
async fn post_melt_quote(State(state): State<AdapterState>, Json(body): Json<Value>) -> Response {
    let request = match parse_melt_quote_request(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match state.call_mint(MintRpcMethod::MeltQuote(request)).await {
        Ok(MintRpcResult::MeltQuote(resp)) => {
            (StatusCode::OK, Json(melt_quote_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("MeltQuote", &other),
        Err(resp) => resp,
    }
}

/// GET /v1/melt/quote/bolt11/{quote} — NUT-05 melt quote lookup.
async fn get_melt_quote(State(state): State<AdapterState>, Path(quote): Path<String>) -> Response {
    let lookup = MeltQuoteLookupRequest { quote };
    match state.call_mint(MintRpcMethod::GetMeltQuote(lookup)).await {
        Ok(MintRpcResult::GetMeltQuote(resp)) => {
            (StatusCode::OK, Json(melt_quote_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("GetMeltQuote", &other),
        Err(resp) => resp,
    }
}

/// POST /v1/melt/bolt11 — NUT-05 melt proofs against a melt quote.
async fn post_melt(State(state): State<AdapterState>, Json(body): Json<Value>) -> Response {
    let request = match parse_melt_request(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match state.call_mint(MintRpcMethod::Melt(request)).await {
        Ok(MintRpcResult::Melt(resp)) => {
            (StatusCode::OK, Json(melt_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("Melt", &other),
        Err(resp) => resp,
    }
}

/// POST /v1/checkstate — NUT-07 spent-state check by Y value.
async fn post_checkstate(State(state): State<AdapterState>, Json(body): Json<Value>) -> Response {
    let request = match parse_check_state_request(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match state.call_mint(MintRpcMethod::CheckState(request)).await {
        Ok(MintRpcResult::CheckState(resp)) => {
            (StatusCode::OK, Json(check_state_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("CheckState", &other),
        Err(resp) => resp,
    }
}

/// POST /v1/restore — NUT-09 restore.
///
/// Accepts the spec-shaped body `{"outputs": [<blinded message>]}` and forwards
/// the `B_` value of each output as the lookup `Y` (the demo mint's CBOR
/// `RestoreRequest` is `Vec<PublicKey>`). The stateless demo mint returns an
/// empty `outputs` list regardless of input.
async fn post_restore(State(state): State<AdapterState>, Json(body): Json<Value>) -> Response {
    let request = match parse_restore_request(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match state.call_mint(MintRpcMethod::Restore(request)).await {
        Ok(MintRpcResult::Restore(resp)) => {
            (StatusCode::OK, Json(restore_response_to_json(&resp))).into_response()
        }
        Ok(other) => unexpected_result_response("Restore", &other),
        Err(resp) => resp,
    }
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Map a `CashuError` returned by the mint into an HTTP response.
///
/// The body matches the NUT-00 `ErrorResponse` shape `{"detail", "code"}` plus
/// a string `error_kind` so clients can branch without parsing the detail.
///
/// Mapping rationale:
/// - **404 Not Found** for missing quotes/keysets the client asked for by id.
/// - **400 Bad Request** for client-side input failures: malformed proofs,
///   unbalanced amounts, insufficient inputs, invalid amounts, quotes that
///   have not been paid yet or have already been issued.
/// - **500 Internal Server Error** for service-side failures: protocol
///   framing errors, crypto failures, transport errors, and anything uncategorized.
fn cashu_error_to_response(err: &CashuError) -> Response {
    let (status, code_label) = match err {
        CashuError::QuoteNotFound => (StatusCode::NOT_FOUND, "QUOTE_NOT_FOUND"),
        CashuError::KeysetNotFound => (StatusCode::NOT_FOUND, "KEYSET_NOT_FOUND"),
        CashuError::InvalidAmount => (StatusCode::BAD_REQUEST, "INVALID_AMOUNT"),
        CashuError::InvalidProof => (StatusCode::BAD_REQUEST, "INVALID_PROOF"),
        CashuError::AmountMismatch => (StatusCode::BAD_REQUEST, "AMOUNT_MISMATCH"),
        CashuError::InsufficientInputs => (StatusCode::BAD_REQUEST, "INSUFFICIENT_INPUTS"),
        CashuError::QuoteNotPaid => (StatusCode::BAD_REQUEST, "QUOTE_NOT_PAID"),
        CashuError::QuoteAlreadyIssued => (StatusCode::BAD_REQUEST, "QUOTE_ALREADY_ISSUED"),
        CashuError::Protocol(_) => (StatusCode::INTERNAL_SERVER_ERROR, "PROTOCOL_ERROR"),
        CashuError::Crypto(_) => (StatusCode::INTERNAL_SERVER_ERROR, "CRYPTO_ERROR"),
        CashuError::Transport(_) => (StatusCode::INTERNAL_SERVER_ERROR, "TRANSPORT_ERROR"),
        CashuError::Unknown(_) => (StatusCode::INTERNAL_SERVER_ERROR, "UNKNOWN"),
        CashuError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    };

    let body = Json(json!({
        "detail": err.to_string(),
        "code": code_label,
        "error_kind": code_label,
    }));
    (status, body).into_response()
}

/// Build a 500 response for an RPC variant mismatch (should not happen with
/// the current mint, but keeps handlers exhaustive).
fn unexpected_result_response(expected: &str, got: &MintRpcResult) -> Response {
    let body = Json(json!({
        "code": "UNEXPECTED_RPC_RESULT",
        "error": format!("expected {expected}, got {got:?}"),
    }));
    (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
}

/// Build a 400 response for a JSON body that did not parse into the expected
/// request shape.
fn bad_request(field: &str, detail: impl Into<String>) -> Response {
    let body = Json(json!({
        "detail": format!("invalid request body: {field}: {}", detail.into()),
        "code": "BAD_REQUEST",
        "error_kind": "BAD_REQUEST",
    }));
    (StatusCode::BAD_REQUEST, body).into_response()
}

// ---------------------------------------------------------------------------
// JSON → CBOR request translators
// ---------------------------------------------------------------------------

fn parse_mint_quote_request(body: &Value) -> Result<nut04::MintQuoteRequest, Response> {
    let amount = body
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| bad_request("amount", "missing or not a non-negative integer"))?;
    let unit = body
        .get("unit")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("unit", "missing or not a string"))?;
    Ok(nut04::MintQuoteRequest {
        amount,
        unit: unit.to_string(),
    })
}

fn parse_mint_request(body: &Value) -> Result<nut04::MintRequest, Response> {
    let quote = body
        .get("quote")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("quote", "missing or not a string"))?;
    let outputs = body
        .get("outputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| bad_request("outputs", "missing or not an array"))?
        .iter()
        .map(parse_blinded_message)
        .collect::<Result<_, _>>()?;
    Ok(nut04::MintRequest {
        quote: quote.to_string(),
        outputs,
    })
}

fn parse_swap_request(body: &Value) -> Result<nut03::SwapRequest, Response> {
    let inputs = body
        .get("inputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| bad_request("inputs", "missing or not an array"))?
        .iter()
        .map(parse_proof)
        .collect::<Result<_, _>>()?;
    let outputs = body
        .get("outputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| bad_request("outputs", "missing or not an array"))?
        .iter()
        .map(parse_blinded_message)
        .collect::<Result<_, _>>()?;
    Ok(nut03::SwapRequest { inputs, outputs })
}

fn parse_melt_quote_request(body: &Value) -> Result<nut05::MeltQuoteRequest, Response> {
    let request = body
        .get("request")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("request", "missing or not a string"))?;
    let unit = body
        .get("unit")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("unit", "missing or not a string"))?;
    Ok(nut05::MeltQuoteRequest {
        request: request.to_string(),
        unit: unit.to_string(),
    })
}

fn parse_melt_request(body: &Value) -> Result<nut05::MeltRequest, Response> {
    let quote = body
        .get("quote")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("quote", "missing or not a string"))?;
    let inputs = body
        .get("inputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| bad_request("inputs", "missing or not an array"))?
        .iter()
        .map(parse_proof)
        .collect::<Result<_, _>>()?;
    let outputs = match body.get("outputs") {
        Some(Value::Array(arr)) if !arr.is_empty() => {
            let parsed: Result<_, _> = arr.iter().map(parse_blinded_message).collect();
            Some(parsed?)
        }
        _ => None,
    };
    Ok(nut05::MeltRequest {
        quote: quote.to_string(),
        inputs,
        outputs,
    })
}

fn parse_check_state_request(body: &Value) -> Result<nut07::CheckStateRequest, Response> {
    let ys = body
        .get("Ys")
        .and_then(|v| v.as_array())
        .ok_or_else(|| bad_request("Ys", "missing or not an array (use capital Ys)"))?
        .iter()
        .map(parse_public_key_value)
        .collect::<Result<_, _>>()?;
    Ok(nut07::CheckStateRequest { ys })
}

fn parse_restore_request(body: &Value) -> Result<nut09::RestoreRequest, Response> {
    // The demo mint's CBOR `RestoreRequest` is `Vec<PublicKey>`. The Cashu
    // HTTP spec sends `{"outputs": [<blinded message>]}` — we accept either
    // a hex-string Y value or a full blinded-message object and forward the
    // point as the lookup key.
    let outputs = body
        .get("outputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| bad_request("outputs", "missing or not an array"))?;
    let mut ys = Vec::with_capacity(outputs.len());
    for item in outputs {
        if let Some(s) = item.as_str() {
            ys.push(parse_public_key_from_hex(s)?);
        } else if let Some(obj) = item.as_object() {
            // Accept a BlindedMessage-shaped object: forward its `B_` as the Y.
            if let Some(b_hex) = obj.get("B_").and_then(|v| v.as_str()) {
                ys.push(parse_public_key_from_hex(b_hex)?);
            } else if let Some(y_hex) = obj.get("Y").and_then(|v| v.as_str()) {
                ys.push(parse_public_key_from_hex(y_hex)?);
            } else {
                return Err(bad_request(
                    "outputs",
                    "expected hex string or {\"B_\": ...} / {\"Y\": ...} object",
                ));
            }
        } else {
            return Err(bad_request("outputs", "expected hex string or object"));
        }
    }
    Ok(nut09::RestoreRequest { outputs: ys })
}

fn parse_proof(value: &Value) -> Result<nut00::Proof, Response> {
    let amount = value
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| bad_request("proof.amount", "missing or not a non-negative integer"))?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("proof.id", "missing or not a string"))?;
    let secret = value
        .get("secret")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("proof.secret", "missing or not a string"))?;
    let c = value
        .get("C")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("proof.C", "missing or not a string (capital C)"))?;
    let c = parse_public_key_from_hex(c)
        .map_err(|_| bad_request("proof.C", "not a valid 33-byte compressed pubkey hex"))?;
    // The optional `witness` field is part of the spec but not present in the
    // demo CBOR Proof; it is ignored here.
    Ok(nut00::Proof {
        amount,
        id: id.to_string(),
        secret: secret.to_string(),
        c,
        dleq: None,
    })
}

fn parse_blinded_message(value: &Value) -> Result<nut00::BlindedMessage, Response> {
    let amount = value
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| bad_request("output.amount", "missing or not a non-negative integer"))?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("output.id", "missing or not a string"))?;
    let b_hex = value
        .get("B_")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("output.B_", "missing or not a string (capital B_)"))?;
    let b = parse_public_key_from_hex(b_hex)
        .map_err(|_| bad_request("output.B_", "not a valid 33-byte compressed pubkey hex"))?;
    Ok(nut00::BlindedMessage {
        amount,
        id: id.to_string(),
        b,
    })
}

/// Parse a serde Value expected to be a hex string into a `PublicKey`.
fn parse_public_key_value(value: &Value) -> Result<PublicKey, Response> {
    let s = value
        .as_str()
        .ok_or_else(|| bad_request("public key", "expected a hex string"))?;
    parse_public_key_from_hex(s)
}

fn parse_public_key_from_hex(s: &str) -> Result<PublicKey, Response> {
    let bytes = hex::decode(s).map_err(|_| bad_request("public key", "not valid hex"))?;
    let arr: [u8; 33] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| bad_request("public key", "expected 33 bytes (66 hex chars)"))?;
    PublicKey::from_bytes(&arr)
        .ok_or_else(|| bad_request("public key", "not a valid compressed secp256k1 point"))
}

// ---------------------------------------------------------------------------
// CBOR → JSON response translators
// ---------------------------------------------------------------------------

fn mint_info_to_json(info: &nut06::MintInfo) -> Value {
    let contacts: Vec<Value> = info
        .contact
        .iter()
        .map(|c| json!({ "method": c.method, "info": c.info }))
        .collect();
    json!({
        "name": info.name,
        "pubkey": info.pubkey,
        "version": info.version,
        "description": info.description,
        "description_long": "",
        "contact": contacts,
        "motd": "",
        "nuts": {
            "supported": info.nuts.supported,
        },
    })
}

/// JSON shape consumed by `cashu-audit` (nut02_keysets.py: `ks.get("keys", {})`
/// returning a dict mapping amount-as-string → pubkey-hex).
fn keyset_to_json(keyset: &nut01::KeySet) -> Value {
    let mut keys_map = serde_json::Map::new();
    for kp in &keyset.keys {
        keys_map.insert(
            kp.amount.to_string(),
            Value::String(hex::encode(kp.pubkey.to_bytes())),
        );
    }
    json!({
        "id": keyset.id,
        "unit": keyset.unit,
        "keys": keys_map,
    })
}

fn keyset_info_to_json(info: &nut02::KeysetInfo) -> Value {
    json!({
        "id": info.id,
        "unit": info.unit,
        "active": info.active,
        "input_fee_ppk": info.input_fee_ppk,
    })
}

fn mint_quote_response_to_json(resp: &nut04::MintQuoteResponse) -> Value {
    json!({
        "quote": resp.quote,
        "request": resp.request,
        "paid": resp.paid,
        "state": resp.state,
        "expiry": resp.expiry,
    })
}

fn mint_response_to_json(resp: &nut04::MintResponse) -> Value {
    json!({
        "signatures": resp.signatures.iter().map(blind_signature_to_json).collect::<Vec<_>>()
    })
}

fn swap_response_to_json(resp: &nut03::SwapResponse) -> Value {
    json!({
        "signatures": resp.signatures.iter().map(blind_signature_to_json).collect::<Vec<_>>()
    })
}

fn melt_quote_response_to_json(resp: &nut05::MeltQuoteResponse) -> Value {
    json!({
        "quote": resp.quote,
        "amount": resp.amount,
        "fee_reserve": resp.fee_reserve,
        "paid": resp.paid,
        "state": resp.state,
        "expiry": resp.expiry,
    })
}

fn melt_response_to_json(resp: &nut05::MeltResponse) -> Value {
    let change = resp
        .change
        .as_ref()
        .map(|sigs| sigs.iter().map(blind_signature_to_json).collect::<Vec<_>>());
    json!({
        "paid": resp.paid,
        "state": resp.state,
        "payment_preimage": resp.payment_preimage,
        "change": change,
    })
}

fn check_state_response_to_json(resp: &nut07::CheckStateResponse) -> Value {
    json!({
        "states": resp.states.iter().map(proof_state_to_json).collect::<Vec<_>>()
    })
}

fn restore_response_to_json(resp: &nut09::RestoreResponse) -> Value {
    json!({
        "outputs": resp.outputs.iter().map(restore_output_to_json).collect::<Vec<_>>()
    })
}

fn proof_state_to_json(state: &nut07::ProofState) -> Value {
    json!({
        "Y": hex::encode(state.y.to_bytes()),
        "state": state.state,
        "witness": state.witness,
    })
}

fn blind_signature_to_json(sig: &nut00::BlindSignature) -> Value {
    let mut obj = json!({
        "amount": sig.amount,
        "id": sig.id,
        "C_": hex::encode(sig.c.to_bytes()),
    });
    if let Some(dleq) = &sig.dleq {
        obj["dleq"] = json!({
            "e": hex::encode(dleq.e.to_secret_bytes()),
            "s": hex::encode(dleq.s.to_secret_bytes()),
        });
    }
    obj
}

fn restore_output_to_json(out: &nut09::RestoreOutput) -> Value {
    // NUT-09 spec response: each entry is `{"Y": ..., "promises": [<blind sig>...]}`.
    json!({
        "Y": hex::encode(out.y.to_bytes()),
        "promises": [blind_signature_to_json(&out.signature)],
    })
}
