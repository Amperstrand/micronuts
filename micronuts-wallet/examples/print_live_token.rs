//! Print a Cashu V4 token minted from a REAL mint over HTTP (default:
//! testnut.cashu.space — auto-settling FakeWallet, test money only per
//! the repo money taxonomy). Used by the physical QR-loop harness:
//! the token's QR is displayed on the CYD, scanned by the GM65 into the
//! F469 wallet. Usage: TESTNUT_AMOUNT=21 [MINT_URL=…] cargo run …
//! The engine prefers the sats keyset on multi-unit mints.

use cashu_core_lite::store::MemoryStore;
use micronuts_wallet::engine::WalletEngine;

fn main() {
    let url =
        std::env::var("MINT_URL").unwrap_or_else(|_| String::from("https://testnut.cashu.space"));
    let fund: u64 = std::env::var("TESTNUT_AMOUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);
    // testnut charges a NUT-08 input fee; fund above the send amount.
    let send_amount: u64 = std::env::var("TESTNUT_SEND")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21);

    // Fresh random seed per run: the deterministic wallet derives blinded
    // secrets from the seed, and a fixed seed re-spends secret slots the
    // mint has already signed (rejecting the mint as duplicates).
    let mut seed = [0u8; 32];
    use rand_core::OsRng;
    use rand_core::RngCore;
    OsRng.fill_bytes(&mut seed);
    let mut wallet = WalletEngine::new(
        &url,
        micronuts_wallet::http::http_mint_client(&url),
        MemoryStore::new(),
        seed,
        Vec::new(),
    )
    .expect("engine");
    wallet.connect().expect("connect");
    println!("mint: {} ({})", wallet.mint_name(), wallet.mint_url());

    let quote = wallet.mint_via_invoice(fund).expect("mint quote");
    let quote_id = quote.quote.clone();
    drop(quote);
    // FakeWallet settles instantly; poll a few times for the PAID state.
    let mut paid_amount = None;
    for i in 0..10 {
        let q = wallet.poll_mint_quote(&quote_id).expect("poll");
        println!("poll {i}: state={} amount={}", q.state, q.amount);
        if q.state == "PAID" {
            paid_amount = Some(q.amount);
            break;
        }
        if q.state == "ISSUED" {
            // Already minted under this quote — create a fresh one.
            let quote = wallet.mint_via_invoice(fund).expect("mint quote retry");
            println!("retry quote {}", quote.quote);
            let _ = &quote_id;
        }
        std::thread::sleep(std::time::Duration::from_millis(700));
    }
    let paid = paid_amount.expect("invoice never settled");
    wallet.mint_paid_quote(&quote_id, paid).expect("mint");
    println!("minted {} sats over HTTPS", paid);

    let token = wallet
        .send_token(send_amount, Some("physical loop"))
        .expect("send");
    println!("{token}");
}
