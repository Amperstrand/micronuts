//! Emitter token corpus (#78): real tokens from the wild, pinned so
//! each emitter's wire quirks fail loudly here instead of on a bench
//! take. Every entry records provenance; envelopes synthesized around
//! bench-proven fields say so.
//!
//! Vacant slots (need a bench session against conwrt-bench
//! data/mint-pool to harvest a genuine token): cashu-ts v4, nutshell
//! V3/V4, cdk-cli token with real signatures. Add them as further
//! entries — never by relaxing an assertion.

use cashu_core_lite::token::{encode_token_wire, Proof as TokenProof, TokenV4, TokenV4Token};
use micronuts_wallet_core::token_compat::decode_token_any;

struct CorpusEntry {
    emitter: &'static str,
    provenance: &'static str,
    token: &'static str,
    mint: &'static str,
    unit: &'static str,
    memo: Option<&'static str>,
    /// (keyset id as decoded, proof count, amount sum) per token group.
    groups: &'static [(&'static str, usize, u64)],
}

const CORPUS: &[CorpusEntry] = &[
    CorpusEntry {
        emitter: "PRTA minter (cashu-ts-derived)",
        provenance: "bench take21 2026-09-27 — verbatim",
        token: "cashuAeyJ0b2tlbiI6W3sibWludCI6Imh0dHA6Ly8xOTIuMTY4LjEzLjIyMTo4MzgzIiwicHJvb2ZzIjpbXSwiaWQiOiJ4In1dLCJ1bml0Ijoic2F0In0",
        mint: "http://192.168.13.221:8383",
        unit: "sat",
        memo: None,
        // Quirk: a proof-less V3 group yields no token group at all.
        groups: &[],
    },
    CorpusEntry {
        emitter: "PRTA minter — truncated keyset id",
        provenance:
            "ids verbatim from the bench active keyset; envelope synthesized around them",
        token: "cashuAeyJ0b2tlbiI6W3sibWludCI6Imh0dHA6Ly8xOTIuMTY4LjEzLjIyMTo4MzgzIiwicHJvb2ZzIjpbeyJpZCI6IjAxZGY5N2I2ZmI4YTU3MmEiLCJhbW91bnQiOjIxLCJzZWNyZXQiOiI1IHNob3J0LWlkIiwiQyI6IjAyMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTEifV19XSwidW5pdCI6InNhdCJ9",
        mint: "http://192.168.13.221:8383",
        unit: "sat",
        memo: None,
        groups: &[("01df97b6fb8a572a", 1, 21)],
    },
    CorpusEntry {
        emitter: "cdk-cli shape — padded standard base64",
        provenance: "envelope synthesized (padded encoding + full 64-hex id)",
        token: "cashuAeyJ0b2tlbiI6W3sibWludCI6Imh0dHA6Ly8xMjcuMC4wLjE6MzAzMCIsInByb29mcyI6W3siaWQiOiIwMWRmOTdiNmZiOGE1NzJhNzE4ZDdkZjdmY2JmNDM4N2UyZDQ1NTEzNGVhODAwNGM5YzhjNTFlMWIzMzkxZjkwOWUiLCJhbW91bnQiOjQsInNlY3JldCI6ImNkayBwYWRkZWQgYSIsIkMiOiIwMjExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExIn0seyJpZCI6IjAxZGY5N2I2ZmI4YTU3MmE3MThkN2RmN2ZjYmY0Mzg3ZTJkNDU1MTM0ZWE4MDA0YzljOGM1MWUxYjMzOTFmOTA5ZSIsImFtb3VudCI6MSwic2VjcmV0IjoiY2RrIHBhZGRlZCBiIiwiQyI6IjAyMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTEifV19XSwidW5pdCI6InNhdCIsIm1lbW8iOiJjZGstY2xpIHNoYXBlIn0=",
        mint: "http://127.0.0.1:3030",
        unit: "sat",
        memo: Some("cdk-cli shape"),
        groups: &[(
            "01df97b6fb8a572a718d7df7fcbf4387e2d455134ea8004c9c8c51e1b3391f909e",
            2,
            5,
        )],
    },
];

#[test]
fn corpus_decodes_with_emitter_quirks_pinned() {
    for (i, e) in CORPUS.iter().enumerate() {
        println!("corpus[{i}]: {} — {}", e.emitter, e.provenance);
        let token = decode_token_any(e.token).unwrap_or_else(|err| {
            panic!("{}: decode failed: {err}", e.emitter);
        });
        assert_eq!(token.mint, e.mint, "{}: mint", e.emitter);
        assert_eq!(token.unit, e.unit, "{}: unit", e.emitter);
        assert_eq!(token.memo.as_deref(), e.memo, "{}: memo", e.emitter);
        assert_eq!(
            token.tokens.len(),
            e.groups.len(),
            "{}: group count",
            e.emitter
        );
        for (group, want) in token.tokens.iter().zip(e.groups) {
            assert_eq!(group.keyset_id, want.0, "{}: keyset id", e.emitter);
            assert_eq!(group.proofs.len(), want.1, "{}: proof count", e.emitter);
            let sum: u64 = group.proofs.iter().map(|p| p.amount).sum();
            assert_eq!(sum, want.2, "{}: amount sum", e.emitter);
        }
    }
}

/// The decoder must keep accepting our own canonical V4 wire form —
/// guards against the compat layer drifting from the core encoder.
#[test]
fn corpus_self_emitted_v4_round_trips() {
    let c = [0x02u8]
        .iter()
        .chain([0x11u8; 32].iter())
        .copied()
        .collect::<Vec<u8>>();
    let token = TokenV4 {
        mint: String::from("http://127.0.0.1:3030"),
        unit: String::from("sat"),
        memo: Some(String::from("self")),
        tokens: vec![TokenV4Token {
            keyset_id: String::from("00deadbeef"),
            proofs: vec![TokenProof {
                amount: 21,
                keyset_id: String::from("00deadbeef"),
                secret: String::from("self v4"),
                c: c.clone(),
                dleq: None,
            }],
        }],
    };
    let wire = encode_token_wire(&token).expect("encode");
    let back = decode_token_any(&wire).expect("round-trip decode");
    assert_eq!(back.mint, "http://127.0.0.1:3030");
    assert_eq!(back.tokens[0].keyset_id, "00deadbeef");
    assert_eq!(back.tokens[0].proofs[0].amount, 21);
    assert_eq!(back.tokens[0].proofs[0].c, c);
}
