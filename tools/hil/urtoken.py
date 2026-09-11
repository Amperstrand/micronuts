"""Host-side UR multi-part encoder + token helpers for the QR scan-in rig.

Fragment format is byte-identical to gm65-scanner's decoder
(`ur:<type>/<i>-<N>/<hash>/<raw-ascii chunk>`): chunks are raw ASCII
slices of the payload string, the hash is an opaque sequence-binding
string (first fragment wins on the device), fragments may arrive in any
order, duplicates are ignored. Chunk budget keeps the WHOLE fragment
inside the gm65-measured CYD→GM65 decode envelope (~92 bytes/frame at
ECC-H, 224px cap).
"""

from __future__ import annotations

import hashlib


def ur_fragments(payload: str, chunk: int = 68, ur_type: str = "bytes") -> list[str]:
    """Split `payload` into `ur:` fragment strings.

    Overhead per frame is `len("ur:bytes/1-12/xxxxxxxxxx/")` ≈ 22 bytes at
    two-digit counts, so chunk=68 keeps frames ≤ ~90 bytes.
    """
    if chunk < 1:
        raise ValueError("chunk must be >= 1")
    data = payload.encode("ascii")
    seq = hashlib.sha256(data).hexdigest()[:8]
    parts = [data[i : i + chunk] for i in range(0, len(data), chunk)] or [b""]
    total = len(parts)
    return [
        f"ur:{ur_type}/{i}-{total}/{seq}/{part.decode('ascii')}"
        for i, part in enumerate(parts, start=1)
    ]


def fragment_frames(payload: str, chunk: int = 68) -> list[bytes]:
    """QR frames (bytes) for a payload string."""
    return [f.encode("ascii") for f in ur_fragments(payload, chunk)]


def play_fragments(cyd, frames, dwell_s: float = 1.2, cycles: int = 8):
    """Cycle the fragment ladder on the CYD until the device has seen every
    frame (animated-QR emulation). Returns the number of full cycles done."""
    for cycle in range(cycles):
        for frame in frames:
            cyd.show_qr(frame)
            import time

            time.sleep(dwell_s)
    return cycles
