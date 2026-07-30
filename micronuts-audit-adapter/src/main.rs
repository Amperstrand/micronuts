//! HTTP-to-CBOR-RPC bridge for the Micronuts demo mint.
//!
//! Spawns `micronuts-mint`'s `mint_server` binary as a subprocess and exposes
//! Cashu REST endpoints by translating each request into a CBOR `MintRpcRequest`
//! frame, writing it as one hex line to the subprocess stdin, reading one hex
//! CBOR `MintRpcResponse` line back from stdout, decoding it, and returning the
//! payload as JSON.
//!
//! Scaffold scope: only `GET /v1/info` is wired. Subsequent endpoints land in
//! T5.2. The framing protocol is documented in `docs/MINT-WALLET-DEMO.md`.
//!
//! Run from the workspace root so `target/debug/mint_server` resolves:
//!   cargo run -p micronuts-audit-adapter
//!
//! Override defaults via env:
//!   MICRONUTS_ADAPTER_PORT=4000     Listen port (default 3030)
//!   MICRONUTS_MINT_BIN=/path/mint   Override subprocess binary path

use std::env;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use cashu_core_lite::nuts::nut06::MintInfo;
use cashu_core_lite::rpc::{
    decode_rpc_response, encode_rpc_request, MintRpcMethod, MintRpcPayload, MintRpcRequest,
    MintRpcResult,
};
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

    /// Send one RPC frame and read one matching response frame.
    /// Returns an error string suitable for an HTTP 503 body when the
    /// subprocess is unhealthy or the framing breaks.
    async fn call(&mut self, method: MintRpcMethod) -> Result<MintRpcResult, String> {
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

        let response_bytes = hex::decode(trimmed)
            .map_err(|e| format!("failed to decode hex rpc response: {e}"))?;
        let response = decode_rpc_response(&response_bytes)
            .map_err(|e| format!("failed to decode rpc response: {e}"))?;

        if response.id != id {
            return Err(format!(
                "rpc response id mismatch: requested {id}, got {}",
                response.id
            ));
        }

        match response.payload {
            MintRpcPayload::Success(result) => Ok(result),
            MintRpcPayload::Error(err) => Err(format!("mint rpc error: {err:?}")),
        }
    }
}

/// Adapter-wide shared state: the long-lived mint subprocess plus a mutex that
/// serializes concurrent HTTP handlers through the single stdin/stdout pair.
#[derive(Clone)]
struct AdapterState {
    mint: Arc<Mutex<MintProcess>>,
}

impl AdapterState {
    async fn call_mint(&self, method: MintRpcMethod) -> Result<MintRpcResult, Response> {
        let mut mint = self.mint.lock().await;
        match mint.call(method).await {
            Ok(result) => Ok(result),
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
        .route("/v1/info", get(get_info))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    eprintln!(
        "micronuts-audit-adapter: listening on http://{addr} (mint_server: {mint_bin})"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// GET /v1/info — NUT-06 mint info.
async fn get_info(State(state): State<AdapterState>) -> Response {
    match state.call_mint(MintRpcMethod::GetInfo).await {
        Ok(MintRpcResult::GetInfo(info)) => {
            let body = mint_info_to_json(&info);
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(other) => {
            let body = Json(json!({
                "code": "UNEXPECTED_RPC_RESULT",
                "error": format!("expected GetInfo, got {other:?}"),
            }));
            (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
        }
        Err(resp) => resp,
    }
}

/// Translate the CBOR-shaped `MintInfo` into the JSON shape returned by
/// `GET /v1/info`. `MintInfo` does not derive `Serialize`, and the CBOR layout
/// is the demo-internal form, so we project it field-by-field. The `nuts`
/// object is intentionally a literal translation of the CBOR shape; full NUT-06
/// settings mapping lands with the rest of the endpoints in T5.2.
fn mint_info_to_json(info: &MintInfo) -> Value {
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
        "contact": contacts,
        "nuts": {
            "supported": info.nuts.supported,
        },
    })
}
