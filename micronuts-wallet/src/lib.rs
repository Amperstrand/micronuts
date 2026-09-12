//! Micronuts wallet: a host Cashu wallet with a Slint UI.
//!
//! Crate layout:
//! - `http`: REST implementation of the `MintClient` transport trait
//! - `engine`: wallet operations on top of `PersistentWallet`
//! - `state`: file-backed persistence (`ProofStore` + wallet metadata)
//! - `ui`: Slint application wiring
