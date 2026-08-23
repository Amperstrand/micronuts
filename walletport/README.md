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

## Fuzzing

Four libFuzzer targets cover every parser surface and the full gate
decision path:

    cargo install cargo-fuzz   # once
    cd walletport
    cargo fuzz run decode_token   # cashuB wire -> CBOR -> roundtrip property
    cargo fuzz run gate_verify    # full offline-gate decision path
    cargo fuzz run envelope       # persistence envelope (corrupt-blob tolerance)
    cargo fuzz run cbor_token     # raw CBOR token decode

Committed minimized corpora live in fuzz/corpus/<target>/ - runs start
deep. Two 150s-per-target campaigns (35M+ execs total) found zero
panics; artifacts, if any ever appear, land in fuzz/artifacts/
(gitignored). The fuzz crate is excluded from the workspace and CI
(build cost > value for now); run locally or on a beefy box before
releases.

## Real-mint end-to-end

`tests/real_mint_gate_e2e.rs` (ignored by default — network + real mint):

    cargo test -p walletport --features std --test real_mint_gate_e2e -- --ignored --nocapture

Mints valueless tokens from the testnut dummy mint through the full
stack with zero mocks: real `/v1/keys` pinned keyset -> NUT-04 quote
(dummy-auto-paid ~5s) -> PersistentWallet blinding/minting over the
mint's REST API -> proof-level NUT-12 DLEQ from the production mint ->
`cashuB` wire -> OfflineGateValidator DLEQ verification against the
pinned keys -> `Open`, replay rejected. Verified from two network
positions (lab LAN + SHC VPS).
