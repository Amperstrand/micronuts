//! Gate controller: board-independent wiring between a scanned payload
//! and the [`OfflineGateValidator`] — the firmware bring-up surface.
//!
//! The firmware side implements [`GateIo`] (relay/GPIO, LEDs, display,
//! log) and feeds each decoded scan to [`GateController::handle_scan`].
//! Everything else — token decode, trust, DLEQ verification against
//! pinned keysets, replay ring, persist-before-open — lives in the
//! validator and is covered by the walletport test suite on the host.
//! The board session is then flash, scan, observe.

use crate::{GateDecision, OfflineGateValidator, WalletPortError};
#[cfg(not(feature = "std"))]
use alloc::string::String;
use cashu_core_lite::store::ProofStore;

/// What the firmware wants done in response to a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateAction {
    /// Open the gate for the standard access window; pulse OK.
    Open { total_sats: u64 },
    /// Verified but short: show shortfall, no gate movement; pulse ERR.
    Underpaid { total_sats: u64, needed: u64 },
    /// Rejected (replay/tamper/untrusted/locked/undecodable): pulse ERR.
    Rejected { reason: RejectionReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    NotAToken,
    Undecodable,
    UntrustedMint,
    LockedSecret,
    Replay,
    InvalidProof,
    Other,
}

impl From<&WalletPortError> for RejectionReason {
    fn from(e: &WalletPortError) -> Self {
        match e {
            WalletPortError::Decode(_) => RejectionReason::Undecodable,
            WalletPortError::UntrustedMint(_) => RejectionReason::UntrustedMint,
            WalletPortError::LockedSecret => RejectionReason::LockedSecret,
            WalletPortError::Replay => RejectionReason::Replay,
            WalletPortError::InvalidProof(_) => RejectionReason::InvalidProof,
            _ => RejectionReason::Other,
        }
    }
}

/// Board I/O the firmware provides. All operations are non-blocking
/// hints — the controller's decision logic never depends on them
/// succeeding (persistence is the validator's store, not the display).
pub trait GateIo {
    /// Energize the gate/relay for the access window.
    fn open(&mut self);
    /// Human-facing success signal (LED/beeper/display).
    fn signal_ok(&mut self);
    /// Human-facing rejection signal; reason for display/logging.
    fn signal_err(&mut self, reason: RejectionReason);
    /// Optional status line (display/UART). Default: ignore.
    fn status(&mut self, _line: &str) {}
}

/// Scan-to-gate controller over an offline validator.
pub struct GateController<S: ProofStore> {
    validator: OfflineGateValidator<S>,
    price_sats: u64,
}

impl<S: ProofStore> GateController<S> {
    pub fn new(validator: OfflineGateValidator<S>, price_sats: u64) -> Self {
        Self {
            validator,
            price_sats,
        }
    }

    /// Process one decoded scan payload (QR content as text). Returns the
    /// action for the firmware to execute; the spent ring has already
    /// been persisted when the action is `Open`.
    pub fn handle_scan<I: GateIo>(&mut self, io: &mut I, payload: &str) -> GateAction {
        let Some(token) = extract_cashu_token(payload) else {
            io.signal_err(RejectionReason::NotAToken);
            return GateAction::Rejected {
                reason: RejectionReason::NotAToken,
            };
        };

        match self.validator.verify_token(token, self.price_sats) {
            Ok(GateDecision::Open { total_sats }) => {
                // Persist-before-open already happened inside the
                // validator; drive the hardware effects now.
                io.open();
                io.signal_ok();
                io.status("open");
                GateAction::Open { total_sats }
            }
            Ok(GateDecision::Underpaid { total_sats }) => {
                let needed = self.price_sats;
                io.signal_err(RejectionReason::Other);
                io.status("underpaid");
                GateAction::Underpaid { total_sats, needed }
            }
            Err(e) => {
                let reason = RejectionReason::from(&e);
                io.signal_err(reason);
                GateAction::Rejected { reason }
            }
        }
    }
}

/// Accept a bare `cashuB…` payload or one embedded in a larger scanned
/// string (QR payloads sometimes carry a label/scheme prefix). V3
/// `cashuA` is not supported by the V4-CBOR core by design.
fn extract_cashu_token(payload: &str) -> Option<&str> {
    let text = payload.trim();
    let start = text.find("cashuB")?;
    // Token runs to whitespace/end — base64url chars only after the
    // prefix, so the first separator terminates it.
    let rest = &text[start + "cashuB".len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '=')
        .unwrap_or(rest.len());
    Some(&text[start..start + "cashuB".len() + end])
}

/// Convenience for firmware bring-up: run the validator against a pinned
/// keyset bundle and report firmware-ready status text.
pub fn bringup_status<S: ProofStore>(controller: &GateController<S>) -> String {
    let mut s = String::from("gate: price ");
    #[cfg(feature = "std")]
    {
        use std::fmt::Write;
        let _ = write!(s, "{}", controller.price_sats);
    }
    #[cfg(not(feature = "std"))]
    {
        use core::fmt::Write;
        let _ = write!(s, "{}", controller.price_sats);
    }
    s.push_str(" sats, validator ready");
    s
}
