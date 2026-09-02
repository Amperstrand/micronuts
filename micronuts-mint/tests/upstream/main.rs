//! Upstream backend tests against a local std-only HTTP mock of an
//! upstream Cashu mint (canned NUT-04/05 wire shapes + REAL blind-signature
//! crypto, so the reserve bootstrap/change paths are exercised truthfully).
//!
//! The live testnut test is `#[ignore]`d (needs network) — see
//! `testnut_live_roundtrip`.

#![cfg(feature = "backend-upstream")]

use cashu_core_lite::error::CashuError;
use micronuts_mint::ln::LightningBackend;
use micronuts_mint::reserve::ReserveWallet;
use micronuts_mint::upstream::UpstreamCashuBackend;

mod mock;

use mock::{MeltMode, MockUpstream, MELT_PREIMAGE};

#[test]
fn create_invoice_settles_after_two_polls() {
    let mock = MockUpstream::start();
    let mut backend = UpstreamCashuBackend::new(&mock.base_url, "sat", 1000);

    let invoice = backend.create_invoice(100, "test").unwrap();
    assert_eq!(invoice, "lnbcmock100sat1mockup");

    assert!(!backend.is_settled(&invoice).unwrap());
    assert!(backend.is_settled(&invoice).unwrap());

    let err = backend.is_settled("lnbcmock999sat1mockup").unwrap_err();
    assert!(matches!(err, CashuError::Protocol(_)));
}

#[test]
fn is_settled_survives_transient_upstream_error() {
    let mock = MockUpstream::start();
    mock.fail_next_mint_quote_poll();
    let mut backend = UpstreamCashuBackend::new(&mock.base_url, "sat", 1000);

    let invoice = backend.create_invoice(5, "test").unwrap();
    assert!(!backend.is_settled(&invoice).unwrap()); // 500 → Ok(false)
    assert!(!backend.is_settled(&invoice).unwrap()); // poll 1
    assert!(backend.is_settled(&invoice).unwrap()); // poll 2
}

#[test]
fn lookup_amount_parses_string_and_number_amounts() {
    let mock = MockUpstream::start();
    let mut backend = UpstreamCashuBackend::new(&mock.base_url, "sat", 1000);

    let invoice = backend.create_invoice(64, "test").unwrap();
    // The mock returns the melt-quote amount as a STRING; mint quotes use
    // numbers — both must parse.
    assert_eq!(backend.lookup_amount(&invoice).unwrap(), 64);
    assert_eq!(backend.lookup_amount(&invoice).unwrap(), 64);
    assert_eq!(mock.melt_quotes_created(), 1, "second lookup is cached");
}

#[test]
fn pay_invoice_melts_reserve_and_returns_preimage() {
    let mock = MockUpstream::start();
    let mut backend = UpstreamCashuBackend::new(&mock.base_url, "sat", 1000);

    let invoice = backend.create_invoice(30, "test").unwrap();
    assert_eq!(backend.lookup_amount(&invoice).unwrap(), 30);

    let preimage = backend.pay_invoice(&invoice, 30).unwrap();
    assert_eq!(preimage, MELT_PREIMAGE);
    // Bootstrapped deficit(30) + margin(1000) = 1030; paid 30 and got the
    // 482-sat selection overpay back as change → 1000.
    assert_eq!(backend.reserve_balance_sats(), 1000);
}

#[test]
fn pay_invoice_maps_definitive_failure_without_touching_reserve() {
    let mock = MockUpstream::with_melt_mode(MeltMode::Failed);
    let mut backend = UpstreamCashuBackend::new(&mock.base_url, "sat", 1000);

    let invoice = backend.create_invoice(10, "test").unwrap();
    backend.lookup_amount(&invoice).unwrap();

    let err = backend.pay_invoice(&invoice, 10).unwrap_err();
    assert_eq!(err, CashuError::PaymentFailed);
    // Proof retention on clean failure: full bootstrap still held.
    assert_eq!(backend.reserve_balance_sats(), 1010);
}

#[test]
fn pay_invoice_parks_proofs_on_ambiguous_pending_state() {
    let mock = MockUpstream::with_melt_mode(MeltMode::Pending);
    let mut backend = UpstreamCashuBackend::new(&mock.base_url, "sat", 1000);

    let invoice = backend.create_invoice(10, "test").unwrap();
    backend.lookup_amount(&invoice).unwrap();

    let err = backend.pay_invoice(&invoice, 10).unwrap_err();
    match err {
        CashuError::Protocol(msg) => assert!(msg.contains("ambiguous")),
        other => panic!("expected Protocol(ambiguous), got {other:?}"),
    }
    // Parked: the selected proofs are gone from the reserve (they may still
    // be consumed by the upstream PENDING quote), so the balance dropped.
    assert!(backend.reserve_balance_sats() < 1010);
}

/// Register a melt quote with the mock and return its id — NUT-08 blank
/// imprinting needs the quote amount server-side to compute the overpay.
fn mock_melt_quote(mock: &MockUpstream, http: &ureq::Agent, amount: u64) -> String {
    let response = http
        .post(&format!("{}/v1/melt/quote/bolt11", mock.base_url))
        .send_json(serde_json::json!({
            "request": format!("lnbcmock{amount}sat1mockup"),
            "unit": "sat",
        }))
        .expect("mock melt quote");
    let body: serde_json::Value =
        serde_json::from_reader(response.into_reader()).expect("mock melt quote JSON");
    body["quote"].as_str().expect("quote id").to_string()
}

#[test]
fn reserve_bootstrap_and_topup_math() {
    let mock = MockUpstream::start();
    let http = ureq::Agent::new();
    let mut reserve = ReserveWallet::new(&mock.base_url, "sat", 1000);

    reserve.bootstrap(64, &http).unwrap();
    assert_eq!(reserve.balance_sats(), 64);
    assert_eq!(reserve.proof_count(), 1);

    // Pay 16 from the 64-sat reserve: the single 64 proof is selected and
    // the mock imprints the 48-sat overpay onto the blank outputs.
    let quote = mock_melt_quote(&mock, &http, 16);
    let preimage = reserve.pay(&quote, 16, &http).unwrap();
    assert_eq!(preimage, MELT_PREIMAGE);
    assert_eq!(reserve.balance_sats(), 48);

    // Pay 100 with only 48 held: top-up bootstraps deficit + 1000; every
    // overpay sat returns via imprinted blanks, so balance lands at
    // 1100 - 100 = 1000 regardless of which proofs selection picks.
    let quote = mock_melt_quote(&mock, &http, 100);
    reserve.pay(&quote, 100, &http).unwrap();
    assert_eq!(reserve.balance_sats(), 1000);
}

#[test]
fn garbage_upstream_responses_are_protocol_errors_not_panics() {
    let mock = MockUpstream::garbage();
    let mut backend = UpstreamCashuBackend::new(&mock.base_url, "sat", 1000);

    let err = backend.create_invoice(21, "test").unwrap_err();
    assert!(matches!(err, CashuError::Protocol(_)));

    let mut reserve = ReserveWallet::new(&mock.base_url, "sat", 1000);
    let err = reserve.bootstrap(21, &ureq::Agent::new()).unwrap_err();
    assert!(matches!(err, CashuError::Protocol(_)));
}

/// Live check against testnut (FakeWallet — fake money, free rein).
/// Requires network; melting testnut's own invoice relies on testnut's
/// FakeWallet paying it (internal-settlement detection is inert).
///
/// Run manually:
/// `cargo test -p micronuts-mint --features backend-upstream --test upstream -- --ignored`
#[test]
#[ignore = "needs network + live https://testnut.cashu.exchange; run manually with --ignored"]
fn testnut_live_roundtrip() {
    let mut backend = UpstreamCashuBackend::new("https://testnut.cashu.exchange", "sat", 1000);

    let invoice = backend
        .create_invoice(21, "micronuts upstream-backend live check")
        .unwrap();
    assert!(
        invoice.starts_with("lnbc"),
        "upstream must return a real bolt11, got: {invoice}"
    );

    let settled = backend.is_settled(&invoice).unwrap() || backend.is_settled(&invoice).unwrap();
    assert!(settled, "FakeWallet should settle within two polls");

    // Melt target: a SECOND, independent testnut invoice (melting the mint's
    // own just-issued invoice couples the two operations; a fresh one keeps
    // them independent — mirrors the e2e_wallet.mjs upstream flow).
    let melt_target = backend.create_invoice(21, "melt target").unwrap();
    assert!(melt_target.starts_with("lnbc"));
    assert_eq!(backend.lookup_amount(&melt_target).unwrap(), 21);

    let preimage = backend.pay_invoice(&melt_target, 21).unwrap();
    assert!(
        !preimage.is_empty(),
        "FakeWallet melt must return a preimage"
    );
    assert!(
        (900..=1000).contains(&backend.reserve_balance_sats()),
        "reserve after bootstrap(1000) + melt(21) = {}",
        backend.reserve_balance_sats()
    );
}
