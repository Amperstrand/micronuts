"""Micronuts CDC scan helpers (quiet-cadence protocol).

Bench-measured 2026-09-11: the firmware captures a triggered scan into a
10 s window via one long UART read; host traffic during that window (0.4 s
poll storms) prevents the capture from completing. The working cadence is
render -> trigger -> QUIET (>=2.5 s) -> single data poll.
"""

from __future__ import annotations

import time

import rig
import gm65qr


def scan_quiet(cdc, cyd, payload: bytes, capture_s: float = 3.0,
               deadline_s: float = 20.0, poll_s: float = 1.0) -> dict:
    """Render `payload`, trigger once, stay quiet, then poll ScannerData.

    Returns {ok, scanned, latency_s}. Retries with fresh renders until the
    deadline (module same-barcode delay defeats instant re-decodes of
    identical content — callers should vary payloads across calls).
    """
    start = time.monotonic()
    seq = 0
    while time.monotonic() - start < deadline_s:
        seq += 1
        cyd.show_qr(payload)
        cdc.trigger()
        time.sleep(capture_s)  # QUIET — let the firmware capture
        status, pl = cdc.read_data()
        if status == rig.STATUS_OK and pl and len(pl) >= 2:
            scanned = gm65qr.sanitize_scan(pl[1:])
            if scanned == gm65qr.sanitize_scan(payload):
                return {"ok": True, "scanned": scanned,
                        "latency_s": round(time.monotonic() - start, 2),
                        "type_byte": pl[0]}
        time.sleep(poll_s)
    return {"ok": False, "scanned": None, "latency_s": None, "type_byte": None}


def scan_ur_token(cdc, cyd, token: str, chunk: int = 68,
                  dwell_s: float = 0.8, cycles: int = 10) -> dict:
    """Deliver a token as animated UR fragments; assert device-side
    reassembly + import via GetTokenInfo.

    Each fragment is rendered, triggered, and given a quiet capture window.
    The firmware's state-level assembler accumulates fragments across
    ScannerData polls; the completing fragment imports the token.
    Returns {ok, fragments, token_info}.
    """
    import urtoken

    frames = urtoken.ur_fragments(token, chunk=chunk)
    seen: set[int] = set()
    clipped: dict[int, list[dict]] = {}
    outcomes: dict[int, int] = {}
    start = time.monotonic()
    # The import assertion is only meaningful from a clean slate: the
    # completing fragment flips GetTokenInfo from Error to Ok. Callers
    # reboot (or otherwise clear imported_token) before invoking this.
    st0, _ = cdc.token_info()
    assert st0 != rig.STATUS_OK, "device already holds a token — reset first"

    # BufferedUart cadence (2026-09-11): poll storms no longer break the
    # capture — render, trigger, poll at will. Fragment budget ~6 s each.
    for cycle in range(cycles):
        if cycle > 0:
            # Soak finding (2026-09-11): the decode engine fatigues on
            # unique-content load; a mid-run module heal restores it fully.
            cdc.send_recv(0x13, timeout=15.0)  # ScannerHeal
            time.sleep(8)
        for i, frame in enumerate(frames):
            cyd.show_qr(frame.encode("ascii"))
            cdc.trigger()
            frag_t0 = time.monotonic()
            want = frame.encode("ascii")
            while time.monotonic() - frag_t0 < 6.0:
                status, pl = cdc.read_data()
                if status == rig.STATUS_OK and pl and len(pl) >= 3 and pl[0] == rig.SCAN_TYPE_UR:
                    outcomes.setdefault(pl[1], 0)
                    outcomes[pl[1]] += 1
                    if pl[2:] == want:
                        seen.add(i)
                    else:
                        clipped.setdefault(i, []).append(
                            {"got": len(pl) - 2, "want": len(want)})
                    break
                time.sleep(0.4)
        if len(seen) == len(frames):
            st, info = cdc.token_info()
            if st == rig.STATUS_OK and len(info) > 12:
                return {"ok": True, "fragments": len(frames), "fragments_seen": sorted(seen),
                        "token_info": rig.parse_token_info(info),
                        "latency_s": round(time.monotonic() - start, 2)}
    return {"ok": False, "fragments": len(frames), "fragments_seen": sorted(seen),
            "clipped": clipped,
            "assembler_outcomes": outcomes,
            "token_info": None,
            "latency_s": round(time.monotonic() - start, 2)}
