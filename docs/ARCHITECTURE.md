# Micronuts Architecture

## Overview

Micronuts is a **Cashu hardware wallet proof of concept** demonstrating blind signature operations on the STM32F469I-Discovery board. The architecture splits responsibilities between the embedded firmware and a host-side demo tool.

The core business logic lives in `micronuts-app/`, which is platform-independent. Two hardware adapters implement the `MicronutsHardware` trait:

- **`firmware/`** — real STM32F469I-Discovery peripherals (LCD, USB CDC, GM65 scanner, HW RNG)
- **`examples/native_sim.rs`** — SDL2 window on your PC (mock display, mock scanner, stdin/stdout transport)

```
┌─────────────────────────────────────────────────────────────────────┐
│                          HOST PC                                     │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    host-mint-tool                            │    │
│  │  ┌─────────────┐   ┌──────────────┐   ┌─────────────────┐   │    │
│  │  │  Demo Mint  │   │   CLI/UI     │   │  USB CDC Serial │   │    │
│  │  │  Private K  │──▶│   Protocol   │──▶│    Port         │   │    │
│  │  └─────────────┘   └──────────────┘   └─────────────────┘   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                              │ USB                   │
└──────────────────────────────────────────────│───────────────────────┘
                                                │
┌──────────────────────────────────────────────│───────────────────────┐
│                   STM32F469I-DISCOVERY        │                       │
│  ┌────────────────────────────────────────────│───────────────────┐  │
│  │  micronuts-app (shared core)              ▼                   │  │
│  │  ┌─────────────┐   ┌──────────────┐   ┌─────────────────┐     │  │
│  │  │ USB CDC     │   │ cashu-core-  │   │  Display (gen   │     │  │
│  │  │ Protocol    │──▶│ lite         │──▶│  over DrawTarget)│     │  │
│  │  └─────────────┘   │ (no_std)     │   └─────────────────┘     │  │
│  │                    └──────────────┘                           │  │
│  └───────────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │  firmware/ (Embassy async, impl MicronutsHardware)            │  │
│  │  embassy-stm32 · embassy-usb · embassy-stm32f469i-disco BSP   │  │
│  │  Display → RawFramebuffer │ Scanner → GM65 USART6 │ RNG → HW  │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                       │
│  Hardware: 180MHz Cortex-M4F, 2MB Flash, 384KB SRAM, 16MB SDRAM      │
└───────────────────────────────────────────────────────────────────────┘

┌───────────────────────────────────────────────────────────────────────┐
│  NATIVE SIMULATOR (same micronuts-app, different HW adapter)          │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │  examples/native_sim.rs — impl MicronutsHardware                │  │
│  │  Display → Sdl2Display (480x800 SDL2 window, mouse→touch)      │  │
│  │  Scanner → mock (stdin paste) │ Transport → stdin/stdout        │  │
│  └─────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────┘
```

## Components

### 0. `micronuts-app/` (Shared Core)

Platform-independent business logic, used by both the firmware and the native simulator.

**Dependencies:**
- `cashu-core-lite` (workspace member)
- `embedded-graphics` — display rendering via `DrawTarget` trait
- `gm65-scanner` (git dependency, pinned commit)
- `k256`, `sha2` — secp256k1 blind signature operations

**Responsibilities:**
- `run()` async main loop — yields on USB transport or 5ms ticker, dispatches commands, renders display
- USB CDC binary protocol (Command, Response, Frame, FrameDecoder)
- Display rendering (generic over `DrawTarget<Color = Rgb565>`)
- All command handlers (token import, blind/unblind, scanner control)
- QR payload classification (Cashu V4/V3, UR, plain text)
- Firmware state machine (SwapState, ScannerInfo)

**Features:**
- `default` — `no_std`, for firmware use
- `std` — enables `std`, `alloc` with std allocator (required for simulator)

### 1. `firmware/` (Embedded Application — Hardware Adapter)

The embedded firmware running on the STM32F469I-Discovery. Initializes peripherals and implements `MicronutsHardware`.

**Dependencies:**
- `micronuts-app` (workspace member) — shared business logic
- `embassy-stm32f469i-disco` BSP (git dependency, pinned commit)
- `gm65-scanner` (git dependency, pinned commit)
- `cashu-core-lite` (workspace member)
- `embassy-stm32`, `embassy-usb`, `embassy-time`, `embassy-executor`, `embassy-futures`, `embassy-sync`
- `cortex-m`, `cortex-m-rt`, `defmt`, `rand_core`

**Responsibilities:**
- Initialize hardware (display, USB, SDRAM, RNG, QR scanner)
- Boot splash animation
- Implement `MicronutsHardware` trait for STM32 peripherals
- USB CDC transport (embassy-usb CDC-ACM `Receiver`/`Sender`)
- Run Embassy executor (single `usb_task` spawned, main loop on executor thread)

**Build target:** `thumbv7em-none-eabihf`

### 2. `cashu-core-lite/` (Minimal Cashu Library)

`no_std + alloc` library for Cashu operations.

**Scope:**
- V4 token decode (CBOR)
- Proof structure
- `hash_to_curve` implementation
- Blinding/unblinding operations

**Dependencies:**
- `k256` — secp256k1 elliptic curve
- `sha2` — SHA-256
- `rand_core` — RngCore trait (no_std)
- `minicbor` — CBOR parsing

### 3. `host-mint-tool/` (Demo Mint Signer)

CLI tool running on the host PC.

**Responsibilities:**
- Hold demo mint private key
- Sign blinded outputs
- Generate test tokens
- Communicate via USB CDC

## Cryptographic Flow

```
DEVICE                                HOST (Demo Mint)
  │                                        │
  │  1. Generate secret x                  │
  │  2. Y = hash_to_curve(x)               │
  │  3. Pick random blinder r (HW RNG)     │
  │  4. B' = Y + r*G  (blinded)            │
  │                                        │
  │──── B' (blinded message) ─────────────▶│
  │                                        │  5. C' = k*B' (sign)
  │◀─── C' (blind signature) ──────────────│
  │                                        │
  │  6. C = C' - r*K  (unblind)            │
  │  7. Store (x, C) as proof              │
  │                                        │
```

Blinder entropy comes from the STM32F469 hardware RNG peripheral (analog ring oscillators, not a PRNG). See [issue #1](https://github.com/Amperstrand/micronuts/issues/1) for security analysis.

## Communication Protocol

USB CDC-ACM binary protocol:

```
Request:  [Cmd:1][Len:2][Payload:N]
Response: [Status:1][Len:2][Payload:N]

Commands:
  0x01 IMPORT_TOKEN    - Send V4 token
  0x02 GET_TOKEN_INFO  - Request summary
  0x03 GET_BLINDED     - Request blinded outputs
  0x04 SEND_SIGNATURES - Send blind signatures
  0x05 GET_PROOFS      - Request unblinded proofs
  0x10 SCANNER_STATUS  - QR scanner connection status
  0x11 SCANNER_TRIGGER - Trigger QR scan
  0x12 SCANNER_DATA    - Read last scanned data
```

## Memory Layout

```
Flash (2MB):   Code + rodata
SRAM (384KB):  Stack + heap + USB buffers
SDRAM (16MB):  Framebuffer (768KB) + heap allocator (128KB) + large allocs
```

## Random Number Generation

The STM32F469NI has a hardware RNG peripheral (reference manual section 24):

- **Source**: Analog ring oscillators — true physical entropy, not a PRNG
- **Output**: 32-bit words via `RNG->DR` register
- **Health checks**: CECS (clock error) and SECS (seed error) detection
- **HAL**: `embassy_stm32::rng::Rng` (interrupt-driven via `HASH_RNG`)
- **Requirement**: `PLL48CLK` must be active (already enabled for USB)
- **Throughput**: ~40 cycles per word at 48 MHz

See [issue #1](https://github.com/Amperstrand/micronuts/issues/1) for remaining security audit items.

## Pinned Dependencies

All git dependencies are pinned to specific commits for reproducibility:

| Crate | Pin | Why |
|-------|-----|-----|
| `embassy-stm32` | `84444a19` | Upstream Embassy. MCU peripheral drivers (RNG, I2C, USART, USB OTG, RCC config). Same rev used for all Embassy crates below. |
| `embassy-usb` | `84444a19` | Upstream Embassy. USB device stack and CDC-ACM class. |
| `embassy-stm32f469i-disco` | `a407fcd` | Embassy BSP for STM32F469I-Discovery. Display (DSI/LTDC/NT35510), SDRAM controller, FT6X06 touch. |
| `gm65-scanner` | `85734ba` | QR scanner async driver. HIL-tested on hardware. |

## Dual-Run Architecture

The `MicronutsHardware` trait (defined in `micronuts-app/src/hardware.rs`) abstracts all hardware dependencies:

| Method | Firmware impl | Simulator impl |
|--------|--------------|----------------|
| `Display` (DrawTarget) | `RawFramebuffer` (direct SDRAM buffer) | `Sdl2Display` → SDL2 texture (RGB565) |
| `Touch` | FT6X06 I2C capacitive touch controller | Mouse click → (x, y) mapping |
| `Scanner` | GM65 module on USART6 (async driver) | Mock (stdin paste) |
| `Transport` | embassy-usb CDC-ACM `Receiver`/`Sender` | stdin/stdout (mock frames) |
| `RNG` | `embassy_stm32::rng::Rng` (ring oscillators) | `rand::thread_rng()` |
| `Delay` | `embassy_time::Timer::after()` | `std::thread::sleep` |

Both adapters call `micronuts_app::run(&mut hw).await` which runs the identical async main loop. The `MicronutsHardware` trait uses RPITIT async methods (no `async_trait` macro). The simulator renders a 480x800 portrait SDL2 window that shows exactly what the LCD would display. Mouse clicks map to touch coordinates.

### Running

```bash
# Simulator (no cross-compiler needed)
sudo apt install libsdl2-dev
cargo run -p micronuts-app --example native_sim --features std

# Firmware
rustup target add thumbv7em-none-eabihf
cargo build --release
probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware
```

## Baseline

The BSP is a git dependency:

```toml
[workspace.dependencies]
embassy-stm32f469i-disco = { git = "https://github.com/Amperstrand/embassy-stm32f469i-disco", rev = "a407fcd" }
```

Embassy crates are pinned to the same upstream commit:

```toml
[workspace.dependencies]
embassy-stm32 = { git = "https://github.com/embassy-rs/embassy", package = "embassy-stm32", rev = "84444a19" }
embassy-usb = { git = "https://github.com/embassy-rs/embassy", package = "embassy-usb", rev = "84444a19" }
embassy-time = { git = "https://github.com/embassy-rs/embassy", package = "embassy-time", rev = "84444a19" }
embassy-executor = { git = "https://github.com/embassy-rs/embassy", package = "embassy-executor", rev = "84444a19" }
embassy-futures = { git = "https://github.com/embassy-rs/embassy", package = "embassy-futures", rev = "84444a19" }
embassy-sync = { git = "https://github.com/embassy-rs/embassy", package = "embassy-sync", rev = "84444a19" }
```

This preserves the BSP and Embassy as separate, versioned dependencies. The executor runs in `executor-thread` mode with a single spawned task (`usb_task`).
