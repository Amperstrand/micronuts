//! Differential tests: the mint's NUT-20 quote-locking and NUT-29 batch
//! minting vs the upstream `cashu` crate 0.18 (house oracle pattern).
//!
//! Signed messages are BUILT with the upstream crate's
//! `MintRequest::msg_to_sign` / `BatchMintRequest::msg_to_sign` and signed
//! with its `SecretKey::sign` (SHA-256 + BIP-340 internally); the mint must
//! accept what upstream produces and reject everything else. NUT-29 limit
//! and ordering behavior is pinned against the conformance-runner numbers
//! (max 50 quotes, max 1000 outputs).

use std::str::FromStr;

use cashu::nuts::nut01::SecretKey as CashuSecretKey;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut04, nut20, nut29};
use micronuts_mint::type_conversion::lite_pk_to_cashu;
use micronuts_mint::DemoMint;

fn seed_key(seed: u8) -> CashuSecretKey {
    CashuSecretKey::from_slice(&[seed; 32]).expect("valid scalar")
}

fn seed_point(seed: u8) -> nut00::BlindedMessage {
    let sk = seed_key(seed);
    let bytes = sk.public_key().to_bytes();
    nut00::BlindedMessage {
        amount: 0,
        id: "00".to_string(),
        b: cashu_core_lite::keypair::PublicKey::from_bytes(&bytes).unwrap(),
    }
}

fn output(amount: u64, seed: u8, keyset_id: &str) -> nut00::BlindedMessage {
    let mut msg = seed_point(seed);
    msg.amount = amount;
    msg.id = keyset_id.to_string();
    msg
}

fn to_upstream_outputs(
    outputs: &[nut00::BlindedMessage],
) -> Vec<cashu::nuts::nut00::BlindedMessage> {
    outputs
        .iter()
        .map(|o| cashu::nuts::nut00::BlindedMessage {
            amount: cashu::Amount::from(o.amount),
            keyset_id: cashu::Id::from_str(&o.id).expect("valid keyset id"),
            blinded_secret: lite_pk_to_cashu(&o.b),
            witness: None,
        })
        .collect()
}

/// Create a quote and settle it (FakeWallet pays on first lookup).
fn paid_quote(mint: &mut DemoMint, amount: u64, pubkey: Option<String>) -> String {
    let resp = mint
        .post_mint_quote(nut04::MintQuoteRequest {
            amount,
            unit: "sat".to_string(),
            pubkey,
        })
        .expect("quote created");
    let checked = mint.get_mint_quote(&resp.quote).expect("quote lookup");
    assert_eq!(checked.state, "PAID", "FakeWallet settles on first lookup");
    resp.quote
}

// ---- NUT-20 message shape (wire oracle) ----

#[test]
fn nut20_message_matches_upstream_mint_request_msg_to_sign() {
    let mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let outputs = vec![
        output(8, 1, &keyset_id),
        output(1, 2, &keyset_id),
        output(256, 3, &keyset_id),
        output(0, 4, &keyset_id),
    ];
    let quote_id = "0192d3c0-7e8a-7c3d-8e9f-1a2b3c4d5e6f".to_string();

    let upstream = cashu::nuts::nut04::MintRequest::<String> {
        quote: quote_id.clone(),
        outputs: to_upstream_outputs(&outputs),
        signature: None,
    };

    assert_eq!(
        nut20::quote_sig_message(&quote_id, &outputs),
        upstream.msg_to_sign()
    );
}

#[test]
fn nut29_batch_message_matches_upstream_batch_msg_to_sign() {
    let mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let outputs = vec![output(8, 5, &keyset_id), output(2, 6, &keyset_id)];
    let quote_id = "019e6d5a-2347-7000-8c81-a1e0dbf3299f".to_string();

    let upstream = cashu::nuts::nut29::BatchMintRequest::<String> {
        quotes: vec![quote_id.clone()],
        quote_amounts: None,
        outputs: to_upstream_outputs(&outputs),
        signatures: None,
    };

    assert_eq!(
        nut20::quote_sig_message(&quote_id, &outputs),
        upstream.msg_to_sign(&quote_id)
    );
}

// ---- NUT-20 enforcement ----

fn locked_mint(mint: &mut DemoMint, amount: u64, lock_seed: u8) -> (String, CashuSecretKey) {
    let lock = seed_key(lock_seed);
    let pubkey_hex = hex::encode(lock.public_key().to_bytes());
    let quote = paid_quote(mint, amount, Some(pubkey_hex));
    (quote, lock)
}

fn sign_quote(lock: &CashuSecretKey, quote_id: &str, outputs: &[nut00::BlindedMessage]) -> String {
    let msg = nut20::quote_sig_message(quote_id, outputs);
    lock.sign(&msg).expect("sign").to_string()
}

#[test]
fn nut20_locked_quote_accepts_upstream_signature() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let (quote, lock) = locked_mint(&mut mint, 8, 0x11);
    let outputs = vec![output(8, 1, &keyset_id)];

    let response = mint
        .post_mint(nut04::MintRequest {
            quote: quote.clone(),
            outputs: outputs.clone(),
            signature: Some(sign_quote(&lock, &quote, &outputs)),
        })
        .expect("valid signature mints");
    assert_eq!(response.signatures.len(), 1);
}

#[test]
fn nut20_locked_quote_rejects_missing_signature() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let (quote, _lock) = locked_mint(&mut mint, 8, 0x21);

    let err = mint
        .post_mint(nut04::MintRequest {
            quote,
            outputs: vec![output(8, 1, &keyset_id)],
            signature: None,
        })
        .expect_err("locked quote without signature");
    assert_eq!(err, CashuError::QuoteSignatureInvalid);
}

#[test]
fn nut20_locked_quote_rejects_wrong_key_signature() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let (quote, _lock) = locked_mint(&mut mint, 8, 0x31);
    let wrong_key = seed_key(0x99);
    let outputs = vec![output(8, 1, &keyset_id)];

    let err = mint
        .post_mint(nut04::MintRequest {
            quote: quote.clone(),
            outputs: outputs.clone(),
            signature: Some(sign_quote(&wrong_key, &quote, &outputs)),
        })
        .expect_err("wrong key must not mint");
    assert_eq!(err, CashuError::QuoteSignatureInvalid);
    // The failed attempt must not consume the quote.
    let state = mint.get_mint_quote(&quote).unwrap();
    assert_eq!(state.state, "PAID");
}

#[test]
fn nut20_locked_quote_rejects_signature_over_other_outputs() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let (quote, lock) = locked_mint(&mut mint, 8, 0x41);
    let signed_outputs = vec![output(8, 1, &keyset_id)];
    let sent_outputs = vec![output(4, 2, &keyset_id), output(4, 3, &keyset_id)];

    let err = mint
        .post_mint(nut04::MintRequest {
            quote: quote.clone(),
            outputs: sent_outputs,
            signature: Some(sign_quote(&lock, &quote, &signed_outputs)),
        })
        .expect_err("signature must cover the sent outputs");
    assert_eq!(err, CashuError::QuoteSignatureInvalid);
}

#[test]
fn nut20_unlocked_quote_unaffected_and_ignores_stray_signature() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let quote = paid_quote(&mut mint, 8, None);
    let outputs = vec![output(8, 1, &keyset_id)];

    mint.post_mint(nut04::MintRequest {
        quote: quote.clone(),
        outputs: outputs.clone(),
        signature: None,
    })
    .expect("unlocked quote mints without signature");

    let quote2 = paid_quote(&mut mint, 8, None);
    mint.post_mint(nut04::MintRequest {
        quote: quote2,
        outputs,
        signature: Some(sign_quote(&seed_key(0x77), "whatever", &[])),
    })
    .expect("unlocked quote ignores a stray signature");
}

#[test]
fn nut20_quote_response_echoes_pubkey() {
    let mut mint = DemoMint::new();
    let lock = seed_key(0x51);
    let pubkey_hex = hex::encode(lock.public_key().to_bytes());

    let created = mint
        .post_mint_quote(nut04::MintQuoteRequest {
            amount: 8,
            unit: "sat".to_string(),
            pubkey: Some(pubkey_hex.clone()),
        })
        .unwrap();
    assert_eq!(created.pubkey.as_deref(), Some(pubkey_hex.as_str()));

    let looked_up = mint.get_mint_quote(&created.quote).unwrap();
    assert_eq!(looked_up.pubkey.as_deref(), Some(pubkey_hex.as_str()));

    let unlocked = mint
        .post_mint_quote(nut04::MintQuoteRequest {
            amount: 8,
            unit: "sat".to_string(),
            pubkey: None,
        })
        .unwrap();
    assert_eq!(unlocked.pubkey, None);
}

// ---- NUT-29 batch check ----

#[test]
fn nut29_batch_check_returns_quotes_in_request_order() {
    let mut mint = DemoMint::new();
    let q1 = paid_quote(&mut mint, 4, None);
    let q2 = paid_quote(&mut mint, 6, None);
    let q3 = paid_quote(&mut mint, 2, None);

    let responses = mint
        .batch_check_mint_quotes(nut29::BatchCheckMintQuoteRequest {
            quotes: vec![q3.clone(), q1.clone(), q2.clone()],
        })
        .expect("batch check");

    let ids: Vec<&str> = responses.iter().map(|r| r.quote.as_str()).collect();
    assert_eq!(ids, vec![q3.as_str(), q1.as_str(), q2.as_str()]);
    assert!(responses.iter().all(|r| r.state == "PAID"));
}

#[test]
fn nut29_batch_check_rejects_unknown_quote_atomically() {
    let mut mint = DemoMint::new();
    let known = paid_quote(&mut mint, 4, None);

    let err = mint
        .batch_check_mint_quotes(nut29::BatchCheckMintQuoteRequest {
            quotes: vec![known, "00000000-0000-7000-8000-00000000dead".to_string()],
        })
        .expect_err("unknown quote rejects the whole batch");
    assert_eq!(err, CashuError::QuoteNotFound);
}

#[test]
fn nut29_batch_check_rejects_too_many() {
    let mut mint = DemoMint::new();
    let ids: Vec<String> = (0..51)
        .map(|i| format!("00000000-0000-7000-8000-{i:012x}"))
        .collect();

    let err = mint
        .batch_check_mint_quotes(nut29::BatchCheckMintQuoteRequest { quotes: ids })
        .expect_err("51 quotes exceed the limit of 50");
    assert_eq!(err, CashuError::BatchTooLarge);
}

// ---- NUT-29 batch mint ----

#[test]
fn nut29_batch_mint_mints_locked_and_unlocked_quotes_atomically() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();

    let lock = seed_key(0x61);
    let pubkey_hex = hex::encode(lock.public_key().to_bytes());
    let locked = paid_quote(&mut mint, 8, Some(pubkey_hex));
    let unlocked = paid_quote(&mut mint, 6, None);

    // 14 sats total: 8 + 4 + 2.
    let outputs = vec![
        output(8, 1, &keyset_id),
        output(4, 2, &keyset_id),
        output(2, 3, &keyset_id),
    ];

    let response = mint
        .batch_mint(nut29::BatchMintRequest {
            quotes: vec![locked.clone(), unlocked.clone()],
            quote_amounts: None,
            outputs: outputs.clone(),
            signatures: Some(vec![Some(sign_quote(&lock, &locked, &outputs)), None]),
        })
        .expect("batch mint succeeds");

    assert_eq!(response.signatures.len(), outputs.len());
    assert_eq!(mint.get_mint_quote(&locked).unwrap().state, "ISSUED");
    assert_eq!(mint.get_mint_quote(&unlocked).unwrap().state, "ISSUED");
}

#[test]
fn nut29_batch_mint_rejects_wrong_lock_signature_atomically() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();

    let lock = seed_key(0x71);
    let pubkey_hex = hex::encode(lock.public_key().to_bytes());
    let locked = paid_quote(&mut mint, 8, Some(pubkey_hex));
    let unlocked = paid_quote(&mut mint, 6, None);

    let outputs = vec![output(8, 1, &keyset_id), output(6, 2, &keyset_id)];
    let wrong = seed_key(0x99);

    let err = mint
        .batch_mint(nut29::BatchMintRequest {
            quotes: vec![locked.clone(), unlocked.clone()],
            quote_amounts: None,
            outputs: outputs.clone(),
            signatures: Some(vec![Some(sign_quote(&wrong, &locked, &outputs)), None]),
        })
        .expect_err("invalid batch signature rejects everything");

    assert_eq!(err, CashuError::QuoteSignatureInvalid);
    // Neither quote was consumed.
    assert_eq!(mint.get_mint_quote(&locked).unwrap().state, "PAID");
    assert_eq!(mint.get_mint_quote(&unlocked).unwrap().state, "PAID");
}

#[test]
fn nut29_batch_mint_rejects_missing_signatures_array_when_locked() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();

    let lock = seed_key(0x81);
    let pubkey_hex = hex::encode(lock.public_key().to_bytes());
    let locked = paid_quote(&mut mint, 8, Some(pubkey_hex));

    let err = mint
        .batch_mint(nut29::BatchMintRequest {
            quotes: vec![locked],
            quote_amounts: None,
            outputs: vec![output(8, 1, &keyset_id)],
            signatures: None,
        })
        .expect_err("locked batch needs the signatures array");
    assert_eq!(err, CashuError::QuoteSignatureInvalid);
}

#[test]
fn nut29_batch_mint_unlocked_rejects_signature_entry() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let unlocked = paid_quote(&mut mint, 8, None);

    let err = mint
        .batch_mint(nut29::BatchMintRequest {
            quotes: vec![unlocked],
            quote_amounts: None,
            outputs: vec![output(8, 1, &keyset_id)],
            signatures: Some(vec![Some("00".repeat(64))]),
        })
        .expect_err("unlocked quote must not carry a signature");
    assert_eq!(err, CashuError::QuoteSignatureInvalid);
}

#[test]
fn nut29_batch_mint_rejects_duplicates() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let quote = paid_quote(&mut mint, 8, None);

    let err = mint
        .batch_mint(nut29::BatchMintRequest {
            quotes: vec![quote.clone(), quote],
            quote_amounts: None,
            outputs: vec![output(8, 1, &keyset_id), output(8, 2, &keyset_id)],
            signatures: None,
        })
        .expect_err("duplicate quote ids rejected");
    assert_eq!(err, CashuError::BatchQuoteNotUnique);
}

#[test]
fn nut29_batch_mint_rejects_amount_mismatch() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let q1 = paid_quote(&mut mint, 8, None);
    let q2 = paid_quote(&mut mint, 6, None);

    // Outputs sum to 12 but quotes sum to 14.
    let err = mint
        .batch_mint(nut29::BatchMintRequest {
            quotes: vec![q1, q2],
            quote_amounts: None,
            outputs: vec![output(8, 1, &keyset_id), output(4, 2, &keyset_id)],
            signatures: None,
        })
        .expect_err("outputs must balance the quote sum");
    assert_eq!(err, CashuError::AmountMismatch);
}

#[test]
fn nut29_batch_mint_rejects_too_many_outputs() {
    let mut mint = DemoMint::new();
    let keyset_id = mint.keyset_id().to_string();
    let fake_outputs: Vec<nut00::BlindedMessage> = (0..1001)
        .map(|i| output(1, (i % 200 + 1) as u8, &keyset_id))
        .collect();

    let err = mint
        .batch_mint(nut29::BatchMintRequest {
            quotes: vec!["fake-quote-id".to_string()],
            quote_amounts: None,
            outputs: fake_outputs,
            signatures: None,
        })
        .expect_err("1001 outputs exceed the limit of 1000");
    assert_eq!(err, CashuError::TooManyOutputs);
}
