# Micronuts

**Cashu Hardware Wallet Proof of Concept** for the STM32F469I-Discovery board.

Micronuts is an experimental hardware wallet implementation for [Cashu](https://github.com/cashubtc/nuts) ecash tokens. This is a **proof of concept** exploring embedded blind signature operations on bare metal.

## ⚠️ Status: Experimental POC

This is NOT a production wallet. Do NOT use with real funds. The goal is to demonstrate that Cashu's blind signature flow can work on constrained embedded hardware.

## Target Hardware

- **Board**: STM32F469I-Discovery (STM32F469NIH6 MCU)
- **MCU**: ARM Cortex-M4F @ 180MHz, 2MB Flash, 384KB SRAM
- **Display**: 4" DSI LCD (NT35510/OTM8009A, 800x480)
- **Touch**: FT6X06 capacitive touch controller
- **Storage**: 16MB SDRAM + microSD via SDIO
- **USB**: USB OTG FS (CDC-ACM for host communication)

## Project Structure

```
micronuts/
├── Cargo.toml              # Workspace definition
├── firmware/               # Embedded app for STM32F469I-Discovery
│   ├── Cargo.toml
│   └── src/
├── cashu-core-lite/        # Minimal Cashu library (no_std + alloc)
│   ├── Cargo.toml
│   └── src/
├── host-mint-tool/         # Demo mint signer CLI for host PC
│   ├── Cargo.toml
│   └── src/
└── docs/
    ├── ARCHITECTURE.md     # System design
    └── DEMO-FLOW.md        # First vertical slice demo
```

## Dependencies

- **BSP**: [`stm32f469i-disc`](https://github.com/Amperstrand/stm32f469i-disc) @ `c71065da588b9256e26557c57c103954cf7915fe`
- **Crypto**: `k256` (secp256k1), `sha2` (SHA-256)
- **CBOR**: `minicbor` (no_std compatible)

## First Vertical Slice Goal

The first demo will:

1. **Decode** a Cashu V4 token on the device
2. **Display** mint URL, unit, amount, proof count
3. **Generate** blinded outputs on-device (secp256k1)
4. **Communicate** with host demo mint tool via USB CDC
5. **Receive** blind signatures from host
6. **Unblind** signatures into proofs on-device
7. **Export** the resulting token

### Explicitly Out of Scope (for now)

- Lightning integration
- Quote/payment flow
- Melt operations
- Multi-mint swap/send
- WebSockets or HTTPS to public mints
- Full CDK wallet port

## Quick Start

```bash
# Install target
rustup target add thumbv7em-none-eabihf

# Build workspace
cargo build --release

# Flash firmware (requires probe-rs)
cargo run --release -p firmware
```

## Documentation

- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design and component split
- [DEMO-FLOW.md](docs/DEMO-FLOW.md) — First vertical slice demo plan

## Baseline

This project builds on the STM32F469I-DISCOVERY board support package:

- **Upstream BSP**: `https://github.com/Amperstrand/stm32f469i-disc`
- **Pinned commit**: `c71065da588b9256e26557c57c103954cf7915fe` (sdio-support)
- **HAL**: stm32f4xx-hal with DSI, SDRAM, SDIO, USB FS support

## Credits

- BSP foundation: [stm32f469i-disc](https://github.com/Amperstrand/stm32f469i-disc)
- Cashu protocol: [cashubtc/nuts](https://github.com/cashubtc/nuts)

## License

[0-clause BSD license](LICENSE-0BSD.txt)
