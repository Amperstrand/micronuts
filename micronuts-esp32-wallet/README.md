# micronuts-esp32-wallet

ESP32 (esp-idf, std Rust) Cashu wallet — the nucula-mode device
(micronuts#68 M1). Standalone crate: build from THIS directory only
(a workspace-root invocation never reads `.cargo/config.toml`).

## Build

```bash
cargo +esp build
```

Default target: plain ESP32 (`xtensa-esp32-espidf`, esp-idf v5.2.4).
WiFi credentials come from the environment at build time —
`MICRONUTS_WIFI_SSID` / `MICRONUTS_WIFI_PASS` — and are never committed.

## TollGate client (docs/ESP32-TOLLGATE-CLIENT-PLAN.md)

The wallet pays its own way through a TollGate captive portal with
on-device ecash. Console commands (serial, `help`):

```
tollgate join <ssid>     join the open TollGate network
tollgate pay [sats]      discover → probe → spend → raw-body POST → verify
                         (amount defaults to the discovered price, else 4)
tollgate status          last flow state + wallet balance
```

Fund the wallet first with the existing `receive <token>` path, then
`tollgate join` / `tollgate pay`. Protocol invariants carried verbatim
from the plan: the token goes out as a RAW BODY POST with
`Content-Type: text/plain` (never a JSON wrapper), and every probe
outside the backend port is IP-literal because pre-auth DNS under
nodogsplash passes tcp/53 only. The already-authenticated fast path
probes internet first and skips the spend when the gate is open.

## ESP32-S3 lane (LilyGo T-Display-S3 class)

Same crate, explicit target override (the default lane stays plain
ESP32):

```bash
cargo +esp build --target xtensa-esp32s3-espidf
```

The S3 build picks up `sdkconfig.defaults.esp32s3` automatically
(esp-idf-sys resolves `sdkconfig.defaults.<mcu>` overlays from the
target triple): 16 MB flash, octal PSRAM, USB-serial-JTAG console,
on top of the shared base defaults. `MCU` is deliberately NOT pinned
in `.cargo/config.toml` — esp-idf-sys derives the chip from the
triple, and a pinned `esp32` would abort S3 builds.

## sdkconfig plumbing (shared target dir caveat)

This machine builds into a shared cargo target dir
(`~/.cargo/config.toml` → `~/.cargo-target`), which breaks embuild's
workspace detection (it pops a fixed number of path components off the
build out dir). `.cargo/config.toml` therefore sets
`CARGO_WORKSPACE_DIR` (the crate root) and glob-copies `partitions.csv`
into the IDF build via `ESP_IDF_GLOB_PARTITIONS_*`. Without these,
`sdkconfig.defaults` and the S3 overlay silently never apply. If a
build fails with a cmake "does not match the source used to generate
cache" error, the shared cache has a poisoned esp-idf-sys dir for that
target: move `~/.cargo-target/<target>/debug/build/esp-idf-sys-<hash>`
aside and rebuild.
