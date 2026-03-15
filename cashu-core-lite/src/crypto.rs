use k256::{
    elliptic_curve::sec1::FromEncodedPoint, EncodedPoint, NonZeroScalar, ProjectivePoint, PublicKey,
};
use rand_core::{CryptoRng, RngCore};
use sha2::{Digest, Sha256};

const DOMAIN_SEPARATOR: &[u8; 28] = b"Secp256k1_HashToCurve_Cashu_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashToCurveError;

pub fn hash_to_curve(message: &[u8]) -> Result<PublicKey, HashToCurveError> {
    let mut counter: u32 = 0;

    let msg_hash = Sha256::new()
        .chain_update(DOMAIN_SEPARATOR)
        .chain_update(message)
        .finalize();

    loop {
        let candidate = Sha256::new()
            .chain_update(&msg_hash)
            .chain_update(counter.to_le_bytes())
            .finalize();

        let point_bytes: [u8; 33] = {
            let mut arr = [0u8; 33];
            arr[0] = 0x02;
            arr[1..33].copy_from_slice(&candidate);
            arr
        };

        if let Some(point) = EncodedPoint::from_bytes(&point_bytes)
            .ok()
            .and_then(|ep| PublicKey::from_encoded_point(&ep).into_option())
        {
            return Ok(point);
        }

        counter = counter.checked_add(1).ok_or(HashToCurveError)?;
    }
}

/// Blinds a message point with a random scalar.
///
/// Computes: blinded_point = Y + r*G
///
/// Where:
/// - Y is the message point (output of hash_to_curve)
/// - r is a random scalar (blinding factor)
/// - G is the generator point
///
/// Returns (blinded_point, blinding_factor).
/// The blinding_factor must be kept secret and used later to unblind the signature.
///
/// Uses constant-time scalar arithmetic.
pub fn blind_message<R: RngCore + CryptoRng>(
    message_point: &PublicKey,
    rng: &mut R,
) -> (PublicKey, NonZeroScalar) {
    let r = NonZeroScalar::random(rng);
    let y = ProjectivePoint::from(message_point.as_affine());
    let r_times_g = ProjectivePoint::GENERATOR * r.as_ref();
    let blinded = y + r_times_g;
    let blinded_pk = PublicKey::from_affine(blinded.to_affine()).expect("valid point");

    (blinded_pk, r)
}

/// Unblinds a blinded signature.
///
/// Computes: unblinded = C' - r*K
///
/// Where:
/// - C' is the blinded signature from the mint
/// - r is the blinding factor (from blind_message)
/// - K is the mint's public key
///
/// Uses constant-time scalar arithmetic.
pub fn unblind_signature(
    blinded_signature: &PublicKey,
    blinding_factor: &NonZeroScalar,
    mint_pubkey: &PublicKey,
) -> PublicKey {
    let c_prime = ProjectivePoint::from(blinded_signature.as_affine());
    let k = ProjectivePoint::from(mint_pubkey.as_affine());
    let r_times_k = k * blinding_factor.as_ref();
    let unblinded = c_prime - r_times_k;
    PublicKey::from_affine(unblinded.to_affine()).expect("valid point")
}
