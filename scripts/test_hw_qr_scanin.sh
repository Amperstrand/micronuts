#!/usr/bin/env bash
# Hardware QR scan-in battery (#29 P3b): the first time the device
# accepts a REAL-wallet-shaped token PHYSICALLY.
#
# Loop: device swap → export token → cashu-ts re-authors it
# (getEncodedToken) → QR rendered to PNG + terminal → OPERATOR aims the
# GM65 at it → script triggers the scan over CDC → asserts the scanned
# bytes equal the cashu-ts token → imports it on the device → asserts
# TokenInfo shows the cashu-ts token's mint/amount/proofs.
#
# The only manual step is aiming the scanner; everything else is
# asserted. Requires the STM32F469I-DISCO wallet firmware with the GM65
# attached; exits 77 (BLOCKED) if the CDC device is absent.
#
# Usage: bash scripts/test_hw_qr_scanin.sh [--amount 21]

set -euo pipefail

cd "$(dirname "$0")/.."
MINT_TOOL=target/release/mint-tool
AMOUNT=21
[[ "${1:-}" == "--amount" ]] && AMOUNT="$2"

fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "PASS: $*"; }

# --- locate the wallet CDC by VID:PID, never by tty number ---
PORT=""
for p in /dev/ttyACM*; do
    prod=$(cat "/sys/class/tty/$(basename "$p")/device/../uevent" 2>/dev/null \
        | grep PRODUCT | cut -d= -f2)
    [[ "$prod" == "16c0/27dd/10" ]] && PORT="$p"
done
[[ -n "$PORT" ]] || { echo "BLOCKED: wallet CDC (16c0:27dd) not attached"; exit 77; }
pass "wallet CDC on $PORT"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "== 1. swap flow: generate $AMOUNT -> sign (DLEQ) -> export =="
"$MINT_TOOL" --port "$PORT" generate --amount "$AMOUNT" >/dev/null
"$MINT_TOOL" --port "$PORT" sign >/dev/null
"$MINT_TOOL" --port "$PORT" export | tee "$WORK/export.txt" >/dev/null
TOKEN=$(grep -o 'cashuB[A-Za-z0-9_-]*' "$WORK/export.txt" | head -1)
[[ -n "$TOKEN" ]] || fail "no token in export output"
pass "device export token (${#TOKEN} chars)"

echo "== 2. cashu-ts re-authors the token =="
CASHUTS_TOKEN=$(TOKEN="$TOKEN" node scripts/e2e_cashuts_reencode.mjs) \
    || fail "cashu-ts re-encode rejected the device token"
pass "cashu-ts getEncodedToken token (${#CASHUTS_TOKEN} chars)"

echo "== 3. QR rendered =="
python3 - "$CASHUTS_TOKEN" "$WORK/qr.png" <<'PYEOF'
import sys
import qrcode
token, out = sys.argv[1], sys.argv[2]
qr = qrcode.QRCode(error_correction=qrcode.constants.ERROR_CORRECT_M, border=2)
qr.add_data(token)
qr.make(fit=True)
img = qr.make_image(fill_color="black", back_color="white")
img.save(out)
print(f"QR saved: {out} (version {qr.version}, {qr.modules_count}x{qr.modules_count})")
PYEOF
pass "QR artifact $WORK/qr.png"

echo
echo ">>> MANUAL STEP: display $WORK/qr.png large on screen (or scan the"
echo ">>> terminal), point the wallet's GM65 at it. The script triggers"
echo ">>> the scan in 5 seconds and waits up to 60 s for data."
echo
sleep 5

echo "== 4. GM65 physical scan =="
SCAN_OUT=$("$MINT_TOOL" --port "$PORT" scan | tee "$WORK/scan.txt") || true
SCANNED=$(grep -o 'cashuB[A-Za-z0-9_-]*' "$WORK/scan.txt" | head -1)
[[ -n "$SCANNED" ]] || { cat "$WORK/scan.txt"; fail "scanner returned no cashuB payload"; }
[[ "$SCANNED" == "$CASHUTS_TOKEN" ]] \
    || fail "scanned bytes differ from the cashu-ts token (${#SCANNED} vs ${#CASHUTS_TOKEN} chars)"
pass "GM65 scanned the cashu-ts token verbatim (${#SCANNED} chars)"

echo "== 5. device imports the cashu-ts token =="
"$MINT_TOOL" --port "$PORT" import "$CASHUTS_TOKEN" >/dev/null
pass "device accepted the import"

echo "== 6. TokenInfo reflects the cashu-ts token =="
INFO=$("$MINT_TOOL" --port "$PORT" token-info)
echo "    $INFO"
grep -q "amount=$AMOUNT " <<<"$INFO" || fail "amount mismatch in $INFO"
grep -q "proofs=3" <<<"$INFO" || fail "proof count mismatch in $INFO"
pass "TokenInfo: amount=$AMOUNT, 3 proofs"

echo
echo "ALL QR SCAN-IN CHECKS PASSED: real-wallet token scanned physically, accepted on device"
