//! Entropic keyset seed path (#56).
//!
//! Every default `DemoMint` key derives from the PUBLIC constant seed
//! `SHA256("micronuts-demo-mint-seed")` — anyone can re-derive the private
//! keys and forge proofs. `DemoMint::with_keyset_seed` swaps in keys derived
//! from a caller-supplied SECRET seed. These tests pin the contract:
//!   - deterministic: same seed ⇒ same keyset id (restarts re-derive the
//!     exact keyset from the seed alone)
//!   - distinct: different seeds ⇒ different ids, never the demo id
//!   - parameter-preserving: unit and NUT-08 input fee carry over

use micronuts_mint::keyset::DemoKeyset;
use micronuts_mint::ln::{FakeWallet, SystemClock};
use micronuts_mint::DemoMint;

/// The one sanctioned public derivation (#56): the demo default keyset id.
/// CI demo-bin smoke, the audit adapter, and the conformance legs pin to it.
const DEMO_KEYSET_ID: &str = "0022e025867793d1";

#[test]
fn demo_default_keyset_id_is_pinned() {
    // Documents the ONE sanctioned public derivation and guards accidental
    // demo-seed drift (a changed seed would silently break every pinned
    // consumer).
    assert_eq!(DemoKeyset::demo_default().id, DEMO_KEYSET_ID);
    assert_eq!(
        DemoKeyset::demo_with_fee(7).id,
        DEMO_KEYSET_ID,
        "fee must not change the demo keyset id"
    );
}

#[test]
fn same_seed_derives_same_keyset_id() {
    let seed: [u8; 32] = core::array::from_fn(|i| i as u8 ^ 0x5A);
    let a = DemoMint::new().with_keyset_seed(&seed);
    let b = DemoMint::new().with_keyset_seed(&seed);
    assert_eq!(
        a.keyset_id(),
        b.keyset_id(),
        "same seed must re-derive the same keyset id (deterministic restore)"
    );
}

#[test]
fn different_seeds_derive_different_keyset_ids() {
    let seed_a: [u8; 32] = core::array::from_fn(|i| i as u8);
    let seed_b: [u8; 32] = core::array::from_fn(|i| i as u8 + 1);
    let a = DemoMint::new().with_keyset_seed(&seed_a);
    let b = DemoMint::new().with_keyset_seed(&seed_b);
    assert_ne!(a.keyset_id(), b.keyset_id());
}

#[test]
fn entropic_seed_never_yields_the_demo_keyset_id() {
    for i in 0..4u8 {
        let seed: [u8; 32] = core::array::from_fn(|j| j as u8 ^ i);
        let mint = DemoMint::new().with_keyset_seed(&seed);
        assert_ne!(
            mint.keyset_id(),
            DEMO_KEYSET_ID,
            "seed #{i} must not collide with the public demo keyset"
        );
    }
}

#[test]
fn with_keyset_seed_preserves_unit_and_input_fee() {
    // Base mint carries a non-zero NUT-08 fee and the sat unit: both must
    // survive the seed swap (derive from the CURRENT keyset's parameters).
    let seed: [u8; 32] = [0xAB; 32];
    let mint = DemoMint::with_backend(Box::new(FakeWallet), Box::new(SystemClock), 1234)
        .with_keyset_seed(&seed);

    let keysets = mint.get_keysets().unwrap();
    assert_eq!(keysets.keysets.len(), 1);
    assert_eq!(keysets.keysets[0].input_fee_ppk, 1234, "fee carries over");
    assert_eq!(keysets.keysets[0].unit, "sat", "unit carries over");

    let keys = mint.get_keys().unwrap();
    assert_eq!(keys.keysets[0].unit, "sat");
    assert_ne!(mint.keyset_id(), DEMO_KEYSET_ID);
}
