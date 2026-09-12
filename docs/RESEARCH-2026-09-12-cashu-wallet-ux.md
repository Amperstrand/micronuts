# Research: Cashu wallet UX + Slint (2026-09-12)

Findings that drove the `micronuts-wallet` prototype. Every section names
its sources; local file references are `path:lines`.

## 1. What a Cashu wallet is (feature baseline)

Sources: docs.cashu.space/wallets; cashubtc/nuts README (NUT adoption
matrix); minibits-cash/minibits_wallet README; nutstash.app FAQ; eNuts
(enuts.cash); BTC Sessions cashu.me walkthrough (YouTube, Jun 2024);
cashudevkit.org examples (single-mint wallet, mint-token).

The wallet—mint split: ecash wallets are always custodial-at-a-distance —
tokens are bearer assets; mints custody the sats. Wallets therefore:

1. Keep all state client-side (Nutstash: "tokens are stored locally …
   if the storage is wiped, funds are lost").
2. Treat mint selection as a trust decision — cashu.me's add-mint flow
   asks "Do you trust this mint? It controls the funds you send it" and
   offers mint discovery; Nutstash frames mints as "custodians".
3. Support two receive/send rails: Lightning (NUT-04 mint quotes /
   NUT-05 melt) and ecash tokens (NUT-00) handed over out-of-band.
4. Recover via seed: Nutstash and minibits both restore from a BIP32/39
   seed; CDK derives all secrets from a wallet seed (NUT-13) and re-fetches
   signatures (NUT-09 restore).

Feature matrix distilled from the sources (implemented in this prototype =
✅):

| Feature | cashu.me | minibits | Nutstash | Nutshell | micronuts-wallet |
|---|---|---|---|---|---|
| Balance + mint chip home | ✅ | ✅ | ✅ | CLI | ✅ |
| Receive via Lightning invoice | ✅ | ✅ | ✅ | ✅ | ✅ |
| Receive ecash token (paste) | ✅ | ✅ (QR+NFC) | ✅ (QR) | ✅ | ✅ |
| Send ecash token (QR/copy) | ✅ | ✅ | ✅ (animated QR) | ✅ | ✅ (QR) |
| Pay Lightning invoice | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multi-mint list + trust gate | ✅ | ✅ | ✅ | ✅ | list+switch (single-active) |
| History | ✅ | ✅ | ✅ | ✅ | ✅ |
| Seed backup / restore | ✅ | ✅ | ✅ | ✅ | seed display + NUT-09 restore API |
| Token health check (NUT-07) | — | ✅ | — | ✅ | ✅ (check before redeem) |
| Nostr/NWC/contacts/animated QR | partial | ✅ | ✅ | ✅ | out of scope |

Minibits' open UX question is our default too: "avoid terms such as token
or proof … propose the term coin/ecash" — the UI says "ecash" and "sat",
never "proof".

## 2. Bitcoin Design guidance applied

Sources: bitcoin.design/guide/daily-spending-wallet (first use, backup &
recovery, privacy); bitcoin.design/guide/designing-products/common-user-flows.

- Home = balance + primary actions; requests and sends are the most common
  flows — our Home is balance, mint chip, Receive/Send (bitcoin.design
  daily-spending "home screen" pattern).
- First-use flexibility: guide users to best practices but allow skipping;
  prompt for backup after funds arrive — our first-run lands on Mints with
  "Add a mint" (no mint = nothing to lose yet), Backup lives one tap away.
- Backup transparency: users are "often confused or unaware of where their
  keys are stored" — our Backup screen states exactly what the seed derives
  (NUT-13) and how restore works (NUT-09).
- Privacy affordance: hide-balance toggle (daily-spending privacy pattern;
  Muun/Phoenix screenshots in the Bitcoin UI Gallery show the same).
- Trust confirmation copy follows cashu.me's wording (see §1).

## 3. The Rust wallet engine pattern (CDK)

Sources: cashubtc/cdk README; cashudevkit.org single-mint-wallet +
mint-token examples; docs.rs/cdk.

CDK's canonical arc — `mint_quote` → poll `check_mint_quote_status` until
`Paid` → `mint` → `prepare_send` → `send` (token string) → `receive` via
swap — maps 1:1 onto our `WalletEngine` methods
(`mint_via_invoice` → `poll_mint_quote` → `mint_paid_quote`,
`send_token`, `receive_token`). Two CDK lessons we copied:

1. Quote polling is the wallet's job (the mint never pushes) — hence the
   UI-side 2 s poll timer.
2. Exact-amount sends pre-swap inputs to the right denominations
   (`prepare_send`) instead of handing over oversized proofs.

One divergence, deliberate: CDK's melt takes caller-selected inputs; our
`PersistentWallet::melt_deterministic` self-selects largest-first
(cashu-core-lite/src/persistent.rs:279-361), so the engine pre-swaps the
balance into an exact `amount+fee_reserve` set with change capped at the
target's lowest denomination before melting — otherwise a 32-sat coin
paying a 30-sat invoice burns 2 sats (NUT-05 change only refunds the fee
reserve).

## 4. Slint architecture (what we followed and why)

Sources: docs.slint.dev globals guide; docs.rs/slint (ModelRc, ComponentHandle,
Weak, invoke_from_event_loop); slint-ui/slint discussions #6733 (state
management), #6165 (struct passing), #2031; zenn.dev dashboard article
(2026-06, worker-thread Weak pattern, `set_vec` bulk replacement);
tools/viewer/screenshot.rs + docs.rs SoftwareRenderer (headless render).

- Logic in globals: `export global WalletLogic` carries all state +
  callbacks; Rust binds via `on_*` setters (globals guide; discussion
  #6733 recommends globals over deep callback forwarding).
- Page switching: enum property + conditional instantiation (discussion
  #6733's enum `Pages` example) — our `Page` enum in logic.slint.
- Never block the UI thread: worker `std::thread` owns the engine; results
  post back with `Weak::upgrade_in_event_loop` (docs.rs slint crate docs:
  "perform the minimum amount of work in the main thread").
- Models: `VecModel` + full `set_vec` replacement for history/mints
  (ModelRc docs; the dashboard article's dirty-region lesson).
- Headless verification: custom `Platform` + `SoftwareRenderer::render`
  into a pixel buffer, no event loop — the slint-viewer screenshot path;
  our `examples/shots.rs` does this for all 8 screen states.
- Embedded path: Slint ships MCU support (renderer-software,
  mcu-board-support examples in the slint repo) — the reason Slint was
  chosen over web-tech UIs for a wallet that must land on the F469.

## 5. Local reuse (no reinvented wheels)

- Engine core: `cashu-core-lite` `PersistentWallet`
  (cashu-core-lite/src/persistent.rs:150-522) — NUT-13 determinism,
  NUT-09 restore, atomic envelope store. The prototype added exactly
  three methods (`swap_deterministic`, `add_proofs`, `remove_proofs`)
  because the inner `Wallet`/transport are private.
- Wire format: V4 CBOR tokens via `encode_token_wire`/`decode_token`
  (cashu-core-lite/src/token.rs:331-386), already cashu-ts-verified by CI.
- REST field names mirror `micronuts-audit-adapter/src/main.rs`
  (parse_*/\*_to_json, lines 595-1110) — the same surface the cashu-ts
  e2e already exercises.
- QR: `qrcodegen-no-heap` (already a workspace dep for the firmware).
