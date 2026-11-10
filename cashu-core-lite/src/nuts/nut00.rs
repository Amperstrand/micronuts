//! NUT-00: Notation, ID, and Units
//!
//! Core data models for blinded messages, blind signatures, and proofs
//! used throughout the Cashu protocol.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/00.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::keypair::PublicKey;
use crate::nuts::nut12::{BlindSignatureDleq, ProofDleq};
use minicbor::{Decode, Encode};

/// A blinded message sent from wallet to mint (NUT-00).
///
/// The wallet blinds a secret `x` into `B_ = Y + r*G` where `Y = hash_to_curve(x)`.
/// The mint signs `B_` with its private key for the given amount.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct BlindedMessage {
    /// The denomination value of this output.
    #[n(0)]
    pub amount: u64,
    /// The keyset ID identifying which mint key to use.
    #[n(1)]
    pub id: String,
    /// `B_`: the blinded secret (a curve point).
    #[n(2)]
    pub b: PublicKey,
}

/// A blind signature returned from mint to wallet (NUT-00).
///
/// The mint computes `C_ = k * B_` where `k` is the mint's private key
/// for the requested amount.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct BlindSignature {
    /// The denomination value of this output.
    #[n(0)]
    pub amount: u64,
    /// The keyset ID used for signing.
    #[n(1)]
    pub id: String,
    /// `C_`: the blinded signature (a curve point).
    #[n(2)]
    pub c: PublicKey,
    /// NUT-12 DLEQ proof (optional — present when the mint supports DLEQ).
    #[n(3)]
    pub dleq: Option<BlindSignatureDleq>,
}

/// A proof of ecash ownership (NUT-00).
///
/// After unblinding: `C = C_ - r*K` where `K` is the mint's public key.
/// The proof is `(secret, C)` which the mint can verify using its private key.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Proof {
    /// The denomination value of this proof.
    #[n(0)]
    pub amount: u64,
    /// The keyset ID that was used to sign this proof.
    #[n(1)]
    pub id: String,
    /// The secret `x` (hex-encoded).
    #[n(2)]
    pub secret: String,
    /// `C`: the unblinded signature (a curve point).
    #[n(3)]
    pub c: PublicKey,
    /// NUT-12 proof-level DLEQ (e, s, r): enables public-key-only offline
    /// verification via [`crate::nuts::nut12::verify_proof_dleq`].
    #[n(4)]
    pub dleq: Option<ProofDleq>,
    /// NUT-10/11 witness: stringified-JSON spending-condition data
    /// (e.g. a P2PK `{"signatures":[…]}`) attached when spending a locked
    /// proof. Absent on ordinary proofs and on locked proofs before spend.
    #[n(5)]
    pub witness: Option<String>,
}

/// Cashu error response (NUT-00).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ErrorResponse {
    #[n(0)]
    pub detail: String,
    #[n(1)]
    pub code: u32,
}

/// Decompose an amount into powers of two (NUT-00 optimal split).
///
/// Returns a vector of power-of-two denominations that sum to the given amount.
/// For example, `decompose_amount(13)` returns `[8, 4, 1]`.
///
/// This intentionally mirrors the upstream Cashu/CDK greedy split semantics
/// (`cashu::Amount::split` with power-of-two denominations and zero fee),
/// while keeping the implementation `no_std`.
pub fn decompose_amount(amount: u64) -> Vec<u64> {
    if amount == 0 {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut remaining = amount;
    let mut denomination = 1u64;

    while denomination <= remaining / 2 {
        denomination <<= 1;
    }

    while denomination > 0 {
        if denomination <= remaining {
            result.push(denomination);
            remaining -= denomination;
        }
        denomination >>= 1;
    }

    result
}

/// Decompose `amount` into denominations the mint's keyset actually has
/// (repeats allowed), largest denomination first. Returns `None` when the
/// keyset cannot represent the amount exactly — e.g. an odd amount when
/// the keyset lacks the 1-unit key, an empty denomination list, or a split
/// that would need more than `max_outputs` outputs (memory bound: repeated
/// small denominations scale linearly with the amount). Callers MUST
/// refuse to mint such amounts before posting NUT-04: the mint rejects
/// unknown denominations only after the quote was created (issue #163).
pub fn decompose_amount_for_keyset(
    amount: u64,
    denominations: &[u64],
    max_outputs: usize,
) -> Option<Vec<u64>> {
    if amount == 0 {
        return None;
    }
    let mut sorted: Vec<u64> = denominations.iter().copied().filter(|&d| d > 0).collect();
    sorted.sort_unstable_by(|a, b| b.cmp(a));

    let mut result = Vec::new();
    let mut remaining = amount;
    while remaining > 0 {
        if result.len() >= max_outputs {
            return None;
        }
        let next = *sorted.iter().find(|&&d| d <= remaining)?;
        result.push(next);
        remaining -= next;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "std"))]
    use alloc::vec;

    #[test]
    fn test_decompose_amount_for_keyset_splits_above_max_with_repeats() {
        // issue #163: 300 against a 1..64 keyset must not emit 256 or 128
        let denominations: Vec<u64> = (0..7).map(|exp| 1u64 << exp).collect();
        let parts = decompose_amount_for_keyset(300, &denominations, 1000).unwrap();
        assert_eq!(parts.iter().sum::<u64>(), 300);
        assert!(
            parts.iter().all(|p| denominations.contains(p)),
            "{:?}",
            parts
        );
        assert_eq!(parts, vec![64, 64, 64, 64, 32, 8, 4]);
        // amounts above the keyset max are fine when repeats can cover them
        assert_eq!(
            decompose_amount_for_keyset(128, &denominations, 1000),
            Some(vec![64, 64])
        );
    }

    #[test]
    fn test_decompose_amount_for_keyset_matches_power_of_two_when_full() {
        for amount in [1u64, 2, 63, 64, 100, 127] {
            let denominations: Vec<u64> = (0..7).map(|exp| 1u64 << exp).collect();
            let parts = decompose_amount_for_keyset(amount, &denominations, 1000).unwrap();
            assert_eq!(parts, decompose_amount(amount));
        }
    }

    #[test]
    fn test_decompose_amount_for_keyset_unrepresentable() {
        let denominations: Vec<u64> = (0..7).map(|exp| 1u64 << exp).collect();
        // no 1-unit denomination: odd amounts are unreachable
        let evens: Vec<u64> = denominations.iter().copied().filter(|d| *d > 1).collect();
        assert!(decompose_amount_for_keyset(3, &evens, 1000).is_none());
        assert!(decompose_amount_for_keyset(1, &evens, 1000).is_none());
        // even amounts still work
        assert_eq!(
            decompose_amount_for_keyset(6, &evens, 1000),
            Some(vec![4, 2])
        );
        // zero amount is not a mint
        assert!(decompose_amount_for_keyset(0, &denominations, 1000).is_none());
        // empty or zero-only denomination lists represent nothing
        assert!(decompose_amount_for_keyset(1, &[], 1000).is_none());
        assert!(decompose_amount_for_keyset(1, &[0], 1000).is_none());
    }

    #[test]
    fn test_decompose_amount_for_keyset_output_budget() {
        // codex P2 (PR #164): 1_000_000_000 sats against a 1..64 keyset
        // would need 15_625_000 outputs — the split must stop at the
        // budget instead of allocating it.
        let denominations: Vec<u64> = (0..7).map(|exp| 1u64 << exp).collect();
        assert!(decompose_amount_for_keyset(1_000_000_000, &denominations, 1000).is_none());
        // exactly at the budget is representable, one past it is not
        let at_budget = 64 * 1000;
        let parts = decompose_amount_for_keyset(at_budget, &denominations, 1000).unwrap();
        assert_eq!(parts.len(), 1000);
        assert!(decompose_amount_for_keyset(at_budget + 1, &denominations, 1000).is_none());
        // tiny budgets still decompose small amounts
        assert_eq!(
            decompose_amount_for_keyset(3, &denominations, 2),
            Some(vec![2, 1])
        );
        assert!(decompose_amount_for_keyset(7, &denominations, 2).is_none());
    }

    #[test]
    fn test_decompose_amount() {
        assert_eq!(decompose_amount(0), Vec::<u64>::new());
        assert_eq!(decompose_amount(1), vec![1]);
        assert_eq!(decompose_amount(2), vec![2]);
        assert_eq!(decompose_amount(3), vec![2, 1]);
        assert_eq!(decompose_amount(13), vec![8, 4, 1]);
        assert_eq!(decompose_amount(64), vec![64]);
        assert_eq!(decompose_amount(100), vec![64, 32, 4]);
        assert_eq!(decompose_amount(1u64 << 32), vec![1u64 << 32]);
        assert_eq!(decompose_amount(1u64 << 63), vec![1u64 << 63]);
        assert_eq!(
            decompose_amount(u64::MAX),
            (0..64).rev().map(|bit| 1u64 << bit).collect::<Vec<_>>()
        );
    }
}
