# Amperstrand Embedded Project Review — STM32/HAL/ESP32

**Date**: 2026-07-30
**Scope**: Identify relevant Amperstrand embedded projects, assess pin currency, recommend improvements.

---

## Project Inventory

### STM32 / HAL Projects

| Repo | MCU | Relevance | Last Push |
|---|---|---|---|
| **embassy-stm32f469i-disco** | STM32F469 | **CRITICAL** — BSP micronuts uses (pinned `365bdff`) | 2026-07-16 |
| **stm32f469i-disc** | STM32F469 | OLD BSP (pre-Embassy, referenced in stale README) | legacy |
| **microfips** | STM32F469 + ESP32 | **CRITICAL** — same board, full networking stack, FIPS transport | 2026-07-27 |
| **fips-lab** | STM32 + ESP32 | Lab orchestration for microfips hardware testing | 2026-07-22 |
| **fips-protocol-defs-mvp** | STM32 | FIPS protocol definitions | — |
| **otm8009a** | (driver) | Display driver fork (embedded-hal 1.0, defmt) | 2026-03-28 |
| **mb963-stm32l152-disco** | STM32L152 | Different board, but Embassy patterns reference | 2026-07-15 |
| **specter-diy** | STM32F469 | **HIGH** — DIY QR-code hardware wallet, has f469-disco support | 2026-07-22 |
| **ccid-firmware-rs** | (smart card) | CCID smart card reader firmware | — |

### ESP32 Projects

| Repo | MCU | Relevance | Notes |
|---|---|---|---|
| **embassy-hello-esp32** | ESP32-3248S035 | Embassy + touch + audio + Slint GUI | Reference for ESP32 Embassy patterns |
| **embassy-hello-tds3** | ESP32-S3 | T-Display S3 clock dashboard | ESP32-S3 Embassy reference |
| **bolty-rs** | ESP32 + MFRC522 | Bolt card firmware (NFC) | NFC patterns |
| **ESP-Miner** | ESP32 | Bitcoin ASIC miner | ESP32 networking reference |

### Testing Infrastructure

| Repo | Relevance |
|---|---|
| **tollgate-lab** | Labgrid-based device orchestration for STM32/ESP32 — could automate micronuts HW tests |
| **fips-lab** | Physical device orchestration for FIPS/microfips — similar labgrid setup |
| **cashu-audit** | Conformance testing — already integrated |

---

## Pin Assessment

### embassy-stm32f469i-disco (rev `365bdff`)

**Status: ✅ HAPPY — only 1 commit behind**

- Our pin: `365bdff`
- Latest: `388f5d2`
- Delta: 1 commit — "fix(ci): formatting + make package non-blocking (nt35510 not on crates.io)" — **CI-only change, no functional difference**
- BSP version: 0.2.0 with major features micronuts already uses:
  - `Board::new()` ergonomic init (SDRAM + display + touch + LEDs + button)
  - `extensive_hw_test` (39 tests, two-phase, CCMRAM result buffer)
  - `embedded-test` on-target HIL test runner
  - Clock presets (180MHz, 168MHz, USB-only)
  - `EdgeFilter` for FT6X06 phantom-touch rejection
  - nt35510 v0.2.1 + otm8009a display drivers
  - CI with feature matrix, clippy gate, doc gate

**Recommendation**: Bump to `388f5d2` for the CI fix. No code changes needed.

### gm65-scanner (rev `fa9b750`)

**Status: ✅ HAPPY — Amperstrand original, no upstream to track**

- Amperstrand-original crate (not a fork)
- Provides sync + async + defmt features
- QR scanner driver for GM65 module

**Recommendation**: Keep pinned. Consider publishing to crates.io for discoverability.

### cortex-m `0.7` (crates.io)

**Status: ⚠️ PROBLEM — wfe/sev inline assembly broken on Linux LLVM**

- The `asm!("wfe")` / `asm!("sev")` in cortex-m 0.7.7's `asm/inline.rs` is not recognized by Linux LLVM's ARM backend
- macOS LLVM handles these mnemonics correctly
- **Current workaround**: Patched cargo registry source (`.inst 0xbf20` / `.inst 0xbf40`) — non-persistent
- embassy-executor (0.7.0 + 0.10.0) also had wfe/sev — patched

**Recommendation (P0)**: Create Amperstrand fork with `.inst` patches, add `[patch.crates-io]` entry.

### otm8009a (crates.io, Amperstrand fork)

**Status: ✅ HAPPY — maintained, embedded-hal 1.0 compliant**

- Amperstrand fork of romixlab/otm8009a
- Upgraded to embedded-hal 1.0, edition 2024, optional defmt
- Used by the BSP for the B07 board revision display panel

**Recommendation**: Keep. Consider upstream PR to romixlab.

---

## Critical Finding: microfips Overlap

**microfips runs on the SAME STM32F469I-Discovery board** and has already solved several problems micronuts is still working on:

| Capability | microfips | micronuts | Action |
|---|---|---|---|
| USB CDC serial | ✅ Proven (bridge auto-reconnect) | ⏳ Not tested | **Reuse microfips USB CDC patterns** |
| WiFi bridge | ✅ ESP8266 bridge (C++/PlatformIO, working) | ✅ ESP32 bridge (Rust, compiles, not tested) | **Compare approaches. Consider ESP8266 as simpler alternative.** |
| BLE | ✅ GATT + L2CAP proven | ❌ Not in scope | Future |
| Transport-neutral service layer | ✅ `microfips-service` | ✅ `MintService` trait (similar pattern) | **Verify API compatibility between the two** |
| `micronuts-fips-bridge` | N/A (microfips IS the FIPS node) | ✅ Bridge crate exists | **This IS the integration point. Verify the ServiceHandler interface matches.** |
| Hardware testing | ✅ labgrid + fips-lab | ⏳ Manual | **Adopt tollgate-lab / fips-lab patterns** |
| Noise handshakes | ✅ Noise_IK/XK | ❌ Not in scope | Future (could secure the mint-wallet channel) |

**Recommendation**: Schedule a microfips ↔ micronuts-fips-bridge integration test. The bridge was designed for exactly this purpose but has never been tested against real microfips firmware.

---

## Critical Finding: specter-diy QR Code Patterns

**specter-diy is a DIY airgapped hardware wallet that uses QR codes for host communication** — exactly what micronuts does. It has:

- `f469-disco/` directory — STM32F469I-Discovery support
- QR code scanning via camera module
- QR code display via LCD
- Python test infrastructure (`simulate.py`, `hwidevice.py`)
- Cashu-aware (specter-diy is Bitcoin-focused)

**Recommendation**: Study specter-diy's QR scanning implementation for:
- Camera/scanner initialization patterns
- QR decoding pipeline (zbar/quirc on embedded)
- Display rendering of QR codes (for showing payment requests)
- Test infrastructure patterns

---

## Recommendations Summary

### P0 — Blocking reproducibility

1. **Fork cortex-m + embassy-executor** with `.inst` patches for wfe/sev. Add `[patch.crates-io]` in workspace Cargo.toml. This makes firmware builds reproducible on Linux.

### P1 — High value, moderate effort

2. **Bump embassy-stm32f469i-disco** to `388f5d2` (1 CI-fix commit ahead). Zero code changes.
3. **microfips ↔ micronuts-fips-bridge integration test**. Verify the ServiceHandler/MintService interface actually works between the two.
4. **Adopt tollgate-lab** for automated hardware testing. Both microfips and tollgate use labgrid — micronuts should join the same testing infrastructure.
5. **Study specter-diy QR patterns**. Reuse their camera/scanner initialization and QR decoding pipeline for micronuts's GM65 scanner.

### P2 — Should-do

6. **Compare ESP32 WiFi bridge approaches**. microfips has a proven ESP8266 bridge (C++/PlatformIO). micronuts has an ESP32 bridge (Rust/embassy). Evaluate which is better for production.
7. **Publish gm65-scanner to crates.io**. It's Amperstrand-original, not a fork.
8. **PR otm8009a changes upstream** to romixlab. The embedded-hal 1.0 + defmt improvements benefit everyone.
9. **Check cashu-cf** for the same verify_proofs bug we fixed. If it hex-decodes secrets, it has the same DHKE spec violation.

### P3 — Future exploration

10. **Noise handshakes** (from microfips) for securing the mint↔wallet channel.
11. **BLE transport** (from microfips) for phone-to-device communication.
12. **NFC** (from bolty-rs) for contactless Cashu payments.
