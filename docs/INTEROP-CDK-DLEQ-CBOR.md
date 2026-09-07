# CDK CBOR DLEQ serialization deviation

Found while building the nucula atom-rig relay (2026-09-07).

## Finding

CDK's V4 token CBOR serialization writes DLEQ scalars (e/s/r) as
**32-element arrays of small integers** (the secp256k1 crate's serde
binary path emits `SecretKey` as a tuple), not as the **byte strings**
the NUT-00 V4 spec defines.

Spec (nuts/00.md, V4 token encoding):
> `"d"` is a map with `"e"`, `"s"`, `"r"` as byte strings

cashu-core-lite follows the spec (nut12.rs: byte string encode/decode).
CDK deviates because it relies on the default serde derive for
`ProofDleq` — only the keyset ID and the C point have custom V4
serializers in CDK's token.rs; the DLEQ fields do not.

## Impact

- CDK-written raw CBOR V4 tokens with DLEQ will fail
  `cashu_core_lite::token::decode_token` (expects bytes, gets array)
- cashu-core-lite-written raw CBOR V4 tokens with DLEQ may fail
  CDK's deserializer (expects the secp256k1 tuple form)
- The walletport CDK interop test does NOT exercise this path:
  it constructs tokens with cashu-core-lite's own encoder from CDK
  artifacts (walletport/tests/cdk_interop.rs:112-118)

## Verification needed

Round-trip a CDK `Token::to_raw_bytes()` output through
`cashu_core_lite::token::decode_token` and observe the failure.

## References

- secp256k1-0.29.1/src/key.rs:361-369 (tuple serialization in binary mode)
- cdk/crates/cashu/src/nuts/nut00/token.rs (no custom DLEQ serializer)
- cashu-core-lite/src/nuts/nut12.rs (spec-compliant byte strings)
- cashu-core-lite/nuts/00.md:266-270 (spec quote)

Per AGENTS.md policy: file an upstream issue only after human review.
