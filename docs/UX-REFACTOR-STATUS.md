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

## Milestones 4–8 — NOT STARTED

### M0 addendum (2026-09-12, post-checkpoint)

- Deployed: `Wallet Pages` workflow dispatched; https://amperstrand.github.io/micronuts/ serving (200).
- Pre-existing red found in remote CI (missed by the local battery):
  gitleaks failed on the last 4 pre-M0 main commits —
  `STM32_REGISTRY_KEY = "stm32f469i-disco"` (labgrid board alias) trips
  generic-api-key. Fixed on main (`7c2c99b`+`c83d322`): inline allow for
  future commits + `.gitleaks.toml` `regexes` allowlist for history.
  gitleaks green at `c83d322`. Lesson recorded: milestone gates must
  check remote CI, not only the local battery.
