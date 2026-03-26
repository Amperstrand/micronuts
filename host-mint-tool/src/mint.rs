use cashu_core_lite::{PublicKey, SecretKey};
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
        use k256::ProjectivePoint;
        let scalar = self.key_1.to_scalar();
        let sig_projective: ProjectivePoint = blinded.into();
        let c_prime = sig_projective * scalar;
        PublicKey::from_affine(c_prime.into()).expect("Invalid signature")
    }
}

impl Default for DemoMint {
    fn default() -> Self {
        Self::new()
    }
}
