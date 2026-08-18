use k256::ProjectivePoint;
#[cfg(feature = "std")]
use k256::Scalar;
use sha2::{Digest, Sha256};

use crate::keypair::{PublicKey, SecretKey};
use crate::nuts::nut12::DleqError;

#[cfg(feature = "std")]
use rand_core::OsRng;

const DOMAIN_SEPARATOR: &[u8; 28] = b"Secp256k1_HashToCurve_Cashu_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashToCurveError;

#[cfg(feature = "std")]
impl std::fmt::Display for HashToCurveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to hash message to curve")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for HashToCurveError {}

/// NUT-00: `Y = PublicKey('02' || SHA256(msg_hash || counter))`
///
/// where `msg_hash = SHA256("Secp256k1_HashToCurve_Cashu_" || x)`,
/// counter is a 4-byte little-endian `u32` in the range `0..2^16`
/// (i.e. `0..=u16::MAX`, 65536 values), matching the upstream Cashu/CDK
/// reference implementation.
pub fn hash_to_curve(message: &[u8]) -> Result<PublicKey, HashToCurveError> {
    let msg_hash = Sha256::new()
        .chain_update(DOMAIN_SEPARATOR)
        .chain_update(message)
        .finalize();

    for counter in 0..=u16::MAX as u32 {
        let candidate = Sha256::new()
            .chain_update(msg_hash)
            .chain_update(counter.to_le_bytes())
            .finalize();

        let point_bytes: [u8; 33] = {
            let mut arr = [0u8; 33];
            arr[0] = 0x02;
            arr[1..33].copy_from_slice(&candidate);
            arr
        };

        if let Some(point) = PublicKey::from_bytes(&point_bytes) {
            return Ok(point);
        }
    }

    Err(HashToCurveError)
}

pub struct BlindedMessage {
    pub blinded: PublicKey,
    pub blinder: SecretKey,
}

/// NUT-00: `B' = Y + rG`
///
/// where `Y = hash_to_curve(secret)` and `r` is a random blinding scalar.
pub fn blind_message(
    secret: &[u8],
    blinder: Option<SecretKey>,
) -> Result<BlindedMessage, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_projective: ProjectivePoint = y.into();

    let r_scalar = match blinder {
        Some(sk) => sk.to_scalar(),
        #[cfg(feature = "std")]
        None => Scalar::generate_vartime(&mut OsRng),
        #[cfg(not(feature = "std"))]
        None => panic!("blinder required for no_std"),
    };

    let r_sk = k256::SecretKey::new(r_scalar.into());
    let r_pk = r_sk.public_key();
    let r_projective: ProjectivePoint = r_pk.into();

    let blinded_projective = y_projective + r_projective;
    let blinded = PublicKey::from_affine(blinded_projective.into()).ok_or(HashToCurveError)?;

    Ok(BlindedMessage {
        blinded,
        blinder: SecretKey::from(r_sk),
    })
}

/// NUT-00: `C = C' - rK`
///
/// Unblinds a mint signature by subtracting the blinding factor times the mint public key.
pub fn unblind_signature(
    blinded_sig: &PublicKey,
    blinder: &SecretKey,
    mint_pubkey: &PublicKey,
) -> Result<PublicKey, ()> {
    let c_prime_projective: ProjectivePoint = blinded_sig.into();
    let r_scalar = blinder.to_scalar();
    let k_projective: ProjectivePoint = mint_pubkey.into();
    let r_k_projective: ProjectivePoint = k_projective * r_scalar;

    let unblinded_projective: ProjectivePoint = c_prime_projective - r_k_projective;
    PublicKey::from_affine(unblinded_projective.into()).ok_or(())
}

/// NUT-00: `C' = k * B'`
///
/// Mint-side signing of a blinded message with private key `a`.
pub fn sign_message(a: &SecretKey, blinded_message: &PublicKey) -> PublicKey {
    let b_prime: ProjectivePoint = blinded_message.into();
    let k_scalar = a.to_scalar();
    let c_prime: ProjectivePoint = b_prime * k_scalar;
    PublicKey::from_affine(c_prime.into()).expect("valid signature point")
}

/// NUT-00: `a * hash_to_curve(x) == C`
///
/// Verifies that an unblinded signature `C` was honestly produced by the mint
/// holding private key `a`. Requires the mint's **private** key.
/// For public-key-only verification, use NUT-12 DLEQ proofs (implemented in verify_signature).
///
/// # Why this is separate from verify_signature
/// This function requires the mint's private key `a` to compute `a * hash_to_curve(x)`.
/// It's useful for **audit/debug** scenarios where the verifier has access to the mint's private key
/// (e.g., mint self-verification, firmware diagnostics). Wallets and clients should use the
/// public-key-only `verify_signature` function which only requires the mint's public key.
pub fn verify_signature_with_privkey(
    secret: &[u8],
    unblinded_sig: &PublicKey,
    a: &SecretKey,
) -> Result<bool, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_projective: ProjectivePoint = y.into();
    let sig_projective: ProjectivePoint = unblinded_sig.into();
    let k_scalar = a.to_scalar();
    let expected: ProjectivePoint = y_projective * k_scalar;

    Ok(expected == sig_projective)
}

/// NUT-00: Public-key-only signature verification using NUT-12 DLEQ proofs
///
/// Verifies that a mint signature was honestly produced using the mint's public key.
/// This is the **primary** verification path for wallets and clients — it only requires
/// the mint's public key, not the mint's private key.
///
/// Uses NUT-12 DLEQ proof verification: given blinded message `B'`, blinded signature `C'`,
/// and DLEQ proof components `e` and `s`, verifies that the mint honestly signed `B'` using
/// the claimed public key `A`.
///
/// # Algorithm
/// - Verifies the DLEQ proof: `e' == e` where `e' = hash(R1, R2, A, C')`
/// - `R1 = s*G + (-e*A)`
/// - `R2 = s*B' - e*C'`
///
/// # Arguments
/// * `blinded_message` — `B'` from the blind step
/// * `blinded_signature` — `C'` from the mint's response
/// * `e` — DLEQ proof witness (discrete log of B' to base A)
/// * `s` — DLEQ proof witness (discrete log of C' to base A)
/// * `mint_pubkey` — Mint's public key `A` (only!)
///
/// # Errors
/// Returns `Err` if the computed `R1`/`R2` are the identity point (cannot be SEC1-encoded).
///
/// # Comparison to verify_signature_with_privkey
/// - **Private-key path**: `verify_signature_with_privkey` requires the mint's private key.
///   Used for mint self-verification, firmware diagnostics, offline audit.
/// - **Public-key path**: `verify_signature` uses DLEQ and only requires the mint's public key.
///   Used by wallets, clients, and standard mint operations.
pub fn verify_signature(
    blinded_message: &PublicKey,
    blinded_signature: &PublicKey,
    e: &SecretKey,
    s: &SecretKey,
    mint_pubkey: &PublicKey,
) -> Result<bool, DleqError> {
    crate::nuts::nut12::verify_dleq(blinded_message, blinded_signature, e, s, mint_pubkey)
}
