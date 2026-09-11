#!/usr/bin/env bash
# Idempotently create the micronuts QR-rig place with its resource matches.
# The tokens come from the microfips bench exporter (one exporter per bench
# machine — this rig shares the physical CYD + ST-Link with gm65 sessions;
# acquiring this place excludes gm65-qr-loopback and microfips-bench).
set -euo pipefail

COORDINATOR="${LABGRID_COORDINATOR:-192.168.13.221:20408}"
EXPORTER_NAME="${LABGRID_EXPORTER_NAME:-ai-legion-small-microfips}"
PLACE="micronuts-qr-rig"
LG="${LABGRID_CLIENT:-labgrid-client}"

lg() { "$LG" -x "$COORDINATOR" -p "$PLACE" "$@"; }

if lg show >/dev/null 2>&1; then
    echo "place $PLACE exists"
else
    lg create
    echo "place $PLACE created"
fi

lg add-match "${EXPORTER_NAME}/cyd-serial/BenchSerialToken"
lg add-match "${EXPORTER_NAME}/stm32-stlink/BenchSerialToken"
lg set-comment "CYD QR source -> GM65 -> F469 micronuts wallet CDC scan-in (micronuts tools/hil)"
lg set-tags "firmware=-" "test=-" "owner=micronuts" "ts=$(date +%Y%m%dT%H%M%S)"
lg show | sed -n '1,8p'

# Documentation-only state place for the wallet board (pattern 15): never
# acquired for exclusivity; carries what the F469 currently runs.
STATE="micronuts-f469-state"
if ! "$LG" -x "$COORDINATOR" -p "$STATE" show >/dev/null 2>&1; then
    "$LG" -x "$COORDINATOR" -p "$STATE" create
fi
"$LG" -x "$COORDINATOR" -p "$STATE" \
    add-match "${EXPORTER_NAME}/stm32-stlink/BenchSerialToken" 2>/dev/null || true
"$LG" -x "$COORDINATOR" -p "$STATE" \
    set-comment "state record for the F469 board (tags: firmware/test/owner/ts)" 2>/dev/null || true
"$LG" -x "$COORDINATOR" -p "$STATE" \
    set-tags "firmware=unknown" "owner=micronuts" "ts=$(date +%Y%m%dT%H%M%S)" 2>/dev/null || true
