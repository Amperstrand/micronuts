# micronuts-app

Platform-independent business logic for the Micronuts Cashu hardware wallet.

## Build

```bash
# Build the library (no_std, for firmware use)
cargo build -p micronuts-app

# Run tests
cargo test -p micronuts-app

# Build the native SDL2 simulator
# Requires: sudo apt install libsdl2-dev libsdl2-gfx-dev
cargo run -p micronuts-app --example native_sim --features std
```

## Architecture

Core crate that contains all hardware-independent logic, extracted from the `firmware` crate.
Designed so the same code runs on both the STM32F469 firmware and the desktop simulator.

```
src/
├── lib.rs              — Entry point: re-exports, pub fn run()
├── hardware.rs          — MicronronutsHardware trait definition
├── protocol.rs          — USB CDC binary protocol (Command, Response, Frame, FrameDecoder)
├── state.rs             — FirmwareState, SwapState, ScannerInfo
├── display.rs           — Display rendering (generic over embedded-graphics DrawTarget)
├── qr/
│   ├── mod.rs           — Re-exports from gm65-scanner + decoder
│   └── decoder.rs       — QR payload classification (Cashu V4/V3, UR, plain text)
├── command_handler.rs   — All handle_* functions (token ops, scanner, crypto)
└── util.rs              — decode_hex, encode_hex, derive_demo_mint_key

examples/
└── native_sim.rs       — SDL2 window simulator with mock hardware
```

## MicronutsHardware Trait

Defined in `hardware.rs`. Abstracts all hardware dependencies:

- `Display` — `embedded-graphics::DrawTarget<Color = Rgb565>` (800x480)
- `RNG` — `fn rng_fill_bytes(&mut [u8])`
- `Scanner` — trigger scan, try read, status
- `Transport` — poll for incoming frames, send responses
- `Touch` — get touch point (x, y, detected)
- `Delay` — `fn delay_ms(ms: u32)`

## Features

- `default` — no_std, for firmware
- `std` — enables `std`, `alloc` with std allocator

## Dependencies

- `cashu-core-lite` — no_std Cashu token operations
- `embedded-graphics` — display rendering (DrawTarget trait)
- `gm65-scanner` — QR scanner protocol (sync driver)
- `k256`, `sha2` — secp256k1 blind signature operations
- `qrcodegen-no-heap` — QR code generation
