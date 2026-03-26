use k256::{PublicKey, SecretKey};
use sha2::{Digest, Sha256};

pub struct DemoMint {
    key_1: SecretKey,
}

impl DemoMint {
    pub fn new() -> Self {
        let mut hasher = Sha256::new();
        Digest::update(&mut hasher, b"demo://micronuts");
        let seed = Digest::finalize(hasher);
        let key_1 = SecretKey::from_slice(&seed).expect("Invalid demo mint key");
        Self { key_1 }
    }

    pub fn public_key(&self) -> PublicKey {
        self.key_1.public_key()
    }

    pub fn sign(&self, blinded: &PublicKey) -> PublicKey {
        let scalar = *self.key_1.to_nonzero_scalar();
        let blinded_affine = blinded.as_affine();
        let sig_projective = *blinded_affine * scalar;
        PublicKey::from_affine(sig_projective.into()).expect("Invalid signature")
    }
}

impl Default for DemoMint {
    fn default() -> Self {
        Self::new()
    }
}
