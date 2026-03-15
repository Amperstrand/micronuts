use alloc::vec::Vec;
use k256::{
    elliptic_curve::{sec1::FromEncodedPoint, AffinePoint, Field},
    AffinePoint as Secp256k1Affine, EncodedPoint, PublicKey, Scalar, SecretKey,
};
use sha2::{Digest, Sha256};

const DOMAIN_SEPARATOR: &[u8; 28] = b"Secp256k1_HashToCurve_Cashu_";

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

pub struct BlindedMessage {
    pub blinded: PublicKey,
    pub blinder: SecretKey,
}

pub fn blind_message(
    secret: &[u8],
    blinder: Option<SecretKey>,
) -> Result<BlindedMessage, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_affine = y.to_affine();

    let r = blinder.unwrap_or_else(|| Scalar::random(&mut rand_core::OsRng));

    let r_sk = SecretKey::new(r);
    let r_pk = r_sk.public_key();
    let r_affine = r_pk.to_affine();

    let blinded_affine = y_affine + r_affine;
    let blinded = PublicKey::from_affine(blinded_affine).map_err(|_| HashToCurveError)?;

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
    let c_prime_affine = blinded_sig.to_affine();
    let r_pk = blinder.public_key();
    let r_k_affine = {
        let r = blinder.to_scalar();
        let k = mint_pubkey.to_affine();
        let r_scalar = r;
        let result = k * r_scalar;
        result
    };

    let unblinded_affine = c_prime_affine - r_k_affine;
    PublicKey::from_affine(unblinded_affine).map_err(|_| ())
}

pub fn verify_signature(
    secret: &[u8],
    unblinded_sig: &PublicKey,
    mint_pubkey: &PublicKey,
) -> Result<bool, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_affine = y.to_affine();
    let sig_affine = unblinded_sig.to_affine();
    let mint_affine = mint_pubkey.to_affine();

    Ok(y_affine == sig_affine || y_affine == -sig_affine)
}
