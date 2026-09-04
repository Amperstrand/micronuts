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

## Persistence (NVS)

First boot after WiFi association draws a 32-byte keyset seed
(`esp_fill_random` — RF-powered hardware RNG), stores it in the default
NVS partition (namespace `micronuts`, key `keyset_seed`), and derives
the served keyset from it; every later boot reloads the seed. The
served keyset is therefore never the public one baked into
`DemoMint::new()` — CI source-scans this crate for demo-keyset
constructor references. Whole-mint snapshots (quotes, spent proofs,
issued outputs) go through the `micronuts_mint::persist::StateStore`
seam as JSON blobs under `mint_state`, bound-checked to 32 KiB before
writing; a corrupt blob refuses to boot (fail-stop, same policy as the
host file backend). The `nvs` partition is grown to 64 KiB in
`partitions.csv` for this (#60, #56; design: ../docs/PERSISTENCE-DESIGN.md).

## Notes

- 32 KiB handler + main-task stacks: Cashu token parsing + k256 secp256k1
  overflow 12 KiB (house evidence, tollgate-s3-rs).
- Custom `partitions.csv` passed to espflash explicitly (esp-idf-sys does not
  copy custom partition CSVs into its build dir).
- Persistence milestone: spent set + quotes + issued outputs into NVS via
  `NvsStateStore` (landed — see "Persistence (NVS)" above); design:
  ../docs/PERSISTENCE-DESIGN.md phase 3, tracked as #60.
- Upstream `cashu` (=0.17.3) compiles for Xtensa ESP-IDF (tollgate-s3-rs
  evidence) — no de-std crypto layer needed on this target.
