# walletport

WalletPort facade over [cashu-core-lite](../cashu-core-lite) — the pattern
proven by tollgate-module-basic-go #299, ported to the MCU wallet, plus the
**offline gate validator** firmware profile.

- `WalletPort` trait + `TrustModel` + refusal policies (overpayment caps
  refused, never silently ignored; untrusted mints rejected)
- `OfflineGateValidator<S: ProofStore>`: decode `cashuB…` → trust check →
  NUT-10/11 reject-by-default → per-proof NUT-12 DLEQ vs **pinned keysets**
  (public keys only, zero mint contact) → value check → persist-before-open
  spent ring (bounded, restart-surviving)

Interop: DLEQ proofs generated and *verified by upstream CDK
(`cashu` 0.17.3)* open this gate (`tests/cdk_interop.rs`); tampered ones are
rejected by both verifiers on identical artifacts.

Try it: `cargo run -p walletport --example gate_demo`

Design: [`docs/WALLETPORT-EDGE-VALIDATOR-DESIGN.md`](../docs/WALLETPORT-EDGE-VALIDATOR-DESIGN.md)
