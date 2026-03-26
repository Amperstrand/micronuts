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
├── self_test.rs         Hardware self-test runner (SDRAM, RNG, heap, display, touch, scanner) — runs at boot, 60s interactive timeouts
├── boot_splash.rs       Retro boot splash animation engine
├── boot_splash_assets.rs Generated RGB565 tile data
├── lib.rs               Module declarations
└── qr/
    ├── mod.rs           Re-exports Gm65ScannerAsync from gm65-scanner
    └── decoder.rs       QR payload classification (Cashu V4/V3, UR, plain text)
```

## Key Dependencies

- `micronuts-app` (workspace member) — shared async business logic (protocol, display, state, commands)
- `embassy-stm32` @ `84444a19` — MCU peripheral drivers (RNG, I2C, USART, USB OTG)
- `embassy-stm32f469i-disco` @ `3646aa87` — BSP for display (DSI/LTDC/NT35510), SDRAM, touch (FT6X06)
- `embassy-usb` @ `84444a19` — USB CDC class
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
5. **Touch** — wait up to 60s for touch event (SKIP if no touch)
6. **Scanner** — trigger scan, wait up to 60s for QR data (SKIP if no scan)

Results logged via defmt RTT. Interactive tests (touch/scanner) SKIP after 60s timeout.

## Hardware Test Evidence

Tested on STM32F469I-Discovery board with ST-Link V2-1 probe.

**Test date**: 2026-03-26
**Commit**: `a46db97` (embassy branch)
**Dependency revs**: Embassy `84444a19`, BSP `3646aa87`, GM65 `c6c9487`, stm32f469i-disc `da9fdb2`

| Subsystem | Result | Notes |
|-----------|--------|-------|
| SDRAM | PASS | 8192 bytes write/read verified |
| RNG | PASS | 159 unique values in 256 bytes |
| Heap | PASS | 1024 bytes alloc + pattern verified |
| Display | PASS | Green fill, 384000 pixels (480x800), readback verified |
| Touch | PASS | FT6X06 detected, touch at x=258 y=382 |
| Scanner init | PASS | GM65 connected, model identified |
| Scanner scan | PASS | 21 bytes received (first run), SKIP (second run, no QR scanned within 60s) |
| Boot splash | PASS | Animation plays, touch-to-skip works |
| USB CDC | PASS | Enumeration as "Micronuts Cashu Hardware Wallet", protocol works |
| App touch | PASS | Multiple button presses detected (x=309 y=411, x=289 y=439, x=251 y=362, etc) |
| App flow | PASS | Screen transitions working (home → scanning → scan result) |
