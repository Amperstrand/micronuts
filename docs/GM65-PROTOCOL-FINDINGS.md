# GM65 Protocol Findings

> Reverse-engineered from the specter-diy project's working QR scanner driver.
> The GM65 datasheet protocol description is **incorrect**. This document captures
> the real protocol as used by specter-diy hardware that ships to end users.

## Command Format

```
[7E 00] [type:1] [len:1] [addr_lo] [addr_hi] [value:N] [AB CD]
```

| Field | Size | Description |
|-------|------|-------------|
| Header | 2 | Always `7E 00` |
| Type | 1 | `07` = get, `08` = set, `09` = save |
| Length | 1 | Number of bytes following (addr + value) |
| Address | 2 | Register address (see table below) |
| Value | N | Data bytes (1 for most settings, 2 for baud rate) |
| Suffix | 2 | Always `AB CD` (sentinel, NOT a checksum) |

**The `AB CD` suffix is NOT a CRC/XOR checksum.** It is a constant sentinel meaning "no checksum". The specter-diy code names it `CRC_NO_CHECKSUM`.

## Response Format

For `get_setting` queries, the scanner responds with exactly 7 bytes:

```
02 00 00 01 [value_byte] 33 31
```

| Field | Offset | Description |
|-------|--------|-------------|
| Prefix | 0-3 | Always `02 00 00 01` |
| Value | 4 | The register value being read |
| Suffix | 5-6 | Always `33 31` |

**Critical**: Responses do NOT start with `7E 00` and do NOT end with `0x55`. The datasheet's response format is wrong.

For `set_setting` commands, the response is the same 7-byte format (value byte may be ignored).

## Register Addresses

These are the addresses used by specter-diy. They differ from the datasheet in several cases.

| Address | Name | Size | Description |
|---------|------|------|-------------|
| `00 0D` | SERIAL_ADDR | 1 | Serial output mode. Mask bits 0-1: `val & 0x03 != 0` means serial is disabled |
| `00 00` | SETTINGS_ADDR | 1 | Scanner mode. Set to `0xD1` for command mode with sound + aim |
| `00 2A` | BAUD_RATE_ADDR | 2 | Baud rate (2-byte value). `1A 00` = 115200 |
| `00 02` | SCAN_ADDR | 1 | Enable/disable scanning. `0x01` = enable, `0x00` = disable |
| `00 06` | TIMEOUT_ADDR | 1 | Scan timeout. `0x00` = infinite |
| `00 05` | SCAN_INTERVAL_ADDR | 1 | Interval between scans in 100ms units. `0x01` = 100ms |
| `00 13` | SAME_BARCODE_DELAY_ADDR | 1 | Delay before re-scanning same barcode. `0x85` = 5 seconds |
| `00 E2` | VERSION_ADDR | 1 | Firmware version. `0x69` needs RAW mode fix |
| `00 BC` | RAW_MODE_ADDR | 1 | RAW mode (undocumented). `0x08` = enable |
| `00 2C` | BAR_TYPE_ADDR | 1 | Barcode type filter. `0x01` = QR only |
| `00 3F` | QR_ADDR | 1 | QR code enable. `0x01` = enable |
| `00 D9` | FACTORY_RESET_ADDR | 1 | Factory reset. Write `0x55` to trigger |

## Initialization Sequence (from specter-diy `configure_gm65()`)

1. **Probe** scanner by reading `SERIAL_ADDR` (`00 0D`). If response matches 7-byte format, scanner is present.
2. **Fix serial mode**: Read `SERIAL_ADDR`, if `val & 0x03 != 0`, write `val & 0xFC` to clear bits.
3. **Configure settings** (read-then-write pattern for each):
   - `SETTINGS_ADDR` -> `0xD1` (command mode + sound + aim)
   - `TIMEOUT_ADDR` -> `0x00` (no timeout)
   - `SCAN_INTERVAL_ADDR` -> `0x01` (100ms)
   - `SAME_BARCODE_DELAY_ADDR` -> `0x85` (5s same barcode delay)
   - `BAR_TYPE_ADDR` -> `0x01` (QR only)
   - `QR_ADDR` -> `0x01` (enable QR)
4. **Check version**: Read `VERSION_ADDR`. If `== 0x69`, enable RAW mode at `RAW_MODE_ADDR`.
5. **Save to EEPROM**: Send save command (`7E 00 09 01 00 00 00 DE C8`).
6. **Set baud rate**: Send 2-byte baud rate command for 115200 (`7E 00 08 02 00 2A 1A 00 AB CD`).
7. **Reinitialize UART** on host side to 115200 baud.

## Multi-Baud Rate Probing

The scanner may be at any of these baud rates from a previous session:

| Baud | Models to try |
|------|--------------|
| 9600 | M3Y, GM65 |
| 57600 | M3Y |
| 115200 | GM65 |

Probe sequence: try reading `SERIAL_ADDR` at 9600, then 57600, then 115200. If the scanner responds with a valid 7-byte response, it's detected.

## UART Configuration

| Parameter | Value |
|-----------|-------|
| Data bits | 8 |
| Stop bits | 1 |
| Parity | None |
| Flow control | None |
| Default baud | 9600 |
| Operating baud | 115200 (after init) |

## Scan Data Format

After a successful scan, the scanner sends the decoded QR data followed by `\r` (or `\r\n`). The host reads bytes until an EOL marker is detected.

## Common Pitfalls

1. **Wrong response format**: The datasheet shows `7E 00 ... 55` responses. Real responses are `02 00 00 01 ... 33 31`.
2. **Wrong CRC**: The `AB CD` suffix is NOT a checksum. Don't try to compute one.
3. **Wrong addresses**: The datasheet's register map doesn't match what the scanner actually uses. Use specter-diy's addresses.
4. **Wrong baud rate**: Scanner defaults to 9600 but may be at 115200 from a previous session. Always probe multiple rates.
5. **Address byte order**: In specter-diy, addresses are passed as big-endian bytes `[0x00, 0x0D]` for `SERIAL_ADDR`. The datasheet may show them differently.
6. **EEPROM save may fail**: Some scanners reject the save command. This is non-fatal; settings take effect immediately.
7. **Version 0x69 RAW mode fix**: Some GM65 firmware versions corrupt binary QR data. Enabling undocumented RAW mode (`0x08` at address `0x00BC`) fixes this. The fix persists across reboots if EEPROM save succeeds.
8. **SDRAM pin conflicts**: On STM32F469I-Discovery, USART6 uses PG14/PG9 via the Arduino headers. These are NOT used by SDRAM (which uses PG0, PG1, PG4, PG5, PG8, PG15). Extract PG14/PG9 from the GPIO port BEFORE the SDRAM init macro consumes the whole port.

## Reference Implementation

The definitive GM65 driver is in specter-diy's `src/hosts/qr.py`:
https://github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py

Key functions:
- `_get_setting_once()` - Send get command, read 7-byte response, extract value from byte 4
- `_set_setting_once()` - Send set command, verify 7-byte success response
- `configure_gm65()` - Full initialization sequence
- `_update_scanner_model()` - Multi-baud probing
