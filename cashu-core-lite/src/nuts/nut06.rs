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
    /// List of supported NUTs with their settings.
    #[n(5)]
    pub nuts: NutSupport,
}

/// Contact entry for mint info (NUT-06).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ContactInfo {
    #[n(0)]
    pub method: String,
    #[n(1)]
    pub info: String,
}

/// Which NUTs this mint supports and their configuration (NUT-06).
///
/// Demo shortcut: only the minimal set is advertised; real mints would
/// include per-NUT settings objects.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct NutSupport {
    /// Supported NUT numbers (e.g. [0, 1, 2, 3, 4, 5, 6]).
    #[n(0)]
    pub supported: Vec<u32>,
}
