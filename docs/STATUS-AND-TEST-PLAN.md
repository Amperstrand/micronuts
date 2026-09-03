# Micronuts — Status, Test Plan & Readiness Review

**Date**: 2026-07-30
**Author**: Sisyphus (oh-my-opencode)
**Status**: Conference-credible v1.0 — software complete, hardware verification in progress

---

## 1. Current Status

### What Works (Verified)

| Feature | Status | Evidence |
|---|---|---|
| **Firmware boots on STM32F469I-Discovery** | ✅ Verified on hardware | SDRAM test PASS, display init, touch controller init, Embassy executor starts. RTT log confirmed on ai-legion via probe-rs. |
| **Native simulator (SDL2)** | ✅ Builds (mac) | SDL2 window renders 480×800, mouse→touch mapping. Not tested this session. |
| **hash_to_curve** | ✅ CDK-parity | Counter range fixed (0..=u16::MAX). 640 differential assertions vs cashu 0.17.3. Official NUT test vectors pass. |
| **NUT-12 DLEQ verification** | ✅ Correct | k256 port from CDK. All 3 official vectors pass. CDK round-trip. Negative test (bit-flip e) rejects. |
| **NUT-12 DLEQ construction** | ✅ Working | Mint produces DLEQ via cashu::BlindSignature::new(). V4 tokens carry DLEQ. |
| **NUT-00 CBOR token parsing** | ✅ V4 only | V3 JSON not supported (by design — embedded constraint). |
| **NUT-01/02/03/04/05/06/07/08/09/13** | ✅ Implemented | Types + mint logic. NUT-09 Restore is stateless demo (no persistence). |
| **cashu-audit conformance** | ✅ 67 PASS / 40 expected FAIL | Swap/melt working after verify_proofs fix. Remaining failures: NUT-20 locking, NUT-29 batch, accounting fields, stateless restore. |
| **Audit HTTP adapter** | ✅ All 13 endpoints | Real crypto round-trip verified (mint→swap→melt→checkstate over HTTP). |
| **ESP32 WiFi bridge** | ✅ Compiles | Xtensa toolchain on ai-legion. Not yet flashed/tested on hardware. |
| **CI (host-tests)** | ✅ Green | All tests pass on Ubuntu. Firmware-build + clippy continue-on-error (LLVM toolchain issues). |

### What Doesn't Work Yet

| Feature | Status | Blocker |
|---|---|---|
| **Firmware USB CDC serial transport** | ✅ Verified 2026-09-03 | Enumeration + full swap flow + decoder battery green on hardware (docs/HARDWARE-TEST-RESULTS-20260903.md). Splash times out after ~18 s — no touch needed. |
| **QR code scanning (GM65)** | ⏳ Not tested | Hardware not connected this session. Firmware has scanner support code. |
| **RNG quality verification** | ⏳ Not tested | Need 1M+ samples from hardware. T7.1 planned. |
| **Firmware CI build** | ⚠️ continue-on-error | Linux LLVM doesn't recognize ARM `wfe`/`sev` mnemonics. Fixed by patching cortex-m + embassy-executor sources to `.inst` binary encodings. Patch is in cargo registry (not persistent across `cargo clean`). |
| **RDP Level 2** | ✗ Cancelled | User decision — run in debug mode, no irreversible locks. |
| **WiFi transport** | 🔜 Future | ESP32 firmware compiles. Needs flashing + UART wiring to STM32. |
| **JavaCard/Satocash** | 🔜 Future | Secure microSD integration. Out of v1.0 scope. |

---

## 2. Pinned Dependencies Review

### Workspace-level pins (Cargo.toml)

| Crate | Pin | Source | Why Pinned | Recommendation |
|---|---|---|---|---|
| `gm65-scanner` | `rev = "fa9b750"` | `Amperstrand/gm65-scanner` | Amperstrand original (not a fork). Custom features: sync + async + defmt. | ✅ Keep pinned. No upstream to track. |
| `embassy-stm32f469i-disco` | `rev = "365bdff"` | `Amperstrand/embassy-stm32f469i-disco` | Amperstrand fork of Embassy BSP. Custom display/touch/USB/SDRAM init for F469I-DISCO. | ⚠️ Track upstream Embassy updates. Consider PRing F469I-DISCO support to main Embassy repo. |
| `embassy-stm32` | `0.6.0` (crates.io) | crates.io | Standard Embassy HAL. | ✅ Track crates.io releases. Bump on minor versions. |
| `cashu` (dev-dep) | `0.17.3` | crates.io | CDK reference implementation. Used as differential oracle in tests. | ✅ Latest stable. Bump when CDK releases new versions. |
| `cortex-m` | `0.7` | crates.io | ARM Cortex-M support. | ⚠️ **Patched on ai-legion** for wfe/sev issue. Need upstream fix or permanent patch. |
| `cortex-m-rt` | `0.7` | crates.io | Runtime/startup. | ✅ Standard. |
| `k256` | `0.13` | crates.io | secp256k1 (pure Rust). Core crypto. | ✅ Critical dependency. Pin minor version. |
| `defmt` | `1.0` | crates.io | Deferred formatting for embedded logging. | ✅ Stable. |

### Firmware-only pins

| Crate | Pin | Why |
|---|---|---|
| `panic-probe` | `1.0` (workspace, optional) | Panic handler with defmt. **Not working on Linux LLVM** — manual `#[panic_handler]` used instead. |
| `panic-halt` | `1.0` (workspace, non-optional) | Fallback panic handler. Always available. |

### The cortex-m / wfe Issue

**Root cause**: Linux LLVM (bundled with Rust nightly-2025-12-08 and stable 1.97.1) doesn't recognize ARM hint instruction mnemonics (`wfe`, `sev`) in inline assembly. macOS LLVM handles them correctly.

**Current workaround**: Patching `cortex-m-0.7.7/asm/inline.rs` and `embassy-executor-0.10.0/src/platform/cortex_m.rs` + `0.7.0/src/arch/cortex_m.rs` in the cargo registry to use `.inst 0xbf20` / `.inst 0xbf40` (binary encodings). This is **non-persistent** — `cargo clean` or CI runners lose the patch.

**Permanent fix options**:
1. Fork cortex-m + embassy-executor, publish patched versions
2. Use `[patch.crates-io]` in Cargo.toml pointing to Amperstrand forks
3. Wait for Rust LLVM to fix the ARM hint instruction recognition
4. Use a macOS CI runner for firmware builds

**Recommended**: Option 2 — create Amperstrand forks of cortex-m and embassy-executor with the `.inst` patches, add `[patch.crates-io]` entries. This makes the fix permanent and reproducible.

---

## 3. Test Plan

### Phase 1: Serial Transport (T6.2) — No QR scanner needed

1. **Skip boot splash** (code change or touch screen)
2. **Verify USB CDC enumerates** — check for new `/dev/ttyACM*` device after firmware boot
3. **Send test command** — `ImportToken` (0x01) with a known V4 token
4. **Verify response** — `GetTokenInfo` (0x02) returns expected token summary
5. **Full round-trip** — `GetBlinded` → `SendSignatures` → `GetProofs` → verify DLEQ

**Test tooling**: `host-mint-tool` (already built) or a Python script using pyserial.

### Phase 2: QR Code Scanning — Needs GM65 module

1. **Display Cashu QR code** on a laptop screen using `qrencode` or a web page
2. **Position GM65 scanner** to read the screen
3. **Send `ScannerTrigger` (0x11)** command via USB CDC
4. **Read `ScannerData` (0x12)** response
5. **Verify token parsing** — firmware displays decoded token on LCD

**QR code generation**:
```bash
# Generate a test Cashu token QR code
python3 -c "
import qrcode
token = 'cashuB...'  # a real V4 token
img = qrcode.make(token)
img.save('/tmp/cashu-qr.png')
img.show()  # display on screen
"
```

Or use the `nutshell` (Python Cashu wallet) to generate real tokens.

### Phase 3: Display & Touch (T6.4 partial)

1. **Boot splash animation** — verify LCD renders the retro nut logo grid
2. **Touch-to-exit** — verify FT6X06 touch controller detects screen taps
3. **Token info display** — after scanning a QR, verify LCD shows:
   - Token version (V4)
   - Keyset ID
   - Proof count + amounts
   - Total value
4. **Swap display** — after swap, LCD shows new denominations

### Phase 4: End-to-End Demo (T6.4)

1. **Mint**: `host-mint-tool` mints 100 sats via USB CDC → LCD displays "Minted: 100 sat"
2. **Swap**: host requests swap into [32,32,16,8,4,4,2,1,1] → LCD shows denominations
3. **Melt**: host requests melt 64 sats → LCD shows "Melted: 64 sat, Remaining: 36 sat"
4. **DLEQ verify**: at each mint step, verify blind signature DLEQ using T2.3's verify_dleq
5. **Photo evidence**: capture LCD screen at each step

### Phase 5: RNG Quality (T7.1)

1. **Diagnostic mode**: firmware draws 1M+ samples from hardware RNG
2. **Stream via RTT or serial**: capture raw bytes
3. **NIST SP 800-22**: run statistical test suite on captured bytes
4. **Document**: explicitly mark as "non-certified statistical check"

### Phase 6: ESP32 WiFi Bridge

1. **Flash ESP32 firmware** on ai-legion's ESP32-D0WD
2. **Configure WiFi** credentials in the firmware
3. **Verify TCP listener**: connect from laptop, echo test
4. **Wire ESP32 UART to STM32 UART** (cross-connect TX/RX)
5. **Full WiFi round-trip**: laptop → WiFi → ESP32 → UART → STM32 → USB CDC response

---

## 4. "Ready for Others" Checklist

### Must-have before public testing

- [ ] **Firmware builds reproducibly** — currently requires manual cortex-m patch on Linux. Need `[patch.crates-io]` solution.
- [ ] **Quick start guide** — step-by-step: install Rust + probe-rs, clone, build, flash, connect
- [ ] **Pre-built firmware binary** — provide a release binary so testers don't need to build
- [ ] **host-mint-tool documentation** — how to use the USB CDC protocol
- [ ] **QR code test vectors** — provide sample Cashu tokens for QR scanning tests
- [ ] **Hardware setup guide** — photos/diagrams of STM32 + GM65 + ESP32 wiring

### Should-have

- [ ] **CI green for firmware-build** — fix the cortex-m patch permanently
- [ ] **Video demo** — record the end-to-end flow on hardware
- [ ] **cashu-audit matrix badge** — show conformance pass rate in README
- [ ] **AGENTS.md at workspace root** — for AI-agent-friendly onboarding
- [ ] **Architecture decision records** — why no_std, why k256 not secp256k1, why CBOR not JSON

### Nice-to-have

- [ ] **Conference demo mode** — automated mint/swap/melt cycle for live demos
- [ ] **Web-based test UI** — simple web page that generates Cashu QR codes for scanning
- [ ] **ESP32 WiFi bridge documentation** — wiring diagram, flash instructions
- [ ] **Nostr integration** — send/receive Cashu tokens over Nostr relay (roadmap item 5)

---

## 5. Related Amperstrand Projects — Review & Recommendations

### Directly Relevant

| Project | Relevance | Action |
|---|---|---|
| **cashu-audit** | Conformance testing framework. We use it for T5.3 matrix runs. | ✅ Already integrated. 67/107 scenarios pass. |
| **cdk** (Cashu Development Kit) | Rust Cashu reference. We use `cashu = "0.17.3"` as dev-dep oracle. | ✅ Integrated. Track new releases. |
| **cashu-cf** | Cashu mint on Cloudflare Workers. | 🔜 **Test against real mint**: run cashu-audit matrix against cashu-cf (not just demo mint). Would validate adapter with real LN backend. |
| **nutshell** | Python Cashu wallet/mint reference. | 🔜 **Generate test tokens**: use nutshell to create real Cashu V4 tokens for QR scanning tests. |
| **cashu-ts** | TypeScript Cashu library. | 🔜 **Web QR generator**: use cashu-ts in a simple web page to display Cashu QR codes for scanning. |
| **microfips** | Minimal FIPS leaf node on STM32F469. | ⚠️ **This is the FIPS transport**. micronuts-fips-bridge is the adapter. Should verify compatibility. |
| **hackathon-tooling** | Reusable prompts/patterns. | 📋 Check for micronuts-specific patterns or audits. |

### Cross-Project Issues to Address

1. **micronuts-fips-bridge ↔ microfips**: The bridge crate assumes a MicroFIPS service interface. Need to verify it matches microfips's actual API. Create an integration test.

2. **Demo mint verify_proofs**: Fixed in this session (secret string bytes per DHKE spec). Check if `cashu-cf` or `nutshell` have the same bug.

3. **CDK version drift**: We're on cashu 0.17.3. CDK main branch may have breaking changes. Set up a monthly CDK compat check.

4. **cashu-audit matrix as CI gate**: Currently run manually. Could add as a CI step (using Python 3.12 container with coincurve).

5. **P2PK/HTLC (NUT-10/11/14)**: Deferred from micronuts v1.0. Check if cashu-cf supports these — if so, micronuts should add them to match.

---

## 6. Dependency Pin Action Items

| Priority | Action | Effort |
|---|---|---|
| **P0** | Create Amperstrand forks of cortex-m + embassy-executor with wfe/sev `.inst` patches. Add `[patch.crates-io]` entries. | 2 hours |
| **P1** | Track embassy-stm32f469i-disco upstream. Document which patches are needed vs which are in main Embassy. | 1 hour |
| **P1** | Set up CDK compat check (CI job that runs cashu_core_lite tests against latest CDK). | 1 hour |
| **P2** | Document all git pins in README with "why" and "how to update". | 30 min |
| **P2** | Consider moving gm65-scanner to crates.io (it's Amperstrand-original, not a fork). | 1 hour |
| **P3** | Evaluate cortex-m 0.8 / embassy-executor latest for wfe/sev fix. | 2 hours |
