# QR Scanner Integration Design for Micronuts

**Status**: Draft  
**Date**: 2026-03-15  
**Author**: Mayor (AI Coordinator)

---

## Executive Summary

This document outlines the design for integrating QR scanner support into Micronuts, a Cashu hardware wallet debug tool for the STM32F469I-Discovery board. The design is informed by research into Specter-DIY's QR implementation, GM65/M3Y scanner hardware specifications, and Cashu token encoding standards.

**Key Decision**: Implement Rust-native QR scanner subsystem using the GM65 module as the primary target, with a clean modular architecture that can be extended to other scanner models.

---

## 1. Research Summary

### 1.1 Specter-DIY QR Architecture (Reference Implementation)

Specter-DIY is an STM32F469-based airgapped hardware wallet that uses QR codes as its primary communication channel. Key findings:

| Aspect | Specter-DIY Implementation |
|--------|---------------------------|
| **Scanner Models** | GM65 (primary), M3Y (alternative) |
| **UART Interface** | USART6, 9600 → 115200 baud |
| **Trigger Pin** | PB2 (active-low) |
| **Framing** | EOL-terminated (`\r` or `\r\n`) |
| **Multi-part QR** | UR (Uniform Resources) protocol |
| **Language** | MicroPython |

**Specter-DIY Source Files**:
- `src/hosts/qr.py`: Core driver and state machine
- `docs/datasheets/`: GM65 and M3Y datasheets
- `src/config_default.py`: Hardware pin definitions

### 1.2 GM65 Scanner Module Specifications

| Parameter | Value |
|-----------|-------|
| **Interface** | TTL-232 UART, USB |
| **Operating Voltage** | 4.2-6.0V DC (5V typical) |
| **Operating Current** | 120mA (scanning), 30mA (standby) |
| **Default Baud Rate** | 9600 bps |
| **Max Baud Rate** | 115200 bps |
| **Scan Speed** | ~1s per scan |
| **Resolution** | 648×488 pixels |

**UART Protocol**:
- Default: 9600 baud, 8N1 (8 data bits, no parity, 1 stop bit)
- Commands start with `7E 00` header
- CRC checksum at end

**Key Configuration Commands**:
```
Factory Reset:     7E 00 08 01 00 D9 55
Set Serial Mode:   7E 00 08 01 00 00 01 ...
Set QR Only:       7E 00 08 01 00 ...
Set Baud 115200:   7E 00 08 01 00 2A 1A 00 ...
Enable RAW Mode:   7E 00 08 01 00 BC 08 ... (critical for binary data)
```

### 1.3 Cashu Token QR Format (NUT-00, NUT-16)

**Token V4 Format**:
- **Prefix**: `cashuB`
- **Encoding**: Base64URL (no padding)
- **Serialization**: CBOR (Concise Binary Object Representation)

**CBOR Structure**:
```
{
  "m": "https://mint.example.com",  // mint URL
  "u": "sat",                       // unit (optional)
  "d": "memo text",                 // memo (optional)
  "p": [                            // proofs array
    {
      "a": 1,                       // amount
      "i": <8 bytes>,               // keyset ID
      "s": "secret_string",         // secret
      "c": <33 bytes>               // signature (compressed pubkey)
    }
  ]
}
```

**Animated QR (NUT-16)**:
- Uses UR (Uniform Resources) protocol
- Format: `ur:cashu/<index>-<total>/<hash>/<data>`
- Fountain coding for reliable multi-part transmission

---

## 2. Hardware Design

### 2.1 STM32F469I-Discovery UART Selection

Based on the board schematic and existing peripheral usage:

| UART | Pins (TX/RX) | Status | Notes |
|------|--------------|--------|-------|
| USART1 | PA9/PA10 | ⚠️ Conflict | USB OTG FS uses PA11/PA12 (nearby) |
| USART2 | PD5/PD6 | ✅ Available | Exposed on CN3 header |
| USART3 | PD8/PD9 | ⚠️ Conflict | SDIO uses these pins |
| USART6 | PC6/PC7 | ✅ Available | **Recommended** - same as Specter-DIY |
| UART4 | PA0/PA1 | ⚠️ Conflict | LCD uses nearby pins |
| UART5 | PC12/PD2 | ✅ Available | Alternative option |

**Recommendation**: Use **USART6** (PC6=TX, PC7=RX) - proven working in Specter-DIY.

### 2.2 Scanner Connection

```
┌─────────────────┐      ┌─────────────────┐
│  STM32F469I-Disco│      │    GM65 Module  │
│                 │      │                 │
│  PC6 (USART6_TX)├──────► RX              │
│  PC7 (USART6_RX)◄──────┤ TX              │
│                 │      │                 │
│  PB2 (GPIO_OUT) ├──────► TRIGGER (opt)   │
│                 │      │                 │
│  3.3V           │      │ VCC (5V)        │
│  GND            ├──────┤ GND             │
└─────────────────┘      └─────────────────┘

Note: Level shifting needed if GM65 runs at 5V logic.
```

### 2.3 Power Management

- **Standby Current**: 30mA continuous = significant for battery
- **Recommendation**: Add GPIO-controlled power switch (MOSFET)
- **Sleep Mode**: Configure scanner to sleep when not in use

---

## 3. Software Architecture

### 3.1 Module Structure

```
firmware/src/
├── qr/
│   ├── mod.rs          # Module exports
│   ├── driver.rs       # Low-level UART scanner driver
│   ├── protocol.rs     # Scanner command/response protocol
│   ├── decoder.rs      # QR payload decoding (Cashu, UR)
│   └── app.rs          # Application-level scanner integration
└── ...
```

### 3.2 Module Responsibilities

#### `driver.rs` - Hardware Abstraction

```rust
/// Scanner hardware models we support
pub enum ScannerModel {
    Gm65,
    M3Y,
    Generic,
}

/// Scanner driver configuration
pub struct ScannerConfig {
    pub model: ScannerModel,
    pub baud_rate: u32,
    pub trigger_pin: Option< embassy_stm32::gpio::Output<'static> >,
    pub continuous_mode: bool,
}

/// Low-level scanner operations
pub trait ScannerDriver {
    /// Detect scanner model at given UART
    async fn detect(uart: &mut Uart) -> Option<ScannerModel>;
    
    /// Initialize scanner with configuration
    async fn init(&mut self, config: &ScannerConfig) -> Result<(), ScannerError>;
    
    /// Trigger a scan (for command-triggered mode)
    async fn trigger_scan(&mut self) -> Result<(), ScannerError>;
    
    /// Read scanned data
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ScannerError>;
    
    /// Check if scanner is present
    fn is_connected(&self) -> bool;
}
```

#### `protocol.rs` - Scanner Protocol

```rust
/// GM65 command structure
pub struct Gm65Command {
    pub header: [u8; 2],      // 0x7E, 0x00
    pub type_: u8,             // Command type
    pub length: u8,            // Payload length
    pub address: [u8; 2],      // Register address
    pub value: Vec<u8>,        // Value to set
    pub crc: u8,                // Checksum
}

/// Scanner response
pub enum ScannerResponse {
    Data(Vec<u8>),             // Scanned QR content
    Ack,                       // Command acknowledged
    Nack(u8),                  // Error with code
    Timeout,                   // No response
}

/// Build GM65 command
pub fn build_gm65_command(address: [u8; 2], value: &[u8]) -> Vec<u8>;

/// Calculate CRC
pub fn calculate_crc(data: &[u8]) -> u8;
```

#### `decoder.rs` - Payload Decoding

```rust
/// Decoded QR payload types
pub enum QrPayload {
    CashuToken(TokenV4),       // Decoded Cashu V4 token
    CashuV3Token(JsonToken),   // Legacy JSON token
    UrPart(UrFragment),        // UR animated QR fragment
    PlainText(Vec<u8>),        // Unknown/plain text
    Invalid,                   // Decode failed
}

/// UR (Uniform Resources) fragment
pub struct UrFragment {
    pub index: u32,
    pub total: u32,
    pub data: Vec<u8>,
}

/// Decode QR content
pub fn decode_qr(data: &[u8]) -> QrPayload;

/// UR decoder state machine
pub struct UrDecoder {
    fragments: Vec<Option<Vec<u8>>>,
    total: Option<u32>,
    received: u32,
}

impl UrDecoder {
    pub fn new() -> Self;
    pub fn feed(&mut self, fragment: UrFragment) -> Result<Option<Vec<u8>>, UrError>;
    pub fn progress(&self) -> (u32, u32);
}
```

#### `app.rs` - Application Integration

```rust
/// Scanner application state
pub struct ScannerApp {
    driver: Box<dyn ScannerDriver>,
    decoder: UrDecoder,
    state: ScannerState,
    last_payload: Option<QrPayload>,
}

pub enum ScannerState {
    Idle,
    Scanning,
    Decoding,
    WaitingForMoreParts,  // For animated QR
    Complete,
    Error(ScannerError),
}

impl ScannerApp {
    /// Create new scanner app
    pub async fn new(uart: Uart, config: ScannerConfig) -> Result<Self, ScannerError>;
    
    /// Start a scan
    pub async fn start_scan(&mut self);
    
    /// Update scanner state (call in main loop)
    pub async fn update(&mut self) -> Option<QrPayload>;
    
    /// Get scanner status for display
    pub fn status(&self) -> ScannerStatus;
    
    /// Get last decoded payload
    pub fn last_payload(&self) -> Option<&QrPayload>;
}

/// Status for debug display
pub struct ScannerStatus {
    pub model: Option<ScannerModel>,
    pub connected: bool,
    pub state: ScannerState,
    pub last_payload_type: Option<&'static str>,
    pub last_payload_len: Option<usize>,
    pub ur_progress: Option<(u32, u32)>,
}
```

### 3.3 Integration with Main Firmware

```rust
// In main.rs

mod qr;

use qr::{ScannerApp, ScannerConfig, ScannerModel, QrPayload};

// In main(), after USB and display init:

let scanner_uart = // ... configure USART6 ... ;
let scanner_config = ScannerConfig {
    model: ScannerModel::Gm65,
    baud_rate: 115200,
    trigger_pin: Some(trigger_pin),
    continuous_mode: false,
};

let mut scanner = match ScannerApp::new(scanner_uart, scanner_config).await {
    Ok(s) => Some(s),
    Err(_) => None,  // Scanner not connected - continue without
};

// In main loop:
loop {
    // ... USB handling ...
    
    if let Some(ref mut scanner) = scanner {
        if let Some(payload) = scanner.update().await {
            handle_qr_payload(payload, &mut state, &mut fb);
        }
    }
}

fn handle_qr_payload(payload: QrPayload, state: &mut FirmwareState, fb: &mut LtdcFramebuffer<u16>) {
    match payload {
        QrPayload::CashuToken(token) => {
            state.imported_token = Some(token.clone());
            firmware::display::render_token_info(fb, &token);
        }
        QrPayload::PlainText(data) => {
            // Try to interpret as USB command
            // ...
        }
        _ => {}
    }
}
```

---

## 4. Protocol Details

### 4.1 GM65 Initialization Sequence

```
1. Power On → Wait 100ms
2. Detect Model:
   - Try 9600 baud, send VERSION query
   - Try 57600 baud, send VERSION query
   - Try 115200 baud, send VERSION query
3. Configure:
   a. Factory reset (if first boot)
   b. Set serial output mode
   c. Disable 1D barcodes (QR only)
   d. Set scanning interval (100ms)
   e. Enable RAW mode (for binary Cashu tokens)
   f. Set baud rate to 115200
4. Re-init UART at 115200
5. Ready to scan
```

### 4.2 QR Scanning Flow

```
┌─────────────┐
│    Idle     │
└──────┬──────┘
       │ trigger_scan() or continuous mode
       ▼
┌─────────────┐
│  Scanning   │◄─────────────┐
└──────┬──────┘              │
       │ data received       │ more data coming
       ▼                     │
┌─────────────┐              │
│  Decoding   │──────────────┘
└──────┬──────┘
       │ EOL detected
       ▼
┌─────────────┐
│   Check     │
│  Payload    │
└──────┬──────┘
       │
       ├─── cashuB... ──► Decode V4 Token
       ├─── cashuA... ──► Decode V3 JSON
       ├─── ur:cashu... ─► UR Decoder
       └─── other ──► Plain Text
```

### 4.3 UR (Animated QR) Handling

For large Cashu tokens that exceed single QR capacity:

1. Detect `ur:cashu/` prefix
2. Parse fragment index and total
3. Store fragment in decoder
4. Display progress: "Scanning 3/10..."
5. Continue until all parts received
6. Reconstruct complete token
7. Decode as normal

---

## 5. Debug Tool Features

### 5.1 First Milestone: Basic Scanner

- [ ] Initialize GM65 over UART
- [ ] Read raw scanned bytes
- [ ] Display payload type on LCD
- [ ] Log raw payload over USB CDC
- [ ] Support single-frame `cashuB...` tokens
- [ ] Support plain text QR

### 5.2 Debug Display

```
┌────────────────────────────────┐
│ Scanner Status                 │
│                                │
│ Model: GM65                    │
│ Connected: Yes                 │
│ Mode: Command Trigger          │
│                                │
│ Last Scan:                     │
│   Type: Cashu V4 Token         │
│   Length: 245 bytes            │
│   Status: Decoded OK           │
│                                │
│ [Waiting for scan...]          │
└────────────────────────────────┘
```

### 5.3 USB CDC Debug Commands

| Command | Description |
|---------|-------------|
| `SCANNER_STATUS` | Return scanner model, connection status |
| `SCANNER_TRIGGER` | Trigger a scan |
| `SCANNER_LAST` | Return last scanned payload (hex) |
| `SCANNER_RAW ON/OFF` | Enable/disable raw mode |

### 5.4 Second Milestone: Animated QR

- [ ] UR protocol support
- [ ] Multi-part accumulation
- [ ] Progress display
- [ ] Timeout/reset logic

---

## 6. Test Strategy

### 6.1 Host-Side Testing

Create a Python test tool that:
1. Generates test QR codes (Cashu tokens, UR sequences)
2. Displays them for the scanner to read
3. Verifies decoded output over USB CDC

### 6.2 Hardware Test Fixtures

| Test | Method |
|------|--------|
| UART Communication | Loopback test (TX→RX) |
| Scanner Detection | Power cycle, verify detection |
| QR Decode | Scan known test QR codes |
| UR Decode | Scan animated QR sequence |
| Power Management | Measure current in sleep/active |

### 6.3 Test QR Codes

Generate test codes with:
```python
import qrcode

# Simple Cashu token
token = "cashuB..."  # V4 token
qr = qrcode.make(token)
qr.save("test_token.png")

# UR sequence (for animated QR)
# Use bc-ur library to generate fragments
```

---

## 7. Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| GM65 unavailable | Low | Medium | Support generic UART scanners |
| Binary data corruption | Medium | High | RAW mode configuration |
| UART conflicts | Low | High | Use USART6 (verified available) |
| Power consumption | Medium | Medium | GPIO power control |
| UR complexity | Medium | Low | Implement as separate module |

---

## 8. Implementation Plan

### Phase 1: Driver Foundation (Priority: P0)
1. Create `qr/` module structure
2. Implement UART driver for GM65
3. Add model detection logic
4. Basic scan and read functionality

### Phase 2: Protocol Layer (Priority: P0)
1. Implement GM65 command protocol
2. Add configuration sequence
3. RAW mode enablement
4. Baud rate switching

### Phase 3: Decoder (Priority: P1)
1. Integrate with existing `cashu-core-lite` token decode
2. Add UR protocol support
3. Payload classification

### Phase 4: Application Integration (Priority: P1)
1. Integrate with main firmware loop
2. Add debug display
3. USB CDC debug commands
4. Test with real QR codes

### Phase 5: Polish (Priority: P2)
1. Power management
2. Error handling
3. Documentation
4. Hardware assembly guide

---

## 9. Open Questions

1. **M3Y Support**: Should we implement M3Y support in Phase 1, or defer?
   - **Recommendation**: Defer to Phase 5. GM65 is better documented.

2. **Level Shifting**: Do we need 5V→3.3V level shifting?
   - **Recommendation**: Test with 3.3V first. Add if signal integrity issues.

3. **Continuous vs Command Trigger**: Which mode for default?
   - **Recommendation**: Command-triggered for security (user initiates).

4. **UR Library**: Use existing Rust UR library or implement minimal decoder?
   - **Recommendation**: Implement minimal decoder. Avoid large dependencies.

---

## 10. References

- Specter-DIY: https://github.com/cryptoadvance/specter-diy
- GM65 Datasheet: Available from vendor
- Cashu NUTs: https://github.com/cashubtc/nuts
- NUT-00 (Token Format): https://cashubtc.github.io/nuts/00/
- NUT-16 (Animated QR): https://cashubtc.github.io/nuts/16/
- UR Protocol: https://github.com/BlockchainCommons/Research/blob/master/papers/bcr-2020-005-ur.md

---

## Appendix A: GM65 Command Reference

| Address | Name | Value | Description |
|---------|------|-------|-------------|
| 0x0000 | Serial Output | 0x01 | Enable serial output |
| 0x002A | Baud Rate | 0x1A00 | 115200 bps |
| 0x00BC | RAW Mode | 0x08 | Enable binary output |
| 0x00D9 | Factory Reset | - | Reset to defaults |

**Command Format**: `7E 00 08 01 <ADDR_HI> <ADDR_LO> <VALUE> <CRC> 55`

---

## Appendix B: STM32F469I-Discovery Header Pinout (Relevant Pins)

**CN3 (Arduino-compatible header)**:
- PD5 (USART2_TX)
- PD6 (USART2_RX)
- PB2 (GPIO - for trigger)

**CN4 (Arduino-compatible header)**:
- PC6 (USART6_TX) ← **Recommended TX**
- PC7 (USART6_RX) ← **Recommended RX**

---

## Appendix C: What We Borrowed vs. What We Didn't

### Borrowed from Specter-DIY:
- ✅ GM65/M3Y scanner model support
- ✅ UART initialization sequence
- ✅ RAW mode fix for binary data
- ✅ UR protocol for animated QR
- ✅ Multi-part QR state machine

### Not Borrowed (MicroPython-specific):
- ❌ MicroPython async framework (using Embassy instead)
- ❌ LVGL UI code (using embedded-graphics instead)
- ❌ File-based buffering (using heap allocation instead)
- ❌ Bitcoin-specific payload handling (Cashu instead)

### Novel for Micronuts:
- ✨ Rust-native implementation
- ✨ Embassy async framework
- ✨ Integrated with existing Cashu token decoder
- ✨ USB CDC debug interface
- ✨ Debug tool focus (not production wallet)
