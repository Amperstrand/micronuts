# UX refactor — milestone ledger

Working branch: `ux/wallet-refactor`. Policy: one checkpoint commit per
green milestone; nothing red is committed; `docs/WALLET-UX-CONTRACT.md`
is the contract this work answers to.

## Milestone 0 — baseline, research, boundaries — PASS (2026-09-12)

- Baseline commit: `5521219` (main). Working tree at start carried one
  user edit (`docs/WALLET-UX-DESIGN.md` QR-entry section — committed
  separately below, preserved) + untracked session screenshots.
- Tests: core-lite/walletport/mint/fips 80 passed; micronuts-wallet 36
  passed; mint backend-upstream 30 passed. clippy `-D warnings` clean;
  fmt clean; `thumbv7em-none-eabihf` no_std builds clean; wasm32 build
  clean (Trunk/Pages pipeline already deploys it).
- Firmware: release build 213,012 B text / 372 B data / 27,584 B bss
  (≈10.4 % of 2 MB flash). Flashed fresh via st-flash
  (`--connect-under-reset`); `scripts/test_hw_swap_gate.sh` (with
  `CARGO_TARGET_DIR=$PWD/target` — the shared global target dir breaks
  the script's repo-local paths, environmental not a regression) =
  ALL HARDWARE CHECKS PASSED: swap flow, DLEQ gate, cashu-ts 4.10
  offline verification, decoder robustness. Prior on-device firmware
  copy had timed out the CDC gate — resolved by the fresh flash.
- Screenshots: 19 current shots (11 embedded-graphics app screens +
  8 Slint wallet screens) in `target/shots/`; `scripts/visual_qa.py`
  0 failures. Manual/vision review of the Slint set is **pending** —
  both multimodal paths timed out this session; screen sources were
  reviewed instead (findings below). No visual change in this
  milestone.
- Research: CDK current (zread: token ops, proof states, quote state
  machines, multi-mint), cashubtc/wallet PRODUCT.md + copy-guidance.md
  (MIT), local RESEARCH doc (cashu.me, minibits, bitcoin.design). All
  folded into the contract.
- Deliverables: `docs/WALLET-UX-CONTRACT.md` (this checkpoint).
- Findings driving later milestones:
  - `WalletLogic` exposes implementation verbs as user controls:
    `check-token`, `melt-quote`, `mint-paid`, `reconcile` → M3/M5.
  - History is pre-rendered `line1/line2` strings → M6.
  - Flow state is stringly (`invoice-state`, `token-check-text`,
    `scan-status`) → M1 state model.
  - Send = "token generated", no pending/claim lifecycle → M4
    (core-lite needs reserved/pending proof state + prepare-send).
  - Home: 4 buttons in one row incl. "Refresh balance" → M2.
- Known limitations: none new.

## Milestone 1 — domain state and UX contract — PASS (2026-09-12)

- New `micronuts-wallet/src/flow.rs`: the UX contract as code —
  `ReceiveEcashPhase`, `ReceiveLightningPhase` (incl. `PaidNotIssued`),
  `PayLightningPhase`, `SendEcashPhase` (incl. `AwaitingClaim`, M4
  target), `FlowFailure` taxonomy (Offline / InvalidToken / AlreadySpent
  / ForeignMint / InsufficientFunds / Expired / UntrustedSignature /
  Recoverable) with `classify(&CashuError)`, and contract-wording
  `user_line()` mappings. Pure + unit-tested; no I/O, no Slint.
- Engine: `inspect_token` (decode → mint match → NUT-07 health →
  `TokenInspection`); `lib::format_amount` is now the single amount
  formatter ("12,450 sats").
- UI (minimal, compiles against the model): check-token routes through
  inspection + phases (foreign-mint and all-spent now surfaced at Check
  with contract wording); invoice status line derives from
  `ReceiveLightningPhase` (`invoice-state` stays raw for the Mint-button
  logic — documented in logic.slint); receive.slint label uses the new
  `invoice-status-text`. No other screen changes.
- Tests: 9 flow unit tests (classifier, quote-state mapping,
  PaidNotIssued ≠ failure, jargon-free user lines, "No fee" contract,
  all-spent verdict) + 4 engine integration tests (inspect valid /
  after-receive-spent / foreign / malformed). Full battery green:
  wallet suites 29/14/7, core-lite 44+, clippy `-D warnings`, fmt,
  thumbv7em no_std, firmware release, wasm32, 8 shots regenerated
  (receive-lightning intentionally reworded — the only visual delta).
- Checkpoint: `wallet: semantic flow states (ux contract, no visual change)`.

## Milestone 2 — wallet shell, home, navigation — PASS (2026-09-12)

- Checkpoint `9a74dfd` (+ e2e infra `a822b10`, merged to main): nav
  contract — Home/Activity/Settings tabs; Receive/Send/Scan as Home
  intents (BackHeader back affordance); Mints+Backup under Settings;
  first-run still lands on Mints. Home per contract: 54px balance +
  privacy eye-toggle, mint chip, calm status, 68px Receive/Send, Scan,
  three most-recent rows. "Refresh balance" removed — reconciliation
  already automatic on connect. 9 shots regenerated.
- **Browser e2e (new capability)**: Slint-wasm renders to canvas →
  Playwright drives canvas-geometry clicks and asserts through a
  `window.__micronuts` mirror (page/balance/mint/flags; no secrets;
  250 ms timer + snapshot hook). 3 tests (boot+amount-format contract,
  tab nav, settings→mints→back). **First run caught a real violation**
  (snapshot hand-formatted "0 sat"; fixed to `format_amount`). 3× green
  locally; `pages.yml` now runs the suite post-deploy against the live
  URL — CI: build ✓ deploy ✓ e2e ✓ (39 s); local run vs live: 3 passed.
  Rust CI + gitleaks green at `a822b10`.
- Hardware note: the Slint wallet is not on firmware yet (M7), so M2's
  physical gate is approximated by fixed 480×800 canvas geometry
  (deterministic targets ≥ 68px) + real-browser verification; on-device
  evaluation lands with M7.
- **Real-mint feasibility (probed)**: signut.cashu.exchange (signet,
  Nutshell-CF/0.0.1) and testnut.cashu.space (cdk-mintd, FakeWallet)
  both send `access-control-allow-origin: *` with POST — a wasm wallet
  can talk to them directly from the browser. Plan: wasm HTTP
  `MintClient` (fetch-based, replaces the embedded DemoMint when a real
  mint URL is added), testnut first (auto-paid invoices = no external
  payer needed), signut for real signet Lightning (money taxonomy:
  signet = free rein; external payer via the ssh→cln-hub recipe).
- Known limitations: host disk pressure (external concurrent writer)
  interrupted builds repeatedly this session; milestone gates still met.

## Milestone 3 — universal scan + ecash receive — PASS (2026-09-12)

- Checkpoint: `ux: unify scanner and ecash receive` (see git log for
  SHA). Payload router (`src/payload.rs`): bare `cashuB/A` tokens,
  `?token=` URLs, `lightning:`-wrapped and bare BOLT11, mint URLs;
  Unknown is inert. One routing point (`ui::route_scanned`) feeds
  GM65 (native) and camera (wasm) decodes: token → Receive+auto-inspect,
  invoice → Send/Lightning prefill, mint URL → Mints prefill. A scan
  resolves an intent; nothing spends without the explicit Receive/Pay
  press.
- Receive ecash: "Check"/"Redeem" buttons gone. Input (typing, paste,
  scan) auto-inspects (inspecting → review | failed); review card shows
  amount+mint (format_amount), fee line per copy contract ("No fee"),
  part-spent warning; single **Receive** confirm; receiving/received/
  failed states from `flow::ReceiveEcashPhase`. `TokenInspection` gained
  `fee` (engine-computed swap fee).
- Tests: 8 payload-router unit tests; inspection integration tests
  extended (fee); wallet suites 36/14/7 green; clippy/fmt/wasm/thumb
  clean. **Browser e2e 4/4 × 3 runs**: typing a genuinely decodable V4
  token (from the new `examples/print_token` against an in-process
  DemoMint) auto-inspects to the foreign-mint failure wording — proving
  decode + mint-match + no-auto-spend on the real surface.
- Hardware: GM65 physical scan not re-exercised this session (module
  path unchanged from 97b89e7's verified wiring; routing covered by
  payload unit tests). Recorded as a gate carry-over for the next
  hardware session alongside M7.
- Deployed: Wallet Pages build → deploy → e2e green at the checkpoint
  SHA (live suite includes the auto-inspect test).

## Milestone 5 — Lightning receive + pay — PASS (2026-09-13)

- Checkpoint: `ux: simplify lightning flows` (SHA in git log). "Mint
  ecash" removed — issuance is automatic when the invoice settles
  (creation-PAID fast path + poll-transition path; `drop_mint_paid`
  shared with a "Try again" button for the PaidNotIssued money-safety
  state, which keeps the quote id and says exactly what happened).
  "Get quote" removed — invoice input (typing or scan routing) produces
  the pay review by itself (`quote_for_review`; `invoice-edited`
  callback; review card via `PayLightningPhase::Review` wording incl.
  "No fee" contract); Pay is the single confirm.
- money.spec mint helper + melt test updated for auto-issuance (PAID is
  transient; balance is the wait target). e2e 9/9 ×3 locally.
- Engine suites 36/20/7 green; clippy/fmt/wasm/shots green.
- Paid-not-issued modeled per contract; expiry inherits the phase
  wording (invoice expiry surfaces via quote errors → FlowFailure).

## Milestones 6–8 — NOT STARTED

### M0 addendum (2026-09-12, post-checkpoint)

- Deployed: `Wallet Pages` workflow dispatched; https://amperstrand.github.io/micronuts/ serving (200).
- Pre-existing red found in remote CI (missed by the local battery):
  gitleaks failed on the last 4 pre-M0 main commits —
  `STM32_REGISTRY_KEY = "stm32f469i-disco"` (labgrid board alias) trips
  generic-api-key. Fixed on main (`7c2c99b`+`c83d322`): inline allow for
  future commits + `.gitleaks.toml` `regexes` allowlist for history.
  gitleaks green at `c83d322`. Lesson recorded: milestone gates must
  check remote CI, not only the local battery.

## Milestone 6 — Activity, copy, backup — PASS (2026-09-13)

- Checkpoint: `ux: refine activity settings and recovery`. Activity rows
  are semantic (kind-first rail labels per the copy contract: "Ecash
  sent", "Lightning received"…; amounts via format_amount; pending rows
  say "waiting for recipient"; reclaimed marked; line2 = Today/Yesterday
  or date + detail via Hinnant civil conversion, web_time for wasm).
- Backup copy: plain-language primary ("These words recover your
  ecash…anyone with these words can spend your ecash") + honest limits
  line (cannot recover handed-over tokens or post-backup spends); NUT
  jargon removed from the flow. Mint trust copy already conformed.
  Amount formatting stays centralized in format_amount.
- Battery: 36/20/7 green, clippy/fmt green, shots regenerated, wasm
  built, e2e 9/9 locally.

## Milestones 7–8 — IN PROGRESS

## Milestone 7 — consolidation + resources — PASS (2026-09-13)

- Boundary review: one `ui.rs` + one `.slint` tree serve host/wasm (cfg
  transports only); money logic entirely in engine/flow (no Slint
  callbacks compute money); transports behind `MintClient`
  (Http native / DemoMint wasm / Rpc embedded); scanner behind the
  gm65/camera seam routing through one `payload::classify`. No duplicate
  flow state machines found. Slint-on-F469 remains the documented M7+
  port plan (WALLET-UX-DESIGN §Firmware migration) — not attempted in
  this session per stop conditions.
- CDK adoption ledger: quote-polling machinery ✓ (M5), exact-amount
  prepare-send ✓ (pre-existing), pending-send lifecycle ✓ (M4,
  remove-and-record divergence documented in 6f0af14), proof states via
  NUT-07 ✓, auto transaction recording ✓ (M6 semantics). Divergences:
  melt largest-first pre-swap (pre-existing, documented); reserved
  proofs out-of-store (M4).
- Resources: firmware unchanged from baseline (213,012 B text / 27,584 B
  bss — the refactor touched no firmware code); wasm dist 38.4 → 40.6 MB
  raw (+2.2 MB: flow/payload modules, mirror, e2e seam); e2e suite 4 →
  9 tests.

## Milestone 8 — hardware + full regression — PASS (2026-09-13)

- Hardware (board attached): fresh flash of current firmware
  (`st-flash --connect-under-reset`), `scripts/test_hw_swap_gate.sh` =
  ALL HARDWARE CHECKS PASSED (swap flow, DLEQ gate, cashu-ts 4.10
  offline verification, decoder robustness). The embedded-graphics
  firmware is intentionally unchanged this refactor; physical
  evaluation of the new wallet UI is browser-at-480×800 + canvas-geometry
  targets (≥56px) until the Slint device port.
- Full regression (local): T1 80 + T2 36/20/7 + T3 30 + clippy
  `-D warnings` + fmt + thumbv7em no_std + firmware release + wasm32 +
  9 e2e (local dist) green; remote: gitleaks + Rust CI + Wallet Pages
  (build → deploy → live e2e) green at every checkpoint.
- README wallet section refreshed to the current flows.

## Follow-up: real mints in the browser — PASS (2026-09-13)

- Refactor: the Cashu wire protocol moved to `mint_wire.rs` as one
  transport-neutral `MintClient` impl over a 2-method `JsonTransport`
  trait; native `http.rs` is now just the ureq transport, and the new
  `wasm_http.rs` supplies a synchronous-XHR transport (rationale in the
  module doc: the trait is deliberately blocking for the embedded RPC
  path; sync XHR avoids an engine-wide async refactor — UI blocks per
  small JSON round-trip while `busy` is set).
- Browser wallets now add and use REAL mints: `ClientForMint` enum
  (demo | fetch), wasm add/switch-mint jobs share the native worker
  fns, engine `connect()` prefers the sats keyset on multi-unit mints
  (testnut lists eur first — the 400 "Unit unsupported" it caused is
  fixed).
- Evidence: live session added testnut.cashu.space and received 11 sats
  over HTTPS (invoice → FakeWallet auto-settle → auto-issuance);
  `realmint.spec.ts` encodes that flow, env-gated
  (`WALLET_E2E_REAL_MINT=1`, CI Pages e2e sets it) so the hermetic
  suite stays network-independent. Local: 9 passed + real-mint 10th
  passing; engine suites 36/20/7; clippy/fmt/wasm green.
- Money taxonomy: testnut = FakeWallet test money (free rein); signut
  (real signet Lightning) is one URL away with the same code path.

## Follow-up: browser persistence — PASS (2026-09-13)

- `browser_store.rs` (wasm): `BrowserStore` implements the core-lite
  `ProofStore` over localStorage (hex envelope, one key per mint —
  full-replace writes keep the FileStore atomicity contract), plus
  `load/save_wallet_state` for the metadata. Boot restores seed, mints,
  history, and pending sends; a fresh demo wallet is only created when
  nothing is stored. Previously a fresh seed per boot made the browser
  wallet a money-destroyer once real mints arrived — closed.
- e2e: persistence test in the hermetic suite (fund 12 → reload →
  "12 sats"; second tab in the same profile reads the same storage).
  Local: 10 passed hermetic + 11 with the real-mint gate (testnut live
  receive included).
- Concurrent-session work preserved and completed: shots.rs
  screen-hash fixtures (Trezor `--ui=record` model) had a type error
  in flight — fixed forward (`unwrap_or_default`), fixtures recorded
  (9 screens) and the compare mode passes.
- Battery: 36/20/7 engine suites, clippy -D warnings, fmt, thumbv7em
  no_std, wasm build green.

## Follow-up: wallet-to-wallet over QR + claim auto-detection — PASS (2026-09-13)

- **QR pixel round-trip e2e (hermetic)**: fund → send → screenshot the
  wallet's actual rendered QR pixels → host-side PNG→jsQR decode →
  byte-identical with tokenOut. The wallet's qrcodegen rendering is
  machine-scannable, proven.
- **Wallet-to-wallet on a real mint (gated e2e)**: two separate browser
  contexts (own seeds + storage), both on testnut — A mints 64 over
  live HTTPS, sends 21, its rendered QR pixels decode on the host, B
  pastes → auto-inspect review (amount+mint+memo, fee-aware — testnut
  charges 1 sat NUT-08 input fee) → Receive → B holds 20–21 sats. A's
  pending send auto-resolves to claimed.
- **Product fix that fell out**: pending-send claim detection was
  connect-only; now a 10 s lifecycle checker re-checks in-flight sends
  while any exist (UX contract: reconciliation is machinery), with the
  count flowing through Snapshot so Home/e2e see it live.
- Local: 13/13 e2e (incl. real-mint pair); engine suites 36/20/7,
  clippy, fmt green.

## Follow-up: physical QR loop — READY, hardware blocker on GM65 — 2026-09-13

- **Live token mint** (`examples/print_live_token.rs`): engine over real
  HTTPS against testnut (fund-above-send for the NUT-08 fee; fresh
  random seed per run — a fixed seed re-spends deterministic secret
  slots the mint already signed). Verified: 21-sat testnut token
  minted and exported.
- **`tools/hil/physical_loop.py`**: complete capstone harness — live
  mint over HTTPS → CYD QR (UR fragments) → GM65 laser scan → F469
  reassemble + import → GetTokenInfo amount/proofs assertion; same
  safety pattern as bringup (BenchLock, labgrid place, backup/restore,
  clean-slate reboot).
- **CYD recovered**: the shared bench CYD had been reflashed with
  foreign firmware (wifi console debris); re-flashed cyd-qr 1.0.0 from
  gm65-scanner (their rig module, `espflash` on ttyUSB0) — answers ID
  again.
- **BLOCKER — GM65 deep wedge** (documented module state, QR-RIG
  session plan 2026-09-11 §7 + "Open"): the module fails `init()` at
  every boot across 3 ST resets (boot-heal ran each time), 60+s of idle
  polls; ScannerStatus connected=0 persistently. Notably `ScannerTrigger`
  (raw UART set_aim) returns OK — the UART and module power are fine;
  the decode/init engine is wedged. Documented recovery: whole-board
  power-cycle (physical unplug — GM65 rides the board 3.3V rail, ST
  resets don't depower it) or long idle recovery. The 2026-09-11
  session left the module in exactly this state and expected
  overnight recovery; it did not recover.
- **Next step (manual)**: unplug both USB cables + ST-Link from the
  F469 board for 10 s, replug, then `python3 tools/hil/physical_loop.py
  --skip-flash` — everything else is ready.
