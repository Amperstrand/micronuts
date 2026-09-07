//! NUT-20: Signature on Mint Quote
//!
//! Quote locking: a mint quote created with a `pubkey` (NUT-04 request
//! extension) can only be minted against a valid BIP-340 signature by the
//! corresponding secret key.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/20.md

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::nut00::BlindedMessage;

/// Domain-separation tag for the NUT-20 quote-locking signature message
/// (raw ASCII bytes, not length-prefixed).
pub const MINT_QUOTE_SIG_DOMAIN_TAG: &[u8] = b"Cashu_MintQuoteSig_v1";

/// The message a wallet signs (and the mint verifies) when minting a
/// NUT-20 locked quote. BIP-340 Schnorr over SHA-256 of these bytes.
///
/// Mirrors upstream `cashu` 0.18 `nut20::mint_quote_msg_to_sign`
/// (differential-tested in `micronuts-mint/tests/nut20_differential.rs`).
// NUT #20: msg_to_sign = b"Cashu_MintQuoteSig_v1"
// NUT #20:               || len32(quote) || quote
// NUT #20:               || for each output i (in request order):
// NUT #20:                    len32(amount_i) || amount_i
// NUT #20:                    || len32(B_i)   || B_i
pub fn quote_sig_message(quote_id: &str, outputs: &[BlindedMessage]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(
        MINT_QUOTE_SIG_DOMAIN_TAG.len() + 4 + quote_id.len() + outputs.len() * (4 + 8 + 4 + 33),
    );
    msg.extend_from_slice(MINT_QUOTE_SIG_DOMAIN_TAG);
    append_len_prefixed(&mut msg, quote_id.as_bytes());
    for output in outputs {
        append_len_prefixed(&mut msg, &amount_to_minimal_be_bytes(output.amount));
        append_len_prefixed(&mut msg, &output.b.to_bytes());
    }
    msg
}

// NUT #20: - `amount_i` is the output amount as canonical minimal big-endian bytes (e.g. `0` → empty byte array, `1` → `0x01`, `256` → `0x0100`); thus `len32(amount_i)` is its length in bytes as a 32-bit integer (e.g. `0` for amount `0`, `1` for amount `1`, `2` for amount `256`).
fn amount_to_minimal_be_bytes(amount: u64) -> Vec<u8> {
    if amount == 0 {
        return Vec::new();
    }
    let bytes = amount.to_be_bytes();
    let first_non_zero = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    bytes[first_non_zero..].to_vec()
}

// NUT #20: - `B_i` is the raw byte representation of the blinded message (e.g. 33-byte compressed secp256k1 or 48-byte BLS12-381 point), decoded from the request's hex string.
// NUT #20: - `||` denotes byte concatenation and `len32(x)` is the 32-bit (4-byte) big-endian length of the byte array `x` in bytes.
fn append_len_prefixed(msg: &mut Vec<u8>, bytes: &[u8]) {
    msg.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    msg.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::{PublicKey, SecretKey};

    fn point(seed: u8) -> PublicKey {
        SecretKey::from_slice(&[seed; 32]).unwrap().public_key()
    }

    /// Minimal big-endian amount encoding edge cases pinned by the spec.
    #[test]
    fn amount_encoding_is_minimal_big_endian() {
        assert_eq!(amount_to_minimal_be_bytes(0), Vec::<u8>::new());
        assert_eq!(amount_to_minimal_be_bytes(1), vec![0x01]);
        assert_eq!(amount_to_minimal_be_bytes(256), vec![0x01, 0x00]);
        assert_eq!(
            amount_to_minimal_be_bytes(0xffff_ffff_ffff_ffff),
            vec![0xff; 8]
        );
    }

    /// Byte-exact message from the upstream `cashu` 0.18 nut20 test vector
    /// (`test_msg_to_sign`): quote 0192d3c0-…, two 1-sat outputs.
    #[test]
    fn message_matches_upstream_test_vector() {
        let outputs = vec![
            BlindedMessage {
                amount: 1,
                id: "009a1f293253e41e".to_string(),
                b: PublicKey::from_bytes(&{
                    let mut arr = [0u8; 33];
                    hex::decode_to_slice(
                        "036d6caac248af96f6afa7f904f550253a0f3ef3f5aa2fe6838a95b216691468e2",
                        &mut arr,
                    )
                    .unwrap();
                    arr
                })
                .unwrap(),
            },
            BlindedMessage {
                amount: 1,
                id: "009a1f293253e41e".to_string(),
                b: PublicKey::from_bytes(&{
                    let mut arr = [0u8; 33];
                    hex::decode_to_slice(
                        "021f8a566c205633d029094747d2e18f44e05993dda7a5f88f496078205f656e59",
                        &mut arr,
                    )
                    .unwrap();
                    arr
                })
                .unwrap(),
            },
        ];

        let msg = quote_sig_message("0192d3c0-7e8a-7c3d-8e9f-1a2b3c4d5e6f", &outputs);

        assert_eq!(
            hex::encode(&msg),
            "43617368755f4d696e7451756f74655369675f76310000002430313932643363302d376538612d376333642d386539662d316132623363346435653666000000010100000021036d6caac248af96f6afa7f904f550253a0f3ef3f5aa2fe6838a95b216691468e2000000010100000021021f8a566c205633d029094747d2e18f44e05993dda7a5f88f496078205f656e59"
        );
    }

    /// Zero-amount outputs contribute an empty (len32 == 0) amount field.
    #[test]
    fn zero_amount_output_contributes_empty_length_prefix() {
        let msg = quote_sig_message(
            "q",
            &[BlindedMessage {
                amount: 0,
                id: "00".to_string(),
                b: point(7),
            }],
        );
        // tag || len32("q")=1 || 'q' || len32(0)=0 || len32(33)=33 || point
        let mut expected = Vec::new();
        expected.extend_from_slice(b"Cashu_MintQuoteSig_v1");
        expected.extend_from_slice(&1u32.to_be_bytes());
        expected.push(b'q');
        expected.extend_from_slice(&0u32.to_be_bytes());
        expected.extend_from_slice(&33u32.to_be_bytes());
        expected.extend_from_slice(&point(7).to_bytes());
        assert_eq!(msg, expected);
    }
}
