//! End-to-end tests proving the full Cashu mint/wallet demo flow.
//!
//! These tests verify:
//!   - Wallet requests mint quote and receives paid quote
//!   - Wallet mints ecash
//!   - Wallet swaps ecash
//!   - Wallet melts ecash
//!   - Mint role responds with expected request/response structures
//!   - Build works in host/native mode without STM32 hardware

use cashu_core_lite::nuts::{nut04, nut05, nut07};
use cashu_core_lite::rpc::RpcMintClient;
use cashu_core_lite::Wallet;
use micronuts_mint::{DemoMint, LoopbackTransport};
use rand::rngs::StdRng;
use rand::SeedableRng;

/// Helper: create a deterministic wallet + mint pair for testing.
fn test_wallet() -> (
    Wallet<RpcMintClient<LoopbackTransport<DemoMint>>>,
    cashu_core_lite::nuts::nut01::KeySet,
) {
    let mint = DemoMint::new();
    let keyset = mint.public_keyset();
    let transport = RpcMintClient::new(LoopbackTransport::from_demo_mint(mint));
    let wallet = Wallet::new("https://demo.micronuts.local", transport);
    (wallet, keyset)
}

/// Helper: create a deterministic RNG for reproducible tests.
fn test_rng() -> StdRng {
    StdRng::seed_from_u64(42)
}

// ---- NUT-06: Mint Info ----

#[test]
fn test_get_mint_info() {
    let (mut wallet, _) = test_wallet();
    let info = wallet.get_info().unwrap();
    assert_eq!(info.name, "Micronuts Demo Mint");
    assert!(!info.pubkey.is_empty());
    assert!(info.nuts.supported.contains(&0));
    assert!(info.nuts.supported.contains(&4));
}

// ---- NUT-01: Mint Keys ----

#[test]
fn test_get_keys() {
    let (mut wallet, _) = test_wallet();
    let keys = wallet.get_keys().unwrap();
    assert_eq!(keys.keysets.len(), 1);
    let ks = &keys.keysets[0];
    assert_eq!(ks.unit, "sat");
    // Should have keys for denominations 1, 2, 4, 8, 16, 32, 64, 128
    assert_eq!(ks.keys.len(), 8);
    assert_eq!(ks.keys[0].amount, 1);
    assert_eq!(ks.keys[7].amount, 128);
}

// ---- NUT-02: Keysets ----

#[test]
fn test_get_keysets() {
    let (mut wallet, _) = test_wallet();
    let keysets = wallet.get_keysets().unwrap();
    assert_eq!(keysets.keysets.len(), 1);
    let ki = &keysets.keysets[0];
    assert!(ki.active);
    assert_eq!(ki.unit, "sat");
    assert_eq!(ki.input_fee_ppk, 0);
    assert!(ki.id.starts_with("00"), "keyset ID should start with version 00");
    assert_eq!(ki.id.len(), 16, "keyset ID should be 16 hex chars");
}

// ---- NUT-04: Mint Quote + Mint ----

#[test]
fn test_mint_quote_and_mint() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    // Step 1: Request mint quote for 13 sats
    let quote = wallet.request_mint_quote(13, "sat").unwrap();
    assert!(quote.paid, "demo quote should be auto-paid");
    assert_eq!(quote.state, nut04::state::PAID);
    assert!(!quote.quote.is_empty());

    // Step 2: Check quote state (should still be PAID)
    let checked = wallet.check_mint_quote(&quote.quote).unwrap();
    assert_eq!(checked.state, nut04::state::PAID);

    // Step 3: Mint ecash — 13 sats decomposes to [1, 4, 8]
    let proofs = wallet
        .mint_tokens(&quote.quote, 13, &keyset_id, &keyset, &mut rng)
        .unwrap();

    assert_eq!(proofs.len(), 3, "13 sats = 3 denominations (1+4+8)");
    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    assert_eq!(total, 13);

    // Verify denomination split
    let mut amounts: Vec<u64> = proofs.iter().map(|p| p.amount).collect();
    amounts.sort();
    assert_eq!(amounts, vec![1, 4, 8]);

    // Check quote is now ISSUED
    let issued = wallet.check_mint_quote(&quote.quote).unwrap();
    assert_eq!(issued.state, nut04::state::ISSUED);
}

#[test]
fn test_mint_quote_zero_amount_rejected() {
    let (mut wallet, _) = test_wallet();
    let result = wallet.request_mint_quote(0, "sat");
    assert!(result.is_err());
}

#[test]
fn test_mint_double_issue_rejected() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    let quote = wallet.request_mint_quote(1, "sat").unwrap();
    let _proofs = wallet
        .mint_tokens(&quote.quote, 1, &keyset_id, &keyset, &mut rng)
        .unwrap();

    // Second mint with same quote should fail (already ISSUED)
    let result = wallet.mint_tokens(&quote.quote, 1, &keyset_id, &keyset, &mut rng);
    assert!(result.is_err());
}

// ---- NUT-03: Swap ----

#[test]
fn test_swap_proofs() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    // Mint 8 sats (single denomination)
    let quote = wallet.request_mint_quote(8, "sat").unwrap();
    let proofs = wallet
        .mint_tokens(&quote.quote, 8, &keyset_id, &keyset, &mut rng)
        .unwrap();
    assert_eq!(proofs.len(), 1);
    assert_eq!(proofs[0].amount, 8);

    // Swap 8 sats into [1, 2, 4, 1] = 8
    // NUT-03: inputs must equal outputs
    let new_proofs = wallet
        .swap(proofs, &[1, 2, 4, 1], &keyset_id, &keyset, &mut rng)
        .unwrap();

    assert_eq!(new_proofs.len(), 4);
    let total: u64 = new_proofs.iter().map(|p| p.amount).sum();
    assert_eq!(total, 8, "swap should preserve total amount");
}

#[test]
fn test_swap_amount_mismatch_rejected() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    let quote = wallet.request_mint_quote(8, "sat").unwrap();
    let proofs = wallet
        .mint_tokens(&quote.quote, 8, &keyset_id, &keyset, &mut rng)
        .unwrap();

    // Try to swap 8 sats into 10 sats of outputs — should fail
    let result = wallet.swap(proofs, &[2, 8], &keyset_id, &keyset, &mut rng);
    assert!(result.is_err(), "swap with mismatched amounts should fail");
}

// ---- NUT-05: Melt Quote + Melt ----

#[test]
fn test_melt_flow() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    // Mint 64 sats
    let mint_quote = wallet.request_mint_quote(64, "sat").unwrap();
    let proofs = wallet
        .mint_tokens(&mint_quote.quote, 64, &keyset_id, &keyset, &mut rng)
        .unwrap();
    assert_eq!(proofs.len(), 1);

    // Request a melt quote for a dummy 64 sat invoice
    let melt_quote = wallet
        .request_melt_quote("lnbcdemo64sat1micronuts", "sat")
        .unwrap();
    assert_eq!(melt_quote.amount, 64);
    assert_eq!(melt_quote.fee_reserve, 0);
    assert_eq!(melt_quote.state, nut05::state::UNPAID);

    // Check the melt quote
    let checked = wallet.check_melt_quote(&melt_quote.quote).unwrap();
    assert_eq!(checked.state, nut05::state::UNPAID);

    // Execute melt
    let melt_result = wallet.melt(&melt_quote.quote, proofs).unwrap();
    assert!(melt_result.paid);
    assert_eq!(melt_result.state, nut05::state::PAID);
    assert!(melt_result.payment_preimage.is_some());
}

#[test]
fn test_melt_insufficient_proofs_rejected() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    // Mint only 1 sat
    let mint_quote = wallet.request_mint_quote(1, "sat").unwrap();
    let proofs = wallet
        .mint_tokens(&mint_quote.quote, 1, &keyset_id, &keyset, &mut rng)
        .unwrap();

    // Try to melt 64 sats with only 1 sat of proofs
    let melt_quote = wallet
        .request_melt_quote("lnbcdemo64sat1micronuts", "sat")
        .unwrap();
    let result = wallet.melt(&melt_quote.quote, proofs);
    assert!(result.is_err(), "melt with insufficient proofs should fail");
}

// ---- NUT-07: Check State ----

#[test]
fn test_check_state_unspent_and_spent() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    // Mint 2 sats
    let quote = wallet.request_mint_quote(2, "sat").unwrap();
    let proofs = wallet
        .mint_tokens(&quote.quote, 2, &keyset_id, &keyset, &mut rng)
        .unwrap();
    assert_eq!(proofs.len(), 1); // 2 = single denomination

    // Check state — should be UNSPENT
    let secrets: Vec<Vec<u8>> = proofs
        .iter()
        .map(|p| hex::decode(&p.secret).unwrap())
        .collect();
    let secret_refs: Vec<&[u8]> = secrets.iter().map(|s| s.as_slice()).collect();
    let state = wallet.check_state(&secret_refs).unwrap();
    assert_eq!(state.states.len(), 1);
    assert_eq!(state.states[0].state, nut07::state::UNSPENT);

    // Swap the proofs (marks them as spent)
    let _new_proofs = wallet
        .swap(proofs, &[1, 1], &keyset_id, &keyset, &mut rng)
        .unwrap();

    // Check state again — should now be SPENT
    let state2 = wallet.check_state(&secret_refs).unwrap();
    assert_eq!(state2.states[0].state, nut07::state::SPENT);
}

// ---- Full End-to-End Demo Flow ----

#[test]
fn test_full_e2e_flow() {
    let (mut wallet, keyset) = test_wallet();
    let mut rng = test_rng();
    let keyset_id = keyset.id.clone();

    // 1. Get mint info
    let info = wallet.get_info().unwrap();
    assert_eq!(info.name, "Micronuts Demo Mint");

    // 2. Get keys
    let keys = wallet.get_keys().unwrap();
    assert!(!keys.keysets.is_empty());

    // 3. Get keysets
    let keysets = wallet.get_keysets().unwrap();
    assert!(keysets.keysets[0].active);

    // 4. Mint 100 sats
    let mint_quote = wallet.request_mint_quote(100, "sat").unwrap();
    assert!(mint_quote.paid);
    let proofs = wallet
        .mint_tokens(&mint_quote.quote, 100, &keyset_id, &keyset, &mut rng)
        .unwrap();
    let minted_total: u64 = proofs.iter().map(|p| p.amount).sum();
    assert_eq!(minted_total, 100);

    // 5. Swap: split 100 into specific denominations
    let new_proofs = wallet
        .swap(proofs, &[32, 32, 16, 8, 4, 4, 2, 1, 1], &keyset_id, &keyset, &mut rng)
        .unwrap();
    let swapped_total: u64 = new_proofs.iter().map(|p| p.amount).sum();
    assert_eq!(swapped_total, 100);

    // 6. Melt 50 sats (use some of the swapped proofs)
    // Take proofs summing to >= 50
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
    assert!(melt_sum >= 50);

    let melt_quote = wallet
        .request_melt_quote(&format!("lnbcdemo{}sat1micronuts", melt_sum), "sat")
        .unwrap();
    let melt_result = wallet.melt(&melt_quote.quote, melt_proofs).unwrap();
    assert!(melt_result.paid);

    // 7. Remaining proofs should still work for another swap
    let remaining_total: u64 = remaining.iter().map(|p| p.amount).sum();
    if remaining_total > 0 {
        let final_amounts: Vec<u64> =
            cashu_core_lite::nuts::nut00::decompose_amount(remaining_total);
        let final_proofs = wallet
            .swap(remaining, &final_amounts, &keyset_id, &keyset, &mut rng)
            .unwrap();
        let final_total: u64 = final_proofs.iter().map(|p| p.amount).sum();
        assert_eq!(final_total, remaining_total);
    }
}
