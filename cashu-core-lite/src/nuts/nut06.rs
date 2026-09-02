//! NUT-06: Mint Information
//!
//! Defines the `GET /v1/info` response, providing metadata about the mint.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/06.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use minicbor::{Decode, Encode};

/// Mint information response for `GET /v1/info` (NUT-06).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MintInfo {
    /// Human-readable mint name.
    #[n(0)]
    pub name: String,
    /// Mint's public key (hex-encoded compressed secp256k1 point).
    #[n(1)]
    pub pubkey: String,
    /// Mint software version string.
    #[n(2)]
    pub version: String,
    /// Short description of the mint.
    #[n(3)]
    pub description: String,
    /// Contact information.
    #[n(4)]
    pub contact: Vec<ContactInfo>,
    /// Supported NUTs with their settings, keyed by NUT number string
    /// (e.g. `("4", NutSettings { methods: [bolt11/sat] })`). Encoded as a
    /// CBOR array of `(String, NutSettings)` pairs; JSON adapters render it
    /// as the spec's nut-number → settings-object map.
    #[n(5)]
    pub nuts: Vec<(String, NutSettings)>,
}

/// Contact entry for mint info (NUT-06).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ContactInfo {
    #[n(0)]
    pub method: String,
    #[n(1)]
    pub info: String,
}

/// Per-NUT settings object (NUT-06 `nuts` map value). An empty `methods`
/// list means the NUT is supported with no method-specific settings.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct NutSettings {
    /// Payment methods this NUT supports (empty for non-payment NUTs).
    #[n(0)]
    pub methods: Vec<PaymentMethod>,
}

/// A payment method entry (e.g. `bolt11` in unit `sat`) inside a
/// `NutSettings` object.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct PaymentMethod {
    /// Method name (e.g. "bolt11").
    #[n(0)]
    pub method: String,
    /// Unit the method operates in (e.g. "sat").
    #[n(1)]
    pub unit: String,
}
