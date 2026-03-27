# micronuts-app

Platform-independent async business logic for the Micronuts Cashu hardware wallet.

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
Uses Embassy async primitives (`embassy-time`, `embassy-futures`).

```
src/
├── lib.rs              — Entry point: pub async fn run() with select/ticker main loop
├── hardware.rs          — MicronutsHardware async trait + Scanner trait (RPITIT futures)
├── protocol.rs          — USB CDC binary protocol (Command, Response, Frame, FrameDecoder)
├── state.rs             — FirmwareState, SwapState, ScannerInfo
├── display.rs           — Display rendering (generic over embedded-graphics DrawTarget, 480x800 portrait)
├── qr/
│   ├── mod.rs           — Re-exports from gm65-scanner + decoder
│   └── decoder.rs       — QR payload classification (Cashu V4/V3, UR, plain text)
├── command_handler.rs   — All handle_* async functions (token ops, scanner, crypto)
└── util.rs              — decode_hex, encode_hex, derive_demo_mint_key

examples/
└── native_sim.rs       — SDL2 window simulator with async mock hardware (embassy_executor std backend)
```

## MicronutsHardware Trait

Defined in `hardware.rs`. All methods use `impl Future` RPITIT syntax (async trait):

- `Display` — `embedded-graphics::DrawTarget<Color = Rgb565>` (480x800 portrait)
- `RNG` — `fn rng_fill_bytes(&mut [u8])`
- `Scanner` — async trigger, read_scan, stop, is_connected, set_aim
- `Transport` — async poll for incoming frames, async send responses
- `Touch` — `fn touch_get() -> Option<TouchPoint>` (sync, polled at 5ms)
- `Delay` — `async fn delay_ms(ms: u32)`

## Main Loop

`run()` uses `embassy_futures::select::select(transport_recv_frame(), poll_ticker.next())`:
- USB transport yields on data arrival
- 5ms ticker polls touch and scanner
- Scanner timeout: 10*200 ticks = 10 seconds

## Features

- `default` — no_std, for firmware
- `std` — enables `std`, `alloc` with std allocator (for native_sim)

## Dependencies

- `cashu-core-lite` — no_std Cashu token operations
- `embedded-graphics` — display rendering (DrawTarget trait)
- `gm65-scanner` — QR scanner protocol (async feature)
- `k256`, `sha2` — secp256k1 blind signature operations
- `qrcodegen-no-heap` — QR code generation
- `embassy-time` — async timers, Ticker
- `embassy-futures` — select macro for concurrent async

## Tests

58 tests total (28 micronuts-app + 30 cashu-core-lite):
- 18 command handler tests (`MockHardware` + `#[tokio::test]`) — protocol parsing, responses, state
- 5 state machine tests — screen flow, transitions
- 5 protocol codec tests — frame encode/decode
- 30 cashu-core-lite tests — crypto, hash-to-curve, token encoding

## Upstream Interaction Policy

**NEVER file PRs or issues on upstream projects (embassy-rs, stm32-rs, etc.) without human review and approval.** AI-generated bug diagnoses can be confidently wrong. Document findings in Amperstrand repos first and let a human decide whether to escalate.
