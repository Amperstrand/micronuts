//! Browser transport for real mints: sync XHR over the shared wire
//! protocol in `mint_wire.rs`.
//!
//! Why synchronous XHR: the `MintClient` trait is deliberately blocking
//! (it serves the embedded RPC transport unchanged), and wasm jobs run
//! on the main thread — a promise-based fetch cannot be driven from the
//! sync trait without an engine-wide async refactor. Sync XHR keeps the
//! one-impl wire protocol; the UI blocks for the duration of each small
//! JSON round-trip while `busy` is set, which is acceptable for the
//! browser playground's interaction scale. (Deprecated-but-supported in
//! all engines; console warning expected.)

use cashu_core_lite::error::CashuError;
use serde_json::Value;

use crate::mint_wire::{JsonTransport, WireMintClient};

/// The browser mint client for real mints: shared wire protocol over
/// synchronous XHR (CORS must be allowed by the mint — testnut and
/// signut both send `access-control-allow-origin: *`).
pub type FetchMintClient = WireMintClient<XhrTransport>;

impl FetchMintClient {
    pub fn new(base_url: &str) -> Self {
        WireMintClient::with_transport(base_url, XhrTransport)
    }
}

/// Single-shot synchronous XHR transport. Stateless; `Clone` is a no-op.
#[derive(Clone, Copy, Default)]
pub struct XhrTransport;

impl JsonTransport for XhrTransport {
    fn get_json(&self, url: &str) -> Result<Value, CashuError> {
        self.request("GET", url, None)
    }

    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CashuError> {
        self.request("POST", url, Some(body))
    }
}

impl XhrTransport {
    fn request(&self, method: &str, url: &str, body: Option<&Value>) -> Result<Value, CashuError> {
        let xhr = web_sys::XmlHttpRequest::new()
            .map_err(|_| CashuError::Transport(String::from("xhr init failed")))?;
        let open = match body {
            Some(_) => xhr.open_with_async(method, url, false),
            None => xhr.open_with_async(method, url, false),
        };
        open.map_err(|_| CashuError::Transport(String::from("xhr open failed")))?;
        if body.is_some() {
            xhr.set_request_header("Content-Type", "application/json")
                .map_err(|_| CashuError::Transport(String::from("xhr header failed")))?;
        }
        let payload = body.map(|value| value.to_string()).unwrap_or_default();
        let send_result = if body.is_some() {
            xhr.send_with_opt_str(Some(&payload))
        } else {
            xhr.send_with_opt_str(None)
        };
        send_result.map_err(|_| CashuError::Transport(String::from("mint unreachable")))?;

        let status = xhr
            .status()
            .map_err(|_| CashuError::Transport(String::from("xhr status unreadable")))?;
        let response_text = xhr.response_text().unwrap_or_default();
        match status {
            200..=299 => {
                let text = response_text
                    .ok_or_else(|| CashuError::Protocol(String::from("empty mint response")))?;
                if text.trim().is_empty() {
                    // 204-style empty body: represent as an empty object
                    // so optional-field parsers treat it uniformly.
                    return Ok(Value::Object(Default::default()));
                }
                serde_json::from_str(&text).map_err(|_| {
                    CashuError::Protocol(format!(
                        "invalid mint JSON: {}",
                        &text[..text.len().min(120)]
                    ))
                })
            }
            400..=599 => {
                // Same contract as the native map: mint error bodies
                // carry {"detail", "code"} back into CashuError variants.
                let detail = response_text
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect::<String>();
                Err(CashuError::Protocol(format!(
                    "mint returned {status}: {detail}"
                )))
            }
            0 => Err(CashuError::Transport(String::from(
                "mint unreachable (network or CORS)",
            ))),
            other => Err(CashuError::Protocol(format!("unexpected status {other}"))),
        }
    }
}
