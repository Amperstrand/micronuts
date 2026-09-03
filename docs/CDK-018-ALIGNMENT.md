# CDK 0.18 Alignment — Research, Audit, Migration (2026-09-03)

Issue #62. Ecosystem moved to 0.18.0 (released 2026-09-02: `cashu`,
`cdk-common`, `cdk` in lockstep). This pass bumped micronuts' oracle and
runtime to `cashu 0.18.0`, audited every ported surface, and answered the
strategic question.

## Decision 1 — keep `cashu-core-lite`; do NOT adopt `cdk-common`

`cdk-common` 0.18 is an application crate (tokio, futures, async-trait,
parking_lot, anyhow, lightning, uuid, url, optional HTTP/Prometheus/tonic/
DB deps) with no `no_std + alloc` contract. `cashu` 0.18 itself (bitcoin,
serde_json, lightning-invoice, uuid, …) is likewise not an embedded core.
The embedded core stays hand-rolled (k256, no_std) with `cashu` as the
host/esp-idf runtime + differential oracle — the existing AGENTS rule 2
architecture, now with evidence attached. Revisit only if CDK publishes an
explicitly feature-gated no_std protocol crate.

## Decision 2 — migrate to `cashu 0.18.0` now (done)

Mechanical surface was tiny by design: the dhke free functions
(`hash_to_curve`, `blind_message`, `sign_message`, `verify_message`) are
UNCHANGED between 0.17.3 and 0.18.0. The only code change required:
`BlindSignature::new` now borrows `&SecretKey` (two call sites).

Verified on 0.18.0:
- ccl 152 tests incl. the CDK-interop suites (DLEQ cross-impl with 0.18's
  deterministic-nonce derivation, keyset-id vectors, cashu_compat)
- walletport 20 (0.18-generated DLEQ proofs open the offline gate)
- mint 54/67 both feature modes; thumb no_std builds; full battery + e2e

## Decision 3 — wire alignment: `method` on quote responses (done)

NUT-04 and NUT-05 (current nuts HEAD) both REQUIRE `"method"` in mint and
melt quote responses, and CDK 0.18 structs enforce it. Added to ccl
(`MintQuoteResponse` n(10), `MeltQuoteResponse` n(8), `MeltResponse`
n(10)), populated (`"bolt11"`) in mint_core, serialized in the adapter,
parsed tolerantly in the walletport test gate. Live: cashu-ts v4 e2e
green with `method` on the wire. Matrix stays 69/109 (cashu-audit does
not test `method` — the beneficiary is CDK-0.18/v4 clients).

## Audit findings (ported-from-CDK functionality vs 0.18)

| Surface | Status vs 0.18 |
|---|---|
| dhke (hash_to_curve/blind/sign/verify) | identical signatures; differential tests green |
| DLEQ construction (BlindSignature::new) | 0.18 borrow-only change applied; deterministic-nonce derivation covered by cross-impl tests |
| nut02 keyset Id v1/v2 derivation | unchanged upstream; ccl vectors green |
| Quote accounting (amount_paid/issued/updated_at) | already aligned (NUT-04 counters; monotonic updated_at) |
| Quote `method` field | WAS missing → added (Decision 3) |
| Melt change | see NUT-08 note below |
| Amounts | wire types stay plain u64 (CDK Amount is a typed wrapper — boundary rule per version-matrix D-series) |
| `PreMint.derivation_index`, typed witnesses, BOLT12 split structs | not ported — NUT-10/11/14 (#51) and BOLT12 are future work, unaffected |

## NUT-08 note — blank outputs are now THE SPEC (finding reversal)

nuts 49a909c (2026-08-23) rewrote NUT-08: wallets send amount-0 blank
outputs (`max(ceil(log2(fee_reserve)), 1)` of them) and the MINT IMPRINTS
the overpay's power-of-two decomposition into them. This REVERSES the
2026-09-02 signut-session conclusion that blank-imprinting was a
"micronuts-local convention": our front mint's imprinting is
spec-conformant, and cashu-cf saga's sign-blanks-as-amount-0 behavior is
the non-conformant side. The reserve's explicit-amount change requests
(upstream.rs) remain the CORRECT compat choice against a non-imprinting
upstream — compat and conformance are different layers. Follow-ups:
cashu-cf should adopt NUT-08 imprinting (their issue tracker); our
wallet-side blank count (when we build a wallet) should use the
fee_reserve formula, not the overpay bits.

## Spec-quote status

`greatspectate check` green against current nuts HEAD (clone up to date
incl. the NUT-08 rewrite). No quote drift.

## Residual / follow-ups

- tollgate-s3-rs still pins cashu =0.17.3 — version-matrix D2 "bump
  together" now has a one-repo exception, recorded in the matrix
- cdk 0.18's NUT-29 (batch mint + max-array advertisement) and NUT-18
  fee fields remain unimplemented here (matrix crumbs, roadmap §4)
- `BlindSignature::new` comment in ccl's nut12 test updated to 0.18
  semantics
