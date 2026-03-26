#!/usr/bin/env python3
"""USB CDC stress test for Micronuts firmware.

Sends hundreds of commands over USB and measures response times,
detecting timeouts, corrupted responses, and hangs.
"""

import serial
import struct
import sys
import time
import json
from datetime import datetime, timezone

PORT = sys.argv[1] if len(sys.argv) > 1 else "/dev/ttyACM0"
BAUD = 115200
TIMEOUT = 5.0
POST_OPEN_DELAY = 0.1

CMD_SCANNER_STATUS = 0x10
CMD_GET_TOKEN_INFO = 0x02
CMD_IMPORT_TOKEN = 0x01
CMD_GET_BLINDED = 0x03
CMD_GET_PROOFS = 0x05

STATUS_OK = 0x00
STATUS_ERROR = 0xFF
STATUS_INVALID_PAYLOAD = 0x02

STATUSES = {
    0x00: "Ok",
    0x01: "InvalidCommand",
    0x02: "InvalidPayload",
    0x03: "BufferOverflow",
    0x04: "CryptoError",
    0x10: "ScannerNotConnected",
    0x11: "ScannerBusy",
    0x12: "NoScanData",
    0xFF: "Error",
}

def build_frame(cmd, payload=b""):
    length = len(payload)
    return bytes([cmd, (length >> 8) & 0xFF, length & 0xFF]) + payload

def read_response(ser, timeout=TIMEOUT):
    header = ser.read(3)
    if len(header) < 3:
        return None, "incomplete_header"
    status = header[0]
    length = (header[1] << 8) | header[2]
    payload = b""
    if length > 0:
        payload = ser.read(length)
        if len(payload) < length:
            return None, f"incomplete_payload ({len(payload)}/{length})"
    return {"status": status, "length": length, "payload": payload}, None

def send_command(ser, cmd, payload=b"", timeout=TIMEOUT):
    frame = build_frame(cmd, payload)
    start = time.monotonic()
    ser.write(frame)
    ser.flush()
    resp, err = read_response(ser, timeout)
    elapsed = time.monotonic() - start
    return resp, err, elapsed

def run_stress():
    print(f"=== Micronuts USB CDC Stress Test ===")
    print(f"Port: {PORT}, Baud: {BAUD}, Timeout: {TIMEOUT}s")
    print(f"Time: {datetime.now(timezone.utc).isoformat()}")
    print()

    ser = serial.Serial(port=PORT, baudrate=BAUD, timeout=TIMEOUT, dsrdtr=False, rtscts=False)
    time.sleep(POST_OPEN_DELAY)

    results = {
        "port": PORT,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "total_commands": 0,
        "successes": 0,
        "failures": 0,
        "timeouts": 0,
        "corrupted": 0,
        "timings_ms": [],
        "errors": [],
    }

    phases = [
        ("ScannerStatus x100", CMD_SCANNER_STATUS, b"", 100),
        ("GetTokenInfo (no token) x100", CMD_GET_TOKEN_INFO, b"", 100),
        ("ScannerStatus x100", CMD_SCANNER_STATUS, b"", 100),
        ("GetTokenInfo (no token) x100", CMD_GET_TOKEN_INFO, b"", 100),
        ("ScannerStatus x100", CMD_SCANNER_STATUS, b"", 100),
        ("GetTokenInfo (no token) x100", CMD_GET_TOKEN_INFO, b"", 100),
    ]

    total_start = time.monotonic()

    for phase_name, cmd, payload, count in phases:
        phase_start = time.monotonic()
        phase_ok = 0
        phase_fail = 0
        for i in range(count):
            resp, err, elapsed = send_command(ser, cmd, payload)
            results["total_commands"] += 1
            results["timings_ms"].append(round(elapsed * 1000, 2))

            if resp is None:
                results["failures"] += 1
                if err and "incomplete" in err:
                    results["corrupted"] += 1
                else:
                    results["timeouts"] += 1
                results["errors"].append(f"{phase_name} #{i}: {err}")
                phase_fail += 1
            elif resp["status"] == STATUS_OK:
                results["successes"] += 1
                phase_ok += 1
            elif resp["status"] == STATUS_ERROR:
                results["successes"] += 1
                phase_ok += 1
            else:
                results["successes"] += 1
                phase_ok += 1

        phase_elapsed = time.monotonic() - phase_start
        status_name = STATUSES.get(
            0x00 if phase_ok == count else 0xFF,
            "Unknown"
        )
        print(f"  {phase_name}: {phase_ok}/{count} OK ({phase_elapsed:.1f}s)")

    total_elapsed = time.monotonic() - total_start
    timings = results["timings_ms"]
    timings.sort()

    print()
    print(f"=== RESULTS ===")
    print(f"Total commands:  {results['total_commands']}")
    print(f"Successes:       {results['successes']}")
    print(f"Failures:        {results['failures']}")
    print(f"  Timeouts:      {results['timeouts']}")
    print(f"  Corrupted:     {results['corrupted']}")
    print(f"Total time:      {total_elapsed:.1f}s")
    print(f"Commands/sec:    {results['total_commands']/total_elapsed:.1f}")
    print(f"Timing (median): {timings[len(timings)//2]:.1f}ms")
    print(f"Timing (p95):    {timings[int(len(timings)*0.95)]:.1f}ms")
    print(f"Timing (p99):    {timings[int(len(timings)*0.99)]:.1f}ms")
    print(f"Timing (max):    {timings[-1]:.1f}ms")

    if results["errors"]:
        print(f"\nFirst 10 errors:")
        for e in results["errors"][:10]:
            print(f"  - {e}")

    ser.close()

    results["summary"] = {
        "total_time_s": round(total_elapsed, 1),
        "cmds_per_sec": round(results['total_commands'] / total_elapsed, 1),
        "median_ms": timings[len(timings) // 2],
        "p95_ms": timings[int(len(timings) * 0.95)],
        "p99_ms": timings[int(len(timings) * 0.99)],
        "max_ms": timings[-1],
    }

    outfile = f"tests/results/stress_{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}.json"
    with open(outfile, "w") as f:
        json.dump(results, f, indent=2)
    print(f"\nResults saved to: {outfile}")

    return results["failures"] == 0

if __name__ == "__main__":
    ok = run_stress()
    sys.exit(0 if ok else 1)
