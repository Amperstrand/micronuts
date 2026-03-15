use k256::{elliptic_curve::sec1::FromEncodedPoint, EncodedPoint, PublicKey};
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
