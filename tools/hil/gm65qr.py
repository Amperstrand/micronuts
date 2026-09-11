"""gm65qr — reference QR-loopback client for the Amperstrand bench.

Reusable bring-along for any project that wants to drive the CYD QR source
and the GM65/F469 scanner rig (micronuts-class consumers). Encodes every
hard-won bench lesson:

- winning render config: INVERTED polarity + ECC-H + 224px cap (only
  configuration that decodes on the GM65+ST7796 rig; matrix 2026-09-10)
- identify F469 CDCs by /dev/serial/by-id PRODUCT string (gm65 sync fw and
  the micronuts wallet share VID:PID 16c0:27dd AND serial F4691)
- never drain between trigger and data poll (drain eats response bytes —
  the source of every clipped payload in early sessions)
- the GM65's 7-byte register responses (02 00 00 01 xx 33 31) can leak
  into the firmware's scan buffer when a trigger ACK races scan data —
  sanitize_scan strips them instead of failing the roundtrip
- the scanner task needs ~10s post-boot before ScannerStatus answers
"""

from __future__ import annotations

import re
import time
from pathlib import Path

import rig

# winning render configuration (matrix experiment 2026-09-10)
WINNING_CAP = rig.QR_WINNING_CAP
WINNING_INVERTED = True
WINNING_ECCH = True

# GM65 register-response frame: prefix 02 00 00 01, value, suffix 33 31.
# Leaks into scan payloads when a trigger ACK races the decode (bench-verified).
_ACK_FRAME = re.compile(rb"^\x02\x00\x00\x01.\x0031")

ALNUM = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz"


def sanitize_scan(data: bytes) -> bytes:
    """Strip leaked register-response frames from the head of scan data."""
    while True:
        stripped = _ACK_FRAME.sub(b"", data, count=1)
        if stripped == data:
            return data
        data = stripped


def unique_payload(seq: int, target_len: int = 10) -> bytes:
    """Deterministic unique printable payload (defeats the module's
    same-barcode 5s delay so every decode is reported)."""
    base = f"Q{seq:05d}"
    if target_len <= len(base):
        return base[:target_len].encode()
    pad = (target_len - len(base) - 1)
    body = base + "-" + (ALNUM * (pad // len(ALNUM) + 1))[:pad]
    return body[:target_len].encode()


def consume_stale(cdc: rig.StmCdcClient) -> None:
    """Eat the firmware's one-shot last-scan buffer so a later negative
    window observes only fresh decodes (stale-buffer false positive)."""
    cdc.send_recv(rig.CMD_DATA)


def arm_winning_config(cyd: rig.CydQrClient) -> None:
    cyd.set_inverted(WINNING_INVERTED)
    cyd.set_ecch(WINNING_ECCH)


def scan_roundtrip(
    cdc: rig.StmCdcClient,
    cyd: rig.CydQrClient,
    payload: bytes,
    cap: int | None = None,
    deadline_s: float = 20.0,
    poll_s: float = 0.4,
    retrigger_every_s: float = 5.0,
) -> dict:
    """Render `payload` on the CYD and wait for the GM65 to decode it.
    Returns {ok, attempts, latency_s, modules, px_per_module, scanned}.

    Never drains between trigger and poll: the async firmware answers
    ScannerData with NoScanData until a decode lands, and draining eats
    in-flight bytes (leading-payload clipping, bench 2026-09-08/10).
    """
    expected = sanitize_scan(payload)
    modules, px = cyd.show_qr_capped(payload, cap or WINNING_CAP)
    start = time.monotonic()
    deadline = start + deadline_s
    last_trigger = 0.0
    attempts = 0
    while time.monotonic() < deadline:
        now = time.monotonic()
        if now - last_trigger >= retrigger_every_s:
            cdc.trigger()
            last_trigger = now
        status, pl = cdc.read_data()
        if status == rig.STATUS_OK and len(pl) >= 2:
            attempts += 1
            scanned = sanitize_scan(pl[1:])
            if scanned == expected:
                return {"ok": True, "attempts": attempts,
                        "latency_s": round(now - start, 2),
                        "modules": modules, "px_per_module": px,
                        "scanned": scanned}
        time.sleep(poll_s)
    return {"ok": False, "attempts": attempts, "latency_s": None,
            "modules": modules, "px_per_module": px, "scanned": None}
