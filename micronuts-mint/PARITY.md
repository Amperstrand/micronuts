# PARITY — `micronuts-mint` ↔ upstream `cashu` crate (v0.18.0)

`micronuts-mint` implements the Cashu mint role for the Micronuts demo using
the upstream [`cashu`](https://crates.io/crates/cashu) crate (v0.18.0) for all
Diffie-Hellman / blind-signature crypto, while still exposing the stable
`MintService` trait (defined in `cashu-core-lite/src/rpc.rs`) that returns
`cashu-core-lite` types. A small conversion layer in `src/type_conversion.rs`
bridges the two type universes at the trait boundary.

This file maps each `cashu` API surface used by `micronuts-mint` to its
`cashu-core-lite` equivalent (or notes when only one of the two has a
given primitive). Future Wave 4 tasks should consult this table before
extending the mint.

## Crypto primitives (`dhke`)

| Operation | `cashu` (upstream) | `cashu-core-lite` (firmware) | Notes |
|-----------|--------------------|------------------------------|-------|
| Hash message to curve | `cashu::dhke::hash_to_curve(&[u8]) -> Result<cashu::PublicKey, cashu::dhke::Error>` | `cashu_core_lite::crypto::hash_to_curve(&[u8]) -> Result<PublicKey, HashToCurveError>` | Identical algorithm: `02 || SHA256(SHA256("Secp256k1_HashToCurve_Cashu_" ‖ msg) ‖ counter_le32)` for `counter ∈ 0..=u16::MAX`. Byte-for-byte equal outputs (verified by `cashu_compat.rs::hash_to_curve_random_matches_upstream_cashu`, 128 iters). |
| Blind a secret | `cashu::dhke::blind_message(&[u8], Option<cashu::SecretKey>) -> Result<(cashu::PublicKey, cashu::SecretKey), Error>` | `cashu_core_lite::crypto::blind_message(&[u8], Option<SecretKey>) -> Result<BlindedMessage, HashToCurveError>` | Computes `B_ = Y + rG`. cashu returns a tuple; lite returns a struct `{ blinded, blinder }`. |
| Sign blinded message | `cashu::dhke::sign_message(&cashu::SecretKey, &cashu::PublicKey) -> Result<cashu::PublicKey, Error>` | `cashu_core_lite::crypto::sign_message(&SecretKey, &PublicKey) -> PublicKey` | Computes `C_ = k · B_`. cashu is fallible (`Result`); lite is infallible (always-valid point). |
| Unblind signature | `cashu::dhke::unblind_message(&cashu::PublicKey, &cashu::SecretKey, &cashu::PublicKey) -> Result<cashu::PublicKey, Error>` | `cashu_core_lite::crypto::unblind_signature(&PublicKey, &SecretKey, &PublicKey) -> Result<PublicKey, ()>` | Computes `C = C_ − rK`. Different argument order: cashu takes `(C_, r, K)`; lite takes `(&C_, &r, &K)`. |
| Verify (privkey path) | `cashu::dhke::verify_message(&cashu::SecretKey, cashu::PublicKey, &[u8]) -> Result<(), Error>` | `cashu_core_lite::crypto::verify_signature_with_privkey(&[u8], &PublicKey, &SecretKey) -> Result<bool, HashToCurveError>` | Both check `a · hash_to_curve(msg) == C`. cashu returns `Ok(())` on success / `Err(TokenNotVerified)` on failure; lite returns `Ok(bool)`. **Argument order differs**: cashu `(a, C, msg)`, lite `(msg, C, a)`. |
| Verify (pubkey DLEQ path) | (internal to `cashu::dhke`) | `cashu_core_lite::crypto::verify_signature(...) -> Result<bool, DleqError>` | NUT-12 DLEQ. Not used by `micronuts-mint` (mint holds privkey, uses privkey path). |

## Key types

| Concept | `cashu` (upstream) | `cashu-core-lite` (firmware) | Conversion |
|---------|--------------------|------------------------------|------------|
| Public key | `cashu::nuts::nut01::PublicKey` (wraps `secp256k1::PublicKey` via `bitcoin` crate) | `cashu_core_lite::keypair::PublicKey` (wraps `k256::PublicKey`) | `cashu_pk_to_lite` / `lite_pk_to_cashu` via 33-byte compressed SEC1 |
| Secret key | `cashu::nuts::nut01::SecretKey` (wraps `bitcoin::secp256k1::SecretKey`) | `cashu_core_lite::keypair::SecretKey` (wraps `k256::SecretKey`) | `cashu_sk_to_lite` / `lite_sk_to_cashu` via 32-byte secret scalar |
| Public key bytes | `cashu::PublicKey::to_bytes() -> [u8; 33]` | `cashu_core_lite::PublicKey::to_bytes() -> [u8; 33]` | Identical wire format |
| Secret key bytes | `cashu::SecretKey::to_secret_bytes() -> [u8; 32]` | `cashu_core_lite::SecretKey::to_secret_bytes() -> [u8; 32]` | Identical wire format |
| Public key from bytes | `cashu::PublicKey::from_slice(&[u8]) -> Result<Self, Error>` | `cashu_core_lite::PublicKey::from_bytes(&[u8; 33]) -> Option<Self>` | cashu takes slice; lite takes fixed array |
| Secret key from bytes | `cashu::SecretKey::from_slice(&[u8]) -> Result<Self, Error>` | `cashu_core_lite::SecretKey::from_slice(&[u8]) -> Result<Self, k256::Error>` | Both take slices; different error types |
| Derive pubkey from secret | `cashu::SecretKey::public_key() -> cashu::PublicKey` | `cashu_core_lite::SecretKey::public_key() -> PublicKey` | Same scalar→point op |
| Pubkey hex encoding | `cashu::PublicKey::to_hex() -> String` | `hex::encode(pk.to_encoded_point(true).as_bytes())` | lite uses generic `hex` crate; cashu has convenience method |

## NUT-02 keyset derivation

| Operation | `cashu` | `cashu-core-lite` | Notes |
|-----------|---------|-------------------|-------|
| Derive v1 keyset ID | `cashu::nuts::nut02::Id::v1_from_keys(&Keys) -> Id` (returns `Id` struct, `Display` gives 16-hex string) | `cashu_core_lite::nuts::nut02::derive_keyset_id(&[PublicKey]) -> String` (returns 16-hex string directly) | Same algorithm: SHA-256 of concatenated 33-byte compressed pubkeys, take first 7 bytes, hex-encode to 14 chars, prefix `"00"`. `micronuts-mint` uses the lite variant directly (keyset.rs stores lite pubkeys). |

## NUT types used by the mint

`micronuts-mint` accepts and returns `cashu-core-lite` NUT types at the
`MintService` trait boundary. The table below maps the lite types to their
`cashu` equivalents for cross-reference. **No conversion is performed on
these types** — the trait boundary stays pure `cashu-core-lite`.

| Concept | `cashu-core-lite` (used) | `cashu` equivalent |
|---------|--------------------------|--------------------|
| Blinded message | `nut00::BlindedMessage { amount: u64, b: PublicKey }` | `cashu::nuts::nut00::BlindedMessage { amount: Amount, keyset_id: Id, blinded_secret: PublicKey, witness: Option<Witness> }` |
| Blind signature | `nut00::BlindSignature { amount: u64, id: String, c: PublicKey }` | `cashu::nuts::nut00::BlindSignature { amount: Amount, keyset_id: Id, c: PublicKey, dleq: Option<BlindSignatureDleq> }` |
| Proof | `nut00::Proof { amount: u64, id: String, secret: String, c: PublicKey }` | `cashu::nuts::nut00::Proof { amount: Amount, keyset_id: Id, secret: Secret, c: PublicKey, witness, dleq, p2pk_e }` |
| Public keyset | `nut01::KeySet { id: String, unit: String, keys: Vec<KeyPair> }` | `cashu::nuts::nut01::KeysResponse { keysets: Vec<KeySet> }` (wrapping `nut02::KeySet`) |
| Keyset metadata | `nut02::KeysetInfo { id: String, unit: String, active: bool, input_fee_ppk: u64 }` | `cashu::nuts::nut02::KeySetInfo { id: Id, unit: CurrencyUnit, active: bool, input_fee_ppk: u64, final_expiry: Option<u64> }` |
| Mint quote req | `nut04::MintQuoteRequest { amount: u64, unit: String, pubkey: Option<String> }` | `cashu::nuts::nut04::MintQuoteBolt11Request { amount, unit, pubkey: Option<PublicKey> }` | NUT-20 quote-locking pubkey (lite carries the point as validated lowercase hex) |
| Mint quote resp | `nut04::MintQuoteResponse { quote, request, paid, state, expiry, pubkey: Option<String> }` | `cashu::nuts::nut04::MintQuoteBolt11Response { …, pubkey: Option<PublicKey> }` | NUT-20: the response echoes the request pubkey |
| Mint request | `nut04::MintRequest { quote, outputs, signature: Option<String> }` | `cashu::nuts::nut04::MintRequest<Q> { quote, outputs, signature: Option<String> }` | NUT-20 BIP-340 signature over `nut20::quote_sig_message` |
| Batch check req | `nut29::BatchCheckMintQuoteRequest { quotes: Vec<String> }` | `cashu::nuts::nut29::BatchCheckMintQuoteRequest<Q> { quotes: Vec<Q> }` | NUT-29 |
| Batch mint req | `nut29::BatchMintRequest { quotes, quote_amounts, outputs, signatures }` | `cashu::nuts::nut29::BatchMintRequest<Q>` | NUT-29; `signatures: Option<Vec<Option<String>>>` (null per unlocked quote) |
| Mint response | `nut04::MintResponse { signatures }` | `cashu::nuts::nut04::MintBolt11Response { signatures }` |
| Melt quote req | `nut05::MeltQuoteRequest { request, unit }` | `cashu::nuts::nut05::MeltQuoteBolt11Request { request, unit }` |
| Melt quote resp | `nut05::MeltQuoteResponse { quote, amount, fee_reserve, paid, state, expiry }` | `cashu::nuts::nut05::MeltQuoteBolt11Response { quote, amount, fee_reserve, paid, state, expiry, payment_preimage }` |
| Melt request | `nut05::MeltRequest { quote, inputs, outputs }` | `cashu::nuts::nut05::MeltBolt11Request { quote, inputs, outputs }` |
| Melt response | `nut05::MeltResponse { paid, state, payment_preimage, change }` | `cashu::nuts::nut05::MeltBolt11Response { paid, state, payment_preimage, change }` |
| Swap | `nut03::SwapRequest { inputs, outputs }` / `SwapResponse { signatures }` | `cashu::nuts::nut03::SwapRequest { inputs, outputs }` / `SwapResponse { signatures }` |
| Check state | `nut07::CheckStateRequest { ys: Vec<PublicKey> }` / `CheckStateResponse { states: Vec<ProofState> }` | `cashu::nuts::nut07::CheckStateRequest { ys: Vec<PublicKey> }` / `CheckStateResponse { states: Vec<ProofState> }` |
| Mint info | `nut06::MintInfo { name, pubkey, version, description, contact, nuts }` | `cashu::nuts::nut06::MintInfo { name, pubkey, version, description, description_long, contact, motd, icon_url, urls, nuts, ... }` | cashu has many more optional fields; lite has the minimum required for the demo |

## State strings (NUT-04 / NUT-05)

| State | `cashu` enum | `cashu-core-lite` string |
|-------|--------------|---------------------------|
| Unpaid | `cashu::nuts::nut23::QuoteState::Unpaid` → `"UNPAID"` | `nut04::state::UNPAID` / `nut05::state::UNPAID` |
| Paid | `cashu::nuts::nut23::QuoteState::Paid` → `"PAID"` | `nut04::state::PAID` / `nut05::state::PAID` |
| Issued (NUT-04 only) | `cashu::nuts::nut23::QuoteState::Issued` → `"ISSUED"` | `nut04::state::ISSUED` |

(Verified by `cashu_compat.rs::quote_state_strings_match_upstream_cashu`.)

## What `micronuts-mint` does NOT use from `cashu`

Per the original Wave 4 task scope, the following were deliberately out of
scope (updated 2026-09-02 — several have since landed, see below):

- **`cdk::Mint`** (full mint server runtime) — too heavy. We use only the
  `cashu` crate's crypto + types, not the Cashu Development Kit server.
- **`cashu::Amount`** wrapper — lite uses plain `u64`. No conversion needed
  because the trait boundary uses lite types end-to-end.
- **`cashu::nuts::nut02::Id`** struct — lite uses a 16-char hex `String`.
  Conversion is trivial (just `to_string()` on the cashu side).
- **`cashu::Secret`** wrapper — lite stores secrets as plain hex `String`.

Landed since (backend-driven rework 2026-09-02):

- **Quote state machines** — NUT-04 UNPAID→PAID→ISSUED with lazy backend
  settlement + NUT-04 accounting fields (`amount`, `amount_paid`,
  `amount_issued`, `updated_at`, partial-mint support); NUT-05
  UNPAID→PENDING→PAID/FAILED with proof rollback on payment failure.
- **NUT-08** — input fees `(sum_ppk+999)/1000` enforced in swap and melt;
  melt change supports explicit outputs AND blank-output imprinting
  (power-of-two decomposition, the cashu-ts v4 path).
- **NUT-09** — real session-scoped restore via the B_→signature index.
- **DLEQ proofs (NUT-12)** — construction IS implemented via
  `cashu::BlindSignature::new` (upstream crypto path).
- **NUT-10/11 (L1+L2, #51)** — well-known Secret model + P2PK witness
  verification on swap/melt inputs (SIG_INPUTS/SIG_ALL, multisig via
  `pubkeys`/`n_sigs`, x-coordinate dedup), enforced before `claim_proofs`;
  locktime pathways gated by the injectable `MintClock`: pre-expiry the
  primary pathway only, post-expiry the refund pathway opens additionally
  (refund keys, `n_sigs_refund`; expired + no refund keys =
  anyone-can-spend), boundary `locktime < now` strict as upstream;
  differential tests vs upstream in `tests/p2pk_differential.rs` including
  frozen-clock boundary tests (MockClock).
- **NUT-14 (L3, #51)** — HTLC locks (hash lock in `data`, receiver
  pathway = preimage + optional `pubkeys` signatures, always available;
  sender pathway = refund keys after `locktime`; expired + no refund keys
  = anyone-can-spend) enforced on both SIG_INPUTS and SIG_ALL message
  paths; differential tests vs upstream in `tests/htlc_differential.rs`.
  Spec-wins divergence (documented in `spending.rs`): a signatures-only
  witness may spend the sender pathway, which upstream's untagged witness
  enum rejects in SIG_INPUTS mode.
- **Payment safety** — atomic batch double-spend rejection, keyset binding,
  spend-before-sign ordering.

- **NUT-20 (quote locking, 2026-09-07)** — mint quotes accept `pubkey`
  (stored on the quote, echoed in every quote response); `post_mint`
  rejects a locked quote without a valid BIP-340 signature by that key.
  The signed message is `cashu_core_lite::nuts::nut20::quote_sig_message`
  (domain-separated, length-prefixed, minimal-BE amounts) — byte-identical
  to upstream `MintRequest::msg_to_sign`; verification via
  `cashu::PublicKey::verify` (SHA-256 internal), differential tests in
  `tests/nut20_nut29_differential.rs`. Unlocked quotes ignore a stray
  signature (upstream behavior).
- **NUT-29 (batch minting, 2026-09-07)** — `POST /v1/mint/quote/bolt11/check`
  (order-preserving, all-or-nothing) and `POST /v1/mint/bolt11/batch`
  (spec-ordered validation: non-empty, unique, existing, same unit,
  PAID, `Σ outputs == Σ quote amounts`, per-quote NUT-20 signatures over
  ALL outputs). Limits `nut29::MAX_BATCH_QUOTES = 50` (advertised as
  `max_batch_size`) and `nut29::MAX_BATCH_OUTPUTS = 1000`; batch-mint
  success responses carry `BlindSignature` objects per the normative text
  (the spec's string-array example is stale — documented divergence).
- **NUT-19 (cache advertisement, 2026-09-07)** — the mint advertises
  `{"19": {ttl: null, cached_endpoints: [POST /v1/mint/bolt11, POST
  /v1/swap]}}`; the actual response cache lives at the HTTP edge
  (micronuts-audit-adapter: method+path+payload keyed, 200 responses
  replayed verbatim).

Still not implemented: multiple keysets/rotation, fee_reserve from a real
backend, async melt polling (PENDING is resolved within the single
post_melt call). The #51 spending-conditions arc is complete (L0-L3). Durable
persistence SHIPPED for the host mint and the
upstream reserve (atomic file snapshots, #52/#59); the device NVS leg is
#60 — see docs/PERSISTENCE-DESIGN.md.

## Architecture invariant

> **The `MintService` trait (in `cashu-core-lite/src/rpc.rs`) NEVER imports or
> returns `cashu::*` types.** All `cashu` types are confined to
> `micronuts-mint`'s internal helpers (`mint_core.rs`'s `sign_outputs` /
> `verify_proofs` / `mark_spent` and `spending.rs`'s NUT-11 Schnorr
> verification), which convert at the boundary via `src/type_conversion.rs`.

This keeps `cashu-core-lite` pure for `no_std` firmware builds (it has `cashu`
only as a `dev-dependency` for differential testing, never a runtime dep).
