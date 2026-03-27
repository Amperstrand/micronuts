# firmware

Embedded application for the STM32F469I-Discovery board. Uses the Embassy async framework.

## Build

```bash
# From workspace root — dev build
cargo build -p firmware --target thumbv7em-none-eabihf

# Release build (for flashing)
cargo build -p firmware --release --target thumbv7em-none-eabihf

# Output binary
target/thumbv7em-none-eabihf/release/firmware
```

## Flash and Run

```bash
# Normal flash + run with RTT defmt output
probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware

# Run hardware self-test (builds, flashes, captures RTT log)
./tests/flash_and_test.sh
```

### Recovery: probe-rs SWD stuck after USB active

When the firmware is running with USB CDC active, the STM32F469 can lock out SWD (SwdDpWait errors). To recover:

```bash
# Option 1: st-flash can connect under reset when probe-rs cannot
st-flash --connect-under-reset reset
# Then immediately run probe-rs (chip is halted briefly)
probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware

# Option 2: Full power cycle
# Unplug ALL USB cables from board, press+hold NRST, plug ST-LINK USB, release NRST

# Option 3: Flash via st-flash, then attach
arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/firmware target/thumbv7em-none-eabihf/release/firmware.bin
st-flash --connect-under-reset write target/thumbv7em-none-eabihf/release/firmware.bin 0x08000000
st-flash --connect-under-reset reset
probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware
```

## Lint / Check

```bash
cargo build -p firmware --target thumbv7em-none-eabihf 2>&1 | grep "error"
```

## Tests

```bash
# Software tests (no hardware needed)
cargo test --workspace --exclude firmware

# Hardware self-test (requires STM32F469I-Discovery board)
./tests/flash_and_test.sh
```

## Architecture

This crate is the **hardware adapter** for the STM32F469I-Discovery board. Business logic lives in `micronuts-app/`; this crate initializes peripherals and implements `MicronutsHardware` (async trait) to wire them together.

```
src/
├── main.rs              Embassy executor, RCC config (no PLLSAI), SDRAM/display/touch/USB/scanner init, boot splash, self-test
├── hardware_impl.rs     impl MicronutsHardware + Scanner for FirmwareHardware, RawFramebuffer, AsyncUart, USB CDC Sender/Receiver
├── build_info.rs        Compile-time build provenance (git hash, dep revs) via build.rs env vars
├── self_test.rs         Hardware self-test runner (SDRAM, RNG, heap, display, touch, scanner) — runs at boot, 5s interactive timeouts
├── boot_splash.rs       Retro boot splash animation engine
├── boot_splash_assets.rs Generated RGB565 tile data
├── lib.rs               Module declarations
└── qr/
    ├── mod.rs           Re-exports Gm65ScannerAsync from gm65-scanner
    └── decoder.rs       QR payload classification (Cashu V4/V3, UR, plain text)
```

## Key Dependencies

- `micronuts-app` (workspace member) — shared async business logic (protocol, display, state, commands)
- `embassy-stm32` @ `84444a19` (upstream) — MCU peripheral drivers (RNG, I2C, USART, USB OTG)
- `embassy-stm32f469i-disco` @ `3646aa87` — BSP for display (DSI/LTDC/NT35510), SDRAM, touch (FT6X06)
- `embassy-usb` @ `84444a19` (upstream) — USB CDC class
- `gm65-scanner` @ `c6c9487` — QR scanner async driver (NOT part of BSP)
- `embedded-hal-02` (0.2.x) — serial Read/Write traits used by AsyncUart for byte-level reads
- `cashu-core-lite` — no_std Cashu token decode, blind/unblind
- `k256` — secp256k1 for blind signatures
- `nb` — `nb::Result` for non-blocking UART operations

## Hardware Setup

- **Board**: STM32F469I-Discovery
- **Probe**: ST-Link v2/v3 via SWD
- **USB**: Connect USB OTG FS (PA11/PA12) to host for CDC communication
- **QR Scanner**: GM65 module connected via USART6 (PG14=TX, PG9=RX) through shield-lite Arduino headers
- **Display**: 4" DSI LCD (480x800 portrait), initialized by BSP's `DisplayCtrl`
- **Touch**: FT6X06 via I2C1 (PB8=SDA, PB9=SCL), blocking reads (acceptable at 5ms poll rate)
- **RNG**: STM32 hardware RNG with HASH_RNG interrupt handler

## USB CDC Protocol

Binary framing: `[Status:1][Len:2][Payload:N]`

| Command | Code | Payload | Response |
|---------|------|---------|----------|
| ImportToken | 0x01 | CBOR token bytes | Status |
| GetTokenInfo | 0x02 | empty | mint, unit, amount, proof_count |
| GetBlinded | 0x03 | empty | compressed pubkey points |
| SendSignatures | 0x04 | compressed pubkey points | Status |
| GetProofs | 0x05 | empty | encoded Cashu V4 token |
| ScannerStatus | 0x10 | empty | connected, initialized, model |
| ScannerTrigger | 0x11 | empty | Status |
| ScannerData | 0x12 | empty | raw scan data |

## Memory Layout

- Flash: 2048K (code ~175KB release, fits easily)
- SRAM: 320K (stack + small allocations)
- CCRAM: 64K
- SDRAM: 16MB (single framebuffer 768KB at offset 0, heap 128KB at offset 768KB)
- No double-buffering (single SDRAM buffer, potential tearing during animation)

## AsyncUart (Scanner UART)

USART6 is configured as `Blocking` mode, then wrapped in our custom `AsyncUart` which adds yield-aware polling:
- Reads via `embedded_hal_02::serial::Read::read()` in a spin loop
- Yields to executor after 2M spins (small buf) or 100K spins (large buf)
- `USART6.disable()` called at init to prevent interrupt handler from consuming UART data
- This is the proven pattern from the gm65-scanner firmware example — NOT native embassy USART

## BSP Pin Note

USART6 for the QR scanner uses PG14 (TX) and PG9 (RX). These pins are NOT consumed by the SDRAM controller.

## Build Provenance

`build.rs` embeds the following at compile time as `env!()` constants (used by `self_test.rs`):
- `GIT_HASH`, `GIT_DATE` — from `git rev-parse HEAD` / `git log`
- `BUILD_DATE` — UTC timestamp of build
- `EMBASSY_REV`, `BSP_REV`, `GM65_REV`, `STM32F469I_DISC_REV` — parsed from workspace `Cargo.toml`

## Self-Test

Runs automatically at boot after all peripherals are initialized. Tests:
1. **SDRAM** — write/read 4096 u16 pattern to framebuffer tail, verify readback
2. **RNG** — fill 256 bytes, check >150 unique values, <10 zeros, <10 0xFF
3. **Heap** — alloc 1024 bytes, write pattern, verify readback
4. **Display** — fill framebuffer green (0x07E0), wait 3s for visual confirmation, verify readback
5. **QR Display** — render "MICRONUTS SELF-TEST OK" as QR code, verify center pixel is non-zero (white QR background), clear to black
6. **Touch** — wait up to 5s for touch event (SKIP if no touch)
7. **Scanner** — enable aim laser, trigger scan, wait up to 5s for QR data, stop scan after (SKIP if no scan)

Results logged via defmt RTT. Interactive tests (touch/scanner) SKIP after 5s timeout (reduced from 60s for faster boot).

## Scanner Bug Fix (2026-03-26)

**Root cause**: `hardware_impl.rs::read_scan()` wrapped `scanner.read_scan()` with a 2-second `embassy_time::with_timeout()`. The gm65-scanner async driver's `do_read_scan()` has NO internal timeout — it yields cooperatively to the executor while waiting for UART data. The 2-second wrapper killed the read before any caller timeout could fire. The scanner was actively scanning (laser on) but `read_scan()` returned `None` after 2s, which callers interpreted as "no data" instead of "timeout expired, try again".

**Why it worked on main (sync branch)**: The sync driver has a built-in 500K spin-loop timeout inside `read_scan()` itself. The sync `try_read_scan()` was truly non-blocking (single `nb::read()` call). The main loop called `try_read()` in a polling pattern, so no inner timeout was needed.

**Fix**: Removed the 2-second inner timeout from `hardware_impl::read_scan()`. Now it delegates directly to `scanner.read_scan()` which yields cooperatively. All callers manage their own timeouts:
- Main loop (`lib.rs`): `with_timeout(100ms, hw.read_scan())` — quick poll, retry every 5ms tick
- Self-test (`self_test.rs`): `with_timeout(60s, hw.read_scan())` — long wait for interactive test

After timeout, callers call `hw.stop()` which sends `stop_scan()` command + `cancel_scan()` to reset both the driver state and GM65 hardware.

## Hardware Test Evidence

Tested on STM32F469I-Discovery board with ST-Link V2-1 probe.

**Commit**: `a46db97` (embassy branch)
**Dependency revs**: Embassy `84444a19`, BSP `a407fcd`, GM65 `85734ba`, stm32f469i-disc `da9fdb2`

### Test run 1 (2026-03-26, initial — before scanner fix)

| Subsystem | Result | Notes |
|-----------|--------|-------|
| SDRAM | PASS | 8192 bytes write/read verified |
| RNG | PASS (threshold issue) | 165 unique values, threshold was 200 (lowered to 150) |
| Heap | PASS | 1024 bytes alloc + pattern verified |
| Display | PASS | Green fill, 384000 pixels (480x800), readback verified |
| Touch | PASS | FT6X06 detected, touch at x=258 y=382 |
| Scanner init | PASS | GM65 connected, model identified |
| Scanner scan | PASS | 21 bytes received (first run) |
| Boot splash | PASS | Animation plays, touch-to-skip works |
| USB CDC | PASS | Enumeration as "Micronuts Cashu Hardware Wallet" |
| App flow | PASS | Screen transitions working (home → scanning → scan result) |

### Test run 2 (2026-03-26, after scanner timeout fix)

| Subsystem | Result | Notes |
|-----------|--------|-------|
| SDRAM | PASS | 8192 bytes write/read verified |
| RNG | PASS | 166 unique values in 256 bytes |
| Heap | PASS | 1024 bytes alloc + pattern verified |
| Display | PASS | Green fill, 384000 pixels (480x800), readback verified |
| Touch | PASS | FT6X06 detected, touch at x=313 y=277 |
| Scanner aim | PASS | Laser enabled via `set_aim(true)` command |
| Scanner scan | PASS | **23 bytes received from QR code scan** (laser on, QR scanned within 60s) |
| Boot splash | PASS | Animation plays |
| USB CDC | PASS | Enumeration works |
| App flow | PASS | Extensive touch interaction after self-test |

### Conclusion: All subsystems verified on hardware. Embassy async port is functional.

**NOTE**: QR display self-test added in `3b28197` but not yet verified on hardware (pending flash).

## Software Tests (CI)

Run with `cargo test --workspace --exclude firmware`. No hardware needed.

### Test counts (2026-03-27)

| Crate | Tests | Status |
|-------|-------|--------|
| micronuts-app | 39 | All passing |
| cashu-core-lite | 30 | All passing |
| **Total** | **69** | All passing |

### micronuts-app tests (39)

| Test | What it verifies |
|------|-----------------|
| `test_full_swap_flow` | ImportToken → GetBlinded → SendSignatures → GetProofs roundtrip |
| `test_import_token_valid` | Token import + state transition |
| `test_import_token_invalid` | Invalid CBOR rejected |
| `test_get_blinded_after_import` | Blinded message generation |
| `test_send_signatures_wrong_count` | Signature count mismatch detected |
| `test_get_proofs_no_proofs` | Error when no proofs available |
| `test_get_token_info_after_import` | Token info response format |
| `test_scanner_status_connected` | Scanner status payload |
| `test_scanner_status_disconnected` | Scanner disconnected status |
| `test_scanner_trigger_connected` | Trigger returns Ok |
| `test_scanner_trigger_disconnected` | Trigger returns ScannerNotConnected |
| `test_scanner_data_no_data` | No data returns NoScanData |
| `test_scanner_data_with_data` | Data returned with type byte |
| `test_render_qr_code_simple_text` | QR rendering produces black+white pixels |
| `test_render_qr_binary_small_data` | Binary QR encoding works |
| `test_render_qr_binary_empty_data` | Empty data rejected |
| `test_render_qr_binary_too_large` | Data >2953 bytes rejected |
| `test_render_show_proofs_with_sample_token` | ShowProofs QR rendering doesn't panic |
| `test_render_qr_code_empty_string` | Empty string encodes as minimal QR |
| `test_render_qr_code_2k_succeeds` | 2000 bytes fits QR capacity |
| `test_render_qr_code_4k_fails` | 4000 bytes exceeds QR capacity |
| `test_build_swap_token` | Token construction preserves mint/unit/memo/proofs |
| `test_build_swap_token_empty_proofs` | Empty proofs handled gracefully |
| `test_build_swap_token_uses_first_keyset_id` | First proof's keyset_id used |
| `test_new_state_is_idle` | Initial state is Idle |
| `test_swap_state_progression` | State transitions work correctly |
| `test_swap_state_equality` | State comparison works |
| `test_swap_state_copy` | State is Copy |
| `test_new_state_is_const` | State is const-constructible |
| + 7 protocol/frame tests | Frame encode/decode, response encoding |

### cashu-core-lite tests (30)

Covers: blind/unblind roundtrip, hash-to-curve (CDK vectors), signature verification, token encode/decode, amount calculation. See `cashu-core-lite/AGENTS.md`.

## USB CDC Stress Test (2026-03-26)

**IMPORTANT**: Do NOT use `probe-rs run` during USB testing. probe-rs sets RTT to blocking mode; if it disconnects without restoring non-blocking mode (probe-rs#2425), defmt calls can freeze the firmware by spin-looping inside a critical section, masking USB interrupts. Flash with `st-flash`, reset, wait 15s for boot+self-test, then test via pyserial.

### Alternative: RTT with `disable-blocking-mode`

```toml
# firmware/Cargo.toml
[features]
rtt-nonblocking = ["defmt", "dep:defmt-rtt", "defmt-rtt/disable-blocking-mode"]
```

Build with `--features rtt-nonblocking` to use probe-rs + RTT alongside USB CDC. Logs may be lost if the buffer fills, but the firmware will never freeze.

### Alternative: ITM/SWO

The STM32F469I-Discovery has SWO wired to the on-board ST-LINK. ITM writes are fire-and-forget (~1 CPU cycle, dropped if FIFO full). Use `defmt-itm` crate. See [Amperstrand/embassy-stm32f469i-disco#7](https://github.com/Amperstrand/embassy-stm32f469i-disco/issues/7) for details.

### Alternative: Build without defmt

```bash
cargo build -p firmware --release --no-default-features --target thumbv7em-none-eabihf
```

Zero defmt, zero RTT, zero overhead. Uses `panic-halt`. For production or USB-only testing.

### Alternative: DEFMT_LOG=off

```bash
DEFMT_LOG=off cargo build -p firmware --release --target thumbv7em-none-eabihf
```

Eliminates all defmt macro calls at compile time. RTT buffer still linked (~1.1KB RAM) but no runtime logging.

**Correct test methodology (st-flash)**:
```bash
arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/firmware target/thumbv7em-none-eabihf/release/firmware.bin
st-flash --connect-under-reset write target/thumbv7em-none-eabihf/release/firmware.bin 0x08000000
st-flash --connect-under-reset reset
# Wait 15s for boot + self-test
python3 tests/usb_stress_test.py /dev/ttyACM0
```

### Stress test results (600 commands, upstream embassy 84444a19)

| Metric | Value |
|--------|-------|
| Total commands | 600 |
| Successes | 600 (100%) |
| Failures | 0 |
| Total time | 1.19s |
| Commands/sec | 504 |
| Median latency | 1.8ms |
| p95 latency | 2.4ms |
| p99 latency | 3.2ms |
| Max latency | 4.2ms |

### Swap flow (raw protocol, all OK)

| Command | Status |
|---------|--------|
| ImportToken (CBOR token) | Ok |
| GetBlinded | Ok (3 blinded pubkey points) |
| SendSignatures | Ok |
| GetProofs | Ok (Cashu V4 token) |

## USB CDC Known Issues

### ZLP (Zero-Length Packet)

When response length is a multiple of 64 bytes (USB FS max packet size), the host won't process the transfer until a short packet arrives. Fixed in `hardware_impl.rs::transport_send()` — sends ZLP after `write_all()` when `len % 64 == 0`.

### probe-rs + RTT breaks USB (root cause of #15, corrected)

**Previous (incorrect) explanation**: "probe-rs halts the CPU for RTT reads."

**Correct explanation**: probe-rs sets defmt-rtt to `MODE_BLOCK_IF_FULL` when it connects. If probe-rs disconnects without restoring non-blocking mode ([probe-rs#2425](https://github.com/probe-rs/probe-rs/issues/2425)), any `defmt::info!()` call acquires a critical section and spin-loops waiting for the buffer to drain. Since probe-rs is gone, the firmware freezes with interrupts masked. The USB OTG ISR can't fire, so the host sees a disconnect.

Without probe-rs ever connecting, RTT stays in `MODE_NON_BLOCKING_TRIM` (default) — writes never block, and USB CDC works perfectly (600/600 stress test proven).

**Evidence**: gm65-scanner firmware (using old `usb-device`/`usbd-serial`, NOT embassy-usb) also fails enumeration with probe-rs attached, confirming it's a probe-rs + RTT coexistence issue, not an embassy bug.

## Embassy USB SNAK Investigation (Closed)

[embassy-rs/embassy#5738](https://github.com/embassy-rs/embassy/pull/5738) was closed after we determined the claimed USB IN endpoint hang was a misdiagnosis caused by probe-rs breaking USB enumeration. The SNAK observation itself is valid (embassy is the only DWC2 driver that sets SNAK on IN endpoints during config), but we could not reproduce any hang on our hardware (600/600 stress test on upstream `84444a19`).

**Current state**: Pinned to upstream `84444a19`.

See [Amperstrand/embassy#1](https://github.com/Amperstrand/embassy/issues/1) for ongoing SNAK investigation and [Amperstrand/micronuts#19](https://github.com/Amperstrand/micronuts/issues/19) for the full retrospective.

## Debugging Guardrails

### USB testing methodology

**CRITICAL: Never use probe-rs during USB testing** unless using `disable-blocking-mode`. probe-rs sets RTT to blocking mode; if it disconnects improperly, defmt calls can freeze the firmware (see probe-rs#2425). Use `st-flash` for deployment:

```bash
arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/firmware target/thumbv7em-none-eabihf/release/firmware.bin
st-flash --connect-under-reset write target/thumbv7em-none-eabihf/release/firmware.bin 0x08000000
st-flash --connect-under-reset reset
sleep 15  # wait for boot + self-test
python3 tests/usb_stress_test.py /dev/ttyACM1
```

### Connect-under-reset recovery

If the firmware freezes (RTT blocking mode, probe-rs#2425):
```bash
st-flash --connect-under-reset reset
st-flash --connect-under-reset write firmware.bin 0x08000000
st-flash --connect-under-reset reset
```

### General debugging rules

1. **Isolate variables before escalating.** Remove the debugger, swap cables, try a different board. File issues on our own repos first — only escalate upstream after reproducing with correct methodology and getting confirmation from another user.

2. **Don't read registers while the debugger has the CPU halted.** The state is undefined. Register captures under probe-rs are not evidence of a firmware bug.

3. **One board with probe-rs is not evidence of a systematic bug.** If we're the only ones seeing it, the problem is probably in our test setup.

4. **Minimal, separated changes.** One concern per PR. Don't bundle cleanup, errata workarounds, and speculative recovery paths into one commit.

5. **AI-assisted analysis needs skepticism.** A confident theory that's internally consistent can still be wrong. Always verify with hardware.
