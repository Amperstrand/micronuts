//! Demo binary showing end-to-end Cashu mint/wallet flow.
//!
//! Build and run:
//!   cargo run -p micronuts-mint --bin demo
//!
//! This demonstrates the full flow without hardware or networking, but now
//! across a real serialized RPC boundary:
//!   1. Wallet gets mint info and keys
//!   2. Wallet mints ecash (mint quote → mint)
//!   3. Wallet swaps ecash (changes denominations)
//!   4. Wallet melts ecash (pays a dummy invoice)

use cashu_core_lite::nuts::nut00;
use cashu_core_lite::rpc::RpcMintClient;
use cashu_core_lite::Wallet;
use micronuts_mint::{DemoMint, LoopbackTransport};
use rand::rngs::StdRng;
use rand::SeedableRng;

fn main() {
    println!("=== Micronuts Wallet ↔ Mint Demo ===\n");

    // Create the demo mint service and connect the wallet through the
    // serialized RPC loopback transport.
    let mint = DemoMint::new();
    let transport = RpcMintClient::new(LoopbackTransport::from_demo_mint(mint));
    let mut wallet = Wallet::new("https://demo.micronuts.local", transport);
    let mut rng = StdRng::seed_from_u64(1337);

    // ---- Step 1: Get mint info (NUT-06) ----
    let info = wallet.get_info().unwrap();
    println!("1. Mint info:");
    println!("   Name: {}", info.name);
    println!("   Version: {}", info.version);
    println!("   Supported NUTs: {:?}", info.nuts.supported);

    // ---- Step 2: Get keys (NUT-01) ----
    let keys = wallet.get_keys().unwrap();
    let keyset = &keys.keysets[0];
    let keyset_id = keyset.id.clone();
    println!("\n2. Active keyset: {} (unit: {})", keyset_id, keyset.unit);
    println!("   Denominations: {:?}", keyset.keys.iter().map(|k| k.amount).collect::<Vec<_>>());

    // ---- Step 3: Get keysets (NUT-02) ----
    let keysets = wallet.get_keysets().unwrap();
    println!("\n3. Keysets: {} active, fee_ppk={}",
        keysets.keysets[0].id, keysets.keysets[0].input_fee_ppk);

    // ---- Step 4: Mint 100 sats (NUT-04) ----
    println!("\n4. Minting 100 sats...");
    let mint_quote = wallet.request_mint_quote(100, "sat").unwrap();
    println!("   Quote ID: {}", mint_quote.quote);
    println!("   Invoice: {}", mint_quote.request);
    println!("   State: {} (auto-paid)", mint_quote.state);

    let proofs = wallet
        .mint_tokens(&mint_quote.quote, 100, &keyset_id, keyset, &mut rng)
        .unwrap();
    println!("   Minted {} proofs:", proofs.len());
    for p in &proofs {
        println!("     {} sat (keyset {})", p.amount, p.id);
    }
    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    println!("   Total: {} sats", total);

    // ---- Step 5: Swap into smaller denominations (NUT-03) ----
    println!("\n5. Swapping {} sats into [32, 32, 16, 8, 4, 4, 2, 1, 1]...", total);
    let new_proofs = wallet
        .swap(proofs, &[32, 32, 16, 8, 4, 4, 2, 1, 1], &keyset_id, keyset, &mut rng)
        .unwrap();
    println!("   Got {} new proofs:", new_proofs.len());
    for p in &new_proofs {
        println!("     {} sat", p.amount);
    }

    // ---- Step 6: Melt 50 sats (NUT-05) ----
    // Collect proofs summing to 50 sats: 32 + 16 + 2 = 50
    let mut melt_proofs = Vec::new();
    let mut melt_sum = 0u64;
    let mut remaining = Vec::new();
    for p in new_proofs {
        if melt_sum < 50 {
            melt_sum += p.amount;
            melt_proofs.push(p);
        } else {
            remaining.push(p);
        }
    }

    println!("\n6. Melting {} sats to pay invoice...", melt_sum);
    let melt_quote = wallet
        .request_melt_quote(&format!("lnbcdemo{}sat1micronuts", melt_sum), "sat")
        .unwrap();
    println!("   Melt quote: {} (amount={}, fee={})", melt_quote.quote, melt_quote.amount, melt_quote.fee_reserve);

    let melt_result = wallet.melt(&melt_quote.quote, melt_proofs).unwrap();
    println!("   Paid: {}", melt_result.paid);
    println!("   State: {}", melt_result.state);
    if let Some(preimage) = &melt_result.payment_preimage {
        println!("   Preimage: {}...", &preimage[..16]);
    }

    // ---- Step 7: Check remaining balance ----
    let remaining_total: u64 = remaining.iter().map(|p| p.amount).sum();
    println!("\n7. Remaining balance: {} sats ({} proofs)", remaining_total, remaining.len());
    for p in &remaining {
        println!("     {} sat", p.amount);
    }

    // ---- Step 8: Verify proofs via swap (NUT-03) ----
    if remaining_total > 0 {
        let amounts: Vec<u64> = nut00::decompose_amount(remaining_total);
        let verified = wallet
            .swap(remaining, &amounts, &keyset_id, keyset, &mut rng)
            .unwrap();
        println!("\n8. Verified remaining proofs via swap: {} proofs, {} sats",
            verified.len(),
            verified.iter().map(|p| p.amount).sum::<u64>());
    }

    println!("\n=== Demo complete ===");
}
