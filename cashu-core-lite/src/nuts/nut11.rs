//! NUT-11: Pay-to-Public-Key (P2PK) — the typed spending-condition model.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/11.md
//!
//! This module owns the P2PK-specific NUT-10 layer: the sigflag model, the
//! multisig tag model parsed out of a [`super::nut10::Secret`], its
//! malformedness rules, and the [`P2pkWitness`] type. Schnorr *verification*
//! is deliberately NOT here — it lives in `micronuts-mint`'s spending
//! module, which delegates to the upstream `cashu` crate's secp256k1 (the
//! house crypto-provider pattern; PARITY.md only requires the boundary
//! TYPES to stay in cashu-core-lite).
//!
//! L2 scope (roadmap #51): `locktime` and the refund pathway are enforced.
//! Before expiry only the primary pathway (data + pubkeys tag) can spend;
//! after expiry the refund pathway opens (refund keys, `n_sigs_refund` with
//! x-dedup) while the primary pathway remains available (the refund path is
//! *additional* — see the Refund Multisig quotes below). Expired with no
//! refund keys is anyone-can-spend. Divergences from upstream `cashu` 0.18
//! are marked with `Upstream divergence:` comments where the local spec
//! files win.

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::json;
use super::nut10::{Secret, SecretKind};
use crate::keypair::PublicKey;

/// Signature flag (NUT-11 §Tags / §Signature flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SigFlag {
    /// Requires valid signatures on all inputs independently.
    #[default]
    SigInputs,
    /// Requires valid signatures on all inputs and on all outputs.
    SigAll,
}

impl SigFlag {
    pub fn as_str(&self) -> &'static str {
        match self {
            SigFlag::SigInputs => "SIG_INPUTS",
            SigFlag::SigAll => "SIG_ALL",
        }
    }

    /// Parse a `sigflag` tag value; anything else is malformed (see the
    /// MUST-reject quote in [`P2pkConditions::from_secret`]).
    pub fn parse(value: &str) -> Option<SigFlag> {
        match value {
            "SIG_INPUTS" => Some(SigFlag::SigInputs),
            "SIG_ALL" => Some(SigFlag::SigAll),
            _ => None,
        }
    }
}

/// Ways a P2PK secret can be malformed (NUT-11 MUST-reject rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P2pkError {
    /// The secret's kind is not `P2PK`.
    KindMismatch,
    /// `Secret.data` is not a valid compressed secp256k1 public key.
    InvalidDataPubkey,
    /// A tag row is empty (no tag key at all).
    EmptyTag,
    /// A tag value is missing or not parseable as its expected type.
    InvalidTagValue,
    /// The `sigflag` value is neither `SIG_INPUTS` nor `SIG_ALL`.
    UnknownSigFlag,
    /// A known tag key appears more than once.
    DuplicateTag,
    /// `n_sigs` / `n_sigs_refund` is zero.
    ZeroSignatures,
    /// `n_sigs` / `n_sigs_refund` exceeds the keys available in its pathway.
    ImpossibleMultisig { required: u64, available: u64 },
    /// A key repeats within one signing pathway when compared by
    /// x-coordinate.
    DuplicatePubkey,
}

/// The P2PK spending condition derived from a well-known Secret.
///
/// Malformed conditions (bad sigflags, duplicate tags, impossible multisig,
/// duplicate keys) are rejected at construction: NUT-11 says such proofs
/// MUST be rejected as unspendable, so the constructor is fallible and the
/// mint treats `Err` as a hard input rejection — never as anyone-can-spend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P2pkConditions {
    /// The locked key from `Secret.data`.
    pub data_pubkey: PublicKey,
    /// Additional primary-pathway keys from the `pubkeys` tag.
    pub pubkeys: Vec<PublicKey>,
    /// Minimum primary-pathway signatures (`n_sigs` tag; default 1).
    pub num_sigs: Option<u64>,
    /// Parsed `locktime` tag: the Unix timestamp at which the refund
    /// pathway opens (see [`P2pkConditions::requirements_at`]).
    pub locktime: Option<u64>,
    /// Parsed `refund` tag keys (spendable only after `locktime`).
    pub refund_keys: Vec<PublicKey>,
    /// Minimum refund-pathway signatures (`n_sigs_refund` tag).
    pub num_sigs_refund: Option<u64>,
    /// `sigflag` tag (defaults to `SIG_INPUTS` when absent).
    pub sigflag: SigFlag,
}

impl P2pkConditions {
    /// Derive and validate the P2PK condition from a parsed well-known
    /// Secret. `Err` means the P2PK secret is malformed and the proof MUST
    /// be rejected as unspendable.
    // NUT #11: In the basic case, when spending a locked Proof, the mint requires one valid Schnorr signature in `Proof.witness.signatures` on `Proof.secret` by the public key in `Proof.secret.data`.
    // NUT #11: To spend Proofs locked with `P2PK`, the spender needs to include signatures in the Proofs used as "inputs" for the spending operation. We use `libsecp256k1`'s serialized 64 byte Schnorr signatures on the SHA256 hash of the message to sign. The message to sign is the field `Proof.secret` in the inputs, unless otherwise indicated by the `Secret.tags.sigflag` in the inputs, as detailed below.
    pub fn from_secret(secret: &Secret) -> Result<Self, P2pkError> {
        if secret.kind != SecretKind::P2PK {
            return Err(P2pkError::KindMismatch);
        }
        let data_pubkey =
            parse_compressed_pubkey(&secret.data).ok_or(P2pkError::InvalidDataPubkey)?;
        let tags = parse_condition_tags(secret.tags.as_deref().unwrap_or_default())?;

        // Threshold sanity. The primary pathway always includes data:
        // NUT #11: If `n_sigs` or `n_sigs_refund` is not a positive integer, or exceeds the total number of keys in its pathway, the P2PK secret is malformed and the Proof **MUST** be rejected as unspendable.
        // Note: upstream divergence — cashu 0.18.0's verification path
        // Note: never checks `n_sigs` against the primary key count (an
        // Note: unsatisfiable threshold just fails signature verification);
        // Note: the outcome is the same rejection, but we follow the spec's
        // Note: "malformed" ruling.
        let primary_available = 1 + tags.pubkeys.len() as u64;
        if let Some(required) = tags.num_sigs {
            if required > primary_available {
                return Err(P2pkError::ImpossibleMultisig {
                    required,
                    available: primary_available,
                });
            }
        }

        // Key canonicalisation / duplicate detection:
        // NUT #11: Public keys **MUST** use the [compressed Secp256k1 public key format](https://learnmeabitcoin.com/technical/public-key#public-key-format).
        // NUT #11: Each key **MUST** appear at most **ONCE** per [multi-signature](#Multisig) pathway. The same key **MAY** appear in both pathways.
        // NUT #11: Keys are compared using their lowercase x-coordinate (`02` or `03` y-parity prefix ignored).
        // NUT #11: If a pathway contains a duplicate key, the P2PK secret is malformed and the Proof **MUST** be rejected as unspendable.
        let mut primary = Vec::with_capacity(tags.pubkeys.len() + 1);
        primary.push(data_pubkey);
        primary.extend(tags.pubkeys.iter().copied());
        if has_duplicate_x_coordinates(&primary) {
            return Err(P2pkError::DuplicatePubkey);
        }
        if has_duplicate_x_coordinates(&tags.refund_keys) {
            return Err(P2pkError::DuplicatePubkey);
        }

        Ok(P2pkConditions {
            data_pubkey,
            pubkeys: tags.pubkeys,
            num_sigs: tags.num_sigs,
            locktime: tags.locktime,
            refund_keys: tags.refund_keys,
            num_sigs_refund: tags.num_sigs_refund,
            sigflag: tags.sigflag,
        })
    }

    /// The keys that can sign the primary pathway: data + `pubkeys` tag.
    pub fn signing_pubkeys(&self) -> Vec<PublicKey> {
        let mut keys = Vec::with_capacity(self.pubkeys.len() + 1);
        keys.push(self.data_pubkey);
        keys.extend(self.pubkeys.iter().copied());
        keys
    }

    /// Minimum distinct-key signatures the primary pathway requires.
    // NUT #11: If the `n_sigs` tag is a positive integer, the mint will require at least `n_sigs` of those public keys to provide a valid signature.
    pub fn required_sigs(&self) -> u64 {
        self.num_sigs.unwrap_or(1)
    }

    /// Minimum distinct-key signatures the refund pathway requires
    /// (`n_sigs_refund` tag; defaults to 1 like upstream).
    pub fn required_sigs_refund(&self) -> u64 {
        self.num_sigs_refund.unwrap_or(1)
    }

    /// Resolve the lock's spending pathways at time `now` (unix seconds).
    ///
    /// The primary pathway is ALWAYS available. The refund pathway exists
    /// only once the lock has expired, and an expired lock with no refund
    /// keys resolves to a refund path with zero required signatures — i.e.
    /// anyone-can-spend.
    // NUT #11: If the `locktime` tag is a valid unix time and the mint's local clock is greater than `locktime`, the lock has "expired".
    // NUT #11: Both [Locktime Multisig](#locktime-multisig) and [Refund Multisig](#refund-multisig) conditions apply if the `refund` tag is present, otherwise the proof is considered unlocked and spendable without a witness signature.
    // Note: expiry boundary — upstream cashu 0.18 computes
    // Note: `locktime_passed = locktime < now` (strictly), so a lock whose
    // Note: locktime equals `now` exactly is NOT yet expired and the refund
    // Note: pathway stays closed. The spec's "greater than locktime" wording
    // Note: agrees; frozen-clock boundary tests pin this.
    // NUT #11: Refund Multisig allows proofs to be _additionally spendable_ by a separate set of public keys once the `locktime` has expired. These public keys are stored in the `refund` tag, and can include keys previously listed in `data` or `pubkeys`.
    // NUT #11: [Locktime Multisig](#locktime-multisig) conditions continue to apply, and the proof can continue to be spent according to Locktime Multisig rules.
    // NUT #11: If the number of `refund` public keys with valid signatures is greater or equal to the number specified in `n_sigs_refund` (or `1` if `n_sigs_refund` is not present), the transaction is valid. The signatures are provided in an array of strings in the `P2PKWitness` object.
    pub fn requirements_at(&self, now: u64) -> SpendingRequirements {
        let expired = self.locktime.is_some_and(|locktime| locktime < now);
        let refund_path = if expired {
            Some(if self.refund_keys.is_empty() {
                RefundPath::anyone()
            } else {
                RefundPath {
                    pubkeys: self.refund_keys.clone(),
                    required_sigs: self.required_sigs_refund(),
                }
            })
        } else {
            None
        };
        SpendingRequirements {
            pubkeys: self.signing_pubkeys(),
            required_sigs: self.required_sigs(),
            refund_path,
        }
    }
}

/// The refund pathway of a lock, resolved after its expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundPath {
    /// Keys that can sign the refund pathway.
    pub pubkeys: Vec<PublicKey>,
    /// Minimum distinct-key signatures required; `0` means the proof is
    /// anyone-can-spend (expired lock with no refund keys).
    pub required_sigs: u64,
}

impl RefundPath {
    /// The anyone-can-spend refund path (expired lock, no refund keys).
    pub fn anyone() -> Self {
        RefundPath {
            pubkeys: Vec::new(),
            required_sigs: 0,
        }
    }

    /// True when the lock expired with no refund keys.
    pub fn is_anyone_can_spend(&self) -> bool {
        self.required_sigs == 0
    }
}

/// The pathways a lock offers at a specific moment.
///
/// Shared shape for P2PK (NUT-11) and HTLC (NUT-14): for P2PK the primary
/// pathway is `data` + `pubkeys`; for HTLC it is the `pubkeys` receiver set
/// (empty means the hash lock alone spends, so `required_sigs == 0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendingRequirements {
    /// Primary-pathway (P2PK) / receiver-pathway (HTLC) keys.
    pub pubkeys: Vec<PublicKey>,
    /// Minimum distinct-key primary/receiver signatures required.
    pub required_sigs: u64,
    /// The refund pathway, present only after expiry.
    pub refund_path: Option<RefundPath>,
}

/// The NUT-11 witness carried (as stringified JSON) in `Proof.witness`.
// NUT #11: Signatures are stored in `P2PKWitness` objects and are provided in either each `Proof.witness` of all inputs separately (for `SIG_INPUTS`) or only in the first input of the transaction (for `SIG_ALL`). `P2PKWitness` is a serialized JSON string of the form
// NUT #11: {
// NUT #11:   "signatures": <Array[<hex_str>]>
// NUT #11: }
// NUT #11: The `signatures` are an array of signatures in hex and correspond to the signatures by one or more signing public keys.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct P2pkWitness {
    /// 64-byte BIP340 Schnorr signatures as hex strings.
    pub signatures: Vec<String>,
}

impl P2pkWitness {
    /// Parse the stringified-JSON witness. `None` for anything that is not
    /// the P2PK witness shape (missing/invalid witnesses fail verification,
    /// so the caller rejects the input).
    pub fn from_json(raw: &str) -> Option<P2pkWitness> {
        let document = json::parse(raw)?;
        let array = document.get("signatures")?.as_array()?;
        let signatures = array
            .iter()
            .map(|item| item.as_str().map(String::from))
            .collect::<Option<Vec<_>>>()?;
        Some(P2pkWitness { signatures })
    }

    /// Serialize to the stringified-JSON wire form (compact, matching how
    /// upstream wallets emit it).
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"signatures\":[");
        for (i, sig) in self.signatures.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&json_escape(sig));
        }
        out.push_str("]}");
        out
    }
}

/// JSON-escape and quote a string (hex signatures never need escapes, but
/// stay correct for arbitrary content).
fn json_escape(raw: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let mut buf = [0u8; 4];
                for byte in c.encode_utf8(&mut buf).bytes() {
                    out.push_str("\\u00");
                    out.push(HEX[(byte >> 4) as usize] as char);
                    out.push(HEX[(byte & 0x0F) as usize] as char);
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The NUT-11 condition tags shared by P2PK and HTLC locks
/// (NUT-14: "All additional tags from P2PK locks can also be used here").
#[derive(Debug, Clone, Default)]
pub(super) struct ConditionTags {
    pub pubkeys: Vec<PublicKey>,
    pub num_sigs: Option<u64>,
    pub locktime: Option<u64>,
    pub refund_keys: Vec<PublicKey>,
    pub num_sigs_refund: Option<u64>,
    pub sigflag: SigFlag,
}

/// Parse and validate the shared condition tags. `Err` means the secret is
/// malformed and the proof MUST be rejected as unspendable. Kind-specific
/// rules (primary-pathway composition, key canonicalisation) are the
/// caller's job.
pub(super) fn parse_condition_tags(tags: &[Vec<String>]) -> Result<ConditionTags, P2pkError> {
    let mut parsed = ConditionTags::default();
    // Per-tag "seen once" markers (a known tag key may appear exactly
    // once — see the MUST-reject quote in the loop). Tracked separately
    // from the parsed fields because e.g. a second `sigflag` row whose
    // value equals the default must still count as a duplicate.
    let (mut saw_sigflag, mut saw_pubkeys, mut saw_n_sigs) = (false, false, false);
    let (mut saw_locktime, mut saw_refund, mut saw_n_sigs_refund) = (false, false, false);

    for row in tags {
        // NUT #11: Tags are arrays with two or more strings being `["key", "value1", "value2", ...]`. We denote a specific tag in a proof by its `key`.
        let key = row.first().map(String::as_str).ok_or(P2pkError::EmptyTag)?;
        // Values are strings on the wire even when semantically ints:
        // NUT #11: The tag serialization type is `[<str>, <str>, ...]` but some tag values are `int`. Wallets and mints must cast types appropriately for de/serialization.
        let value = || -> Result<&str, P2pkError> {
            row.get(1)
                .map(String::as_str)
                .ok_or(P2pkError::InvalidTagValue)
        };
        // NUT #11: Each of the above tags may appear exactly **ONCE** in a P2PK secret. If a tag appears more than once, the P2PK secret is malformed and the Proof **MUST** be rejected as unspendable.
        // Note: upstream divergence — cashu 0.18.0 silently keeps the
        // Note: FIRST occurrence of a repeated tag
        // Note: (`test_duplicate_tags_first_match`); the spec mandates
        // Note: rejection, and the spec wins here.
        match key {
            "sigflag" => {
                if saw_sigflag {
                    return Err(P2pkError::DuplicateTag);
                }
                saw_sigflag = true;
                parsed.sigflag = SigFlag::parse(value()?)
                    // NUT #11: If a P2PK secret has any other signature flag value, the P2PK secret is malformed and the Proof **MUST** be rejected as unspendable.
                    .ok_or(P2pkError::UnknownSigFlag)?;
            }
            "pubkeys" => {
                if saw_pubkeys {
                    return Err(P2pkError::DuplicateTag);
                }
                saw_pubkeys = true;
                parsed.pubkeys = parse_pubkey_row(row)?;
            }
            "n_sigs" => {
                if saw_n_sigs {
                    return Err(P2pkError::DuplicateTag);
                }
                saw_n_sigs = true;
                let count: u64 = value()?.parse().map_err(|_| P2pkError::InvalidTagValue)?;
                if count == 0 {
                    return Err(P2pkError::ZeroSignatures);
                }
                parsed.num_sigs = Some(count);
            }
            "locktime" => {
                if saw_locktime {
                    return Err(P2pkError::DuplicateTag);
                }
                saw_locktime = true;
                parsed.locktime = Some(value()?.parse().map_err(|_| P2pkError::InvalidTagValue)?);
            }
            "refund" => {
                if saw_refund {
                    return Err(P2pkError::DuplicateTag);
                }
                saw_refund = true;
                let keys = parse_pubkey_row(row)?;
                if keys.is_empty() {
                    // An empty `refund` row is an unsatisfiable 1-of-0
                    // refund pathway (upstream: Impossible refund
                    // multisig, required 1 available 0).
                    return Err(P2pkError::ImpossibleMultisig {
                        required: 1,
                        available: 0,
                    });
                }
                parsed.refund_keys = keys;
            }
            "n_sigs_refund" => {
                if saw_n_sigs_refund {
                    return Err(P2pkError::DuplicateTag);
                }
                saw_n_sigs_refund = true;
                let count: u64 = value()?.parse().map_err(|_| P2pkError::InvalidTagValue)?;
                if count == 0 {
                    return Err(P2pkError::ZeroSignatures);
                }
                parsed.num_sigs_refund = Some(count);
            }
            // Unknown tags are ignored (upstream keeps them as Custom).
            _ => {}
        }
    }

    // n_sigs_refund without (enough) refund keys is unsatisfiable (the
    // refund pathway defaults to requiring 1 key).
    if let Some(required) = parsed.num_sigs_refund {
        let available = parsed.refund_keys.len() as u64;
        if required > available {
            return Err(P2pkError::ImpossibleMultisig {
                required,
                available,
            });
        }
    }

    Ok(parsed)
}

/// Parse a hex string into a compressed secp256k1 public key.
pub(super) fn parse_compressed_pubkey(hex_str: &str) -> Option<PublicKey> {
    let bytes = hex::decode(hex_str).ok()?;
    let compressed: &[u8; 33] = bytes.as_slice().try_into().ok()?;
    PublicKey::from_bytes(compressed)
}

/// Parse the value entries of a `pubkeys`/`refund` tag row into keys.
pub(super) fn parse_pubkey_row(row: &[String]) -> Result<Vec<PublicKey>, P2pkError> {
    row.iter()
        .skip(1)
        .map(|value| parse_compressed_pubkey(value).ok_or(P2pkError::InvalidTagValue))
        .collect()
}

/// True if any two keys share an x-coordinate (02/03 parity ignored).
pub(super) fn has_duplicate_x_coordinates(pubkeys: &[PublicKey]) -> bool {
    let mut seen: Vec<[u8; 32]> = Vec::with_capacity(pubkeys.len());
    for pubkey in pubkeys {
        let compressed = pubkey.to_bytes();
        let mut x = [0u8; 32];
        x.copy_from_slice(&compressed[1..33]);
        if seen.contains(&x) {
            return true;
        }
        seen.push(x);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA_PUBKEY: &str = "0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7";

    fn p2pk_secret(tags: Option<&str>) -> Secret {
        let raw = match tags {
            Some(tags) => {
                format!("[\"P2PK\",{{\"nonce\":\"n\",\"data\":\"{DATA_PUBKEY}\",\"tags\":{tags}}}]")
            }
            None => format!("[\"P2PK\",{{\"nonce\":\"n\",\"data\":\"{DATA_PUBKEY}\"}}]"),
        };
        Secret::parse(&raw).unwrap()
    }

    #[test]
    fn basic_case_defaults_to_sig_inputs_and_one_sig() {
        let conditions = P2pkConditions::from_secret(&p2pk_secret(None)).unwrap();
        assert_eq!(conditions.sigflag, SigFlag::SigInputs);
        assert_eq!(conditions.required_sigs(), 1);
        assert_eq!(conditions.signing_pubkeys().len(), 1);
        assert!(conditions.locktime.is_none());
        assert!(conditions.refund_keys.is_empty());
    }

    #[test]
    fn parses_all_tags_from_complex_example() {
        // The spec §Complex Example shape (2-of-3 with refund keys and
        // locktime). The spec's second refund key (02e2aeb97…) is not a
        // point on secp256k1 — substituting a valid key keeps the example
        // parseable; a non-curve "pubkey" is itself rejected as malformed.
        let raw = "[\"P2PK\",{\"nonce\":\"da62796403af76c80cd6ce9153ed3746\",\"data\":\"033281c37677ea273eb7183b783067f5244933ef78d8c3f15b1a77cb246099c26e\",\"tags\":[[\"sigflag\",\"SIG_ALL\"],[\"n_sigs\",\"2\"],[\"locktime\",\"1689418329\"],[\"refund\",\"033281c37677ea273eb7183b783067f5244933ef78d8c3f15b1a77cb246099c26e\",\"02698c4e2b5f9534cd0687d87513c759790cf829aa5739184a3e3735471fbda904\"],[\"pubkeys\",\"023192200a0cfd3867e48eb63b03ff599c7e46c8f4e41146b2d281173ca6c50c54\",\"0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7\"]]}]";
        let conditions = P2pkConditions::from_secret(&Secret::parse(raw).unwrap()).unwrap();
        assert_eq!(conditions.sigflag, SigFlag::SigAll);
        assert_eq!(conditions.num_sigs, Some(2));
        assert_eq!(conditions.locktime, Some(1689418329));
        assert_eq!(conditions.refund_keys.len(), 2);
        assert_eq!(conditions.pubkeys.len(), 2);
        assert_eq!(conditions.required_sigs(), 2);
        assert_eq!(conditions.signing_pubkeys().len(), 3);
    }

    #[test]
    fn rejects_malformed_conditions() {
        let cases: &[(&str, P2pkError)] = &[
            // Unknown sigflag value.
            ("[[\"sigflag\",\"SIG_SOME\"]]", P2pkError::UnknownSigFlag),
            // Zero required signatures.
            ("[[\"n_sigs\",\"0\"]]", P2pkError::ZeroSignatures),
            // n_sigs above the pathway key count.
            (
                "[[\"n_sigs\",\"2\"]]",
                P2pkError::ImpossibleMultisig {
                    required: 2,
                    available: 1,
                },
            ),
            // Non-numeric n_sigs / locktime.
            ("[[\"n_sigs\",\"two\"]]", P2pkError::InvalidTagValue),
            ("[[\"locktime\",\"soon\"]]", P2pkError::InvalidTagValue),
            // Missing tag value.
            ("[[\"sigflag\"]]", P2pkError::InvalidTagValue),
            // Empty tag row.
            ("[[]]", P2pkError::EmptyTag),
            // Duplicate tags (spec mandates rejection).
            (
                "[[\"sigflag\",\"SIG_INPUTS\"],[\"sigflag\",\"SIG_ALL\"]]",
                P2pkError::DuplicateTag,
            ),
            (
                "[[\"n_sigs\",\"1\"],[\"n_sigs\",\"1\"]]",
                P2pkError::DuplicateTag,
            ),
            // Bad pubkey in tags.
            ("[[\"pubkeys\",\"zzz\"]]", P2pkError::InvalidTagValue),
            // n_sigs_refund without refund keys.
            (
                "[[\"n_sigs_refund\",\"1\"]]",
                P2pkError::ImpossibleMultisig {
                    required: 1,
                    available: 0,
                },
            ),
            // Empty refund row is unsatisfiable.
            (
                "[[\"refund\"],[\"n_sigs_refund\",\"1\"]]",
                P2pkError::ImpossibleMultisig {
                    required: 1,
                    available: 0,
                },
            ),
        ];
        for (tags, expected) in cases {
            let error = P2pkConditions::from_secret(&p2pk_secret(Some(tags)))
                .expect_err(&format!("expected rejection for tags {tags}"));
            assert_eq!(&error, expected, "tags {tags}");
        }
    }

    #[test]
    fn rejects_duplicate_keys_by_x_coordinate() {
        // Spec example: 02… and 03… with the SAME x-coordinate are one key.
        let raw = "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"02698c4e2b5f9534cd0687d87513c759790cf829aa5739184a3e3735471fbda904\",\"tags\":[[\"pubkeys\",\"03698c4e2b5f9534cd0687d87513c759790cf829aa5739184a3e3735471fbda904\"]]}]";
        let error = P2pkConditions::from_secret(&Secret::parse(raw).unwrap()).unwrap_err();
        assert_eq!(error, P2pkError::DuplicatePubkey);
    }

    #[test]
    fn allows_same_key_in_both_pathways() {
        // Spec: "The same key **MAY** appear in both pathways."
        let raw = "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"02698c4e2b5f9534cd0687d87513c759790cf829aa5739184a3e3735471fbda904\",\"tags\":[[\"refund\",\"03698c4e2b5f9534cd0687d87513c759790cf829aa5739184a3e3735471fbda904\"],[\"locktime\",\"9999999999\"]]}]";
        let conditions = P2pkConditions::from_secret(&Secret::parse(raw).unwrap()).unwrap();
        assert_eq!(conditions.refund_keys.len(), 1);
    }

    #[test]
    fn htlc_secret_is_kind_mismatch() {
        let raw = "[\"HTLC\",{\"nonce\":\"n\",\"data\":\"5c23fc3aec9d985bd5fc88ca8bceaccc52cf892715dd94b42b84f1b43350751e\"}]";
        let error = P2pkConditions::from_secret(&Secret::parse(raw).unwrap()).unwrap_err();
        assert_eq!(error, P2pkError::KindMismatch);
    }

    #[test]
    fn witness_json_round_trip() {
        let witness = P2pkWitness {
            signatures: vec![
                "60f3c9b766770b46caac1d27e1ae6b77c8866ebaeba0b9489fe6a15a837eaa6fcd6eaa825499c72ac342983983fd3ba3a8a41f56677cc99ffd73da68b59e1383"
                    .to_string(),
            ],
        };
        let encoded = witness.to_json();
        assert_eq!(
            encoded,
            "{\"signatures\":[\"60f3c9b766770b46caac1d27e1ae6b77c8866ebaeba0b9489fe6a15a837eaa6fcd6eaa825499c72ac342983983fd3ba3a8a41f56677cc99ffd73da68b59e1383\"]}"
        );
        assert_eq!(P2pkWitness::from_json(&encoded), Some(witness));

        // The spec's inline witness example parses.
        let spec_form = "{\"signatures\":[\"60f3c9b766770b46caac1d27e1ae6b77c8866ebaeba0b9489fe6a15a837eaa6fcd6eaa825499c72ac342983983fd3ba3a8a41f56677cc99ffd73da68b59e1383\"]}";
        assert!(P2pkWitness::from_json(spec_form).is_some());
    }

    #[test]
    fn witness_rejects_non_p2pk_shapes() {
        assert_eq!(P2pkWitness::from_json("{}"), None);
        assert_eq!(
            P2pkWitness::from_json("{\"signatures\":\"not-an-array\"}"),
            None
        );
        assert_eq!(P2pkWitness::from_json("{\"signatures\":[1]}"), None);
        assert_eq!(P2pkWitness::from_json("not json"), None);
        // Extra keys are ignored (upstream serde ignores unknown fields).
        assert!(P2pkWitness::from_json("{\"signatures\":[],\"preimage\":\"00\"}").is_some());
        // Empty signatures array parses (verification then fails counts).
        assert_eq!(
            P2pkWitness::from_json("{\"signatures\":[]}"),
            Some(P2pkWitness::default())
        );
    }
}
