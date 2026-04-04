//! Host-side demo role helpers.
//!
//! This module keeps the wallet-role and mint-role entrypoints explicit while
//! reusing the same RPC boundary that later transports will carry.

use std::io::{self, BufRead, Write};

use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::nut00;
use cashu_core_lite::rpc::{MintRpcHandler, RpcMintClient};
use cashu_core_lite::Wallet;
use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::{DemoMint, LoopbackTransport};

/// Construct the host-side mint-role RPC handler.
pub fn demo_mint_handler() -> MintRpcHandler<DemoMint> {
    MintRpcHandler::new(DemoMint::new())
}

/// NUT-01 through NUT-07: handle one encoded mint RPC request frame.
pub fn handle_demo_mint_request_bytes(
    handler: &mut MintRpcHandler<DemoMint>,
    request_bytes: &[u8],
) -> Result<Vec<u8>, CashuError> {
    handler.handle_bytes(request_bytes)
}

/// NUT-01 through NUT-07: handle one hex-encoded RPC frame for the stdio demo
/// mint role.
pub fn handle_demo_mint_hex_request_line(
    handler: &mut MintRpcHandler<DemoMint>,
    line: &str,
) -> Result<String, CashuError> {
    let request_bytes = hex::decode(line.trim())
        .map_err(|err| CashuError::Protocol(format!("failed to decode hex rpc request: {err}")))?;
    let response_bytes = handle_demo_mint_request_bytes(handler, &request_bytes)?;
    Ok(hex::encode(response_bytes))
}

/// Host-side mint-role demo server.
///
/// Each stdin line is one hex-encoded `MintRpcRequest` frame and each stdout
/// line is one hex-encoded `MintRpcResponse` frame. This keeps the mint role
/// explicit without introducing networking before serial or microfips framing.
pub fn run_mint_server_stdio() -> Result<(), Box<dyn std::error::Error>> {
    let mut handler = demo_mint_handler();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    eprintln!("Micronuts demo mint server ready; send hex CBOR RPC frames on stdin.");

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response_line = handle_demo_mint_hex_request_line(&mut handler, &line)?;
        writeln!(out, "{response_line}")?;
        out.flush()?;
    }

    Ok(())
}

/// Host-side wallet-role demo that talks to the demo mint over the serialized
/// RPC loopback transport.
pub fn run_wallet_demo() -> Result<(), CashuError> {
    println!("=== Micronuts Wallet ↔ Mint Demo ===\n");

    // Wallet role: speak `MintClient` over encoded RPC bytes.
    let mint = DemoMint::new();
    let transport = RpcMintClient::new(LoopbackTransport::from_demo_mint(mint));
    let mut wallet = Wallet::new("https://demo.micronuts.local", transport);
    let mut rng = StdRng::seed_from_u64(1337);

    // ---- Step 1: Get mint info (NUT-06) ----
    let info = wallet.get_info()?;
    println!("1. Mint info:");
    println!("   Name: {}", info.name);
    println!("   Version: {}", info.version);
    println!("   Supported NUTs: {:?}", info.nuts.supported);

    // ---- Step 2: Get keys (NUT-01) ----
    let keys = wallet.get_keys()?;
    let keyset = &keys.keysets[0];
    let keyset_id = keyset.id.clone();
    println!("\n2. Active keyset: {} (unit: {})", keyset_id, keyset.unit);
    println!(
        "   Denominations: {:?}",
        keyset.keys.iter().map(|k| k.amount).collect::<Vec<_>>()
    );

    // ---- Step 3: Get keysets (NUT-02) ----
    let keysets = wallet.get_keysets()?;
    println!(
        "\n3. Keysets: {} active, fee_ppk={}",
        keysets.keysets[0].id, keysets.keysets[0].input_fee_ppk
    );

    // ---- Step 4: Mint 100 sats (NUT-04) ----
    println!("\n4. Minting 100 sats...");
    let mint_quote = wallet.request_mint_quote(100, "sat")?;
    println!("   Quote ID: {}", mint_quote.quote);
    println!("   Invoice: {}", mint_quote.request);
    println!("   State: {} (auto-paid)", mint_quote.state);

    let proofs = wallet.mint_tokens(&mint_quote.quote, 100, &keyset_id, keyset, &mut rng)?;
    println!("   Minted {} proofs:", proofs.len());
    for p in &proofs {
        println!("     {} sat (keyset {})", p.amount, p.id);
    }
    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    println!("   Total: {} sats", total);

    // ---- Step 5: Swap into smaller denominations (NUT-03) ----
    println!(
        "\n5. Swapping {} sats into [32, 32, 16, 8, 4, 4, 2, 1, 1]...",
        total
    );
    let new_proofs = wallet.swap(
        proofs,
        &[32, 32, 16, 8, 4, 4, 2, 1, 1],
        &keyset_id,
        keyset,
        &mut rng,
    )?;
    println!("   Got {} new proofs:", new_proofs.len());
    for p in &new_proofs {
        println!("     {} sat", p.amount);
    }

    // ---- Step 6: Melt selected proofs (NUT-05) ----
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
    let melt_quote = wallet.request_melt_quote(&format!("lnbcdemo{}sat1micronuts", melt_sum), "sat")?;
    println!(
        "   Melt quote: {} (amount={}, fee={})",
        melt_quote.quote, melt_quote.amount, melt_quote.fee_reserve
    );

    let melt_result = wallet.melt(&melt_quote.quote, melt_proofs)?;
    println!("   Paid: {}", melt_result.paid);
    println!("   State: {}", melt_result.state);
    if let Some(preimage) = &melt_result.payment_preimage {
        println!("   Preimage: {}...", &preimage[..16]);
    }

    // ---- Step 7: Check remaining balance ----
    let remaining_total: u64 = remaining.iter().map(|p| p.amount).sum();
    println!(
        "\n7. Remaining balance: {} sats ({} proofs)",
        remaining_total,
        remaining.len()
    );
    for p in &remaining {
        println!("     {} sat", p.amount);
    }

    // ---- Step 8: Verify remaining proofs via swap (NUT-03) ----
    if remaining_total > 0 {
        let amounts: Vec<u64> = nut00::decompose_amount(remaining_total);
        let verified = wallet.swap(remaining, &amounts, &keyset_id, keyset, &mut rng)?;
        println!(
            "\n8. Verified remaining proofs via swap: {} proofs, {} sats",
            verified.len(),
            verified.iter().map(|p| p.amount).sum::<u64>()
        );
    }

    println!("\n=== Demo complete ===");
    Ok(())
}
