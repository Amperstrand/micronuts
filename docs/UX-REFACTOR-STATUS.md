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

## Milestone 1 — domain state and UX contract — NOT STARTED

## Milestones 2–8 — NOT STARTED

### M0 addendum (2026-09-12, post-checkpoint)

- Deployed: `Wallet Pages` workflow dispatched; https://amperstrand.github.io/micronuts/ serving (200).
- Pre-existing red found in remote CI (missed by the local battery):
  gitleaks failed on the last 4 pre-M0 main commits —
  `STM32_REGISTRY_KEY = "stm32f469i-disco"` (labgrid board alias) trips
  generic-api-key. Fixed on main (`7c2c99b`+`c83d322`): inline allow for
  future commits + `.gitleaks.toml` `regexes` allowlist for history.
  gitleaks green at `c83d322`. Lesson recorded: milestone gates must
  check remote CI, not only the local battery.
