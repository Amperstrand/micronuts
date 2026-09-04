//! WalletPort facade over cashu-core-lite.
//!
//! Ports the pattern proven by tollgate-module-basic-go #299: business
//! logic depends on a library-agnostic wallet interface; adapters sit
//! behind it. See `docs/WALLETPORT-EDGE-VALIDATOR-DESIGN.md`.
//!
//! Two deliverables live here:
//!
//! * [`WalletPort`] - the tmbg-shaped trait plus [`TrustModel`] and the
//!   refusal policies the #299 review demanded (overpayment caps are
//!   *refused*, never silently ignored; untrusted mints rejected).
//! * [`OfflineGateValidator`] - the edge firmware profile: decode a V4
//!   token, verify every proof's NUT-12 DLEQ against **pinned keysets**
//!   (public-key-only, no mint contact), check value, reject spending
//!   conditions by default, and mark secrets spent *before* opening the
//!   gate (persist-before-effect, same ordering as `PersistentWallet`).

#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use cashu_core_lite::nuts::nut00;
use cashu_core_lite::nuts::nut01;
use cashu_core_lite::token::TokenV4;

pub mod gate;
pub mod offline;

pub use gate::{GateAction, GateController, GateIo, RejectionReason};
pub use offline::{GateDecision, OfflineGateValidator};

/// Facade-level errors (mirror of tmbg's port-level error surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletPortError {
    /// Token could not be decoded.
    Decode(String),
    /// The token's mint is not in the accepted list and untrusted mints
    /// are disabled (the edge profile's only mode).
    UntrustedMint(String),
    /// A proof failed offline verification (missing/bad DLEQ, wrong
    /// keyset, tampered signature).
    InvalidProof(String),
    /// The secret carries NUT-10/11 spending conditions — rejected by
    /// default; an offline gate cannot evaluate locks.
    LockedSecret,
    /// Value below the required price.
    InsufficientAmount { got: u64, need: u64 },
    /// This secret (token) was already used at this gate.
    Replay,
    /// Persistence layer failure.
    Storage(String),
    /// Operation not supported by this adapter (e.g. Lightning on MCU).
    Unsupported(&'static str),
}

/// Mint trust policy, ported from tmbg #299's review semantics.
#[derive(Debug, Clone, Default)]
pub struct TrustModel {
    /// Mints whose tokens this wallet accepts.
    pub accepted_mints: Vec<String>,
    /// Escape hatch mirroring `allowAndSwapUntrustedMints` — the edge
    /// profile keeps this `false` forever.
    pub allow_untrusted: bool,
}

impl TrustModel {
    pub fn new(accepted_mints: Vec<String>, allow_untrusted: bool) -> Self {
        Self {
            accepted_mints,
            allow_untrusted,
        }
    }

    pub fn mint_accepted(&self, mint: &str) -> bool {
        self.accepted_mints.iter().any(|m| m == mint) || self.allow_untrusted
    }
}

/// Reject-by-default check for NUT-10/11 spending conditions: wallets and
/// mints serialize condition-carrying secrets as JSON, plain secrets as
/// hex. An offline gate cannot sign/unlock, so anything JSON-shaped is
/// refused. This is parity with tmbg's `ErrLockedToken` on receive.
pub fn secret_has_spending_conditions(secret: &str) -> bool {
    secret.starts_with('{')
}

/// The tmbg-shaped wallet interface (method-for-method port of
/// tollgate-module-basic-go #299 `WalletPort`; online operations require
/// a transport, the edge profile implements only the offline subset).
pub trait WalletPort {
    /// Parse a V4 token string; caller owns the decoded value.
    fn decode_token(&self, token: &str) -> Result<TokenV4, WalletPortError>;

    /// Credit a token to the wallet (online; verify + optionally swap).
    fn receive(&mut self, token: &TokenV4) -> Result<u64, WalletPortError>;

    /// Total balance across mints.
    fn balance(&self) -> u64;

    /// Select and hand out proofs covering `amount` (may overshoot;
    /// exact change requires a swap).
    fn spend(&mut self, amount: u64) -> Result<Vec<nut00::Proof>, WalletPortError>;

    /// Send with overpayment caps. Adapters without cap enforcement must
    /// REFUSE non-zero caps — silently ignoring them is the #299 bug.
    fn send_with_overpayment(
        &mut self,
        amount: u64,
        max_overpayment_percent: u64,
        max_overpayment_absolute: u64,
    ) -> Result<String, WalletPortError>;

    /// Create a token for the full balance of a mint.
    fn drain(&mut self, mint: &str) -> Result<(TokenV4, u64), WalletPortError>;

    /// Release resources; must be idempotent.
    fn shutdown(&mut self) -> Result<(), WalletPortError>;
}

/// Shared decode helper with the facade error mapping.
pub fn decode_token_or_err(token: &str) -> Result<TokenV4, WalletPortError> {
    let bytes = strip_prefix(token);
    cashu_core_lite::token::decode_token(&bytes)
        .map_err(|e| WalletPortError::Decode(alloc_string(e)))
}

#[cfg(not(feature = "std"))]
fn alloc_string<E: core::fmt::Display>(e: E) -> String {
    use core::fmt::Write;
    let mut s = String::new();
    let _ = write!(s, "{}", e);
    s
}

#[cfg(feature = "std")]
fn alloc_string<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Accept `cashuB<base64url>` (V4 wire form) or raw CBOR bytes as hex;
/// returns the bytes to feed `decode_token`. V3 (`cashuA`) is rejected —
/// cashu-core-lite is V4-CBOR by design (gap G1 in the design doc).
fn strip_prefix(token: &str) -> Vec<u8> {
    let t = token.trim();
    const PREFIX: &str = "cashuB";
    if let Some(rest) = t.strip_prefix(PREFIX) {
        base64url_decode(rest)
    } else {
        hex::decode(t).unwrap_or_default()
    }
}

/// Minimal base64url (no padding) decoder — `alloc` only, no external
/// dependency, sized for tokens (a few KiB).
fn base64url_decode(input: &str) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for &b in input.as_bytes() {
        if b == b'=' {
            break;
        }
        let Some(pos) = TABLE.iter().position(|&t| t == b) else {
            continue;
        };
        buf = (buf << 6) | pos as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

/// Encode a V4 token to the `cashuB…` wire form (inverse of
/// [`decode_token_or_err`]). Delegates to cashu-core-lite so every
/// consumer (walletport, the device firmware, host-mint-tool) shares
/// ONE wire encoder — the 2026-09-04 integer-key lesson.
pub fn encode_token_wire(token: &TokenV4) -> Result<String, WalletPortError> {
    cashu_core_lite::token::encode_token_wire(token)
        .map_err(|e| WalletPortError::Decode(alloc_string(e)))
}

/// Keyset lookup used by the offline validator: pinned keysets keyed by
/// keyset ID.
pub fn find_keyset<'a>(pinned: &'a [nut01::KeySet], keyset_id: &str) -> Option<&'a nut01::KeySet> {
    pinned.iter().find(|k| k.id == keyset_id)
}
