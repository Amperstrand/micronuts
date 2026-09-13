//! Mint a demo token with the browser wallet's exact engine + embedded
//! mint (the pinned device keyset) and print the cashuB token — the
//! mint leg of the browser→device QR handoff
//! (`scripts/test_qr_handoff.sh`). Stdout carries ONLY the token.

use cashu_core_lite::store::MemoryStore;
use micronuts_wallet::demo_mint::{DemoMintClient, DEMO_MINT_URL};
use micronuts_wallet::engine::WalletEngine;
use rand_core::{OsRng, RngCore};

fn main() {
    let amount: u64 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("21"))
        .parse()
        .expect("amount in sats");
    assert!(
        (1..=255).contains(&amount),
        "demo denominations are 1,2,…,128 (max sum 255 sats)"
    );

    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let mut engine = WalletEngine::new(
        DEMO_MINT_URL,
        DemoMintClient::new(),
        MemoryStore::new(),
        seed,
        Vec::new(),
    )
    .expect("engine boots");
    engine.connect().expect("demo mint connects");

    let quote = engine
        .mint_via_invoice(amount)
        .expect("demo quote auto-pays");
    let minted = engine
        .mint_paid_quote(&quote.quote, amount)
        .expect("mint completes");
    assert_eq!(minted, amount);

    let token = engine.send_token(amount, None).expect("send token");
    assert!(
        token.starts_with("cashuB"),
        "engine must emit V4 wire tokens"
    );
    eprintln!("minted {amount} sats via the pinned demo keyset (browser engine path)");
    println!("{token}");
}
