//! NUT-14: Hashed Timelock Contracts (HTLC) — the condition and witness
//! model (L3, roadmap #51).
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/14.md
//!
//! Like [`super::nut11`], this module owns only the typed model: the
//! [`HtlcConditions`] parsed out of an HTLC-kind [`super::nut10::Secret`],
//! its malformedness rules, and the [`HtlcWitness`] wire shape. Signature
//! verification lives in `micronuts-mint`'s spending module (the house
//! crypto-provider pattern); preimage hashing is pure data and stays here.
//!
//! Semantics mirror upstream `cashu` 0.18's `verify_htlc` /
//! `verify_sig_all_htlc` (the differential oracle; see
//! `micronuts-mint/tests/htlc_differential.rs`), with the spec files winning
//! on disagreement — each divergence is marked with an
//! `Upstream divergence:` comment.

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use sha2::{Digest, Sha256};

use super::nut10::{Secret, SecretKind};
use super::nut11::{
    has_duplicate_x_coordinates, parse_condition_tags, RefundPath, SigFlag, SpendingRequirements,
};
use crate::keypair::PublicKey;

/// Ways an HTLC secret can be malformed (NUT-10/11/14 MUST-reject rules;
/// reuses NUT-11's tag errors plus the hash-specific ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtlcError {
    /// The secret's kind is not `HTLC`.
    KindMismatch,
    /// `Secret.data` is not a 32-byte SHA-256 hash as 64 hex characters.
    InvalidDataHash,
    /// Inherited NUT-11 tag malformation (empty row, bad value, unknown
    /// sigflag, duplicate tag, zero or impossible threshold, duplicate key).
    Tag(super::nut11::P2pkError),
}

/// The HTLC spending condition derived from a well-known Secret.
///
/// `data` is the hash lock (SHA-256 of the 32-byte preimage); the receiver
/// pathway is the `pubkeys` tag plus the preimage, the sender pathway is the
/// NUT-11 refund pathway after `locktime`. Malformed conditions reject at
/// construction exactly like [`super::nut11::P2pkConditions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtlcConditions {
    /// The hash lock from `Secret.data`.
    pub data_hash: [u8; 32],
    /// Receiver-pathway keys from the `pubkeys` tag (may be empty: the
    /// preimage alone then spends).
    pub pubkeys: Vec<PublicKey>,
    /// Minimum receiver-pathway signatures (`n_sigs` tag).
    pub num_sigs: Option<u64>,
    /// Parsed `locktime` tag (sender-pathway opening time).
    pub locktime: Option<u64>,
    /// Sender-pathway keys from the `refund` tag.
    pub refund_keys: Vec<PublicKey>,
    /// Minimum sender-pathway signatures (`n_sigs_refund` tag).
    pub num_sigs_refund: Option<u64>,
    /// `sigflag` tag (defaults to `SIG_INPUTS` when absent).
    pub sigflag: SigFlag,
}

impl HtlcConditions {
    /// Derive and validate the HTLC condition from a parsed well-known
    /// Secret. `Err` means the HTLC secret is malformed and the proof MUST
    /// be rejected as unspendable.
    // NUT #14: If for a `Proof`, `Proof.secret` is a `Secret` of kind `HTLC`, the hash of the lock is in `Proof.secret.data`. The preimage for unlocking the HTLC is in the witness `Proof.witness.preimage`. All additional tags from P2PK locks can also be used here, allowing a locktime, signature flag, and multisig (see [NUT-11][11]).
    // NUT #14: The hash lock in `Secret.data` and the preimage in `Proof.witness.preimage` are treated as 32-byte data, encoded as 64-character hexadecimal strings.
    pub fn from_secret(secret: &Secret) -> Result<Self, HtlcError> {
        if secret.kind != SecretKind::Htlc {
            return Err(HtlcError::KindMismatch);
        }
        let data_hash = decode_32_bytes(&secret.data).ok_or(HtlcError::InvalidDataHash)?;
        let tags = parse_condition_tags(secret.tags.as_deref().unwrap_or_default())
            .map_err(HtlcError::Tag)?;

        // The receiver pathway has no `data` key (data is a hash, not a
        // pubkey), so its threshold is bounded by the pubkeys tag alone.
        // Note: upstream divergence — cashu 0.18.0's verification path
        // Note: computes the receiver threshold as 0 whenever the pubkeys
        // Note: tag is absent, silently ignoring an unsatisfiable `n_sigs`;
        // Note: its authoring path does reject it. The spec's NUT-11 rule
        // Note: ("exceeds the total number of keys in its pathway ... MUST
        // Note: be rejected as unspendable") wins here.
        if let Some(required) = tags.num_sigs {
            let available = tags.pubkeys.len() as u64;
            if required > available {
                return Err(HtlcError::Tag(
                    super::nut11::P2pkError::ImpossibleMultisig {
                        required,
                        available,
                    },
                ));
            }
        }
        if has_duplicate_x_coordinates(&tags.pubkeys) {
            return Err(HtlcError::Tag(super::nut11::P2pkError::DuplicatePubkey));
        }
        if has_duplicate_x_coordinates(&tags.refund_keys) {
            return Err(HtlcError::Tag(super::nut11::P2pkError::DuplicatePubkey));
        }

        Ok(HtlcConditions {
            data_hash,
            pubkeys: tags.pubkeys,
            num_sigs: tags.num_sigs,
            locktime: tags.locktime,
            refund_keys: tags.refund_keys,
            num_sigs_refund: tags.num_sigs_refund,
            sigflag: tags.sigflag,
        })
    }

    /// True when `preimage_hex` (64 hex chars, 32 bytes) hashes to the
    /// `data_hash` lock.
    // NUT #14: To successfully spend a Proof via the **Receiver Pathway**, the spender must present the matching `preimage_bytes`, encoded as a 64-character lowercase hexadecimal string in the `Proof.witness.preimage`.
    // NUT #14: Mints and wallets **must verify** this equality before accepting the spend as valid:
    pub fn matches_preimage(&self, preimage_hex: &str) -> bool {
        let Some(preimage) = decode_32_bytes(preimage_hex) else {
            return false;
        };
        let digest = Sha256::digest(preimage);
        self.data_hash[..] == digest[..]
    }

    /// Resolve the lock's pathways at time `now` (unix seconds): the
    /// receiver pathway is always available (preimage + pubkeys signatures;
    /// an absent `pubkeys` tag means the preimage alone spends), the
    /// sender/refund pathway opens after `locktime` exactly as in NUT-11.
    // NUT #14: The receiver(s) listed in the `pubkeys` tag can spend the proof by providing **BOTH** of the following:
    // NUT #14: This pathway is **ALWAYS** available to the receivers, as possession of the preimage confirms performance of the Sender's wishes.
    // NUT #14: **NOTE:** If the `pubkeys` tag is absent, the preimage alone spends the proof; no signature is required.
    // NUT #14: The sender(s) listed in the `refund` tag can spend the proof once the `locktime` lock has "expired" by providing signature(s) as per the [NUT-11][11] rules for **Refund MultiSig**.
    // NUT #14: **NOTE:** As per the [NUT-11][11] rules, if the `refund` tag is not present, the HTLC proof would become "anyone can spend" after the `locktime` lock has "expired". Likewise, if the `locktime` is not present, or is not a valid unix timestamp, the HTLC proof will be permanently locked and can only be spent using the Receiver (hash lock) pathway.
    pub fn requirements_at(&self, now: u64) -> SpendingRequirements {
        let expired = self.locktime.is_some_and(|locktime| locktime < now);
        let refund_path = if expired {
            Some(if self.refund_keys.is_empty() {
                RefundPath::anyone()
            } else {
                RefundPath {
                    pubkeys: self.refund_keys.clone(),
                    required_sigs: self.num_sigs_refund.unwrap_or(1),
                }
            })
        } else {
            None
        };
        SpendingRequirements {
            pubkeys: self.pubkeys.clone(),
            required_sigs: if self.pubkeys.is_empty() {
                0
            } else {
                self.num_sigs.unwrap_or(1)
            },
            refund_path,
        }
    }
}

/// The NUT-14 witness carried (as stringified JSON) in `Proof.witness`.
///
/// Both fields are optional on the wire: the receiver pathway sends the
/// preimage (plus signatures when the lock has pubkeys); the sender pathway
/// after expiry sends signatures only (a NUT-11 `P2PKWitness` shape).
// NUT #14: `HTLCWitness` is a serialized JSON string of the form
// NUT #14: {
// NUT #14:   "preimage": <hex_str>,
// NUT #14:   "signatures": <Array[<hex_str>]>
// NUT #14: }
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HtlcWitness {
    /// The hash-lock preimage, 64 hex characters.
    pub preimage: Option<String>,
    /// 64-byte BIP340 Schnorr signatures as hex strings.
    pub signatures: Option<Vec<String>>,
}

impl HtlcWitness {
    /// Parse the stringified-JSON witness; `None` for anything that is not
    /// the witness object shape.
    pub fn from_json(raw: &str) -> Option<HtlcWitness> {
        let document = super::json::parse(raw)?;
        let object = document.as_object()?;
        let preimage = object
            .iter()
            .find(|(k, _)| k == "preimage")
            .and_then(|(_, v)| v.as_str())
            .map(String::from);
        let signatures = object
            .iter()
            .find(|(k, _)| k == "signatures")
            .and_then(|(_, v)| v.as_array())
            .and_then(|array| {
                array
                    .iter()
                    .map(|item| item.as_str().map(String::from))
                    .collect::<Option<Vec<_>>>()
            });
        if preimage.is_none() && signatures.is_none() {
            return None;
        }
        Some(HtlcWitness {
            preimage,
            signatures,
        })
    }
}

fn decode_32_bytes(hex_str: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(hex_str).ok()?;
    bytes.as_slice().try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nuts::nut11::P2pkError;

    const DATA_PUBKEY: &str = "0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7";
    // The spec §Hash lock example pair: SHA256(0x00..01) = ec4916dd….
    const HASH: &str = "ec4916dd28fc4c10d78e287ca5d9cc51ee1ae73cbfde08c6b37324cbfaac8bc5";
    const PREIMAGE: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn htlc_secret(tags: Option<&str>) -> Secret {
        let raw = match tags {
            Some(tags) => {
                format!("[\"HTLC\",{{\"nonce\":\"n\",\"data\":\"{HASH}\",\"tags\":{tags}}}]")
            }
            None => format!("[\"HTLC\",{{\"nonce\":\"n\",\"data\":\"{HASH}\"}}]"),
        };
        Secret::parse(&raw).unwrap()
    }

    #[test]
    fn basic_case_defaults_to_sig_inputs_and_preimage_only() {
        let conditions = HtlcConditions::from_secret(&htlc_secret(None)).unwrap();
        assert_eq!(conditions.sigflag, SigFlag::SigInputs);
        assert!(conditions.pubkeys.is_empty());
        let requirements = conditions.requirements_at(1_000_000_000);
        assert_eq!(requirements.required_sigs, 0);
        assert_eq!(requirements.pubkeys.len(), 0);
        assert!(requirements.refund_path.is_none());
    }

    #[test]
    fn parses_all_tags_from_complex_example() {
        // The spec §Complex Example shape (2-of-3 receiver + 2 refund keys
        // + locktime). The spec's second refund key (02e2aeb97…) is not a
        // point on secp256k1 — substituting a valid key keeps the example
        // parseable (a non-curve "pubkey" is itself rejected as malformed).
        let raw = "[\"HTLC\",{\"nonce\":\"da62796403af76c80cd6ce9153ed3746\",\"data\":\"ec4916dd28fc4c10d78e287ca5d9cc51ee1ae73cbfde08c6b37324cbfaac8bc5\",\"tags\":[[\"sigflag\",\"SIG_ALL\"],[\"n_sigs\",\"2\"],[\"locktime\",\"1689418329\"],[\"refund\",\"033281c37677ea273eb7183b783067f5244933ef78d8c3f15b1a77cb246099c26e\",\"0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7\"],[\"pubkeys\",\"02698c4e2b5f9534cd0687d87513c759790cf829aa5739184a3e3735471fbda904\",\"0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798\",\"023192200a0cfd3867e48eb63b03ff599c7e46c8f4e41146b2d281173ca6c50c54\"]]}]";
        let conditions = HtlcConditions::from_secret(&Secret::parse(raw).unwrap()).unwrap();
        assert_eq!(conditions.sigflag, SigFlag::SigAll);
        assert_eq!(conditions.num_sigs, Some(2));
        assert_eq!(conditions.locktime, Some(1_689_418_329));
        assert_eq!(conditions.refund_keys.len(), 2);
        assert_eq!(conditions.pubkeys.len(), 3);
        let requirements = conditions.requirements_at(1_000_000_000);
        assert_eq!(requirements.required_sigs, 2);
        assert!(requirements.refund_path.is_none());
    }

    #[test]
    fn rejects_malformed_conditions() {
        // Non-64-hex data hash.
        let raw = "[\"HTLC\",{\"nonce\":\"n\",\"data\":\"zzz\"}]";
        assert_eq!(
            HtlcConditions::from_secret(&Secret::parse(raw).unwrap()).unwrap_err(),
            HtlcError::InvalidDataHash
        );
        // A 33-byte compressed pubkey is not a 32-byte hash.
        let raw = format!("[\"HTLC\",{{\"nonce\":\"n\",\"data\":\"{DATA_PUBKEY}\"}}]");
        assert_eq!(
            HtlcConditions::from_secret(&Secret::parse(&raw).unwrap()).unwrap_err(),
            HtlcError::InvalidDataHash
        );
        // Wrong kind.
        let raw = format!("[\"P2PK\",{{\"nonce\":\"n\",\"data\":\"{DATA_PUBKEY}\"}}]");
        assert_eq!(
            HtlcConditions::from_secret(&Secret::parse(&raw).unwrap()).unwrap_err(),
            HtlcError::KindMismatch
        );
        // Inherited tag malformations.
        for (tags, expected) in [
            ("[[\"sigflag\",\"SIG_SOME\"]]", P2pkError::UnknownSigFlag),
            ("[[\"n_sigs\",\"0\"]]", P2pkError::ZeroSignatures),
            (
                "[[\"pubkeys\",\"0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7\"],[\"n_sigs\",\"2\"]]",
                P2pkError::ImpossibleMultisig { required: 2, available: 1 },
            ),
            (
                "[[\"n_sigs\",\"1\"]]",
                P2pkError::ImpossibleMultisig { required: 1, available: 0 },
            ),
            ("[[\"locktime\",\"1\"],[\"locktime\",\"2\"]]", P2pkError::DuplicateTag),
        ] {
            assert_eq!(
                HtlcConditions::from_secret(&htlc_secret(Some(tags))).unwrap_err(),
                HtlcError::Tag(expected),
                "tags {tags}"
            );
        }
    }

    #[test]
    fn preimage_matching() {
        let conditions = HtlcConditions::from_secret(&htlc_secret(None)).unwrap();
        assert!(conditions.matches_preimage(PREIMAGE));
        assert!(!conditions
            .matches_preimage("0000000000000000000000000000000000000000000000000000000000000002"));
        // Wrong lengths / bad hex never match.
        assert!(!conditions.matches_preimage("00"));
        assert!(!conditions.matches_preimage("zz"));
        assert!(!conditions.matches_preimage(""));
    }

    #[test]
    fn requirements_open_sender_path_after_expiry() {
        let tags = "[[\"locktime\",\"100\"],[\"refund\",\"0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7\"]]";
        let conditions = HtlcConditions::from_secret(&htlc_secret(Some(tags))).unwrap();
        // Boundary: refund closed at now == locktime (strict `<`),
        // open at now == locktime + 1.
        assert!(conditions.requirements_at(100).refund_path.is_none());
        let expired = conditions.requirements_at(101).refund_path.unwrap();
        assert_eq!(expired.required_sigs, 1);
        assert_eq!(expired.pubkeys.len(), 1);
    }

    #[test]
    fn expired_without_refund_keys_is_anyone_can_spend() {
        let tags = "[[\"locktime\",\"100\"]]";
        let conditions = HtlcConditions::from_secret(&htlc_secret(Some(tags))).unwrap();
        assert!(conditions.requirements_at(100).refund_path.is_none());
        let expired = conditions.requirements_at(101).refund_path.unwrap();
        assert!(expired.is_anyone_can_spend());
    }

    #[test]
    fn witness_json_shapes() {
        // Full receiver witness.
        let full = HtlcWitness::from_json("{\"preimage\":\"00\",\"signatures\":[\"aa\"]}").unwrap();
        assert_eq!(full.preimage.as_deref(), Some("00"));
        assert_eq!(full.signatures.as_deref(), Some(&["aa".to_string()][..]));
        // Sender-pathway shape: signatures only.
        let sigs_only = HtlcWitness::from_json("{\"signatures\":[\"aa\"]}").unwrap();
        assert!(sigs_only.preimage.is_none());
        // Preimage only (hash lock, no pubkeys).
        let preimage_only = HtlcWitness::from_json("{\"preimage\":\"00\"}").unwrap();
        assert!(preimage_only.signatures.is_none());
        // Empty signatures array parses (verification then fails counts).
        assert_eq!(
            HtlcWitness::from_json("{\"signatures\":[]}")
                .unwrap()
                .signatures,
            Some(Vec::new())
        );
        // Non-witness shapes.
        assert!(HtlcWitness::from_json("{}").is_none());
        assert!(HtlcWitness::from_json("not json").is_none());
        assert!(HtlcWitness::from_json("{\"signatures\":\"nope\"}").is_none());
        assert!(HtlcWitness::from_json("{\"signatures\":[1]}").is_none());
    }
}
