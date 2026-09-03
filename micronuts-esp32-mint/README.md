# micronuts-esp32-mint

ESP32 (classic ESP32 / ESP32-D0WD class, esp-idf, **std Rust**) WiFi
front-end for the micronuts Cashu mint. House-style scaffold following
bolty-rs / ccid-firmware-rs / tollgate-s3-rs conventions.

Status (2026-09-02): scaffold. GET /v1/info, /v1/keys, /v1/keysets wired to
`DemoMint`; POST NUT routes answer 501 pending the shared JSON mapping with
the host `micronuts-audit-adapter`. The fully functional prototype today is
the host server (see workspace README + docs/AUDIT-2026-09-02-mint-prototype.md).

## Build

Toolchain once:

```bash
cargo install espup
espup install
. ~/.espressif/export-esp.sh  # or: source export-esp.sh from espup output
```

Then — **from this directory** (a workspace-root build resolves the wrong
target and silently yields a host binary; verify with `file`):

```bash
MICRONUTS_WIFI_SSID="..." MICRONUTS_WIFI_PASS="..." \
  cargo +esp build
file target/xtensa-esp32-espidf/debug/micronuts-esp32-mint  # must say Xtensa
```

## Flash

```bash
espflash flash --partition-table partitions.csv \
  target/xtensa-esp32-espidf/debug/micronuts-esp32-mint --monitor
```

HTTP API on port **3338**.

## Notes

- 32 KiB handler + main-task stacks: Cashu token parsing + k256 secp256k1
  overflow 12 KiB (house evidence, tollgate-s3-rs).
- Custom `partitions.csv` passed to espflash explicitly (esp-idf-sys does not
  copy custom partition CSVs into its build dir).
- Persistence milestone: spent set + quotes + issued outputs into NVS
  (`EspDefaultNvsPartition` already taken by WiFi; bounded blobs + explicit
  commits per house NVS rules) — design: ../docs/PERSISTENCE-DESIGN.md
  phase 3, tracked as #60.
- Upstream `cashu` (=0.17.3) compiles for Xtensa ESP-IDF (tollgate-s3-rs
  evidence) — no de-std crypto layer needed on this target.
