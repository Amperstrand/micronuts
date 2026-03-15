use k256::{
    elliptic_curve::sec1::FromEncodedPoint, EncodedPoint, ProjectivePoint, PublicKey, SecretKey,
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
    blinder: SecretKey,
) -> Result<BlindedMessage, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_proj: ProjectivePoint = y.into();

    let r_pk = blinder.public_key();
    let r_proj: ProjectivePoint = r_pk.into();

    let blinded_proj = y_proj + r_proj;
    let blinded = blinded_proj.to_affine();

    Ok(BlindedMessage {
        blinded: PublicKey::from_affine(blinded).map_err(|_| HashToCurveError)?,
        blinder,
    })
}

pub fn unblind_signature(
    blinded_sig: &PublicKey,
    blinder: &SecretKey,
    mint_pubkey: &PublicKey,
) -> Result<PublicKey, ()> {
    let c_prime_proj: ProjectivePoint = (*blinded_sig).into();

    let r_scalar = blinder.to_nonzero_scalar();
    let k_proj: ProjectivePoint = (*mint_pubkey).into();
    let r_k_proj = k_proj * r_scalar.as_ref();

    let unblinded_proj = c_prime_proj - r_k_proj;
    let unblinded = unblinded_proj.to_affine();

    PublicKey::from_affine(unblinded).map_err(|_| ())
}

pub fn verify_signature(
    secret: &[u8],
    unblinded_sig: &PublicKey,
    _mint_pubkey: &PublicKey,
) -> Result<bool, HashToCurveError> {
    let y = hash_to_curve(secret)?;
    let y_affine = y.as_affine();
    let sig_affine = unblinded_sig.as_affine();

    Ok(y_affine == sig_affine || y_affine == &-*sig_affine)
}
