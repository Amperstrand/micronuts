# QR Rig Session Plan — 2026-09-11

Goal: full e2e "receive by QR" on hardware, labgrid-orchestrated and
repeatable: token minted/signed on host → CYD renders animated UR QR →
GM65 scans → **micronuts wallet reassembles + imports on device** →
asserted over CDC. Ports the gm65-scanner rig learnings (bench flock →
labgrid place → backup/restore → regression gate → characterization
campaign → measured LIMITATIONS) into this repo.

## Current state (gap analysis, 2026-09-11)

- Scanning infra exists (GM65 on USART6, single-frame scans verified
  over CDC trigger/data), UR decoder wrappers exist in
  `firmware/src/qr` + `micronuts-app/src/qr` — **but nothing feeds
  them**: the Scanning screen exits to ScanResult after one frame.
  Animated-QR reassembly and scan→import are unwired.
- `scripts/test_hw_qr_scanin.sh` (#29 P3b) encodes the whole flow with
  ONE manual step (a human aiming the GM65 at a screen). This session
  replaces the human+screen with the bench CYD (cyd-qr firmware, UART
  line protocol) and automates everything.
- Real tokens never fit one QR frame: the gm65-measured CYD→GM65
  decode envelope is ~92 bytes/frame (ECC-H, 224px) — every practical
  Cashu token needs UR multi-part framing
  (`ur:bytes/<i>-<N>/<hash>/<chunk>`, raw-ASCII chunks, any-order feed,
  hash-bound — see gm65-scanner `decoder.rs`).

## Work plan

### M1 firmware feature: animated UR scan-in (testable core)
- New `micronuts-app/src/scanflow.rs`: `ScanAssembler` +
  `ScanOutcome { KeepScanning{received,total}, TokenReady(TokenV4),
  ShowPayload(QrPayload), Invalid }` — pure logic, unit-tested
  (fragment sequences, out-of-order, hash mismatch, single-frame
  cashu token, plain text).
- `run()` Scanning screen: feed assembler; KeepScanning → progress
  bar + keep scanning; TokenReady → `state.imported_token`,
  `SwapState::TokenImported`, TokenInfo screen (same path as CDC
  import); ShowPayload → old ScanResult behavior; back/reset clears
  the assembler.

### M2 host harness: `tools/hil/` (gm65 pattern, blessed copy)
- `rig.py` + `gm65qr.py` adapted from gm65-scanner (they document
  them as copy-for-other-projects); CYD client identical, STM32 flash
  via st-flash + `--connect-under-reset reset`, F469 image
  backup/restore around every flash session (the board is shared
  with gm65-scanner sessions), CDC by product string (16c0:27dd).
- `urtoken.py`: host UR encoder (chunk ladder, sha256-prefix hash) +
  token source helpers (device export round-trip; cashu-ts re-auth
  optional).
- labgrid: place `micronuts-qr-rig` (binds
  `ai-legion-small-microfips/cyd-serial` + `.../stm32-stlink`),
  idempotent `labgrid-place.sh`, best-effort state tags
  (`owner=micronuts`), documentation places. Lock order: BenchLock →
  place, never reversed.

### M3 tests
- Regression gate `make test-qr-scanin` (pytest): scanner status,
  single-frame payload roundtrip, UR token ladder (4/8/12 frames),
  TokenInfo assertions, negatives (corrupted token, hash-mismatched
  fragments), backup/restore proof.
- Campaign `make test-qr-campaign`: E1 transfer soak (success/latency
  vs fragment count), E2 chunk-size envelope, E3 CYD dwell-time
  sweep, E4 negative battery, E5 full wallet flow (scan-in → blinded
  → sign → proofs → export → re-scan the export), timing profiles.
  Fault-isolated; artifacts under `tools/hil/results/campaign-*/`.

### M4 docs
- `tools/hil/LIMITATIONS.md` (measured envelope for cashu payloads),
  AGENTS.md rig section, run ledger `results/history.jsonl`.

## Safety / bench rules

- fips-lab `boards.toml` is the flash gate (stm32f469i-disco,
  cyd-ch340). We are the micronuts-wallet owners (F4691) — our
  flashes — but gm65 sessions expect their image restored.
- BenchLock first; coordinator 192.168.13.221:20408; place tags are
  the cross-project state surface.
- No mainnet. Demo keyset / FakeWallet only.

## Session results (2026-09-11, appended at close)

**F1–F3 GREEN on hardware; F4 (UR token e2e) partially proven — fragment
delivery characterized, full-coverage cadence left as the follow-up.**

What was proven end-to-end: CYD→GM65→F469 single-frame scans over the
wallet CDC (`scan_quiet`: render → trigger → QUIET ≥2.5 s → poll), the
on-device UR assembler (unit + CDC-path tests), device self-mint via
mint-tool, and the full harness stack (rig.py/gm65qr.py/urtoken.py/
mnscan.py + labgrid place `micronuts-qr-rig` + BenchLock ordering).

Root causes found and fixed (each cost a bench cycle — keep them quoted):

1. **USB CDC only appears after the boot splash — measured ~60 s wall**
   (41a5f7e correction), not the nominal 17.8 s. `wait_wallet_cdc`
   budgets 150 s.
2. **CDC scan capture**: ScannerTrigger must be followed by a firmware-side
   capture window (run-loop ticker harvests `read_scan` for 10 s). Host
   polling DURING the window breaks capture — the quiet cadence is a
   protocol requirement, not a preference.
3. **gm65-scanner #75**: Command mode ACKs ScanEnable writes but never
   scans. Firmware now enters silent Continuous mode after init
   (ScannerSettings read_mode=Continuous, buzzer=false → 0x92).
4. **Buzzer is Settings bit 6** (0xD2 beeps, 0x92 silent) — the crate
   documents this now (gm65-scanner findings + quotes).
5. **ACK-frame leak** (`02 00 00 01 <v> 00 33 31`) races decodes into scan
   data → firmware `read_scan` strips leading frames (gm65qr lesson).
6. **gm65-scanner pin was 4 months stale** (fa9b750, May) — bumped to
   8c043d1 (continuous-mode fixes, settings API); API migration in
   `set_aim` (bitflags → struct).
7. **Module-level sustained-load degradation** (gm65 #92): ~10 min of
   back-to-back renders/triggers collapses fresh-render decoding; static
   QRs keep decoding. Recovery: xHCI port power-cycle
   (`rig.recover_xhci_port`) + settle. Campaigns must pace scans
   (gm65 measured 8.5 s/scan sustained) and bake in the recovery.

F4 status: mint→UR(15×~70 B frames)→CYD cycle→scan works per-fragment
(type 0x03 returns observed), but full 15/15 coverage within the cycle
budget was not yet achieved (module pacing + the degradation above).
The assertion path is honest: reboot-to-clean-slate → token_info must
flip Error→Ok only via the QR path.

Follow-ups (highest value first):
- Adopt the gm65 crate `ScanPolicy::start_scanning` API once it lands
  (gm65-scanner AUDIT-2026-09-11-library-api.md) and delete the inline
  mode dance.
- F4 cadence sweep as the first campaign experiment (dwell 3–8 s, cycle
  caps, inter-token cooldown), then the E1–E5 campaign from the plan.
- Embassy `BufferedUart` for USART6 (research: gm65 #88) — removes the
  quiet-cadence contract and the polling shim.
