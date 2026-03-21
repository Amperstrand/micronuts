# firmware

Embedded application for the STM32F469I-Discovery board.

## Build

```bash
# From workspace root
cargo build --release

# Output binary
target/thumbv7em-none-eabihf/release/firmware
```

## Flash and Run

```bash
# Flash and run with RTT output
probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware

# Flash only (no RTT)
probe-rs download --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware
```

## Lint / Check

```bash
cargo build 2>&1 | grep "error"
```

No formal linter is configured. The project compiles with warnings that should be addressed.

## Architecture

```
src/main.rs          Entry point: hardware init, main loop, command dispatch
src/usb.rs           USB CDC binary protocol (frame codec, CdcPort wrapper)
src/display.rs       LCD rendering via embedded-graphics (LtdcFramebuffer)
src/firmware_state.rs  Swap state machine for Cashu token operations
src/prng.rs          Simple PRNG using DWT cycle counter
src/qr/
  driver.rs          GM65 scanner UART driver (synchronous, specter-diy protocol)
  protocol.rs        GM65 command builder (legacy, see docs/GM65-PROTOCOL-FINDINGS.md)
  decoder.rs         QR payload classification (Cashu V4, UR, plain text)
```

## Key Dependencies

- `stm32f469i-disc` (BSP) @ `fa6dc86` — display, SDRAM, USB, GPIO
- `embedded-hal-02` (0.2.x) — serial Read/Write traits for scanner UART
- `cashu-core-lite` — no_std Cashu token decode, blind/unblind
- `k256` — secp256k1 for blind signatures
- `nb` — `nb::Result` for non-blocking UART operations

## Hardware Setup

- **Board**: STM32F469I-Discovery
- **Probe**: ST-Link v2/v3 via SWD
- **USB**: Connect USB OTG FS (PA11/PA12) to host for CDC communication
- **QR Scanner**: GM65 module connected via USART6 (PG14=TX, PG9=RX) through shield-lite Arduino headers
- **Display**: 4" DSI LCD, initialized automatically at boot

## USB CDC Protocol

Binary framing: `[Cmd:1][Len:2][Payload:N]`

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

- Flash: 2048K (code ~163KB, plenty of room)
- SRAM: 320K (stack + small allocations)
- CCRAM: 64K
- SDRAM: 16MB (heap allocator in `firmware/src/main.rs` — 128KB after framebuffers)
- Framebuffers: 2x 800x480x2 = 1.5MB in SDRAM

## BSP Pin Note

USART6 for the QR scanner uses PG14 (TX) and PG9 (RX). These pins are NOT consumed by the SDRAM controller. They must be extracted from the `gpiog` port BEFORE the `sdram_pins!` macro takes ownership. See `main.rs` lines 78-81.

## build.rs

Copies `memory.x` to `OUT_DIR` and adds it to the linker search path. This resolves conflicts with the ft6x06 crate which also ships a `memory.x`. The linker flags in `.cargo/config.toml` (`-Tlink.x -Tdefmt.x`) handle the actual linking.
