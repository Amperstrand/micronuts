//! REST transport for the wallet: `ureq` over the shared wire protocol
//! in `mint_wire.rs`.

use std::time::Duration;

use cashu_core_lite::error::CashuError;
use serde_json::Value;

use micronuts_wallet_core::mint_wire::{JsonTransport, WireMintClient};

/// The native mint client: shared wire protocol over `ureq`.
pub type HttpMintClient = WireMintClient<UreqTransport>;

/// Build a client for `base_url` (e.g. `http://127.0.0.1:3030`).
pub fn http_mint_client(base_url: &str) -> HttpMintClient {
    WireMintClient::with_transport(base_url, UreqTransport::new())
}

/// ureq-backed JSON transport (10 s connect / 30 s per-call timeout).
/// `Clone` shares the connection pool.
#[derive(Clone)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build();
        Self { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonTransport for UreqTransport {
    fn get_json(&self, url: &str) -> Result<Value, CashuError> {
        let response = self.agent.get(url).call().map_err(map_ureq_error)?;
        read_json_body(response)
    }

    fn post_json(&self, url: &str, body: &Value) -> Result<Value, CashuError> {
        let response = self
            .agent
            .post(url)
            .set("Content-Type", "application/json")
            .send_json(body.clone())
            .map_err(map_ureq_error)?;
        read_json_body(response)
    }
}

fn map_ureq_error(err: ureq::Error) -> CashuError {
    match err {
        ureq::Error::Status(_code, response) => {
            let body: Value = response.into_json().unwrap_or(Value::Null);
            let detail = body
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("mint returned an error")
                .to_string();
            match body.get("code").and_then(Value::as_str) {
                Some("QUOTE_NOT_FOUND") => CashuError::QuoteNotFound,
                Some("KEYSET_NOT_FOUND") => CashuError::KeysetNotFound,
                Some("INVALID_AMOUNT") => CashuError::InvalidAmount,
                Some("INVALID_PROOF") => CashuError::InvalidProof,
                Some("AMOUNT_MISMATCH") => CashuError::AmountMismatch,
                Some("INSUFFICIENT_INPUTS") => CashuError::InsufficientInputs,
                Some("QUOTE_NOT_PAID") => CashuError::QuoteNotPaid,
                Some("QUOTE_ALREADY_ISSUED") => CashuError::QuoteAlreadyIssued,
                Some("TOKENS_ALREADY_SPENT") => CashuError::TokensAlreadySpent,
                Some("MELT_ALREADY_PAID") => CashuError::MeltAlreadyPaid,
                Some("SPEND_CONDITIONS_NOT_MET") => CashuError::SpendConditionsNotMet,
                Some("PAYMENT_FAILED") => CashuError::PaymentFailed,
                Some("QUOTE_SIGNATURE_INVALID") => CashuError::QuoteSignatureInvalid,
                Some("BATCH_TOO_LARGE") => CashuError::BatchTooLarge,
                Some("TOO_MANY_OUTPUTS") => CashuError::TooManyOutputs,
                Some("BATCH_QUOTE_NOT_UNIQUE") => CashuError::BatchQuoteNotUnique,
                _ => CashuError::Protocol(detail),
            }
        }
        ureq::Error::Transport(transport) => CashuError::Transport(transport.to_string()),
    }
}

fn read_json_body(response: ureq::Response) -> Result<Value, CashuError> {
    response
        .into_json()
        .map_err(|e| CashuError::Transport(format!("invalid mint response body: {e}")))
}
