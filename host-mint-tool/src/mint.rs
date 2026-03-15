use k256::{PublicKey, SecretKey};

pub struct DemoMint {
    key_1: SecretKey,
}

impl DemoMint {
    pub fn new() -> Self {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let key_1 = SecretKey::from_slice(&bytes).expect("Invalid key");
        Self { key_1 }
    }

    pub fn public_key(&self) -> PublicKey {
        self.key_1.public_key()
    }

    pub fn sign(&self, blinded: &PublicKey) -> PublicKey {
        use k256::ProjectivePoint;

        let scalar = self.key_1.to_nonzero_scalar();
        let blinded_projective: ProjectivePoint = blinded.into();
        let sig_projective = blinded_projective * *scalar;
        PublicKey::from_affine(sig_projective.into()).expect("Invalid signature")
    }
}

impl Default for DemoMint {
    fn default() -> Self {
        Self::new()
    }
}
