# Hardware Test Results — 2026-07-30

**Hardware**: STM32F469I-Discovery on ai-legion via ST-LINK/V2-1
**Firmware**: Built from main @ dfd53c6 with [patch.crates-io] embassy-executor fix
**Toolchain**: nightly-2025-12-08 on Ubuntu Linux (ai-legion)

## Self-Test Results: 7/9 PASS, 0 FAIL, 2 SKIP

| Test | Result | Details |
|---|---|---|
| SDRAM | ✅ PASS | Quick test + full read/write verified |
| RNG | ✅ PASS | Hardware RNG produces valid random data |
| Heap | ✅ PASS | 131072 bytes from SDRAM, alloc/dealloc works |
| Heap stress | ✅ PASS | Stress allocation patterns pass |
| Display | ✅ PASS | LTDC + panel initialized, framebuffer works |
| Crypto blinding | ✅ PASS | hash_to_curve + blind_message on real 180MHz M4F |
| USB CDC protocol | ✅ PASS | Encode/decode round-trip verified |
| Touch | ⚠️ SKIP | No tap within 5s (expected — remote testing) |
| Scanner | ⚠️ SKIP | No QR code scanned, but GM65 laser turned ON and scanner acked commands |

## Scanner Hardware Verified

The GM65 scanner IS connected and responding:
- `Trigger ScanEnable: ack ok` — scanner accepted trigger command
- `Stop ScanEnable: ack ok` — scanner accepted stop command
- Laser turned on/off correctly

This proves the gm65-scanner driver + USART6 connection works on hardware.

## What's NOT Tested Yet

1. **USB CDC enumeration** — the STM32's USB OTG FS port (micro-B, CN5) needs a separate USB cable. Currently only the ST-LINK USB (mini-B) is connected.
2. **USB CDC data round-trip** — blocked on USB enumeration
3. **QR scan decode** — GM65 hardware works (acks commands) but needs a QR code positioned in front of it
4. **Touch accuracy** — FT6X06 initializes correctly but needs physical screen interaction
5. **Display content verification** — display initializes but we haven't photographed the screen to verify rendered content
6. **Full mint/swap/melt/DLEQ cycle** — blocked on USB CDC

## Build System

The `[patch.crates-io]` approach is the PERMANENT fix for the cortex-m/embassy-executor wfe/sev issue:
- `embassy-executor` patched to call `cortex_m::asm::wfe()`/`sev()` instead of inline `asm!()`
- cortex-m uses precompiled `.a` files (correct ARM instructions, no LLVM integrated assembler dependency)
- Zero manual cargo registry patches needed
- Survives `cargo clean`
- Works on Linux (ai-legion, GitHub Actions) AND macOS
- Repo: https://github.com/Amperstrand/embassy-executor-linux-fix

## Next Steps

1. Connect micro-B USB cable to CN5 (USB OTG FS) → test USB CDC enumeration
2. Display a Cashu QR code on a screen → point GM65 at it → test QR scan decode
3. Tap the screen → test touch accuracy + boot splash exit
4. Photograph the LCD → verify display content
