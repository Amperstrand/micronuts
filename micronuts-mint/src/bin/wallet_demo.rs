//! Explicit wallet-role demo binary.
//!
//! Build and run:
//!   cargo run -p micronuts-mint --bin wallet_demo

fn main() -> Result<(), cashu_core_lite::CashuError> {
    micronuts_mint::run_wallet_demo()
}
