#!/usr/bin/env bash
# Hardware NVS keyset + state battery (#60 #56 device leg): first-boot
# seed generation, persistence across resets, and erase-regeneration on
# the ESP32-D0WD.
#
# Everything is asserted; the only physical requirement is the board on
# the bench (CP210x USB-serial) and reachable bench WiFi. Exits 77
# (BLOCKED) if the board is not attached.
#
# Phases:
#   1. flash + first boot  → "first boot: generated mint keyset seed (NVS)",
#      keyset id ≠ demo id, /v1/keys serves that id
#   2. reset               → "loaded persisted mint keyset seed", SAME id,
#      /v1/keys byte-identical
#   3. NVS region erase    → fresh seed (id ≠ both prior), "first boot" line
#
# Usage: MICRONUTS_WIFI_SSID=... MICRONUTS_WIFI_PASS=... \
#        bash scripts/test_hw_nvs_keyset.sh
set -euo pipefail

cd "$(dirname "$0")/.."
ESP_DIR=micronuts-esp32-mint
BIN="$(pwd)/$ESP_DIR/target/xtensa-esp32-espidf/debug/micronuts-esp32-mint"
DEMO_KEYSET_ID=0022e025867793d1
BOOT_WAIT_SECS=45
TAP_PID=""
# Serial tap: reads lines at 115200 with DTR/RTS never asserted (the
# CP210x auto-reset circuit otherwise resets or holds the board).
TAP_PY='
import sys, serial
port, out = sys.argv[1], sys.argv[2]
s = serial.Serial()
s.port, s.baudrate = port, 115200
s.dtr = False
s.rts = False
s.open()
with open(out, "ab") as f:
    while True:
        line = s.readline()
        if line:
            f.write(line)
            f.flush()
'

fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "PASS: $*"; }

cleanup() {
    [[ -n "$TAP_PID" ]] && kill "$TAP_PID" 2>/dev/null || true
}
trap cleanup EXIT

# --- locate the mint board by VID:PID, never by tty number ---
PORT=""
for p in /dev/ttyUSB*; do
    vid=$(cat "/sys/class/tty/$(basename "$p")/device/../uevent" 2>/dev/null \
        | grep PRODUCT | cut -d= -f2)
    [[ "$vid" == "10c4/ea60/100" ]] && PORT="$p"
done
[[ -n "$PORT" ]] || { echo "BLOCKED: ESP32-D0WD (CP210x 10c4:ea60) not attached"; exit 77; }
pass "mint board on $PORT"

[[ -n "${MICRONUTS_WIFI_SSID:-}" && -n "${MICRONUTS_WIFI_PASS:-}" ]] \
    || fail "set MICRONUTS_WIFI_SSID / MICRONUTS_WIFI_PASS"

WORK=$(mktemp -d)
echo "work dir: $WORK (kept on failure for evidence)"

stop_tap() { [[ -n "$TAP_PID" ]] && { kill "$TAP_PID" 2>/dev/null; wait "$TAP_PID" 2>/dev/null || true; TAP_PID=""; }; }
start_tap() {
    stop_tap
    # CP210x auto-reset circuit: never assert DTR/RTS from a plain reader.
    nohup python3 -c "$TAP_PY" "$PORT" "$1" > /dev/null 2>&1 &
    TAP_PID=$!
    sleep 1
}

echo "== 0. build (esp toolchain) =="
( . /home/ubuntu/export-esp.sh && cd "$ESP_DIR" \
    && MICRONUTS_WIFI_SSID="$MICRONUTS_WIFI_SSID" \
       MICRONUTS_WIFI_PASS="$MICRONUTS_WIFI_PASS" \
    cargo +esp build ) || fail "esp build failed"
[[ -x "$BIN" ]] || fail "no binary at $BIN"

echo "== 1. flash + first boot =="
fuser -k "$PORT" 2>/dev/null || true; sleep 1
( . /home/ubuntu/export-esp.sh && cd "$ESP_DIR" \
    && RUSTUP_TOOLCHAIN=esp espflash flash -p "$PORT" --chip esp32 \
        --partition-table partitions.csv "$BIN" ) || fail "flash failed"
start_tap "$WORK/boot1.log"
sleep "$BOOT_WAIT_SECS"
grep -q "first boot: generated mint keyset seed (NVS)" "$WORK/boot1.log" \
    || { cp "$WORK"/boot1.log .; fail "no first-boot seed line (log copied to ./boot1.log)"; }
ID1=$(grep -o 'DemoMint up (keyset [0-9a-f]*' "$WORK/boot1.log" | head -1 | grep -o '[0-9a-f]\{16\}')
[[ -n "$ID1" ]] || fail "no keyset id in boot log"
[[ "$ID1" != "$DEMO_KEYSET_ID" ]] || fail "device served the DEMO keyset id"
pass "first boot: generated seed, keyset $ID1 ≠ demo"
IP=$(grep -o 'WiFi connected: ip=[0-9.]*' "$WORK/boot1.log" | head -1 | cut -d= -f2)
[[ -n "$IP" ]] || fail "no IP in boot log"
curl -s "http://$IP:3338/v1/keys" -o "$WORK/keys1.json"
grep -q "$ID1" "$WORK/keys1.json" || fail "/v1/keys does not serve the generated keyset"
pass "/v1/keys serves keyset $ID1"

echo "== 2. reset → persisted seed must survive =="
stop_tap
( . /home/ubuntu/export-esp.sh && RUSTUP_TOOLCHAIN=esp \
    espflash reset -p "$PORT" --chip esp32 ) || fail "reset failed"
start_tap "$WORK/boot2.log"
sleep "$BOOT_WAIT_SECS"
grep -q "loaded persisted mint keyset seed" "$WORK/boot2.log" \
    || { cp "$WORK"/boot2.log .; fail "no persisted-seed line (log copied to ./boot2.log)"; }
ID2=$(grep -o 'DemoMint up (keyset [0-9a-f]*' "$WORK/boot2.log" | head -1 | grep -o '[0-9a-f]\{16\}')
[[ "$ID2" == "$ID1" ]] || fail "keyset id changed across reset: $ID1 -> $ID2"
pass "reset: same keyset $ID2 after reload"
curl -s "http://$IP:3338/v1/keys" -o "$WORK/keys2.json"
cmp -s "$WORK/keys1.json" "$WORK/keys2.json" \
    || fail "/v1/keys differs after reset"
pass "/v1/keys byte-identical across reset"

echo "== 3. NVS erase → fresh identity =="
stop_tap
( . /home/ubuntu/export-esp.sh && RUSTUP_TOOLCHAIN=esp \
    esptool --chip esp32 --port "$PORT" erase_region 0x9000 0x10000 ) \
    || fail "nvs erase failed"
( . /home/ubuntu/export-esp.sh && RUSTUP_TOOLCHAIN=esp \
    espflash reset -p "$PORT" --chip esp32 ) || fail "reset after erase failed"
start_tap "$WORK/boot3.log"
sleep "$BOOT_WAIT_SECS"
grep -q "first boot: generated mint keyset seed (NVS)" "$WORK/boot3.log" \
    || { cp "$WORK"/boot3.log .; fail "no first-boot line after erase (log copied to ./boot3.log)"; }
ID3=$(grep -o 'DemoMint up (keyset [0-9a-f]*' "$WORK/boot3.log" | head -1 | grep -o '[0-9a-f]\{16\}')
[[ -n "$ID3" && "$ID3" != "$ID1" && "$ID3" != "$DEMO_KEYSET_ID" ]] \
    || fail "erase did not regenerate a fresh keyset (got '$ID3')"
pass "erase: fresh keyset $ID3 (≠ $ID1, ≠ demo)"

stop_tap
rm -rf "$WORK"
echo "ALL PASS: NVS keyset battery green (ids: $ID1 -> $ID2 -> $ID3)"
