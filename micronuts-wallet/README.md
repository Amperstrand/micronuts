# micronuts-wallet

Host Cashu wallet with a Slint UI: balance, top up via Lightning invoice,
send/receive ecash tokens (QR + text), pay Lightning invoices, mint
management with a trust gate, history, and seed backup. Money logic rides
`cashu-core-lite`'s `PersistentWallet` (NUT-13 deterministic secrets,
NUT-09 restore) behind a REST `MintClient`; every credited proof is
NUT-12-verified against the mint's published keys.

The same UI also builds to **WebAssembly** with an embedded in-browser
demo mint (no network, no persistence — a playground, not a wallet you
keep funds in): `trunk build` here, or grab the deployed build from
GitHub Pages (`Wallet Pages` workflow).

Design: [`docs/WALLET-UX-DESIGN.md`](../docs/WALLET-UX-DESIGN.md) ·
Research: [`docs/RESEARCH-2026-09-12-cashu-wallet-ux.md`](../docs/RESEARCH-2026-09-12-cashu-wallet-ux.md)

## Run

Terminal 1 — the mint (adapter + auto-settling FakeWallet):

```bash
cargo build -p micronuts-mint --bin mint_server
cargo run -p micronuts-audit-adapter
```

Terminal 2 — the wallet (480×800 window):

```bash
cargo run -p micronuts-wallet
```

First run opens on **Mints**: add `http://127.0.0.1:3030`, trust it, then
use Receive (invoice or paste a `cashuB…` token) / Send. Data lives in
`$XDG_DATA_HOME/micronuts-wallet` (`wallet.json` + one proof store per
mint) — override with `--dir <path>`.

## Headless checks

```bash
cargo run -p micronuts-wallet --example shots   # 8 screen PNGs → target/shots/
MICRONUTS_WALLET_MINT=http://127.0.0.1:3030 \
  cargo run -p micronuts-wallet -- --demo        # STEP 1–6 ladder, ends "Final balance: 70 sats"
```

`--demo` runs mint → send token → receive token → melt against a live
mint; point `MICRONUTS_WALLET_MINT` at any spec-conformant mint (works
with the adapter, testnut, …).

## Knobs

| Knob | Default | Meaning |
|---|---|---|
| `MICRONUTS_WALLET_MINT` | `http://127.0.0.1:3030` | mint base URL for `--demo` |
| `MICRONUTS_ADAPTER_PORT` | `3030` | adapter listen port |
| `--dir <path>` | `$XDG_DATA_HOME/micronuts-wallet` | wallet data directory |

## What's verified (CI)

- Engine over the real DemoMint via loopback RPC: full cycle + failure
  paths (insufficient funds, foreign mint, double-receive) + forged /
  dleq-less mint signatures rejected with rollback (NUT-12).
- HTTP client wire shapes vs a mock mint (`B_`/`C_`/`C`/`Ys`, dleq,
  error-code mapping).
- NUT-07 reconciliation prunes proofs spent elsewhere.
- `cargo nextest run -p micronuts-wallet` + clippy `-D warnings` +
  fmt are blocking in CI; the adapter-test job runs the `--demo` ladder.

## Non-goals (for now)

Nostr/NWC, animated QR, cross-mint swaps, P2PK send UI, NUT-17
websockets, wallet-wide spent-scan scheduling. See the design doc's
follow-up list.
