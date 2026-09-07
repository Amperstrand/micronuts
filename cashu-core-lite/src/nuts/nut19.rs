//! NUT-19: Cached Responses
//!
//! Advertisement shape for response caching on critical endpoints. The
//! cache itself lives at the HTTP edge (see micronuts-audit-adapter); this
//! module carries the settings the mint publishes via NUT-06.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/19.md

#[cfg(not(feature = "std"))]
use alloc::string::String;

use minicbor::{Decode, Encode};

/// One cached route: HTTP method + path (NUT-19 `cached_endpoints` entry).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
// NUT #19: {
// NUT #19:     "method": "POST",
// NUT #19:     "path": "/v1/mint/bolt11",
// NUT #19: },
pub struct CachedEndpoint {
    /// HTTP method of the cached route.
    #[n(0)]
    pub method: String,
    /// Path of the cached route.
    #[n(1)]
    pub path: String,
}
