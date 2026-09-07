//! NUT-10: Spending Conditions — the well-known `Secret` model (L0).
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/10.md
//!
//! Decision record (JSON-in-secret): `cashu-core-lite` stays free of a
//! serde_json dependency (no_std, minicbor-only), but NUT-10 secrets are
//! JSON embedded in the `proof.secret` string. The typed model AND its JSON
//! codec live here, backed by the crate-private [`super::json`] scanner;
//! `micronuts-mint` consumes the parsed model (the MintService/CBOR-RPC
//! boundary stays pure cashu-core-lite types — PARITY.md invariant). The
//! signature-verification ENGINE deliberately lives in the mint, which
//! already delegates crypto to the upstream `cashu` crate.
//!
//! Fail-open / fail-closed contract (mirrors upstream `cashu` 0.18 and the
//! spec Caution note): a secret that does not parse as the well-known
//! format — or whose `kind` this mint does not know — is an ordinary
//! anyone-can-spend secret. A secret that parses with kind `P2PK` but then
//! violates NUT-11 rules (see [`super::nut11`]) is REJECTED as unspendable;
//! a malformed condition must never fall back to anyone-can-spend.

#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::json;

/// Kind of a NUT-10 spending condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// NUT-11 Pay-to-Public-Key.
    P2PK,
    /// NUT-14 Hashed Timelock Contract — reserved for L3 (roadmap #51):
    /// parsed for recognition, but the mint rejects HTLC-locked proofs as
    /// unspendable until preimage verification lands.
    Htlc,
}

impl SecretKind {
    /// The wire string used as the first element of the well-known Secret.
    pub fn as_str(&self) -> &'static str {
        match self {
            SecretKind::P2PK => "P2PK",
            SecretKind::Htlc => "HTLC",
        }
    }

    fn from_wire(s: &str) -> Option<Self> {
        match s {
            "P2PK" => Some(SecretKind::P2PK),
            "HTLC" => Some(SecretKind::Htlc),
            _ => None,
        }
    }
}

/// The NUT-10 well-known `Secret` parsed out of a `Proof.secret` string.
///
/// Syntax-level only: this says "the secret IS a spending condition of this
/// kind with these raw fields". Whether the condition is WELL-FORMED (valid
/// pubkeys, known sigflags, sane multisig thresholds) is NUT-11/14 layer
/// validation ([`super::nut11::P2pkConditions::from_secret`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret {
    /// Kind of the spending condition.
    pub kind: SecretKind,
    /// Unique random string.
    pub nonce: String,
    /// Spending-condition-specific data (for P2PK: hex compressed pubkey).
    pub data: String,
    /// Optional tags: arrays of one-or-more strings (key first).
    ///
    /// `None` means the `tags` field was absent (or JSON `null`); an empty
    /// row inside `Some(..)` is preserved for the NUT-11 layer to reject,
    /// matching upstream's split between `SecretData` deserialization and
    /// `Tag` validation.
    pub tags: Option<Vec<Vec<String>>>,
}

impl Secret {
    // NUT #10: Spending conditions are expressed in a well-known secret format that is revealed to the mint when spending (unlocking) a token, not when the token is minted (locked). The mint parses each `Proof`'s `secret`. If it can deserialize it into the following format it executes additional spending conditions that are further specified in additional NUTs.
    // NUT #10: The well-known `Secret` stored in `Proof.secret` is a JSON of the format:
    // NUT #10: [
    // NUT #10: kind <str>,
    // NUT #10:   {
    // NUT #10:     "nonce": <str>,
    // NUT #10:     "data": <str>,
    // NUT #10:     "tags": [[ "key", "value1", "value2", ...],  ... ], // (optional)
    // NUT #10:   }
    // NUT #10: ]
    // NUT #10: - `kind` is the kind of the spending condition
    // NUT #10: - `nonce` is a unique random string
    // NUT #10: - `data` expresses the spending condition specific to each kind
    // NUT #10: - `tags` hold additional data committed to and can be used for feature extensions

    /// Parse the raw `proof.secret` string into the well-known Secret.
    ///
    /// Returns `None` for ordinary (non-condition) secrets — including JSON
    /// that is not exactly the two-element well-known shape, an unknown
    /// `kind`, or tag rows that are not arrays of strings — so the caller
    /// treats the proof as anyone-can-spend (see the module docs and the
    /// Caution note below).
    // NUT #10: Caution: If the mint does not support spending conditions or a specific `kind` of spending condition, proofs may be treated as a regular anyone-can-spend tokens. Applications need to make sure to check whether the mint supports a specific `kind` of spending condition by checking the mint's [info][06] endpoint.
    pub fn parse(raw: &str) -> Option<Secret> {
        let document = json::parse(raw)?;
        let array = document.as_array()?;
        if array.len() != 2 {
            // Upstream's serde visitor requires exactly two elements
            // (`invalid_length` on 0/1/3+); mirror that strictness.
            return None;
        }
        let kind = SecretKind::from_wire(array[0].as_str()?)?;
        let object = array[1].as_object()?;
        let nonce = object
            .iter()
            .find(|(k, _)| k == "nonce")
            .and_then(|(_, v)| v.as_str())?
            .to_string();
        let data = object
            .iter()
            .find(|(k, _)| k == "data")
            .and_then(|(_, v)| v.as_str())?
            .to_string();
        let tags = match object.iter().find(|(k, _)| k == "tags") {
            None => None,
            Some((_, value)) => {
                // `{"tags": null}` deserializes to `None` upstream (serde
                // Option semantics); keep that equivalence.
                if matches!(value, super::json::Json::Null) {
                    None
                } else {
                    Some(parse_tag_rows(value.as_array()?)?)
                }
            }
        };
        Some(Secret {
            kind,
            nonce,
            data,
            tags,
        })
    }
}

/// Convert a tags JSON array into typed rows.
///
/// Every row must be an array of strings (possibly empty — emptiness is a
/// NUT-11-level malformation, not a syntax error, mirroring upstream's
/// `Vec<Vec<String>>` deserialization followed by `Tag::try_from`).
fn parse_tag_rows(rows: &[super::json::Json]) -> Option<Vec<Vec<String>>> {
    let mut parsed = Vec::with_capacity(rows.len());
    for row in rows {
        let row = row.as_array()?;
        let mut strings = Vec::with_capacity(row.len());
        for item in row {
            strings.push(item.as_str()?.to_string());
        }
        parsed.push(strings);
    }
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The basic-case P2PK secret from NUT-11 §Basic Case.
    const SPEC_BASIC: &str = "[\"P2PK\",{\"nonce\":\"859d4935c4907062a6297cf4e663e2835d90d97ecdd510745d32f6816323a41f\",\"data\":\"0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7\",\"tags\":[[\"sigflag\",\"SIG_INPUTS\"]]}]";

    #[test]
    fn parses_spec_basic_case() {
        let secret = Secret::parse(SPEC_BASIC).unwrap();
        assert_eq!(secret.kind, SecretKind::P2PK);
        assert_eq!(
            secret.nonce,
            "859d4935c4907062a6297cf4e663e2835d90d97ecdd510745d32f6816323a41f"
        );
        assert_eq!(
            secret.data,
            "0249098aa8b9d2fbec49ff8598feb17b592b986e62319a4fa488a3dc36387157a7"
        );
        assert_eq!(
            secret.tags,
            Some(vec![vec!["sigflag".to_string(), "SIG_INPUTS".to_string()]])
        );
    }

    #[test]
    fn parses_complex_example_with_all_tags() {
        // The spec §Complex Example (2-of-3 with refund keys and locktime).
        let raw = "[\"P2PK\",{\"nonce\":\"da62796403af76c80cd6ce9153ed3746\",\"data\":\"033281c37677ea273eb7183b783067f5244933ef78d8c3f15b1a77cb246099c26e\",\"tags\":[[\"sigflag\",\"SIG_ALL\"],[\"n_sigs\",\"2\"],[\"locktime\",\"1689418329\"],[\"refund\",\"033281c37677ea273eb7183b783067f5244933ef78d8c3f15b1a77cb246099c26e\",\"02e2aeb97f47690e3c418592a5bcda77282d1339a3017f5558928c2441b7731d50\"],[\"pubkeys\",\"02698c4e2b5f9534cd0687d87513c759790cf829aa5739184a3e3735471fbda904\",\"023192200a0cfd3867e48eb63b03ff599c7e46c8f4e41146b2d281173ca6c50c54\"]]}]";
        let secret = Secret::parse(raw).unwrap();
        assert_eq!(secret.kind, SecretKind::P2PK);
        let tags = secret.tags.unwrap();
        assert_eq!(tags.len(), 5);
        assert_eq!(tags[0], vec!["sigflag", "SIG_ALL"]);
        assert_eq!(tags[2], vec!["locktime", "1689418329"]);
        assert_eq!(tags[3].len(), 3, "refund tag: key + 2 pubkeys");
    }

    #[test]
    fn ordinary_secrets_return_none() {
        for raw in [
            "",
            "deadbeef",
            "0123456789abcdef0123456789abcdef",
            "{}",
            "[1,2]",
            "[\"P2PK\"]",
            "[\"P2PK\",{\"nonce\":\"n\"}]",
            "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\"},\"extra\"]",
            "[\"P2PK\",\"not-an-object\"]",
            "[\"P2PK\",{\"nonce\":1,\"data\":\"d\"}]",
            "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\",\"tags\":\"nope\"}]",
            "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\",\"tags\":[[\"sigflag\",1]]}]",
            "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\",\"tags\":[\"flat\"]}]",
            "[\"FOO\",{\"nonce\":\"n\",\"data\":\"d\"}]",
        ] {
            assert_eq!(Secret::parse(raw), None, "expected None for {raw:?}");
        }
    }

    #[test]
    fn tags_field_is_optional_and_null_tolerant() {
        let no_tags = "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\"}]";
        assert_eq!(Secret::parse(no_tags).unwrap().tags, None);
        let null_tags = "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\",\"tags\":null}]";
        assert_eq!(Secret::parse(null_tags).unwrap().tags, None);
        let empty_tags = "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\",\"tags\":[]}]";
        assert_eq!(Secret::parse(empty_tags).unwrap().tags, Some(Vec::new()));
    }

    #[test]
    fn htlc_kind_is_recognized_but_reserved() {
        let raw = "[\"HTLC\",{\"nonce\":\"n\",\"data\":\"5c23fc3aec9d985bd5fc88ca8bceaccc52cf892715dd94b42b84f1b43350751e\"}]";
        let secret = Secret::parse(raw).unwrap();
        assert_eq!(secret.kind, SecretKind::Htlc);
    }

    #[test]
    fn unknown_object_keys_are_ignored_and_escapes_decode() {
        let raw = "[\"P2PK\",{\"nonce\":\"a\\\"b\",\"data\":\"d\",\"future\":[1,{\"x\":true}],\"tags\":[[\"custom\",\"v\"]]}]";
        let secret = Secret::parse(raw).unwrap();
        assert_eq!(secret.nonce, "a\"b");
        assert_eq!(
            secret.tags,
            Some(vec![vec!["custom".to_string(), "v".to_string()]])
        );
    }

    /// Differential (L0): our parse agrees with the upstream `cashu` crate's
    /// `Secret` on accept/reject and on the extracted fields.
    #[test]
    fn agrees_with_upstream_cashu_secret() {
        let vectors = [
            SPEC_BASIC,
            "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\"}]",
            "[\"HTLC\",{\"nonce\":\"n\",\"data\":\"5c23fc3aec9d985bd5fc88ca8bceaccc52cf892715dd94b42b84f1b43350751e\"}]",
            "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\",\"tags\":[]}]",
            // Upstream also rejects these:
            "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"d\"},3]",
            "[\"P2PK\",{\"nonce\":1,\"data\":\"d\"}]",
            "[\"HTLC\",{\"nonce\":\"n\",\"data\":\"d\",\"tags\":[[\"locktime\",1765300829]]}]",
            "plain-secret",
            "[\"P2PK\", 42]",
        ];
        for raw in vectors {
            let ours = Secret::parse(raw);
            let upstream = raw
                .parse::<cashu::secret::Secret>()
                .ok()
                .and_then(|s| cashu::nuts::nut10::Secret::try_from(s).ok());
            assert_eq!(ours.is_some(), upstream.is_some(), "divergence on {raw:?}");
            if let (Some(ours), Some(upstream)) = (ours, upstream) {
                assert_eq!(
                    ours.kind.as_str(),
                    match upstream.kind() {
                        cashu::nuts::nut10::Kind::P2PK => "P2PK",
                        cashu::nuts::nut10::Kind::HTLC => "HTLC",
                    }
                );
                assert_eq!(ours.nonce, upstream.secret_data().nonce());
                assert_eq!(ours.data, upstream.secret_data().data());
                assert_eq!(ours.tags, upstream.secret_data().tags().cloned());
            }
        }
    }
}
