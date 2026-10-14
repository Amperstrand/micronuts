#!/usr/bin/env python3
"""Capture authoritative wire vectors from the bench SatoCash card.

Runs a SAFE command set (no SETUP, no channel ops, no wedge-triggering
APDU forms) through the real chain — pcscd → libifdvpcd → nucula
wifi-vpcd (ccid-firmware-rs) → PN7160 → SatoCash v0.1 — and records
command/response pairs as bench-vectors.json. The Rust test suite
replays these vectors: the crate's encoders must reproduce the command
bytes and its parsers must decode the captured responses. This closes
the self-referential-mock gap (mocks that echo our own bugs back).

Usage (on the bench host with the reader):
  python3 scripts/capture_wire_vectors.py [reader-name-substring]

Output: bench-vectors.json beside this script (overwrite; the file is
committed — re-capture on bench sessions and review the diff).
"""

import json
import sys
import time
from pathlib import Path

from smartcard.System import readers
from smartcard.util import toBytes, toHexString

SATOCASH_AID = "5361746F63617368"

# import_mint inner data is [url_size(1b) | url] per SPEC-wire.md —
# build it programmatically (hand-typed hex once shipped a wrong Lc).
URL = b"https://mint.example"
IMPORT_MINT = "B0B10000" + f"{1 + len(URL):02X}" + f"{len(URL):02X}" + URL.hex().upper()

# Every vector is a SAFE, read-only or expected-reject exchange.
# The 6-byte zero-data form is deliberately NOT sent (it wedges the
# board session — see ccid-firmware-rs runbook "Known limitations").
VECTORS = [
    ("select_satocash", f"00A4040008{SATOCASH_AID}"),
    ("select_satocash_case4_le", f"00A4040008{SATOCASH_AID}00"),
    ("get_status_case2", "B03C000000"),
    ("satocash_status_case2", "B0B0000000"),
    ("select_wrong_aid", "00A4040005FFFFFFFFFF00"),
    ("import_mint_locked", IMPORT_MINT),
]


def main() -> int:
    needle = sys.argv[1] if len(sys.argv) > 1 else "Nucula"
    rs = [r for r in readers() if needle in str(r)]
    if not rs:
        print(f"no reader matching {needle!r}")
        return 1
    conn = rs[0].createConnection()
    conn.connect()
    atr = toHexString(conn.getATR()).replace(" ", "")
    out = {
        "provenance": {
            "captured": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "reader": str(rs[0]),
            "atr": atr,
            "chain": "pcscd > libifdvpcd > nucula wifi-vpcd (ccid-firmware-rs) > PN7160 > SatoCash v0.1",
        },
        "vectors": [],
    }
    for name, cmd in VECTORS:
        data, s1, s2 = conn.transmit(toBytes(cmd))
        out["vectors"].append(
            {
                "name": name,
                "command": cmd,
                "sw": f"{s1:02X}{s2:02X}",
                "response": bytes(data).hex().upper(),
            }
        )
        print(f"{name}: {s1:02X}{s2:02X} ({len(data)}B)")
    conn.disconnect()
    Path(__file__).parent.parent.joinpath("bench-vectors.json").write_text(
        json.dumps(out, indent=2) + "\n"
    )
    print("wrote bench-vectors.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
