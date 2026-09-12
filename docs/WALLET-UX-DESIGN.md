# Micronuts Wallet — UX design (2026-09-12)

The wallet-shaped front end for the micronuts stack: a host prototype
today, the F469 device tomorrow. Research basis:
docs/RESEARCH-2026-09-12-cashu-wallet-ux.md.

## Product shape

A single-active-mint ecash wallet with the cashu.me/minibits-class flows:

```
            ┌──────────────────────────────────────────┐
            │                HOME                      │
            │  mint chip · balance (+hide) · status    │
            │  [Receive] [Send] [Hide]                 │
            │  recent activity (3 rows)                │
            └───────┬──────────────────────┬───────────┘
                    ▼                      ▼
              ┌───────────┐          ┌───────────┐
              │  RECEIVE  │          │   SEND    │
              │ tab: Ecash│          │ tab: Ecash│
              │  paste +  │          │  amount + │
              │  check +  │          │  memo →   │
              │  redeem   │          │  token+QR │
              │ tab: LN   │          │ tab: LN   │
              │  amount → │          │  invoice →│
              │  invoice+ │          │  quote →  │
              │  QR → poll│          │  pay      │
              │  → mint   │          │           │
              └───────────┘          └───────────┘
   MINTS: list · add (trust gate) · switch · → BACKUP (seed reveal)
   HISTORY: full activity list
```

Screen inventory (ui/*.slint): home, receive (2 tabs), send (2 tabs),
history, mints (+ trust confirmation), backup. Navigation: bottom bar,
5 tabs; Backup hangs off Mints (settings-adjacent, per bitcoin.design).

## Flows

- **Top up (Lightning → ecash):** amount → `POST /v1/mint/quote/bolt11` →
  show invoice string + QR → 2 s poll `GET …/quote/{id}` → on PAID enable
  "Mint ecash" → NUT-04 mint (NUT-13 deterministic outputs) → balance.
- **Receive ecash:** paste token → optional NUT-07 health check
  (SPENT proofs flagged before redemption) → redeem via NUT-03 swap into
  fresh deterministic outputs (secret rotation — sender can't track).
- **Send ecash:** amount (+memo) → exact-denomination swap → NUT-00 V4
  token (`cashuB…`) as text + QR.
- **Pay invoice:** paste → NUT-05 quote (amount + fee_reserve shown
  before confirming) → melt. The engine pre-composes exact inputs (see
  §Engine) so nothing burns.
- **Mints:** add URL → info probe → trust gate → per-mint proof store;
  switch = reconnect + rebalance view. Backup: seed reveal + NUT-09
  restore path.

## Architecture

```
ui/*.slint ── WalletLogic global (state + callbacks)
     │  callbacks (never block; set busy=true)
     ▼
src/ui.rs ── mpsc jobs ──► worker thread
     ▲                        │ WalletEngine<T: MintClient + Clone, S: ProofStore>
     │ Weak::upgrade_         │  ├─ PersistentWallet<HttpMintClient, FileStore>
     │  _in_event_loop        │  │    (NUT-13 seed, NUT-09 restore, atomic store)
     └── snapshots ◄──────────┘  └─ meta: HttpMintClient (quotes/info/keys)
                                     │ REST (ureq, blocking)
                                     ▼
                          standard Cashu mint HTTP surface
                          (micronuts-audit-adapter today, any spec mint)
```

- `HttpMintClient` (src/http.rs) implements the core-lite `MintClient`
  trait over the standard REST routes; JSON DTOs mirror the adapter
  translators (`B_`, `C_`, `C`, `Ys`, `dleq`).
- Engine (src/engine.rs) owns all money decisions; the UI never touches
  proofs.
- State (src/state.rs): `wallet.json` (mints, seed, history; corrupt =
  refuse) + one envelope blob per mint (`proofs-<host>.bin`, atomic
  temp+rename per the `ProofStore` contract).
- Money-safety details: send/melt pre-swap to exact denominations
  (`compose_exact`; melt caps change at the target's lowest set bit so the
  wallet's largest-first melt selection lands exactly); failed swaps roll
  the selection back via `undo_spend`.

## Verification rig

- `cargo nextest run -p micronuts-wallet` — engine over the real DemoMint
  via loopback RPC (full cycle + 4 failure paths) and HTTP client over a
  one-shot mock mint (routes, field names, dleq, error mapping).
- `cargo run -p micronuts-wallet --example shots` — all 8 screen states
  rendered headless to `target/shots/wallet-*.png` (software renderer,
  no X).
- `micronuts-audit-adapter` + `micronuts-wallet --demo` — live ladder
  (STEP 1–6, final balance 70 sats) against the FakeWallet mint.

## Firmware migration path (STM32F469I)

The UI was built Slint-first precisely so it can move to the device:

1. **Renderer:** `renderer-software` already works without GPU; on the
   F469 the 16 MB SDRAM absorbs the 480×800 framebuffer (RGB565 line
   rendering via `render_by_line` keeps heap pressure off the 320 KB
   SRAM). Slint's `mcu-board-support` examples (slint repo) show the
   embassy/BSP integration shape.
2. **Event loop:** replace the winit backend with the embassy executor —
   one Slint timer task + touch events from FT6X06 via the BSP; the
   `WalletLogic` callback surface is backend-agnostic.
3. **Engine:** `WalletEngine<T, S>` already compiles against
   `no_std`-compatible pieces except `ureq`; the device keeps the
   USB-CDC/microfips transport (`RpcMintClient<…>` implements the same
   `MintClient` trait) — swap the transport, keep the engine.
4. **QR scanning** stays the gm65 crate's job (modularity rule); QR
   display code (`qr_image`) is portable as-is.
5. **What changes:** mint management becomes host-assisted (no keyboard);
   the trust gate moves to a physical confirm flow. Estimate: the slint
   UI + engine port is firmware-scale work (~1-2 weeks of sessions), not
   a rewrite — `micronuts-app`'s current embedded-graphics screens remain
   until the Slint build fits the flash budget (2 MB flash, current
   firmware uses a fraction).

## Non-goals (documented deliberately)

Nostr/NWC/contacts, animated QR (NUT-16), cross-mint swaps, P2PK sending
UI, multi-mint balance aggregation, wallet-wide spent-scan (needs a
proof-list accessor on `PersistentWallet` — follow-up), NUT-20 quote
locking UI. All are additive follow-ups on the same `WalletLogic` surface.

## Runtime knobs

- `MICRONUTS_WALLET_MINT` — mint base URL for `--demo`
  (default `http://127.0.0.1:3030`).
- `--dir <path>` — data directory (default
  `$XDG_DATA_HOME/micronuts-wallet`); `wallet.json` + `proofs-*.bin`.
