//! Wallet operations using the `MintClient` transport trait.
//!
//! The `Wallet` struct provides high-level Cashu wallet operations:
//! minting, swapping, and melting ecash. It uses the `MintClient` trait
//! for communication with the mint, making it transport-neutral.
//!
//! Crypto operations (blinding, unblinding) use functions from `crate::crypto`.

#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use rand_core::RngCore;

use crate::crypto::{blind_message, hash_to_curve, unblind_signature};
use crate::error::CashuError;
use crate::keypair::SecretKey;
use crate::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07};
use crate::transport::MintClient;

/// Internal record kept while minting/swapping.
/// Holds the blinder (r) needed to unblind the mint's response.
struct PendingOutput {
    /// The secret `x` for this output (raw bytes, not hex).
    secret: Vec<u8>,
    /// The blinding factor `r`.
    blinder: SecretKey,
    /// The denomination amount.
    amount: u64,
}

/// A transport-neutral Cashu wallet.
///
/// Generic over `T: MintClient` so it can work with any transport
/// (direct in-process, USB, HTTP, microfips, …).
pub struct Wallet<T: MintClient> {
    /// The mint URL (used as an identifier, not for actual HTTP).
    pub mint_url: String,
    /// The transport to communicate with the mint.
    pub transport: T,
}

impl<T: MintClient> Wallet<T> {
    /// Create a new wallet with the given mint URL and transport.
    pub fn new(mint_url: &str, transport: T) -> Self {
        Self {
            mint_url: String::from(mint_url),
            transport,
        }
    }

    /// NUT-06: Fetch mint info.
    pub fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        self.transport.get_info()
    }

    /// NUT-01: Fetch the mint's active public keys.
    pub fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        self.transport.get_keys()
    }

    /// NUT-02: Fetch keyset metadata.
    pub fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        self.transport.get_keysets()
    }

    /// NUT-04: Request a mint quote.
    pub fn request_mint_quote(
        &mut self,
        amount: u64,
        unit: &str,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.transport.post_mint_quote(nut04::MintQuoteRequest {
            amount,
            unit: String::from(unit),
        })
    }

    /// NUT-04: Check a mint quote's state.
    pub fn check_mint_quote(
        &mut self,
        quote_id: &str,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.transport.get_mint_quote(quote_id)
    }

    /// NUT-04: Mint ecash tokens.
    ///
    /// Given a paid quote, generates blinded outputs for the requested amount,
    /// sends them to the mint, and unblinds the returned signatures into proofs.
    ///
    /// `rng` is used to generate random blinding factors and secrets.
    pub fn mint_tokens(
        &mut self,
        quote_id: &str,
        amount: u64,
        keyset_id: &str,
        mint_keys: &nut01::KeySet,
        rng: &mut dyn RngCore,
    ) -> Result<Vec<nut00::Proof>, CashuError> {
        // NUT-00: decompose amount into power-of-two denominations
        let denominations = nut00::decompose_amount(amount);

        // Generate blinded outputs
        let (blinded_messages, pending) =
            self.create_blinded_outputs(&denominations, keyset_id, rng)?;

        // NUT-04: send mint request
        let response = self.transport.post_mint(nut04::MintRequest {
            quote: String::from(quote_id),
            outputs: blinded_messages,
        })?;

        // Unblind signatures into proofs
        self.unblind_to_proofs(&pending, &response.signatures, mint_keys)
    }

    /// NUT-03: Swap existing proofs for new proofs with different denominations.
    ///
    /// The total value of the new denominations must equal the total value
    /// of the input proofs (minus any fees, which are 0 in the demo).
    pub fn swap(
        &mut self,
        proofs: Vec<nut00::Proof>,
        new_amounts: &[u64],
        keyset_id: &str,
        mint_keys: &nut01::KeySet,
        rng: &mut dyn RngCore,
    ) -> Result<Vec<nut00::Proof>, CashuError> {
        // Generate blinded outputs for the new denominations
        let (blinded_messages, pending) =
            self.create_blinded_outputs(new_amounts, keyset_id, rng)?;

        // NUT-03: send swap request
        let response = self.transport.post_swap(nut03::SwapRequest {
            inputs: proofs,
            outputs: blinded_messages,
        })?;

        // Unblind signatures into proofs
        self.unblind_to_proofs(&pending, &response.signatures, mint_keys)
    }

    /// NUT-05: Request a melt quote.
    pub fn request_melt_quote(
        &mut self,
        invoice: &str,
        unit: &str,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.transport.post_melt_quote(nut05::MeltQuoteRequest {
            request: String::from(invoice),
            unit: String::from(unit),
        })
    }

    /// NUT-05: Check a melt quote's state.
    pub fn check_melt_quote(
        &mut self,
        quote_id: &str,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.transport.get_melt_quote(quote_id)
    }

    /// NUT-05: Melt (spend) ecash to pay a Lightning invoice.
    ///
    /// Demo shortcut: no real Lightning payment occurs; the mint auto-approves.
    pub fn melt(
        &mut self,
        quote_id: &str,
        proofs: Vec<nut00::Proof>,
    ) -> Result<nut05::MeltResponse, CashuError> {
        self.transport.post_melt(nut05::MeltRequest {
            quote: String::from(quote_id),
            inputs: proofs,
            outputs: None,
        })
    }

    /// NUT-07: Check proof states (spent/unspent).
    pub fn check_state(
        &mut self,
        secrets: &[&[u8]],
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        let mut ys = Vec::new();
        for secret in secrets {
            let y = hash_to_curve(secret)
                .map_err(|_| CashuError::Crypto("hash_to_curve failed".into()))?;
            ys.push(y);
        }
        self.transport
            .post_check_state(nut07::CheckStateRequest { ys })
    }

    // ---- Internal helpers ----

    /// Generate blinded outputs for the given denomination amounts.
    ///
    /// Returns the protocol-level blinded messages (to send to the mint) and
    /// the internal pending records (to unblind later).
    fn create_blinded_outputs(
        &self,
        amounts: &[u64],
        keyset_id: &str,
        rng: &mut dyn RngCore,
    ) -> Result<(Vec<nut00::BlindedMessage>, Vec<PendingOutput>), CashuError> {
        let mut messages = Vec::with_capacity(amounts.len());
        let mut pending = Vec::with_capacity(amounts.len());

        for &amount in amounts {
            // Generate a random secret (32 bytes, hex-encoded when stored in proof)
            let mut secret_bytes = [0u8; 32];
            rng.fill_bytes(&mut secret_bytes);

            // Generate a random blinding factor
            let mut blinder_bytes = [0u8; 32];
            rng.fill_bytes(&mut blinder_bytes);
            let blinder = SecretKey::from_slice(&blinder_bytes)
                .map_err(|_| CashuError::Crypto("bad blinder scalar".into()))?;

            // NUT-00: B_ = Y + r*G where Y = hash_to_curve(secret)
            let bm = blind_message(&secret_bytes, Some(blinder.clone()))
                .map_err(|_| CashuError::Crypto("blind_message failed".into()))?;

            messages.push(nut00::BlindedMessage {
                amount,
                id: String::from(keyset_id),
                b: bm.blinded.clone(),
            });

            pending.push(PendingOutput {
                secret: secret_bytes.to_vec(),
                blinder: bm.blinder,
                amount,
            });
        }

        Ok((messages, pending))
    }

    /// Unblind a set of blind signatures into proofs.
    ///
    /// For each signature `C_`, computes `C = C_ - r*K` where `K` is the
    /// mint's public key for that denomination.
    fn unblind_to_proofs(
        &self,
        pending: &[PendingOutput],
        signatures: &[nut00::BlindSignature],
        mint_keys: &nut01::KeySet,
    ) -> Result<Vec<nut00::Proof>, CashuError> {
        if pending.len() != signatures.len() {
            return Err(CashuError::Protocol(
                "signature count mismatch".to_string(),
            ));
        }

        let mut proofs = Vec::with_capacity(pending.len());

        for (p, sig) in pending.iter().zip(signatures.iter()) {
            // Find the mint's public key for this denomination
            let mint_pubkey = mint_keys
                .keys
                .iter()
                .find(|kp| kp.amount == p.amount)
                .map(|kp| &kp.pubkey)
                .ok_or(CashuError::KeysetNotFound)?;

            // NUT-00: C = C_ - r*K
            let c = unblind_signature(&sig.c, &p.blinder, mint_pubkey)
                .map_err(|_| CashuError::Crypto("unblind failed".into()))?;

            // Hex-encode the secret for the proof
            let secret_hex = hex_encode(&p.secret);

            proofs.push(nut00::Proof {
                amount: p.amount,
                id: sig.id.clone(),
                secret: secret_hex,
                c,
            });
        }

        Ok(proofs)
    }
}

/// Hex-encode bytes into a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0F) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_encode(&[0x00, 0xff]), "00ff");
        assert_eq!(hex_encode(&[]), "");
    }
}
