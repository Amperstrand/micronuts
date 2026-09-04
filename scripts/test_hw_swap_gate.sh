#!/usr/bin/env bash
# Hardware swap-flow E2E: device swap → gate verification + USB CDC
# decoder robustness on the wire.
#
# Requires the STM32F469I-DISCO firmware (default build) running with the
# USB OTG FS cable connected (CN5, micro-B) — the CDC device enumerates as
# VID:PID 16c0:27dd. Without the cable this script exits 77 (BLOCKED).
#
# Verifies on real silicon:
#   1. generate → sign → export swap flow (host-mint-tool demo mint)
#   2. the exported token OPENS the walletport offline gate pinned to the
#      demo keyset — string-convention secrets + NUT-12 DLEQ + keyset
#      pinning, end to end through the device
#   3. FrameDecoder robustness on the live wire (the #55/ae78814 fixes):
#      oversized-length garbage followed by a valid frame in ONE USB write
#      must still answer; a 1030-byte garbage flood must not wedge the
#      device; a frame split across two writes must decode.
#
# --selftest runs only the in-process gate check (no hardware needed).

set -euo pipefail

cd "$(dirname "$0")/.."
GATE="cargo run -q -p walletport --example gate_verify --"
MINT_TOOL=target/release/mint-tool

fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "PASS: $*"; }

# --- locate the device CDC by VID:PID, never by tty number ---
find_cdc() {
    for p in /dev/ttyACM*; do
        [ -e "$p" ] || continue
        prod=$(cat "/sys/class/tty/$(basename "$p")/device/../uevent" 2>/dev/null \
            | grep PRODUCT | cut -d= -f2)
        [ "$prod" = "16c0/27dd/10" ] && { echo "$p"; return 0; }
    done
    return 1
}

if [ "${1:-}" = "--selftest" ]; then
    $GATE --selftest
    echo "SELFTEST PASSED (no hardware exercised)"
    exit 0
fi

echo "== gate wiring selftest (hardware-independent)"
$GATE --selftest

PORT=$(find_cdc) || {
    cat >&2 <<'EOF'
BLOCKED: no Micronuts CDC device (16c0:27dd) found.
The USB OTG FS cable (CN5, micro-B) is not connected — see
docs/HARDWARE-TEST-RESULTS-20260730.md and the 2026-09-03 addendum.
EOF
    exit 77
}
pass "device CDC on $PORT"

[ -x "$MINT_TOOL" ] || cargo build -p host-mint-tool --release

echo "== swap flow: generate --amount 21 (3 proofs: 16+4+1)"
"$MINT_TOOL" --port "$PORT" generate --amount 21 >/dev/null
pass "token imported"

echo "== sign (device DLEQ-verifies against the pinned demo key)"
"$MINT_TOOL" --port "$PORT" sign >/dev/null
pass "swap signed + accepted by the device"

echo "== export"
EXPORT=$("$MINT_TOOL" --port "$PORT" export)
TOKEN=$(echo "$EXPORT" | grep -o 'cashuB[A-Za-z0-9_=-]*' | head -1)
[ -n "$TOKEN" ] || { echo "$EXPORT"; fail "no token in export output"; }
pass "exported ${TOKEN:0:24}..."

echo "== exported-token wire checks (NUT-00 V4, cbor2 — graduated 2026-09-04)"
python3 - "$TOKEN" <<'PYEOF2'
import base64, cbor2, sys

token = sys.argv[1][len("cashuB"):]
t = cbor2.loads(base64.urlsafe_b64decode(token + "=" * (-len(token) % 4)))

# NUT-00 V4: single-character string keys, unknown fields tolerated.
assert set(t) <= {"m", "u", "d", "t"}, f"non-V4 keys: {sorted(t)}"
assert t["m"] == "demo://micronuts", f"mint: {t['m']!r}"
assert t["u"] == "sat", f"unit: {t['u']!r}"
assert t["d"] == "Swapped via Micronuts", f"memo: {t['d']!r}"

groups = t["t"]
assert len(groups) == 1, f"groups: {len(groups)}"
assert bytes(groups[0]["i"]) == b"\x00", f"keyset id: {groups[0]['i']!r}"

proofs = groups[0]["p"]
amounts = sorted(p["a"] for p in proofs)
assert amounts == [1, 4, 16], f"amounts: {amounts}"
for i, p in enumerate(proofs):
    assert set(p) <= {"a", "s", "c", "d"}, f"proof[{i}] keys: {sorted(p)}"
    assert isinstance(p["s"], str) and len(p["s"]) == 64, f"proof[{i}] secret hex string"
    assert len(bytes(p["c"])) == 33, f"proof[{i}] C not 33B compressed: {len(bytes(p['c']))}"
    d = p.get("d")
    assert d is not None, f"proof[{i}] missing dleq"
    assert all(len(bytes(d[k])) == 32 for k in ("e", "s", "r")), f"proof[{i}] dleq scalars"
print("PASS: 3 proofs 16/4/1, C=33B compressed, dleq {e,s,r}, mint/unit/memo intact")
PYEOF2

echo "== gate verification (pinned demo keyset, expect 21 sats)"
$GATE --token "$TOKEN" --expect 21

echo "== cashu-ts v4 real-wallet conformance (decodes + verifies DLEQ offline)"
TOKEN="$TOKEN" node scripts/e2e_cashuts_conformance.mjs

echo "== decoder robustness on the wire"
python3 - "$PORT" <<'PYEOF'
import serial, sys, time

port = sys.argv[1]
s = serial.Serial(port, 115200, timeout=3)
time.sleep(0.2)
s.reset_input_buffer()

def rx_ok():
    hdr = s.read(3)
    if len(hdr) < 3 or hdr[0] != 0x00:
        return False
    n = (hdr[1] << 8) | hdr[2]
    s.read(n)
    return True

# (a) #55 W2 class: 1030 bytes of garbage must not wedge the device.
s.write(bytes([0xFF, 0x04, 0x00]) + b"A" * 1027)
time.sleep(0.3)
s.reset_input_buffer()
s.write(bytes([0x10, 0x00, 0x00]))  # ScannerStatus
assert rx_ok(), "device wedged after garbage flood"
print("PASS: 1030-byte garbage flood survived, device still answers")

# (b) ae78814 on the wire: oversized-length header + valid frame in ONE
# write — the valid frame must decode (chunking-invariance fix).
s.reset_input_buffer()
s.write(bytes([0x05, 0x57, 0x00, 0x10, 0x00, 0x00]))
assert rx_ok(), "valid frame after bad header in the same chunk was swallowed"
print("PASS: resync-within-chunk verified on the wire")

# (c) frame split across two writes must decode.
s.reset_input_buffer()
s.write(bytes([0x10, 0x00]))
time.sleep(0.2)
s.write(bytes([0x00]))
assert rx_ok(), "split-write frame did not decode"
print("PASS: split-write frame decoded")

s.close()
PYEOF

echo
echo "ALL HARDWARE CHECKS PASSED: swap flow + gate verification + decoder robustness"
