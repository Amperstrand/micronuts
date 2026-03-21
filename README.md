# Micronuts

**Cashu Hardware Wallet Proof of Concept** for the STM32F469I-Discovery board.

Micronuts is an experimental hardware wallet implementation for [Cashu](https://github.com/cashubtc/nuts) ecash tokens. This is a **proof of concept** exploring embedded blind signature operations on bare metal.

## Status: Experimental POC

This is NOT a production wallet. Do NOT use with real funds. The goal is to demonstrate that Cashu's blind signature flow can work on constrained embedded hardware.

## Target Hardware

- **Board**: STM32F469I-Discovery (STM32F469NIH6 MCU)
- **MCU**: ARM Cortex-M4F @ 180MHz, 2MB Flash, 384KB SRAM
- **Display**: 4" DSI LCD (NT35510/OTM8009A, 800x480)
- **Touch**: FT6X06 capacitive touch controller
- **QR Scanner**: GM65 module via USART6 (PG14=TX, PG9=RX through shield-lite adapter)
- **Storage**: 16MB SDRAM + microSD via SDIO
- **USB**: USB OTG FS (CDC-ACM for host communication)

## Project Structure

```
micronuts/
├── Cargo.toml              # Workspace definition
├── firmware/               # Embedded app for STM32F469I-Discovery
│   ├── Cargo.toml
│   ├── build.rs            # Copies memory.x to OUT_DIR for linker
│   ├── memory.x            # STM32F469 memory layout (2048K flash, 320K RAM)
│   └── src/
│       ├── main.rs         # Entry point, hardware init, main loop
│       ├── usb.rs          # USB CDC binary protocol
│       ├── display.rs      # LCD rendering
│       ├── firmware_state.rs
│       ├── prng.rs
│       └── qr/             # QR scanner module
│           ├── driver.rs   # GM65 UART driver (sync, modeled on specter-diy)
│           ├── protocol.rs # GM65 command builder (legacy, unused in driver)
│           └── decoder.rs  # QR payload classification (Cashu, UR, plain text)
├── cashu-core-lite/        # Minimal Cashu library (no_std + alloc)
├── host-mint-tool/         # Demo mint signer CLI for host PC
└── docs/
    ├── ARCHITECTURE.md
    ├── DEMO-FLOW.md
    ├── QR-SCANNER.md
    ├── QR-SCANNER-DESIGN.md
    └── GM65-PROTOCOL-FINDINGS.md
```

## Dependencies

- **BSP**: [`stm32f469i-disc`](https://github.com/Amperstrand/stm32f469i-disc) @ `fa6dc86`
- **HAL**: stm32f4xx-hal @ `789e5e86` (via BSP)
- **Crypto**: `k256` (secp256k1), `sha2` (SHA-256)
- **CBOR**: `minicbor` (no_std compatible)
- **Scanner reference**: [specter-diy qr.py](https://github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py)

## Quick Start

```bash
# Install target and probe-rs
rustup target add thumbv7em-none-eabihf
cargo install probe-rs-tools

# Build
cargo build --release

# Flash and run (output via RTT)
probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware
```

## USB CDC Protocol

Binary protocol: `[Cmd:1][Len:2][Payload:N]` / `[Status:1][Len:2][Payload:N]`

| Command | Code | Description |
|---------|------|-------------|
| ImportToken | 0x01 | Send V4 token |
| GetTokenInfo | 0x02 | Request summary |
| GetBlinded | 0x03 | Request blinded outputs |
| SendSignatures | 0x04 | Send blind signatures |
| GetProofs | 0x05 | Request unblinded proofs |
| ScannerStatus | 0x10 | QR scanner connection status |
| ScannerTrigger | 0x11 | Trigger QR scan |
| ScannerData | 0x12 | Read last scanned data |

## Documentation

- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design and component split
- [DEMO-FLOW.md](docs/DEMO-FLOW.md) — First vertical slice demo plan
- [QR-SCANNER.md](docs/QR-SCANNER.md) — QR scanner integration design
- [GM65-PROTOCOL-FINDINGS.md](docs/GM65-PROTOCOL-FINDINGS.md) — GM65 protocol reverse-engineering notes

## Baseline

This project builds on the STM32F469I-DISCOVERY board support package:

- **Upstream BSP**: `https://github.com/Amperstrand/stm32f469i-disc`
- **Pinned commit**: `fa6dc86` (display/SDRAM working)
- **HAL**: stm32f4xx-hal with DSI, SDRAM, SDIO, USB FS support

## Credits

- BSP foundation: [stm32f469i-disc](https://github.com/Amperstrand/stm32f469i-disc)
- Scanner protocol: [specter-diy](https://github.com/cryptoadvance/specter-diy)
- Cashu protocol: [cashubtc/nuts](https://github.com/cashubtc/nuts)

## License

[0-clause BSD license](LICENSE-0BSD.txt)
