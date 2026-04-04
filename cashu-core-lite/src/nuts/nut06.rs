//! NUT-06: Mint Information
//!
//! Defines the `GET /v1/info` response, providing metadata about the mint.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/06.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Mint information response for `GET /v1/info` (NUT-06).
#[derive(Debug, Clone)]
pub struct MintInfo {
    /// Human-readable mint name.
    pub name: String,
    /// Mint's public key (hex-encoded compressed secp256k1 point).
    pub pubkey: String,
    /// Mint software version string.
    pub version: String,
    /// Short description of the mint.
    pub description: String,
    /// Contact information.
    pub contact: Vec<ContactInfo>,
    /// List of supported NUTs with their settings.
    pub nuts: NutSupport,
}

/// Contact entry for mint info (NUT-06).
#[derive(Debug, Clone)]
pub struct ContactInfo {
    pub method: String,
    pub info: String,
}

/// Which NUTs this mint supports and their configuration (NUT-06).
///
/// Demo shortcut: only the minimal set is advertised; real mints would
/// include per-NUT settings objects.
#[derive(Debug, Clone)]
pub struct NutSupport {
    /// Supported NUT numbers (e.g. [0, 1, 2, 3, 4, 5, 6]).
    pub supported: Vec<u32>,
}
