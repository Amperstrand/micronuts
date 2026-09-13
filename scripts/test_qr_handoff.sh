#!/usr/bin/env bash
# Browser-wallet → device QR handoff e2e (roadmap item 3).
#
# The browser demo wallet's embedded mint signs with THE pinned device
# keyset — SHA256("demo://micronuts"), keyset id "00", mint URL
# demo://micronuts — so a wallet-minted token is verifiable offline by
# the F469 device and the walletport gate. This script proves the chain:
#
#   1. mint leg   — the wallet engine (the exact DemoMintClient + engine
#      the wasm browser demo runs) mints AMOUNT sats and prints a
#      cashuB token
#   2. device leg — mint-tool swap consumes the WALLET token (not the
#      harness's self-generated one): import → blind → pinned-key sign →
#      device SendSignatures DLEQ gate → export. Default runs the
#      in-process device (CI); `--wire` runs the same script over USB
#      CDC (exit 77 when no device, house convention — cf.
#      scripts/test_hw_swap_gate.sh)
#   3. gate leg   — the device export opens the walletport offline gate
#      pinned to the demo keyset; replay is rejected
#
# Usage: scripts/test_qr_handoff.sh [amount_sats] [--wire]

set -euo pipefail
cd "$(dirname "$0")/.."

AMOUNT=21
WIRE=0
for arg in "$@"; do
    case "$arg" in
        --wire) WIRE=1 ;;
        *) AMOUNT="$arg" ;;
    esac
done

GATE="cargo run -q -p walletport --example gate_verify --"

echo "== mint leg (wallet engine + pinned demo keyset)"
TOKEN=$(cargo run -q -p micronuts-wallet --example mint_demo_token -- "$AMOUNT")
case "$TOKEN" in
    cashuB*) ;;
    *) echo "$TOKEN"; echo "FAIL: mint leg did not print a cashuB token" >&2; exit 1 ;;
esac
echo "minted ${TOKEN:0:24}… ($AMOUNT sats)"
TOKEN_FILE=$(mktemp)
trap 'rm -f "$TOKEN_FILE"' EXIT
printf '%s\n' "$TOKEN" > "$TOKEN_FILE"

echo "== device leg ($([ "$WIRE" = 1 ] && echo 'USB CDC wire' || echo 'in-process device'))"
if [ "$WIRE" = 1 ]; then
    SWAP_OUT=$(cargo run -q -p host-mint-tool -- swap --token-file "$TOKEN_FILE")
else
    SWAP_OUT=$(cargo run -q -p host-mint-tool -- swap --selftest --token-file "$TOKEN_FILE")
fi
echo "$SWAP_OUT" | grep -vE '^EXPORT:'
EXPORT=$(printf '%s\n' "$SWAP_OUT" | grep -o 'cashuB[A-Za-z0-9_=-]*' | head -1)
[ -n "$EXPORT" ] || { echo "FAIL: no export token in swap output" >&2; exit 1; }

echo "== gate leg (walletport offline gate, pinned demo keyset)"
$GATE --token "$EXPORT" --expect "$AMOUNT"

echo
echo "QR HANDOFF PASS: browser-engine token accepted by the device ($([ "$WIRE" = 1 ] && echo wire || echo in-process)) and verified offline ($AMOUNT sats)"
