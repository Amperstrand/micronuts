# Browser → Device QR Handoff (demo keyset unification)

**2026-09-13 · roadmap item 3** · script: `scripts/test_qr_handoff.sh`

The browser demo wallet's embedded mint now signs with **the pinned
device keyset**, so a token minted by the wasm wallet is verifiable
offline by the F469 hardware wallet — no mint server, no network.

## One demo mint, one identity

Before this change the browser mint derived per-amount keys from a
per-instance seed; its tokens failed the device's DLEQ gate. The demo
mint now has exactly one identity, everywhere:

| Field | Value | Pinned by |
|---|---|---|
| Mint URL | `demo://micronuts` | walletport trusted-mint list |
| Keyset id | `00` | walletport pinned keyset, harness tokens |
| Amount keys | one key: `SHA256("demo://micronuts")` for every denomination | `micronuts-app` `pinned_demo_mint_key` (#54 trust root) |

This is the same triple `host-mint-tool`'s `DemoMint` has always used.
The derivation is public — **never custody value** with it (cf. #56).
Persisted browser wallets from the old URL
(`https://demo.micronuts.invalid`) are migrated on load; old-keyset
demo proofs were valueless playground state.

## The chain

```
wallet engine ──cashuB──▶ device (ImportToken → GetBlinded →
   (DemoMintClient,        pinned-key sign → SendSignatures DLEQ gate)
   the exact code the                   │
   wasm demo runs)                      ▼ GetProofs export
                                       │
                    walletport offline gate (keyset "00", pinned key,
                    NUT-12 DLEQ, replay ring) ──▶ OPEN at 21 sats
```

Three legs, one script:

1. **Mint** — `micronuts-wallet --example mint_demo_token -- 21` runs
   the full engine (NUT-04 quote → blind → mint → send) against
   `DemoMintClient` and prints a `cashuB` token. This is the same
   code path the wasm browser demo executes.
2. **Device** — `mint-tool swap --token-file` consumes the *wallet*
   token (not the harness's self-generated one): import → blind →
   `DemoMint` sign → the device's `SendSignatures` NUT-12 gate →
   export. `--selftest` runs the in-process device (CI);
   `--port`/autodetect runs the wire leg (exit 77 without hardware).
3. **Gate** — the device export must open the `walletport` offline
   gate pinned to the demo keyset, and replay must be rejected.

## Run it

```bash
bash scripts/test_qr_handoff.sh            # CI leg (in-process device)
bash scripts/test_qr_handoff.sh 63         # any amount 1..=255
bash scripts/test_qr_handoff.sh 21 --wire  # real silicon over CDC
```

CI: the `host-tests` job runs the CI leg after the swap selftest.

## Evidence (2026-09-13)

- CI leg: `QR HANDOFF PASS: browser-engine token accepted by the device
  (in-process) and verified offline (21 sats)` — including the
  corrupted-DLEQ negative leg (rejected, no proofs).
- Wire leg, live silicon on `/dev/ttyACM1`:
  `SWAP WIRE PASS on /dev/ttyACM1: 21 sats, 3 proofs, export 721 bytes`
  → gate `OPEN: 21 sats verified against the pinned demo keyset`,
  `replay correctly rejected`.
- Wallet suite: `cargo test -p micronuts-wallet` green (65 tests);
  wasm32 check green.
