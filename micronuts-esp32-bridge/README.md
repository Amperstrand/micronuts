# Micronuts ESP32 WiFi Bridge

WiFi-to-serial bridge firmware for the ESP32-D0WD. Connects the STM32F469I-Discovery
to WiFi, exposing a TCP server that forwards data between WiFi and UART.

## Build

Requires the Xtensa ESP Rust toolchain:

```bash
# Install espup
cargo install espup
espup install
source export-esp.sh

# Build
cargo +esp build -Z build-std
```

## Flash

```bash
espflash flash target/xtensa-esp32-espidf/debug/bridge --port /dev/ttyUSB0 --monitor
```

## Configuration

Set WiFi credentials in `src/main.rs`:
```rust
const WIFI_SSID: &str = "YOUR_SSID";
const WIFI_PASS: &str = "YOUR_PASSWORD";
```

The TCP server listens on port 3333. Connect from a laptop/phone to exchange
data with the STM32 over UART.
