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

/// A blinded message sent from wallet to mint (NUT-00).
///
/// The wallet blinds a secret `x` into `B_ = Y + r*G` where `Y = hash_to_curve(x)`.
/// The mint signs `B_` with its private key for the given amount.
#[derive(Debug, Clone)]
pub struct BlindedMessage {
    /// The denomination value of this output.
    pub amount: u64,
    /// The keyset ID identifying which mint key to use.
    pub id: String,
    /// `B_`: the blinded secret (a curve point).
    pub b: PublicKey,
}

/// A blind signature returned from mint to wallet (NUT-00).
///
/// The mint computes `C_ = k * B_` where `k` is the mint's private key
/// for the requested amount.
#[derive(Debug, Clone)]
pub struct BlindSignature {
    /// The denomination value of this output.
    pub amount: u64,
    /// The keyset ID used for signing.
    pub id: String,
    /// `C_`: the blinded signature (a curve point).
    pub c: PublicKey,
}

/// A proof of ecash ownership (NUT-00).
///
/// After unblinding: `C = C_ - r*K` where `K` is the mint's public key.
/// The proof is `(secret, C)` which the mint can verify using its private key.
#[derive(Debug, Clone)]
pub struct Proof {
    /// The denomination value of this proof.
    pub amount: u64,
    /// The keyset ID that was used to sign this proof.
    pub id: String,
    /// The secret `x` (hex-encoded).
    pub secret: String,
    /// `C`: the unblinded signature (a curve point).
    pub c: PublicKey,
}

/// Cashu error response (NUT-00).
#[derive(Debug, Clone)]
pub struct ErrorResponse {
    pub detail: String,
    pub code: u32,
}

/// Decompose an amount into powers of two (NUT-00 optimal split).
///
/// Returns a sorted (ascending) vector of power-of-two denominations that sum
/// to the given amount. For example, `decompose_amount(13)` returns `[1, 4, 8]`.
pub fn decompose_amount(amount: u64) -> Vec<u64> {
    let mut result = Vec::new();
    let mut remaining = amount;
    let mut denomination = 1u64;
    while remaining > 0 {
        if remaining & 1 == 1 {
            result.push(denomination);
        }
        remaining >>= 1;
        denomination <<= 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompose_amount() {
        assert_eq!(decompose_amount(0), Vec::<u64>::new());
        assert_eq!(decompose_amount(1), vec![1]);
        assert_eq!(decompose_amount(2), vec![2]);
        assert_eq!(decompose_amount(3), vec![1, 2]);
        assert_eq!(decompose_amount(13), vec![1, 4, 8]);
        assert_eq!(decompose_amount(64), vec![64]);
        assert_eq!(decompose_amount(100), vec![4, 32, 64]);
    }
}
