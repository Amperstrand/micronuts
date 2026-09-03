//! Property-based invariants for the crypto core (plan fill-in 3).
//!
//! These complement the pinned cross-vectors: vectors lock exact bytes
//! against other implementations; properties lock the algebraic contracts
//! over the whole input space — DLEQ prove↔verify, blind↔unblind inverse,
//! the V4 token codec roundtrip, and the NUT-00 power-of-two split.

use cashu_core_lite::crypto::{
    blind_message, hash_to_curve, sign_message, unblind_signature, verify_signature_with_privkey,
};
use cashu_core_lite::keypair::SecretKey;
use cashu_core_lite::nuts::nut00::decompose_amount;
use cashu_core_lite::nuts::nut12::{prove_dleq, verify_dleq, ProofDleq};
use cashu_core_lite::{decode_token, encode_token, Proof, TokenV4, TokenV4Token};
use proptest::prelude::*;

fn any_secret() -> impl Strategy<Value = SecretKey> {
    any::<[u8; 32]>().prop_filter_map("valid secp256k1 scalar", |b| SecretKey::from_slice(&b).ok())
}

fn any_hexish_string() -> impl Strategy<Value = String> {
    "[0-9a-f]{0,64}".prop_map(|s| s)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// NUT-12: a honestly-produced DLEQ proof verifies; the same proof with
    /// a different response scalar must not.
    #[test]
    fn dleq_prove_verify_roundtrip(
        a in any_secret(),
        r in any_secret(),
        other in any_secret(),
        msg in any::<Vec<u8>>(),
    ) {
        let bm = blind_message(&msg, Some(r)).unwrap();
        let c_prime = sign_message(&a, &bm.blinded);
        let a_pk = a.public_key();

        let dleq = prove_dleq(&bm.blinded, &a, None).unwrap();
        prop_assert!(verify_dleq(&bm.blinded, &c_prime, &dleq.e, &dleq.s, &a_pk).unwrap());

        prop_assert!(!verify_dleq(&bm.blinded, &c_prime, &dleq.e, &other, &a_pk).unwrap());
    }

    /// NUT-00 DHKE: unblind(sign(a, blind(Y, r)), r, a) == a*Y for every
    /// secret, blinder, and mint key — the blind↔unblind inverse.
    #[test]
    fn blind_unblind_inverse(
        a in any_secret(),
        r in any_secret(),
        msg in any::<Vec<u8>>(),
    ) {
        let _ = hash_to_curve(&msg).unwrap(); // proves hash_to_curve total on all inputs
        let bm = blind_message(&msg, Some(r)).unwrap();
        let c_prime = sign_message(&a, &bm.blinded);
        let c = unblind_signature(&c_prime, &bm.blinder, &a.public_key()).unwrap();
        prop_assert!(verify_signature_with_privkey(&msg, &c, &a).unwrap());
    }

    /// Token V4 codec: decode(encode(t)) == t across arbitrary contents.
    #[test]
    fn token_v4_roundtrip(
        mint in "mint[0-9]{0,8}",
        unit in "sat|msat|usd",
        n_tokens in 1usize..3,
        proofs in prop::collection::vec(
            (any::<u64>(), any_hexish_string(), any::<Vec<u8>>(), any_secret(), any_secret(), any_secret(), 0u8..2),
            1..4
        ),
    ) {
        let tokens = (0..n_tokens)
            .map(|_| {
                let proofs = proofs
                    .iter()
                    .map(|(amount, secret, c, e, s, rr, with_dleq)| Proof {
                        amount: *amount,
                        keyset_id: String::from("00"),
                        secret: secret.clone(),
                        c: c.clone(),
                        dleq: if *with_dleq == 0 {
                            Some(ProofDleq::new(e.clone(), s.clone(), rr.clone()))
                        } else {
                            None
                        },
                    })
                    .collect();
                TokenV4Token { keyset_id: String::from("00"), proofs }
            })
            .collect();
        let t = TokenV4 { mint, unit, memo: None, tokens };
        let wire = encode_token(&t).unwrap();
        prop_assert_eq!(decode_token(&wire).unwrap(), t);
    }

    /// NUT-00 split: the decomposition sums back exactly, uses only
    /// strictly-decreasing powers of two, and is empty only for zero.
    #[test]
    fn decompose_amount_sums_back(amount in any::<u64>()) {
        let parts = decompose_amount(amount);
        if amount == 0 {
            prop_assert!(parts.is_empty());
        } else {
            prop_assert_eq!(parts.iter().sum::<u64>(), amount);
            prop_assert!(parts.iter().all(|&p| p.is_power_of_two()));
            prop_assert!(parts.windows(2).all(|w| w[0] > w[1]));
            prop_assert_eq!(parts.len() as u32, amount.count_ones());
        }
    }
}
