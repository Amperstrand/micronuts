# QR Scanner Integration Design

> **Status**: Draft  
> **Author**: Mayor (Gas Town)  
> **Date**: 2026-03-15

---

## 1. Overview

This document outlines the design and implementation plan for integrating a QR code scanner into **Micronuts**, the Cashu hardware wallet proof-of-concept for the STM32F469I-Discovery board.

### Purpose
Enable the device to **ingest Cashu tokens via QR codes** for airgapped operation and transforming Micronuts from a **debug/developer tool** for Cashu flows.

### Scope
- **First milestone**: Single-frame QR scanning,- **Second milestone**: Multipart/animated QR support

---

## 2. Specter-DIY Research Summary

### 2.1 Architecture Overview

Specter-DIY is an STM32F469-based airgapped hardware wallet that uses QR codes as its primary communication channel. Key findings:

- **Uses UART scanner modules** (GM65, M3Y), not cameras
- **Scanner handles all decoding internally**
- **Supports multipart QR** via UR protocol

### 2.2 Supported Hardware

| Model | Interface | Baud Rate | Notes |
|------|-----------|----------|-------|
| **GM65** | TTL-232 UART | 9600/115200 | Best documented, widely available |
| **M3Y** | TTL-232 UART | 9600/115200 | Higher scan rates, less common |

**Recommendation**: Use **GM65** for Micronuts due better documentation, lower cost, proven reliability

### 2.3 UART Configuration (from Specter-DIY)

| Parameter | Value |
|-----------|-------|
| **Default Baud Rate** | 9600 bps |
| **High-Speed Baud Rate** | 115200 bps |
| **Data Bits** | 8 |
| **Stop Bits** | 1 |
| **Parity** | None |
| **Flow Control** | None |

### 2.4 Initialization Sequence

1. **Probe scanner model** at different baud rates
2. **Configure settings**:
   - QR-only mode (disable 1D barcodes)
   - Set scanning interval (100ms)
   - Enable RAW mode for binary data
3. **Switch to high-speed baud** (115200)
4. **Begin continuous scanning**

### 2.5 What We're NOT Copying

| Aspect | Reason |
|--------|--------|
| **MicroPython runtime** | We use Rust |
| **LVGL GUI** | We use embedded-graphics |
| **Bitcoin-specific logic** | We need Cashu-specific handling |
| **PSBT handling** | We handle Cashu tokens instead |
| **HD camera support** | UART scanners are simpler |

---

## 3. Hardware Design

### 3.1 STM32F469I-Discovery UART Resources

| UART | TX Pin | RX Pin | Notes |
|------|--------|-------|-------|
| **USART1** | PA9 | PA10 | **Available** - recommended |
| **USART2** | PD5 | PD6 | Available |
| **USART3** | PD8 | PD9 | Available |
| **USART6** | PC6 | PC7 | **Used by LCD** - avoid |

**Recommendation**: Use **USART1 (PA9/PA10)** for QR scanner.

### 3.2 GM65 Module Connections

| GM65 Pin | STM32 Pin | Function |
|----------|-----------|----------|
| VCC | 5V | Power |
| GND | GND | Ground |
| TX | PA10 (USART1_RX) | Scanner TX → MCU RX |
| RX | PA9 (USART1_TX) | MCU TX → Scanner RX |
| PWR_EN (optional) | GPIO (e.g., PD7) | Power control |

**Note**: No level shifter needed - STM32F469 is 5V tolerant on I/O pins.

### 3.3 Power Considerations

| Mode | Current | Notes |
|------|---------|-------|
| **Active scanning** | ~120mA | Brief bursts |
| **Standby** | ~30mA | Continuous |
| **Sleep** | <10mA | If supported |

**Recommendation**: Implement optional GPIO power control for battery-powered scenarios.

---

## 4. Software Architecture

### 4.1 Module Structure

```
firmware/src/qr/
├── mod.rs           # Module exports
├── driver.rs       # Low-level UART driver
├── protocol.rs     # Scanner protocol handling
├── decoder.rs      # Payload classification & decoding
└── app.rs          # Application-level integration
```

### 4.2 Module Responsibilities

| Module | Responsibility |
|--------|---------------|
| `driver.rs` | UART init, scanner config, polling |
| `protocol.rs` | Command protocol, scanner detection |
| `decoder.rs` | Cashu token decode, payload classification |
| `app.rs` | Integration with main firmware flow |

### 4.3 Key Types

```rust
// driver.rs
pub enum ScannerModel {
    Unknown,
    GM65,
    M3Y,
}

pub struct ScannerConfig {
    pub model: ScannerModel,
    pub baud_rate: u32,
    pub raw_mode: bool,
}

pub struct ScanResult {
    pub data: Vec<u8>,
    pub timestamp: u32,
}

// protocol.rs
pub enum PayloadType {
    CashuToken,
    PlainText,
    Unsupported,
}

pub enum TokenType {
    V3Json,   // cashuA prefix
    V4Cbor,   // cashuB prefix
    UR,        // ur:cashu/ prefix
}

// decoder.rs
pub struct DecodedToken {
    pub token_type: TokenType,
    pub mint: Option<String>,
    pub unit: Option<String>,
    pub proofs: Vec<ProofInfo>,
}

pub struct ProofInfo {
    pub amount: u64,
    pub keyset_id: [u8; 8],
    pub secret: Vec<u8>,
    pub signature: [u8; 33],
}
```

---

## 5. First Milestone Implementation

### 5.1 Features

- [x] Initialize UART and detect scanner
- [x] Configure scanner settings
- [x ] Continuous scanning mode
- [x ] Read scanned data
- [x ] Detect `cashuB` prefix
- [x ] Decode Cashu V4 token
- [x ] Display scan results on LCD
- [x ] Export scan over USB CDC

### 5.2 Commands

| Command | Code | Description |
|---------|------|-------------|
| `QR_START` | 0x01 | Start scanning |
| `QR_STOP` | 0x02 | Stop scanning |
| `QR_STATUS` | 0x03 | Get scanner status |
| `QR_DATA` | 0x04 | Get last scan data |

### 5.3 USB CDC Protocol Extensions

Add to existing USB protocol:

```rust
pub enum Command {
    // ... existing commands ...
    ImportToken,
    GetTokenInfo,
    GetBlinded,
    SendSignatures,
    GetProofs,
    // NEW QR commands
    QrStart,       // Start QR scanner
    QrStop,        // Stop QR scanner
    QrStatus,      // Get scanner status
    QrData,         // Get last scanned data (raw)
}
```

### 5.4 Display Integration

Add new display screens:
- **QR Scanner Screen**:
  - Scanner status (detected/not detected)
  - Scanner model (if known)
  - Last scan time
  - Last payload type
  - Last payload length
  - Decode status (success/failure)

### 5.5 Error Handling

| Error | Handling |
|-------|----------|
| Scanner not detected | Display "No scanner" message |
| Decode failure | Display error, keep raw data |
| Timeout | Display "Scanning..." timeout message |
| Buffer overflow | Discard scan, display error |

---

## 6. Second Milestone (Future)

### 6.1 Multipart QR Support

- Implement UR (Uniform Resources) decoder
- Support animated QR codes
- Add fragment reassembly
- Display progress indicator

### 6.2 Enhanced Debug Features

- Export all scanned payloads (raw)
- Configurable timeout settings
- Scanner power management

---

## 7. Cashu Token Format (NUT-00, NUT-16)

### 7.1 Token V4 Format

**Prefix**: `cashuB`

**Encoding**: Base64URL → CBOR

**CBOR Structure**:
```
{
    "m": "mint_url",       // string
    "u": "unit",           // string (optional)
    "d": "memo",           // string (optional)
    "p": [               // proofs array
        {
            "a": amount,     // integer
            "i": keyset_id,  // 8 bytes
            "s": secret,     // string
            "c": signature   // 33 bytes (compressed pubkey)
        }
    ]
}
```

### 7.2 Animated QR (NUT-16)

**Format**: `ur:cashu/<index>-<total>/<hash>/<data>`

**Protocol**: UR (Uniform Resources) by Blockchain Commons

---

## 8. Risks & Mitigation

| Risk | Likelihood | Mitigation |
|------|------------|-------------|
| Scanner module unavailable | Medium | Support multiple models (GM65, M3Y) |
| UART timing issues | Low | Use DMA for reliable reception |
| Memory overflow | Medium | Limit payload size, streaming decode |
| Power consumption | Medium | GPIO power control, scanning timeouts |
| Binary data corruption | Low | RAW mode, checksums |

---

## 9. Test Strategy

### 9.1 Unit Tests

- Test payload classification
- Test token decoding (V3, V4)
- Test error handling

### 9.2 Integration Tests

- Test with real GM65 scanner
- Test with various QR code types
- Test USB CDC output

### 9.3 Hardware Test Fixtures

- **Scanner simulator**: Python script to send test QR codes over serial
- **Test QR codes**: Pre-generated Cashu tokens in various formats

---

## 10. References

- [Cashu NUT-00: Token Format](https://github.com/cashubtc/nuts/blob/main/00.md)
- [Cashu NUT-16: Animated QR](https://github.com/cashubtc/nuts/blob/main/16.md)
- [Specter-DIY QR Implementation](https://github.com/cryptoadvance/specter-diy)
- [GM65 User Manual](https://www.waveshare.net/wiki/GM65_Barcode_Scanner_Module)
