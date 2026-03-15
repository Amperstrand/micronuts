# Micronuts Architecture

## Overview

Micronuts is a **Cashu hardware wallet proof of concept** demonstrating blind signature operations on the STM32F469I-Discovery board. The architecture splits responsibilities between the embedded firmware and a host-side demo tool.

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
│                          STM32F469I-DISCOVERY │                       │
│  ┌────────────────────────────────────────────│───────────────────┐  │
│  │                          firmware          ▼                   │  │
│  │  ┌─────────────┐   ┌──────────────┐   ┌─────────────────┐     │  │
│  │  │ USB CDC     │   │ cashu-core-  │   │   BSP / HAL     │     │  │
│  │  │ Receiver    │──▶│ lite         │──▶│   (display,     │     │  │
│  │  └─────────────┘   │ (no_std)     │   │    sdram, etc)  │     │  │
│  │                    └──────────────┘   └─────────────────┘     │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                       │
│  Hardware: 180MHz Cortex-M4F, 2MB Flash, 384KB SRAM, 16MB SDRAM      │
└───────────────────────────────────────────────────────────────────────┘
```

## Components

### 1. `firmware/` (Embedded Application)

The embedded firmware running on the STM32F469I-Discovery.

**Dependencies:**
- `stm32f469i-disc` BSP (git dependency, pinned commit)
- `cashu-core-lite` (workspace member)
- `cortex-m`, `cortex-m-rt`, `defmt`

**Responsibilities:**
- Initialize hardware (display, USB, SDRAM)
- Receive commands via USB CDC
- Decode Cashu tokens
- Generate blinded outputs
- Unblind signatures
- Display token info

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
  │  3. Pick random blinder r              │
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
```

## Memory Layout

```
Flash (2MB): Code + rodata
SRAM (384KB): Stack + heap + USB buffers
SDRAM (16MB): Framebuffer + token storage + large allocs
```

## Baseline

The BSP is a git dependency:

```toml
[dependencies]
stm32f469i-disc = { git = "https://github.com/Amperstrand/stm32f469i-disc", rev = "c71065da588b9256e26557c57c103954cf7915fe" }
```

This preserves the BSP as a separate, versioned dependency rather than modifying it directly.
