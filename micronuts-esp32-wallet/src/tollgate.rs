//! TollGate captive-portal client (docs/ESP32-TOLLGATE-CLIENT-PLAN.md).
//!
//! A headless paying client: join the portal network, discover the
//! TollGate backend on the DHCP gateway, and buy internet access with
//! on-device ecash. The bench-verified protocol facts from the plan are
//! carried verbatim:
//!
//! 1. Token submission is a RAW BODY POST to tcp/2121 with
//!    `Content-Type: text/plain` — a JSON wrapper breaks backend prefix
//!    parsing (the #1 integration gotcha).
//! 2. Pre-auth DNS under nodogsplash passes tcp/53 only — every probe
//!    outside the backend port is IP-literal.
//! 3. The backend on the gateway answers `GET /` with a kind-10021 JSON
//!    carrying `price_per_step` tags — no portal-HTML dance needed.
//! 4. An already-authenticated MAC stays authed — probe internet first,
//!    pay only when gated.
//! 5. Post-payment verification is an IP-literal HTTP fetch, never a
//!    DNS-dependent one.

use core::fmt;
use core::time::Duration;

use cashu_core_lite::store::ProofStore;
use cashu_core_lite::transport::MintClient;
use embedded_svc::http::client::Client as HttpClient;
use embedded_svc::http::Method;
use embedded_svc::io::Write as _;
use esp_idf_svc::http::client::{Configuration, EspHttpConnection, FollowRedirectsPolicy};
use log::info;
use micronuts_wallet_core::engine::WalletEngine;

use crate::wifi::{WifiError, WifiManager};

/// TollGate backend port on the gateway — reachable from WiFi clients by
/// design (`users_to_router` allow), so discovery and payment need no
/// portal HTML at all.
pub const BACKEND_PORT: u16 = 2121;

/// Well-known public anycast resolver that serves plain HTTP on :80
/// (answers 301). IP-literal by protocol necessity (fact 2: pre-auth DNS
/// is gated); a global public service constant, not bench infrastructure.
const PROBE_URL: &str = "http://1.1.1.1/";

/// Hard bound for every HTTP exchange: the esp-idf client applies
/// `timeout` to connect and read, so the flow always fails loudly
/// instead of hanging.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Price fallback when discovery carries no usable `price_per_step` tag.
const DEFAULT_PRICE_SATS: u64 = 4;

/// Router-side quirks heal asynchronously — one retry, small backoff.
const VERIFY_RETRY_BACKOFF: Duration = Duration::from_secs(2);

/// Body read caps per call kind (bounded reads, bounded RAM).
const DISCOVERY_BODY_CAP: usize = 8 * 1024;
const SUBMIT_BODY_CAP: usize = 2 * 1024;
const PROBE_BODY_CAP: usize = 512;

#[derive(Debug)]
pub enum TollgateError {
    Wifi(WifiError),
    /// HTTP transport failure (connect/write/read/timeout) — the flow
    /// fails loudly with the exact step and URL.
    Http(String),
    /// The gateway answered, but not with anything payable.
    Protocol(String),
    /// No discovery yet — `tollgate join` (or `tollgate pay`, which
    /// auto-discovers) must run first.
    NotDiscovered,
}

impl fmt::Display for TollgateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wifi(err) => write!(f, "wifi: {err}"),
            Self::Http(detail) => write!(f, "http: {detail}"),
            Self::Protocol(detail) => write!(f, "protocol: {detail}"),
            Self::NotDiscovered => {
                write!(f, "no TollGate discovery yet — join first")
            }
        }
    }
}

impl std::error::Error for TollgateError {}

impl From<WifiError> for TollgateError {
    fn from(value: WifiError) -> Self {
        Self::Wifi(value)
    }
}

/// What the gateway told us about itself (`GET /` on :2121).
#[derive(Debug, Clone)]
pub struct Discovery {
    /// DHCP-lease gateway address — where the backend lives.
    pub gateway: String,
    /// `http://<gateway>:2121/` — discovery and payment endpoint.
    pub backend_url: String,
    /// Nostr kind of the discovery document (10021 = healthy offer).
    pub kind: i64,
    /// Satoshis per step from the first `price_per_step` tag, if any.
    pub price_sats: Option<u64>,
    /// Unit of the price tag (normally "sat").
    pub price_unit: Option<String>,
    /// Mint URL from the price tag (tag index 4), if any.
    pub mint_url: Option<String>,
}

/// Outcome of `tollgate pay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayOutcome {
    /// The gate was already open — nothing spent (idempotence fact 4).
    AlreadyAuthed,
    /// Spent, submitted, and end-to-end internet verified.
    Paid { amount: u64, http_status: u16 },
}

/// Result of the IP-literal internet probe. Gated ⟺ nodogsplash
/// intercepted :80 and answered with a redirect back to the gateway;
/// any other answer means the packet flowed to the real destination.
#[derive(Debug, Clone)]
pub enum GateProbe {
    Open { status: u16 },
    Gated { status: u16, location: String },
    Unreachable { reason: String },
}

struct HttpReply {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
}

/// One bounded HTTP exchange (fact-1/2 discipline lives here: raw
/// bodies, no redirect following, 10 s cap).
fn http_exchange(
    method: Method,
    url: &str,
    content_type: Option<&str>,
    body: Option<&[u8]>,
    body_cap: usize,
) -> Result<HttpReply, TollgateError> {
    let config = Configuration {
        buffer_size: Some(4096),
        buffer_size_tx: Some(2048),
        timeout: Some(HTTP_TIMEOUT),
        // The pre-auth probe MUST see the portal's 3xx as-is; following
        // it would serve the portal page as a fake 200 "open" verdict.
        follow_redirects_policy: FollowRedirectsPolicy::FollowNone,
        ..Default::default()
    };
    let connection = EspHttpConnection::new(&config)
        .map_err(|e| TollgateError::Http(format!("conn {url}: {e:?}")))?;
    let mut client = HttpClient::wrap(connection);

    let headers: Vec<(&str, &str)> = match content_type {
        Some(ct) => vec![("Content-Type", ct)],
        None => vec![],
    };
    let mut request = client
        .request(method, url, &headers)
        .map_err(|e| TollgateError::Http(format!("request {url}: {e:?}")))?;

    if let Some(bytes) = body {
        request
            .write_all(bytes)
            .map_err(|e| TollgateError::Http(format!("body write {url}: {e:?}")))?;
    }

    let mut response = request
        .submit()
        .map_err(|e| TollgateError::Http(format!("submit {url}: {e:?}")))?;

    let status = response.status();
    let location = response.header("Location").map(str::to_string);

    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < body_cap {
        let want = (body_cap - buf.len()).min(chunk.len());
        let n = response
            .read(&mut chunk[..want])
            .map_err(|e| TollgateError::Http(format!("read {url}: {e:?}")))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    Ok(HttpReply {
        status,
        location,
        body: buf,
    })
}

/// Parse the kind-10021 discovery document. Price tag shape (PRTA
/// bench-verified): `["price_per_step", "cashu", "<amount>", "<unit>",
/// "<mint-url>", ...]`.
fn parse_discovery(gateway: &str, body: &[u8]) -> Result<Discovery, TollgateError> {
    let doc: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| TollgateError::Protocol(format!("discovery is not JSON: {e}")))?;

    let kind = doc.get("kind").and_then(|k| k.as_i64()).unwrap_or(-1);
    if kind == 21023 {
        return Err(TollgateError::Protocol(String::from(
            "backend degraded (kind 21023): no reachable mint — cannot pay",
        )));
    }
    if kind != 10021 {
        return Err(TollgateError::Protocol(format!(
            "unexpected discovery kind {kind} (want 10021)"
        )));
    }

    let mut price_sats = None;
    let mut price_unit = None;
    let mut mint_url = None;
    if let Some(tags) = doc.get("tags").and_then(|t| t.as_array()) {
        for tag in tags {
            let Some(fields) = tag.as_array() else {
                continue;
            };
            if fields.first().and_then(|f| f.as_str()) != Some("price_per_step") {
                continue;
            }
            if fields.len() < 5 {
                continue;
            }
            if price_sats.is_none() {
                price_sats = fields
                    .get(2)
                    .and_then(|a| a.as_str())
                    .and_then(|a| a.parse().ok());
                price_unit = fields.get(3).and_then(|u| u.as_str()).map(str::to_string);
                mint_url = fields.get(4).and_then(|m| m.as_str()).map(str::to_string);
            }
        }
    }

    Ok(Discovery {
        gateway: gateway.to_string(),
        backend_url: format!("http://{gateway}:{BACKEND_PORT}/"),
        kind,
        price_sats,
        price_unit,
        mint_url,
    })
}

/// The paying TollGate client state machine. Console-driven from the
/// main loop: `tollgate join <ssid>` → `tollgate pay [sats]` →
/// `tollgate status`.
pub struct TollgateClient {
    discovery: Option<Discovery>,
    state: String,
}

impl Default for TollgateClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TollgateClient {
    pub fn new() -> Self {
        Self {
            discovery: None,
            state: String::from("idle"),
        }
    }

    /// Join a TollGate WiFi. TollGate networks are open — the empty
    /// passphrase is the signal `WifiManager::connect` uses to select
    /// `AuthMethod::None`.
    pub fn join(&mut self, wifi: &mut WifiManager, ssid: &str) -> Result<(), TollgateError> {
        println!("tollgate: joining open network");
        wifi.connect(ssid, "")?;
        // A new network invalidates any previous discovery.
        self.discovery = None;
        self.state = String::from("joined");
        info!("tollgate: joined, waiting for commands");
        Ok(())
    }

    /// Take the gateway from the DHCP lease, then `GET /` on :2121 for
    /// the backend's identity and pricing (fact 3).
    pub fn discover(&mut self, wifi: &WifiManager) -> Result<&Discovery, TollgateError> {
        let gateway = wifi.gateway_ip()?;
        let backend_url = format!("http://{gateway}:{BACKEND_PORT}/");
        println!("tollgate: discovering backend at {backend_url}");

        let reply = http_exchange(Method::Get, &backend_url, None, None, DISCOVERY_BODY_CAP)?;
        if !(200..300).contains(&reply.status) {
            return Err(TollgateError::Protocol(format!(
                "backend answered HTTP {} on discovery",
                reply.status
            )));
        }

        let discovery = parse_discovery(&gateway, &reply.body)?;
        match (discovery.price_sats, discovery.mint_url.as_deref()) {
            (Some(price), Some(mint)) => println!(
                "tollgate: price {price} {}/step, mint {mint}",
                discovery.price_unit.as_deref().unwrap_or("sat")
            ),
            _ => println!(
                "tollgate: no price tag in discovery — pay defaults to {DEFAULT_PRICE_SATS} sats"
            ),
        }
        info!(
            "tollgate: discovered kind={} price={:?} mint={:?}",
            discovery.kind, discovery.price_sats, discovery.mint_url
        );

        self.state = format!("discovered backend at {backend_url}");
        self.discovery = Some(discovery);
        Ok(self.discovery.as_ref().expect("just stored"))
    }

    fn gateway(&self) -> Result<&str, TollgateError> {
        self.discovery
            .as_ref()
            .map(|d| d.gateway.as_str())
            .ok_or(TollgateError::NotDiscovered)
    }

    /// IP-literal internet probe (facts 2 + 5). Redirects pointing back
    /// at the gateway are the nodogsplash interception signature.
    fn probe(&self, gateway: &str) -> GateProbe {
        match http_exchange(Method::Get, PROBE_URL, None, None, PROBE_BODY_CAP) {
            Ok(reply) => {
                if (300..400).contains(&reply.status)
                    && reply
                        .location
                        .as_deref()
                        .is_some_and(|loc| loc.contains(gateway))
                {
                    return GateProbe::Gated {
                        status: reply.status,
                        location: reply.location.unwrap_or_default(),
                    };
                }
                GateProbe::Open {
                    status: reply.status,
                }
            }
            Err(e) => GateProbe::Unreachable {
                reason: e.to_string(),
            },
        }
    }

    /// Idempotence fast path (fact 4): only pay when actually gated.
    /// An unreachable probe is a loud error — paying blind cannot fix a
    /// network with no path at all.
    pub fn already_authed(&self) -> Result<bool, TollgateError> {
        let gateway = self.gateway()?;
        match self.probe(gateway) {
            GateProbe::Open { .. } => Ok(true),
            GateProbe::Gated { .. } => Ok(false),
            GateProbe::Unreachable { reason } => Err(TollgateError::Http(format!(
                "internet probe {PROBE_URL} unreachable: {reason} — refusing to pay blind"
            ))),
        }
    }

    /// The full flow: discover (if needed) → probe (skip spend when
    /// already authed) → spend → raw-body submit → verify → report.
    pub fn pay<T: MintClient + Clone, S: ProofStore>(
        &mut self,
        engine: &mut WalletEngine<T, S>,
        wifi: &WifiManager,
        amount: Option<u64>,
    ) -> Result<PayOutcome, TollgateError> {
        if self.discovery.is_none() {
            self.discover(wifi)?;
        }
        let discovery = self.discovery.as_ref().expect("discover just ran");

        if self.already_authed()? {
            self.state = String::from("already authenticated — no spend");
            println!("tollgate: already authenticated, internet open — not paying");
            info!("tollgate: already-authed fast path, no spend");
            return Ok(PayOutcome::AlreadyAuthed);
        }

        let amount = amount
            .or(discovery.price_sats)
            .unwrap_or(DEFAULT_PRICE_SATS);
        println!("tollgate: gate closed — paying {amount} sats");

        // Spend: the engine composes and hands over the exact token.
        let token = engine
            .send_token(amount, None)
            .map_err(|e| TollgateError::Protocol(format!("spend {amount} sats failed: {e:?}")))?;
        self.state = format!("spent {amount} sats — token in flight");
        info!("tollgate: token composed ({amount} sats)");

        // Submit: RAW BODY POST, text/plain — never a JSON wrapper
        // (fact 1, the #1 integration gotcha).
        let reply = http_exchange(
            Method::Post,
            &discovery.backend_url,
            Some("text/plain"),
            Some(token.as_bytes()),
            SUBMIT_BODY_CAP,
        )?;
        if !(200..300).contains(&reply.status) {
            return Err(TollgateError::Protocol(format!(
                "payment rejected: HTTP {} — {}",
                reply.status,
                String::from_utf8_lossy(&reply.body)
            )));
        }
        self.state = format!("submitted {amount} sats (HTTP {})", reply.status);
        println!("tollgate: token submitted (HTTP {})", reply.status);
        info!("tollgate: payment submitted, HTTP {}", reply.status);

        // Verify: IP-literal probe; heal is asynchronous — retry once
        // with a small backoff (plan, router-side quirks).
        let gateway = discovery.gateway.as_str();
        let probe_status = match self.probe(gateway) {
            GateProbe::Open { status } => status,
            GateProbe::Gated { .. } | GateProbe::Unreachable { .. } => {
                std::thread::sleep(VERIFY_RETRY_BACKOFF);
                match self.probe(gateway) {
                    GateProbe::Open { status } => status,
                    GateProbe::Gated { status, location } => {
                        return Err(TollgateError::Protocol(format!(
                            "payment accepted but gate still closed (probe HTTP {status}, redirect {location})"
                        )))
                    }
                    GateProbe::Unreachable { reason } => {
                        return Err(TollgateError::Http(format!(
                            "post-payment probe unreachable: {reason}"
                        )))
                    }
                }
            }
        };

        self.state =
            format!("internet verified (probe HTTP {probe_status}) after {amount} sats payment");
        println!("tollgate: internet access verified (probe HTTP {probe_status})");
        info!("tollgate: verified, probe HTTP {probe_status}");
        Ok(PayOutcome::Paid {
            amount,
            http_status: reply.status,
        })
    }

    /// Last flow state + discovery summary for `tollgate status`.
    pub fn status(&self) -> String {
        match &self.discovery {
            Some(d) => format!(
                "{}; backend {} (kind {}, price {} {}/step, mint {})",
                self.state,
                d.backend_url,
                d.kind,
                d.price_sats
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| format!("default {DEFAULT_PRICE_SATS}")),
                d.price_unit.as_deref().unwrap_or("sat"),
                d.mint_url.as_deref().unwrap_or("?"),
            ),
            None => self.state.clone(),
        }
    }
}
