"""Shared HIL harness — micronuts phone-vs-device suites (T5).

Trimmed copy of the nucula suite harness (Amperstrand/nucula
tests/hil/phone/harness.py, 2026-10-21) — kept per-repo on purpose:
each repo's HIL must run standalone on the bench runner. Differences:
the esp32-wallet REPL has no prompt marker (banner
`nucula-mode wallet ready`), so commands use idle-detection instead of
prompt-sync.
"""

from __future__ import annotations

import os
import re
import subprocess
import time
from pathlib import Path

import serial

COORDINATOR = os.environ.get("LG_COORDINATOR", "ai-legion:20408")
PHONE_SERIAL = os.environ.get("HIL_PHONE_SERIAL", "RF8NB01ZYAH")

M5STICK_CONSOLE_PORT = os.environ.get("HIL_M5STICK_CONSOLE_PORT", "")
WALLET_BANNER = "nucula-mode wallet ready"


def resolve_serial_port(place: str = "m5stick") -> str:
    """SerialPort resource `port` from the labgrid place (no hardcoded
    device paths); env override for offline collection."""
    if M5STICK_CONSOLE_PORT:
        return M5STICK_CONSOLE_PORT
    r = lg(place, "show")
    m = re.search(r"'port':\s*'([^']+)'", r.stdout)
    if not m:
        raise RuntimeError(
            f"no SerialPort 'port' param on place {place}; "
            "set HIL_M5STICK_CONSOLE_PORT explicitly")
    return m.group(1)


# --------------------------------------------------------------------------
# labgrid client (same shape as nucula tests/hil/phone/harness.py)
# --------------------------------------------------------------------------

class LabgridBusy(Exception):
    def __init__(self, place: str, holder: str | None, detail: str = ""):
        self.place, self.holder = place, holder
        super().__init__(
            f"labgrid place '{place}' held by {holder or 'unknown session'} {detail}".strip()
        )


def lg(place: str, *args: str, timeout: int = 30) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["labgrid-client", "-x", COORDINATOR, "-p", place, *args],
        capture_output=True, text=True, timeout=timeout, check=False,
    )


def place_holder(place: str) -> str | None:
    r = lg(place, "who")
    if r.returncode != 0:
        return None
    for line in r.stdout.strip().splitlines()[1:]:
        parts = line.split()
        if len(parts) >= 4 and parts[2] == place:
            return f"{parts[0]}@{parts[1]}"
    return ""


def acquire_place(place: str) -> str:
    r = lg(place, "acquire")
    if r.returncode != 0:
        if "already acquired" in r.stderr:
            return place
        raise LabgridBusy(place, place_holder(place), r.stderr.strip())
    return place


def release_place(place: str) -> None:
    lg(place, "release")


class AcquiredPlace:
    def __init__(self, place: str):
        self.place = place

    def __enter__(self) -> "AcquiredPlace":
        acquire_place(self.place)
        return self

    def __exit__(self, *exc) -> None:
        release_place(self.place)


# --------------------------------------------------------------------------
# esp32-wallet console (idle-detecting REPL)
# --------------------------------------------------------------------------

class WalletConsole:
    """The esp32-wallet REPL answers each verb with a deterministic
    first token (`nfc: …`, `balance: …`, `connected: …`) and NO prompt.
    Command completion = 1.5 s of line silence (bench console cadence).
    ONE session per suite: a fresh CP210x open may DTR-reset the stick —
    the banner sync turns that into the deterministic boot we assert."""

    IDLE_S = 1.5

    def __init__(self, port: str = M5STICK_CONSOLE_PORT, baud: int = 115200,
                 label: str = "esp32-wallet", evidence_dir: Path | None = None):
        self.label = label
        self.evidence_dir = evidence_dir
        self.transcript_path = (
            evidence_dir / f"{label}.transcript.log"
            if evidence_dir else None
        )
        s = serial.Serial()
        s.port = port
        s.baudrate = baud
        s.timeout = 0.05
        s.write_timeout = 5.0
        s.dtr = False
        s.rts = False
        s.open()
        self.ser = s
        self.buf = ""
        self.transcript = ""

    def _pump(self) -> str:
        n = self.ser.inWaiting()
        chunk = self.ser.read(n) if n else b""
        text = ""
        if chunk:
            text = chunk.decode("utf-8", errors="replace")
            self.buf += text
            self.transcript += text
        return text

    def read_until(self, pattern: str, timeout_s: float) -> str:
        pat = re.compile(pattern)
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            m = pat.search(self.buf)
            if m:
                out, self.buf = self.buf[:m.end()], self.buf[m.end():]
                return out
            self._pump()
            time.sleep(0.02)
        out, self.buf = self.buf, ""
        raise TimeoutError(
            f"[{self.label}] {pattern!r} not seen in {timeout_s}s; "
            f"tail: {out[-400:]!r}")

    def banner(self, timeout_s: float = 25.0) -> str:
        try:
            return self.read_until(re.escape(WALLET_BANNER), timeout_s)
        except TimeoutError:
            # Silent-console recovery (AGENTS.md pattern): a running
            # board prints nothing (no prompt); pulse RTS to reboot into
            # a fresh banner.
            self.ser.rts = True
            time.sleep(0.1)
            self.ser.rts = False
            return self.read_until(re.escape(WALLET_BANNER), timeout_s)

    def cmd(self, line: str, timeout_s: float = 60.0) -> str:
        # Long lines (cashu tokens are ~2-4 KB) overflow the console's
        # 2 KiB UART ring if written in one burst: chunk + pace so the
        # poll loop (20 ms sleeps) keeps the ring below watermark.
        data = (line + "\n").encode()
        CHUNK, PACE = 512, 0.15
        for i in range(0, len(data), CHUNK):
            self.ser.write(data[i:i + CHUNK])
            self.ser.flush()
            if i + CHUNK < len(data):
                time.sleep(PACE)
        # response = read until the line goes idle
        deadline = time.monotonic() + timeout_s
        idle = time.monotonic() + self.IDLE_S
        while time.monotonic() < deadline:
            if self._pump():
                idle = time.monotonic() + self.IDLE_S
            elif time.monotonic() > idle:
                out, self.buf = self.buf, ""
                return out
            time.sleep(0.05)
        out, self.buf = self.buf, ""
        return out

    def close(self) -> None:
        try:
            if self.transcript_path:
                self.transcript_path.write_text(self.transcript)
        finally:
            self.ser.close()
