#!/usr/bin/env bash
# test_usb_variant.sh — Build firmware with a specific embassy USB fix variant
#
# Usage:
#   ./tests/test_usb_variant.sh <variant>
#   ./tests/test_usb_variant.sh upstream          # baseline (no patch)
#   ./tests/test_usb_variant.sh remove-snak-only  # one-line SNAK removal
#   ./tests/test_usb_variant.sh ahbidl-only       # AHBIDL waits before flushes
#   ./tests/test_usb_variant.sh remove-snak+ahbidl # both combined
#   ./tests/test_usb_variant.sh remove-snak+ahbidl+disable # full fix minus write recovery
#   ./tests/test_usb_variant.sh debug-register-dump # instrumentation only
#
# The script modifies Cargo.toml to add [patch] for the chosen variant,
# builds release firmware, and prints the flash + test commands.
#
# To restore upstream baseline: ./tests/test_usb_variant.sh upstream

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CARGO_TOML="$WORKSPACE_ROOT/Cargo.toml"
TARGET="thumbv7em-none-eabihf"
RELEASE_DIR="$WORKSPACE_ROOT/target/$TARGET/release"

FORK_REPO="https://github.com/Amperstrand/embassy.git"
UPSTREAM_REV="84444a19"

declare -A VARIANTS
VARIANTS=(
    ["upstream"]="$UPSTREAM_REV"
    ["remove-snak-only"]="a3d17b042732f0c11961de79ba330a6d0e50a747"
    ["ahbidl-only"]="b33a0bf79d6fbe691d68f48b97dbdf5a38fbe3c8"
    ["remove-snak+ahbidl"]="26a725859ad7a61b59dd00a8b0ab08f04c56a096"
    ["remove-snak+ahbidl+disable"]="eeb508b9bb4adefcb58e08f92bbf315d02214dbc"
    ["debug-register-dump"]="a7ed3eb9a31e8d5b35408bcae522466897ba3987"
)

VARIANT="${1:-}"
if [ -z "$VARIANT" ]; then
    echo "Usage: $0 <variant>"
    echo ""
    echo "Available variants:"
    for v in upstream remove-snak-only ahbidl-only "remove-snak+ahbidl" "remove-snak+ahbidl+disable" debug-register-dump; do
        echo "  $v"
    done
    exit 1
fi

REV="${VARIANTS[$VARIANT]:-}"
if [ -z "$REV" ]; then
    echo "ERROR: Unknown variant '$VARIANT'"
    echo "Available: ${!VARIANTS[*]}"
    exit 1
fi

echo "=== USB Variant Test: $VARIANT ==="
echo "Rev: $REV"
echo ""

if [ "$VARIANT" = "upstream" ]; then
    echo "Removing [patch] section from Cargo.toml (upstream baseline)..."
    python3 -c "
import re
with open('$CARGO_TOML', 'r') as f:
    content = f.read()
# Remove [patch] section
content = re.sub(r'\n\[patch\..*?\n(?=\[|\Z)', '', content, flags=re.DOTALL)
with open('$CARGO_TOML', 'w') as f:
    f.write(content)
"
else
    echo "Adding [patch] for $VARIANT ($REV) to Cargo.toml..."

    python3 -c "
import re

with open('$CARGO_TOML', 'r') as f:
    content = f.read()

# Remove existing [patch] section
content = re.sub(r'\n\[patch\..*?\n(?=\[|\Z)', '', content, flags=re.DOTALL)

# Add new [patch] section
patch_section = '''

[patch.\"https://github.com/embassy-rs/embassy\"]
embassy-time = { git = \"$FORK_REPO\", package = \"embassy-time\", rev = \"$REV\" }
embassy-time-driver = { git = \"$FORK_REPO\", package = \"embassy-time-driver\", rev = \"$REV\" }
embassy-executor = { git = \"$FORK_REPO\", package = \"embassy-executor\", rev = \"$REV\" }
embassy-stm32 = { git = \"$FORK_REPO\", package = \"embassy-stm32\", rev = \"$REV\" }
embassy-usb = { git = \"$FORK_REPO\", package = \"embassy-usb\", rev = \"$REV\" }
embassy-usb-synopsys-otg = { git = \"$FORK_REPO\", package = \"embassy-usb-synopsys-otg\", rev = \"$REV\" }
embassy-sync = { git = \"$FORK_REPO\", package = \"embassy-sync\", rev = \"$REV\" }
embassy-futures = { git = \"$FORK_REPO\", package = \"embassy-futures\", rev = \"$REV\" }
'''

content = content.rstrip() + patch_section

with open('$CARGO_TOML', 'w') as f:
    f.write(content)
"
fi

echo ""
echo "Building firmware..."
cargo build -p firmware --release --target "$TARGET" 2>&1

echo ""
echo "Converting to binary..."
arm-none-eabi-objcopy -O binary "$RELEASE_DIR/firmware" "$RELEASE_DIR/firmware.bin"

echo ""
echo "=== Build complete ==="
echo ""
echo "Flash and test:"
echo ""
echo "  st-flash --connect-under-reset write $RELEASE_DIR/firmware.bin 0x08000000"
echo "  st-flash --connect-under-reset reset"
echo "  # Wait 15s for boot + self-test"
echo "  python3 $SCRIPT_DIR/usb_stress_test.py   # auto-detects wallet port"
echo ""
echo "Or with RTT logging (for debug-register-dump variant):"
echo "  # Flash with st-flash, then attach probe-rs AFTER USB test fails"
echo "  probe-rs attach --chip STM32F469NIHx"
