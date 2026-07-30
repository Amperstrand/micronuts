//! NUT-12: Offline ecash signature validation (DLEQ proofs).
//!
//! Ports CDK's private `verify_dleq` (from `crates/cashu/src/nuts/nut12.rs`
//! at commit f4171368c8) from the `secp256k1` C FFI crate to pure-Rust
//! `k256`. This lets a wallet verify a mint's blind signature using only
//! the mint's *public* key — the trustless path that does not require the
//! mint's private key (unlike [`crate::crypto::verify_signature`]).
//!
//! Reference: <https://github.com/cashubtc/nuts/blob/main/12.md>

#[cfg(not(feature = "std"))]
use alloc::string::String;

use k256::ProjectivePoint;
use sha2::{Digest, Sha256};

use crate::keypair::{PublicKey, SecretKey};

/// Error returned by [`verify_dleq`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DleqError {
    /// A computed point was the identity (point at infinity), which has no
    /// SEC1 encoding. With well-formed inputs this never happens; reaching
    /// it indicates a malformed or adversarially crafted proof.
    IdentityPoint,
}

#[cfg(feature = "std")]
impl std::fmt::Display for DleqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdentityPoint => write!(f, "dleq: computed point was the identity"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DleqError {}

/// DLEQ proof attached to a [`crate::nuts::nut00::BlindSignature`].
///
/// Produced by the mint alongside `C'` and verified by the walletting user
/// (Alice) using only the mint's public key `A`.
///
/// Defined in [NUT-12](https://github.com/cashubtc/nuts/blob/main/12.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlindSignatureDleq {
    /// Challenge scalar.
    pub e: SecretKey,
    /// Response scalar.
    pub s: SecretKey,
}

impl<C> minicbor::Encode<C> for BlindSignatureDleq {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.map(2)?
            .u64(0)?
            .bytes(&self.e.to_secret_bytes())?
            .u64(1)?
            .bytes(&self.s.to_secret_bytes())?;
        Ok(())
    }
}

impl<'b, C> minicbor::Decode<'b, C> for BlindSignatureDleq {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        let len = d.map()?;
        if len != Some(2) {
            return Err(minicbor::decode::Error::message("expected map of 2 entries"));
        }
        d.u64()?;
        let e = SecretKey::from_slice(d.bytes()?)
            .map_err(|_| minicbor::decode::Error::message("invalid e scalar"))?;
        d.u64()?;
        let s = SecretKey::from_slice(d.bytes()?)
            .map_err(|_| minicbor::decode::Error::message("invalid s scalar"))?;
        Ok(Self { e, s })
    }
}

/// DLEQ proof attached to a [`crate::token::Proof`].
///
/// Forwarded by Alice to another user (Carol) along with the blinding
/// factor `r`, so Carol can reconstruct `B'` and `C'` and run the same
/// verification herself.
///
/// Defined in [NUT-12](https://github.com/cashubtc/nuts/blob/main/12.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofDleq {
    /// Challenge `e = hash(R1, R2, A, C')`.
    pub e: SecretKey,
    /// Response `s = (r + e*a) mod n`.
    pub s: SecretKey,
    /// Blinding factor `r` Alice used when producing `B' = Y + r*G`.
    pub r: SecretKey,
}

impl ProofDleq {
    /// Create a new [`ProofDleq`] from its three secret scalars.
    pub fn new(e: SecretKey, s: SecretKey, r: SecretKey) -> Self {
        Self { e, s, r }
    }
}

/// NUT-12 `hash_e`: SHA-256 over the concatenated *hex-encoded ASCII* forms of
/// the four uncompressed (65-byte) SEC1 public points.
///
/// Per the NUT-12 spec the 65-byte uncompressed encoding of each point is
/// first hex-encoded (130 lowercase characters) and the resulting strings
/// concatenated; SHA-256 is then taken over the UTF-8 bytes of that
/// concatenation. This matches CDK's `dhke::hash_e` and the reference
/// Python implementation in the spec.
fn hash_e(r1: &PublicKey, r2: &PublicKey, a: &PublicKey, c_prime: &PublicKey) -> [u8; 32] {
    // 4 points * 65 bytes * 2 hex chars = 520 ASCII chars.
    let mut e_string = String::with_capacity(4 * 130);
    for pk in [r1, r2, a, c_prime] {
        // SEC1 uncompressed: 0x04 || x[32] || y[32] = 65 bytes.
        let uncompressed = pk.to_encoded_point(false);
        e_string.push_str(&hex::encode(uncompressed.as_bytes()));
    }
    Sha256::digest(e_string.as_bytes()).into()
}

/// Verify a NUT-12 DLEQ proof on a blind signature.
///
/// Implements the Alice-side verification:
///
/// ```text
/// R1 = s*G - e*A
/// R2 = s*B' - e*C'
/// e == hash(R1, R2, A, C')   // must hold
/// ```
///
/// where `A` is the mint's public key, `B'` the blinded message Alice
/// produced, `C'` the blind signature returned by the mint, and `(e, s)`
/// the DLEQ challenge/response. If the proof holds, the same scalar `a`
/// was used to derive both `A = a*G` and `C' = a*B'` — i.e. the mint
/// signed `B'` with the key it claims.
///
/// This is a faithful 1:1 port of CDK's private `verify_dleq`
/// (`crates/cashu/src/nuts/nut12.rs:99-138` at f4171368c8), translated
/// from `secp256k1` C FFI calls to `k256` scalar/point arithmetic.
///
/// Returns `Ok(true)` if the proof is valid, `Ok(false)` if it is not
/// (i.e. `e' != e`), and `Err` only if a computed `R1`/`R2` is the
/// identity point (which cannot be SEC1-encoded).
pub fn verify_dleq(
    blinded_message: &PublicKey,   // B'
    blinded_signature: &PublicKey, // C'
    e: &SecretKey,
    s: &SecretKey,
    mint_pubkey: &PublicKey,       // A
) -> Result<bool, DleqError> {
    let e_scalar = e.to_scalar();
    let s_scalar = s.to_scalar();

    // a = e * A
    let a_proj: ProjectivePoint = mint_pubkey.into();
    let a_e = a_proj * e_scalar;

    // R1 = s*G - a  (= s*G + (-e*A))
    let s_g = ProjectivePoint::GENERATOR * s_scalar;
    let r1_proj = s_g - a_e;

    // b = s * B'
    let b_prime_proj: ProjectivePoint = blinded_message.into();
    let b = b_prime_proj * s_scalar;

    // c = e * C'
    let c_prime_proj: ProjectivePoint = blinded_signature.into();
    let c = c_prime_proj * e_scalar;

    // R2 = b - c  (= s*B' - e*C')
    let r2_proj = b - c;

    // Both R1 and R2 must be encodable (not the identity point).
    let r1 = PublicKey::from_affine(r1_proj.into()).ok_or(DleqError::IdentityPoint)?;
    let r2 = PublicKey::from_affine(r2_proj.into()).ok_or(DleqError::IdentityPoint)?;

    // e' = hash(R1, R2, A, C')
    let e_prime = hash_e(&r1, &r2, mint_pubkey, blinded_signature);

    // Verify: e' == e
    Ok(e.to_secret_bytes() == e_prime)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Inline the official NUT-12 `hash_e` test vector so a `cargo test -p
    // cashu-core-lite --lib` run exercises it without pulling the integration
    // test harness. Mirrors CDK's `dhke::tests::test_hash_e`.
    #[test]
    fn hash_e_matches_nut12_spec_vector() {
        let c = PublicKey::from_bytes(
            &hex_decode_33("02a9acc1e48c25eeeb9289b5031cc57da9fe72f3fe2861d264bdc074209b107ba2"),
        )
        .unwrap();
        let g = PublicKey::from_bytes(
            &hex_decode_33("020000000000000000000000000000000000000000000000000000000000000001"),
        )
        .unwrap();

        let e = hash_e(&g, &g, &g, &c);
        assert_eq!(
            hex::encode(e),
            "a4dc034b74338c28c6bc3ea49731f2a24440fc7c4affc08b31a93fc9fbe6401e"
        );
    }

    fn hex_decode_33(s: &str) -> [u8; 33] {
        let mut out = [0u8; 33];
        let bytes = hex::decode(s).unwrap();
        out.copy_from_slice(&bytes);
        out
    }
}
