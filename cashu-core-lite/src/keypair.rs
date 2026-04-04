#[cfg(not(feature = "std"))]
use alloc::fmt;
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::fmt;
#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::vec::Vec;

use core::ops::Deref;
use k256::{
    elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint},
    EncodedPoint, ProjectivePoint, PublicKey as K256PublicKey, SecretKey as K256SecretKey,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(K256PublicKey);

#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey(K256SecretKey);

impl PublicKey {
    pub fn from_affine(affine: k256::AffinePoint) -> Option<Self> {
        K256PublicKey::from_affine(affine).ok().map(Self)
    }

    pub fn from_bytes(compressed: &[u8; 33]) -> Option<Self> {
        let encoded = EncodedPoint::from_bytes(compressed).ok()?;
        K256PublicKey::from_encoded_point(&encoded)
            .into_option()
            .map(Self)
    }

    pub fn from_sec1_bytes(bytes: &[u8]) -> Result<Self, k256::elliptic_curve::Error> {
        K256PublicKey::from_sec1_bytes(bytes).map(Self)
    }

    pub fn to_sec1_bytes(&self) -> Vec<u8> {
        self.0.to_encoded_point(false).as_bytes().to_vec()
    }

    /// Return the compressed SEC1 encoding.
    ///
    /// This mirrors the upstream `cashu` crate's `PublicKey::to_bytes()` helper
    /// and makes interop/adaptation tests straightforward.
    pub fn to_bytes(&self) -> [u8; 33] {
        let encoded = self.0.to_encoded_point(true);
        let mut bytes = [0u8; 33];
        bytes.copy_from_slice(encoded.as_bytes());
        bytes
    }

    pub fn to_encoded_point(&self, compress: bool) -> EncodedPoint {
        self.0.to_encoded_point(compress)
    }

    pub fn as_affine(&self) -> k256::AffinePoint {
        *self.0.as_affine()
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoded = self.0.to_encoded_point(true);
        let hex_str = encode_hex(encoded.as_bytes());
        write!(f, "PublicKey({})", hex_str)
    }
}

impl Deref for PublicKey {
    type Target = K256PublicKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<K256PublicKey> for PublicKey {
    fn from(pk: K256PublicKey) -> Self {
        Self(pk)
    }
}

impl From<PublicKey> for ProjectivePoint {
    fn from(pk: PublicKey) -> Self {
        ProjectivePoint::from(pk.0)
    }
}

impl From<&PublicKey> for ProjectivePoint {
    fn from(pk: &PublicKey) -> Self {
        ProjectivePoint::from(pk.0)
    }
}

impl SecretKey {
    pub fn from_slice(bytes: &[u8]) -> Result<Self, k256::elliptic_curve::Error> {
        K256SecretKey::from_slice(bytes).map(Self)
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }

    pub fn to_scalar(&self) -> k256::Scalar {
        *self.0.to_nonzero_scalar()
    }

    /// Return the canonical 32-byte secret scalar encoding.
    ///
    /// This mirrors the upstream `cashu` crate's `SecretKey::to_secret_bytes()`
    /// pattern to ease future adapter work.
    pub fn to_secret_bytes(&self) -> [u8; 32] {
        self.0.to_bytes().into()
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey([REDACTED])")
    }
}

impl Deref for SecretKey {
    type Target = K256SecretKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<K256SecretKey> for SecretKey {
    fn from(sk: K256SecretKey) -> Self {
        Self(sk)
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX_CHARS[(*byte >> 4) as usize] as char);
        result.push(HEX_CHARS[(*byte & 0x0F) as usize] as char);
    }
    result
}
