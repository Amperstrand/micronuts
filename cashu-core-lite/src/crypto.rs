use k256::{
    elliptic_curve::sec1::FromEncodedPoint, EncodedPoint, ProjectivePoint, PublicKey, Scalar,
    SecretKey,
};
use sha2::{Digest, Sha256};

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

pub struct BlindedMessage {
    pub blinded: PublicKey,
    pub blinder: SecretKey,
}

pub fn blind_message(
    secret: &[u8],
    blinder: Option<SecretKey>,
) -> Result<BlindedMessage, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_projective: ProjectivePoint = y.into();

    let r_scalar = match blinder {
        Some(sk) => *sk.to_nonzero_scalar(),
        #[cfg(feature = "std")]
        None => Scalar::random(&mut OsRng),
        #[cfg(not(feature = "std"))]
        None => panic!("blinder required for no_std"),
    };

    let r_sk = SecretKey::new(r_scalar.into());
    let r_pk = r_sk.public_key();
    let r_projective: ProjectivePoint = r_pk.into();

    let blinded_projective = y_projective + r_projective;
    let blinded =
        PublicKey::from_affine(blinded_projective.into()).map_err(|_| HashToCurveError)?;

    Ok(BlindedMessage {
        blinded,
        blinder: r_sk,
    })
}

pub fn unblind_signature(
    blinded_sig: &PublicKey,
    blinder: &SecretKey,
    mint_pubkey: &PublicKey,
) -> Result<PublicKey, ()> {
    let c_prime_projective: ProjectivePoint = blinded_sig.into();
    let r_scalar = *blinder.to_nonzero_scalar();
    let k_projective: ProjectivePoint = mint_pubkey.into();
    let r_k_projective: ProjectivePoint = k_projective * r_scalar;

    let unblinded_projective: ProjectivePoint = c_prime_projective - r_k_projective;
    PublicKey::from_affine(unblinded_projective.into()).map_err(|_| ())
}

pub fn verify_signature(
    secret: &[u8],
    unblinded_sig: &PublicKey,
    _mint_pubkey: &PublicKey,
) -> Result<bool, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_projective: ProjectivePoint = y.into();
    let sig_projective: ProjectivePoint = unblinded_sig.into();

    Ok(y_projective == sig_projective || y_projective == -sig_projective)
}
