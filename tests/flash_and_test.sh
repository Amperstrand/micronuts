#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
LOG_DIR="$SCRIPT_DIR/results"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/${TIMESTAMP//:/_}.log"
SUMMARY_FILE="$LOG_DIR/latest_summary.txt"

echo "============================================"
echo " Micronuts Hardware Self-Test"
echo " Timestamp: $TIMESTAMP"
echo " Branch:    $(git branch --show-current)"
echo " Commit:    $(git rev-parse --short HEAD)"
echo " Log:       $LOG_FILE"
echo "============================================"
echo ""

cargo build -p firmware --release --target thumbv7em-none-eabihf 2>&1 | tee -a "$LOG_FILE"
echo "---" | tee -a "$LOG_FILE"

echo "Flashing firmware..."
echo ">>> Watch the board display during self-test <<<"
echo ">>> The screen will turn GREEN during the display test <<<"
echo ">>> Tap the screen when prompted (5s timeout) <<<"
echo ">>> Scan a QR code when prompted (5s timeout) <<<"
echo ""

if ! probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware 2>&1 | tee -a "$LOG_FILE"; then
    echo "" | tee -a "$LOG_FILE"
    echo "probe-rs failed (likely SWD stuck). Attempting recovery via st-flash..." | tee -a "$LOG_FILE"
    st-flash --connect-under-reset reset 2>&1 | tee -a "$LOG_FILE" || true
    sleep 1
    echo "Retrying probe-rs..." | tee -a "$LOG_FILE"
    probe-rs run --chip STM32F469NIHx target/thumbv7em-none-eabihf/release/firmware 2>&1 | tee -a "$LOG_FILE" || true
fi

echo "" | tee -a "$LOG_FILE"
echo "---" | tee -a "$LOG_FILE"
echo "Test run complete. Log saved to: $LOG_FILE"
echo ""

{
    echo "MICRONUTS HARDWARE SELF-TEST SUMMARY"
    echo "Date:   $TIMESTAMP"
    echo "Branch: $(git branch --show-current)"
    echo "Commit: $(git rev-parse HEAD)"
    echo ""
    echo "Dependency revisions:"
    grep -E "Embassy rev|BSP rev|GM65 rev|stm32f469i-disc rev" "$LOG_FILE" | head -10 || true
    echo ""
    echo "Results:"
    grep -E "\[PASS\]|\[FAIL\]|\[SKIP\]" "$LOG_FILE" || true
    echo ""
    grep "RESULTS:" "$LOG_FILE" || true
} | tee "$SUMMARY_FILE"
