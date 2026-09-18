//! esp-idf HTTP transport for the wallet-core wire protocol.
//!
//! Implements `JsonTransport` (`get_json`/`post_json`) over
//! `EspHttpConnection` — the same wire protocol as the host `ureq`
//! transport (`micronuts-wallet/src/http.rs`), sharing
//! `mint_wire.rs`'s single definition of every Cashu REST route.

use cashu_core_lite::error::CashuError;
use embedded_svc::http::client::Client as HttpClient;
use esp_idf_svc::http::client::EspHttpConnection;
use embedded_svc::http::Status;
use esp_idf_svc::hal::sys::EspError;

use micronuts_wallet_core::mint_wire::JsonTransport;

pub type EspIdfMintClient = micronuts_wallet_core::mint_wire::WireMintClient<EspIdfTransport>;

pub fn esp_idf_mint_client(base_url: &str) -> EspIdfMintClient {
    micronuts_wallet_core::mint_wire::WireMintClient::with_transport(
        base_url,
        EspIdfTransport::new(),
    )
}

pub struct EspIdfTransport {
    config: esp_idf_svc::http::client::Configuration,
}

impl EspIdfTransport {
    pub fn new() -> Self {
        Self {
            config: esp_idf_svc::http::client::Configuration {
                buffer_size: Some(4096),
                buffer_size_tx: Some(2048),
                ..Default::default()
            },
        }
    }
}

impl Default for EspIdfTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for EspIdfTransport {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl EspIdfTransport {
    fn round_trip(
        &self,
        method: embedded_svc::http::Method,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, CashuError> {
        let mut connection =
            EspHttpConnection::new(&self.config).map_err(|e| CashuError::Transport(format!("http conn: {e:?}")))?;
        let mut client = HttpClient::wrap(connection);

        let headers: Vec<(&str, &str)> = match body {
            Some(_) => vec![("Content-Type", "application/json")],
            None => vec![],
        };

        let mut request = client
            .request(method, url, &headers)
            .map_err(|e| CashuError::Transport(format!("http conn: {e:?}")))?;

        if let Some(json_body) = body {
            let serialized =
                serde_json::to_vec(json_body).map_err(|e| CashuError::Transport(e.to_string()))?;
            use embedded_svc::io::Write as _;
            request
                .write_all(&serialized)
                .map_err(|e| CashuError::Transport(format!("body write: {e:?}")))?;
        }

        let response = request
            .submit()
            .map_err(|e| CashuError::Transport(format!("submit: {e:?}")))?;

        let status = response.status();
        let body = read_body(response)?;

        if status < 200 || status >= 300 {
            // Try to extract the mint's error detail
            let detail = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("detail").or_else(|| v.get("error")).cloned())
                .and_then(|d| d.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(CashuError::Protocol(format!("mint error: {detail}")));
        }

        serde_json::from_slice(&body)
            .map_err(|e| CashuError::Transport(format!("json parse: {e}")))
    }
}

impl JsonTransport for EspIdfTransport {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, CashuError> {
        self.round_trip(embedded_svc::http::Method::Get, url, None)
    }

    fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<serde_json::Value, CashuError> {
        self.round_trip(embedded_svc::http::Method::Post, url, Some(body))
    }
}

fn read_body(
    mut response: embedded_svc::http::client::Response<&mut EspHttpConnection>,
) -> Result<Vec<u8>, CashuError> {
    use embedded_svc::io::Read as _;
    let mut body = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = response
            .read(&mut chunk)
            .map_err(|e| CashuError::Transport(format!("read: {e:?}")))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > 64 * 1024 {
            return Err(CashuError::Transport("response body exceeds 64 KiB".into()));
        }
    }
    Ok(body)
}

fn map_esp_err(e: &EspError) -> CashuError {
    CashuError::Transport(format!("esp-idf http: {e}"))
}
