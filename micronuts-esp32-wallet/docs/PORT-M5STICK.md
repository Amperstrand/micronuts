# M5StickC-Plus2 port — the phone-free TollGate client on a stick

For the fellow researcher who'll be testing with an M5Stick. This guide
covers everything needed to go from a stock M5StickC-Plus2 to a working
TollGate wallet take client.

## Why the port is small

The CYD wallet firmware (this crate's `main.rs`) uses **no CYD-specific
hardware** for the take flow — the WiFi, the NDS discovery, the ecash
wallet, and the serial console are all plain ESP32. The only CYD-specific
code is in the `terminal_demo`/`terminal_kiosk` bins (display + LED),
which are feature-gated. The M5StickC-Plus2 is the same ESP32 target.

## Pin map (CYD → M5StickC-Plus2)

| Signal | CYD (ESP32-2432S028) | M5StickC-Plus2 |
|---|---|---|
| LCD SCLK | GPIO 14 | GPIO 18 |
| LCD MOSI | GPIO 13 | GPIO 19 |
| LCD DC | GPIO 2 | GPIO 16 |
| LCD CS | GPIO 15 | GPIO 5 |
| LCD RST | — (none) | GPIO 23 |
| Backlight | GPIO 27 | AXPL192 PMIC (not GPIO) |
| Display | ST7796 320×480 | ST7789 135×240 |
| RGB LED | GPIO 4/16/17 | GPIO 10 (single) |
| USB-UART | CH340 (external) | CP2102 (onboard) |

## Build + flash

```bash
cd micronuts-esp32-wallet

# The wallet firmware itself (serial console, no display):
MICRONUTS_WIFI_SSID=<house-ssid> \
MICRONUTS_WIFI_PASS=<house-psk> \
MICRONUTS_MINT_URL=http://<your-mint>:8383 \
  cargo +esp build --release --target xtensa-esp32-espidf

# Make the flashable image:
esptool.py --chip esp32 elf2image \
  --flash-mode dio --flash-freq 40m --flash-size 4MB \
  ~/.cargo-target/xtensa-esp32-espidf/release/micronuts-esp32-wallet

# Flash (M5Stick factory offset — check your partition table):
esptool.py --chip esp32 --port /dev/ttyUSB? \
  write_flash 0x10000 micronuts-esp32-wallet.bin
```

## The take flow (identical to the CYD)

Serial console at 115200 baud (the M5Stick's CP2102 shows as /dev/ttyUSB*
on Linux, /dev/cu.usbserial* on macOS):

```
connect                          → connect to your mint
receive <cashu-token>            → fund the wallet
tollgate join <TollGate-SSID>    → join the open portal
tollgate pay                     → discover price, pay, verify internet
tollgate status                  → check state + balance
```

## The four lessons that made the CYD take work (all apply here)

1. **OWE must be off** — `CONFIG_ESP_WIFI_ENABLE_WPA3_OWE_STA=n` in
   sdkconfig.defaults (already in this crate). Without it, open captive
   portal APs flake on every post-first join.

2. **Fresh tokens only** — stale-keyset tokens fail the spend with
   "Unknown Keyset". Mint → receive → pay in one session.

3. **Pre-auth mint allowlist on the DUT** — the wallet does a mint
   round-trip during the spend while pre-auth on the TollGate net. The
   DUT must allow it (ip filter ndsNET accept for the mint's address:port).
   See the TollGate side: `conwrt-bench/tools/cyd-take.sh` does this
   automatically.

4. **USB re-enumeration cures link wedges** — if the serial link goes
   silent or WiFi scan returns ESP_FAIL, unbind/bind the USB device.

## Adding the display (optional, for a standalone kiosk)

Copy `src/bin/terminal_kiosk.rs` → `src/bin/m5stick_kiosk.rs`, change:
- `ST7796` model → `ST7789`
- Pin assignments per the table above
- `display_size(320, 480)` → `display_size(135, 240)`
- Remove the RGB LED struct (M5 has a single LED on GPIO 10)
- Feature-gate as `display-m5stick` (already declared in Cargo.toml)

The display is NOT needed for testing — the serial console is the rig
interface (a researcher can drive everything from a laptop terminal).
