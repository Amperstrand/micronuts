//! Lightning backend seam and clock abstraction for the demo mint.
//!
//! `LightningBackend` isolates every Lightning-network side effect (invoice
//! creation, settlement lookup, amount lookup, payment) behind one trait so
//! the mint core stays a pure state machine that can be tested against a
//! fake wallet, a failing backend, or a real node adapter.
//!
//! `MintClock` isolates time so quote expiry is testable.

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use cashu_core_lite::error::CashuError;
use sha2::{Digest, Sha256};

/// Lightning payment backend used by the mint core.
pub trait LightningBackend: Send {
    /// Create an invoice for `amount_sat` and return its payment request
    /// string (bolt11 in a real backend).
    fn create_invoice(&mut self, amount_sat: u64, description: &str) -> Result<String, CashuError>;
    /// Return whether the invoice has been settled (paid) yet.
    fn is_settled(&mut self, invoice: &str) -> Result<bool, CashuError>;
    /// Return the invoice's amount in sats.
    fn lookup_amount(&mut self, invoice: &str) -> Result<u64, CashuError>;
    /// Pay the invoice and return the payment preimage as hex.
    fn pay_invoice(&mut self, invoice: &str, amount_sat: u64) -> Result<String, CashuError>;
}

/// Auto-settling fake Lightning wallet (mirrors cashu-cf's FakeWallet).
///
/// Invoices use the demo format `lnbcdemo{amount}sat1micronuts`, every
/// invoice counts as settled immediately, and payments always succeed with
/// the deterministic preimage `sha256(invoice)` hex.
pub struct FakeWallet;

impl LightningBackend for FakeWallet {
    fn create_invoice(
        &mut self,
        amount_sat: u64,
        _description: &str,
    ) -> Result<String, CashuError> {
        Ok(format!("lnbcdemo{amount_sat}sat1micronuts"))
    }

    fn is_settled(&mut self, _invoice: &str) -> Result<bool, CashuError> {
        Ok(true)
    }

    fn lookup_amount(&mut self, invoice: &str) -> Result<u64, CashuError> {
        parse_demo_invoice_amount(invoice)
            .ok_or_else(|| CashuError::Protocol("invalid demo invoice amount".to_string()))
    }

    fn pay_invoice(&mut self, invoice: &str, _amount_sat: u64) -> Result<String, CashuError> {
        Ok(hex::encode(Sha256::digest(invoice.as_bytes())))
    }
}

/// Attempt to parse an amount from a demo invoice string
/// (`lnbcdemo{amount}sat1micronuts`).
pub fn parse_demo_invoice_amount(invoice: &str) -> Option<u64> {
    let stripped = invoice.strip_prefix("lnbcdemo")?;
    let end = stripped.find("sat")?;
    stripped[..end].parse().ok()
}

/// Wall-clock source for quote expiry timestamps.
pub trait MintClock: Send {
    /// Current time as unix seconds.
    fn now_secs(&self) -> u64;
}

/// Real system clock (unix seconds since the epoch).
pub struct SystemClock;

impl MintClock for SystemClock {
    fn now_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Test clock: a settable/advancable unix-seconds cell.
pub struct MockClock {
    now: Cell<u64>,
}

impl MockClock {
    /// Create a clock pinned to `start` unix seconds.
    pub fn new(start: u64) -> Self {
        Self {
            now: Cell::new(start),
        }
    }

    /// Jump the clock to `now` unix seconds.
    pub fn set(&self, now: u64) {
        self.now.set(now);
    }

    /// Advance the clock by `secs` seconds.
    pub fn advance(&self, secs: u64) {
        self.now.set(self.now.get().saturating_add(secs));
    }
}

impl MintClock for MockClock {
    fn now_secs(&self) -> u64 {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_wallet_invoice_roundtrip() {
        let mut wallet = FakeWallet;
        let invoice = wallet.create_invoice(100, "micronuts").unwrap();
        assert_eq!(invoice, "lnbcdemo100sat1micronuts");
        assert_eq!(wallet.lookup_amount(&invoice).unwrap(), 100);
        assert!(wallet.is_settled(&invoice).unwrap());
    }

    #[test]
    fn fake_wallet_preimage_is_sha256_of_invoice() {
        let mut wallet = FakeWallet;
        let invoice = wallet.create_invoice(64, "micronuts").unwrap();
        let preimage = wallet.pay_invoice(&invoice, 64).unwrap();
        assert_eq!(preimage, hex::encode(Sha256::digest(invoice.as_bytes())));
        assert_eq!(preimage.len(), 64);
    }

    #[test]
    fn fake_wallet_rejects_foreign_invoice_format() {
        let mut wallet = FakeWallet;
        assert!(wallet.lookup_amount("garbage").is_err());
    }

    #[test]
    fn mock_clock_set_and_advance() {
        let clock = MockClock::new(1_000);
        assert_eq!(clock.now_secs(), 1_000);
        clock.advance(3_600);
        assert_eq!(clock.now_secs(), 4_600);
        clock.set(5);
        assert_eq!(clock.now_secs(), 5);
    }
}
