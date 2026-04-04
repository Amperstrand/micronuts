//! Demo Cashu Mint Core
//!
//! A minimal in-memory Cashu mint implementing NUT-00 through NUT-07.
//! All state is in RAM — no persistence, no real Lightning backend.
//!
//! Demo shortcuts (vs production Cashu):
//! - No durable spent-proof state (NUT-07 stub only)
//! - Quotes auto-approve immediately (no real Lightning)
//! - No persistence across restarts
//! - One hardcoded active keyset
//! - Fee reserve is always 0
//! - Payment preimages are dummy hex strings

pub mod keyset;

mod mint_core;
pub use mint_core::DemoMint;

mod direct_transport;
pub use direct_transport::DirectTransport;
