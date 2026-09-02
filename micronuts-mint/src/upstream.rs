//! Upstream Cashu mint settlement backend — the cashu-cf "rugs02" model.
//!
//! Our mint is the user-facing front; a real upstream Cashu mint settles
//! "Lightning": `create_invoice` hands out the upstream mint quote's REAL
//! bolt11, `is_settled` polls the upstream quote state, `lookup_amount`
//! caches an upstream melt quote for the invoice, and `pay_invoice` melts
//! our reserve of upstream proofs ([`crate::reserve::ReserveWallet`]).
//!
//! Host-only, behind the `backend-upstream` feature (ureq/rustls). Enable
//! at runtime with `MICRONUTS_UPSTREAM_MINT` (see [`upstream_backend_from_env`]).
//!
//! Payment-safety caveat (cashu-cf ISSUE-061 lesson): an upstream melt
//! response in a non-terminal state (PENDING/unknown) is reported as
//! `CashuError::Protocol("upstream melt state ambiguous: …")` and the
//! reserve proofs it selected are parked — never reused, since the
//! upstream quote may still consume them. A definitive upstream FAILED —
//! or an HTTP error on the melt POST (prototype simplification) — reports
//! `CashuError::PaymentFailed`. The PENDING-ambiguity poller that later
//! re-resolves parked melts is a documented follow-up.

use std::collections::HashMap;
use std::time::Duration;

use cashu_core_lite::error::CashuError;
use serde_json::Value;

use crate::ln::{LightningBackend, MintClock, SystemClock};
use crate::reserve::ReserveWallet;

/// Default initial reserve size / auto-top-up margin in sats.
const DEFAULT_BOOTSTRAP_SATS: u64 = 1000;

/// Cached upstream melt quote for one of our invoices.
struct UpstreamMeltQuote {
    quote_id: String,
    amount: u64,
    fee_reserve: u64,
}

/// [`crate::ln::LightningBackend`] backed by an upstream Cashu mint.
pub struct UpstreamCashuBackend {
    base_url: String,
    unit: String,
    http: ureq::Agent,
    /// our-invoice (upstream bolt11) → upstream mint-quote id
    mint_quotes: HashMap<String, String>,
    /// invoice → upstream melt quote
    melt_quotes: HashMap<String, UpstreamMeltQuote>,
    reserve: ReserveWallet,
}

impl UpstreamCashuBackend {
    /// Build a backend for `base_url` (e.g. `https://testnut.cashu.exchange`).
    ///
    /// `bootstrap_sats` is the initial reserve size and auto-top-up margin.
    pub fn new(base_url: &str, unit: &str, bootstrap_sats: u64) -> Self {
        let http = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            unit: unit.to_string(),
            http,
            mint_quotes: HashMap::new(),
            melt_quotes: HashMap::new(),
            reserve: ReserveWallet::new(base_url, unit, bootstrap_sats),
        }
    }

    /// Current upstream-proof reserve balance in sats (test/ops getter).
    pub fn reserve_balance_sats(&self) -> u64 {
        self.reserve.balance_sats()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

impl LightningBackend for UpstreamCashuBackend {
    fn create_invoice(
        &mut self,
        amount_sat: u64,
        _description: &str,
    ) -> Result<String, CashuError> {
        let quote = upstream_post(
            &self.http,
            &self.url("/v1/mint/quote/bolt11"),
            &serde_json::json!({"amount": amount_sat, "unit": self.unit}),
        )?;
        let quote_id = json_str("quote", &quote["quote"])?;
        let invoice = json_str("request", &quote["request"])?;
        self.mint_quotes.insert(invoice.clone(), quote_id);
        Ok(invoice)
    }

    fn is_settled(&mut self, invoice: &str) -> Result<bool, CashuError> {
        let Some(quote_id) = self.mint_quotes.get(invoice) else {
            return Err(CashuError::Protocol(
                "is_settled: invoice has no upstream mint quote".to_string(),
            ));
        };
        let url = self.url(&format!("/v1/mint/quote/bolt11/{quote_id}"));
        let quote = match upstream_get(&self.http, &url) {
            Ok(quote) => quote,
            // Pollers must survive upstream blips: report not-yet-paid.
            Err(err) => {
                eprintln!("upstream: mint-quote poll failed ({err}); treating as unsettled");
                return Ok(false);
            }
        };
        let state = json_str("state", &quote["state"])?;
        Ok(matches!(state.as_str(), "PAID" | "ISSUED"))
    }

    fn lookup_amount(&mut self, invoice: &str) -> Result<u64, CashuError> {
        if let Some(quote) = self.melt_quotes.get(invoice) {
            return Ok(quote.amount);
        }
        let quote = upstream_post(
            &self.http,
            &self.url("/v1/melt/quote/bolt11"),
            &serde_json::json!({"request": invoice, "unit": self.unit}),
        )?;
        let entry = UpstreamMeltQuote {
            quote_id: json_str("quote", &quote["quote"])?,
            amount: json_amount("amount", &quote["amount"])?,
            fee_reserve: json_amount_or("fee_reserve", quote.get("fee_reserve"), 0)?,
        };
        let amount = entry.amount;
        self.melt_quotes.insert(invoice.to_string(), entry);
        Ok(amount)
    }

    fn pay_invoice(&mut self, invoice: &str, _amount_sat: u64) -> Result<String, CashuError> {
        // `_amount_sat` (our quote's view) is deliberately ignored: the
        // cached upstream melt quote is the payment's source of truth.
        if !self.melt_quotes.contains_key(invoice) {
            self.lookup_amount(invoice)?;
        }
        let Some(quote) = self.melt_quotes.get(invoice) else {
            return Err(CashuError::Protocol(
                "pay_invoice: invoice has no upstream melt quote".to_string(),
            ));
        };
        // The cached upstream quote is the payment's source of truth.
        let needed = quote
            .amount
            .checked_add(quote.fee_reserve)
            .ok_or(CashuError::InvalidAmount)?;
        let quote_id = quote.quote_id.clone();
        self.reserve.pay(&quote_id, needed, &self.http)
    }
}

/// Build the upstream backend from the environment:
/// `MICRONUTS_UPSTREAM_MINT` (URL), `MICRONUTS_UPSTREAM_UNIT` (default
/// `sat`), `MICRONUTS_RESERVE_BOOTSTRAP_SATS` (default 1000).
///
/// Returns `None` when `MICRONUTS_UPSTREAM_MINT` is unset/empty (caller
/// falls back to the FakeWallet demo backend).
pub fn upstream_backend_from_env(
) -> Option<(Box<dyn LightningBackend + Send>, Box<dyn MintClock + Send>)> {
    let base_url = std::env::var("MICRONUTS_UPSTREAM_MINT").ok()?;
    if base_url.trim().is_empty() {
        return None;
    }
    let unit = std::env::var("MICRONUTS_UPSTREAM_UNIT").unwrap_or_else(|_| "sat".to_string());
    let bootstrap_sats = std::env::var("MICRONUTS_RESERVE_BOOTSTRAP_SATS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_BOOTSTRAP_SATS);
    Some((
        Box::new(UpstreamCashuBackend::new(&base_url, &unit, bootstrap_sats)),
        Box::new(SystemClock),
    ))
}

// ---- shared upstream HTTP + JSON helpers (also used by the reserve) ----

pub(crate) fn upstream_get(http: &ureq::Agent, url: &str) -> Result<Value, CashuError> {
    let response = http
        .get(url)
        .call()
        .map_err(|err| CashuError::Transport(format!("upstream GET {url}: {err}")))?;
    serde_json::from_reader(response.into_reader())
        .map_err(|err| CashuError::Protocol(format!("upstream GET {url}: invalid JSON: {err}")))
}

pub(crate) fn upstream_post(
    http: &ureq::Agent,
    url: &str,
    body: &Value,
) -> Result<Value, CashuError> {
    match http.post(url).send_json(body.clone()) {
        Ok(response) => serde_json::from_reader(response.into_reader()).map_err(|err| {
            CashuError::Protocol(format!("upstream POST {url}: invalid JSON: {err}"))
        }),
        Err(ureq::Error::Status(code, response)) => {
            let text = response
                .into_string()
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            Err(CashuError::Protocol(format!(
                "upstream POST {url}: status {code}: {text}"
            )))
        }
        Err(err) => Err(CashuError::Transport(format!("upstream POST {url}: {err}"))),
    }
}

/// Amounts may arrive as JSON numbers or decimal strings (cashu-ts/cashu-cf
/// serialize u64 as a string in some fields); anything else is a protocol
/// error — never a panic.
pub(crate) fn json_amount(field: &str, value: &Value) -> Result<u64, CashuError> {
    match value {
        Value::Number(n) => n.as_u64().ok_or_else(|| amount_err(field, value)),
        Value::String(s) => s.parse::<u64>().map_err(|_| amount_err(field, value)),
        _ => Err(amount_err(field, value)),
    }
}

/// [`json_amount`] for optional fields: missing/null → `default`.
pub(crate) fn json_amount_or(
    field: &str,
    value: Option<&Value>,
    default: u64,
) -> Result<u64, CashuError> {
    match value {
        None | Some(Value::Null) => Ok(default),
        Some(v) => json_amount(field, v),
    }
}

pub(crate) fn json_str(field: &str, value: &Value) -> Result<String, CashuError> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| CashuError::Protocol(format!("upstream field '{field}' is not a string")))
}

fn amount_err(field: &str, value: &Value) -> CashuError {
    CashuError::Protocol(format!(
        "upstream field '{field}' is not an amount: {value}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_amount_accepts_number_and_string() {
        assert_eq!(json_amount("amount", &json!(100)).unwrap(), 100);
        assert_eq!(json_amount("amount", &json!("100")).unwrap(), 100);
        assert_eq!(json_amount("amount", &json!(0)).unwrap(), 0);
        assert!(json_amount("amount", &json!(-1)).is_err());
        assert!(json_amount("amount", &json!(1.5)).is_err());
        assert!(json_amount("amount", &json!(null)).is_err());
        assert!(json_amount("amount", &json!("abc")).is_err());
    }

    #[test]
    fn json_amount_or_defaults_on_missing_or_null() {
        assert_eq!(json_amount_or("fee_reserve", None, 7).unwrap(), 7);
        assert_eq!(
            json_amount_or("fee_reserve", Some(&Value::Null), 7).unwrap(),
            7
        );
        assert_eq!(
            json_amount_or("fee_reserve", Some(&json!(3)), 7).unwrap(),
            3
        );
        assert!(json_amount_or("fee_reserve", Some(&json!("x")), 7).is_err());
    }

    #[test]
    fn json_str_rejects_non_strings() {
        assert_eq!(json_str("quote", &json!("abc")).unwrap(), "abc");
        assert!(json_str("quote", &json!(42)).is_err());
        assert!(json_str("quote", &json!(null)).is_err());
    }
}
