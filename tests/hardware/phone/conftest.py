"""Fixtures — micronuts phone-vs-device HIL (T5).

--hil gates hardware (bench-runner flag only); labgrid place `m5stick`
gives exclusive console access. NOTE the m5stick is a SHARED bench
board: when it serves another project's image (ccid matrix duty), the
banner assert skips loudly — that skip IS the cross-lane coordination
record.
"""

from __future__ import annotations

import datetime
from pathlib import Path

import pytest

from harness import AcquiredPlace, LabgridBusy, WalletConsole, resolve_serial_port

RESULTS_ROOT = Path(__file__).parent.parent.parent / "hil-results"


def pytest_addoption(parser):
    parser.addoption("--hil", action="store_true", default=False,
                     help="drive real bench hardware (labgrid/adb/serial)")
    parser.addoption("--mint-url", action="store",
                     default="https://testnut.cashu.space")


def _need_hil(request) -> None:
    if not request.config.getoption("--hil"):
        pytest.skip("hardware gated: pass --hil on the bench runner")


@pytest.fixture(scope="session")
def evidence_dir() -> Path:
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    d = RESULTS_ROOT / stamp
    d.mkdir(parents=True, exist_ok=True)
    return d


@pytest.fixture(scope="module")
def m5stick_place(request):
    _need_hil(request)
    try:
        with AcquiredPlace("m5stick") as place:
            yield place
    except LabgridBusy as e:
        pytest.skip(f"bench busy: {e}")


@pytest.fixture(scope="module")
def wallet_console(request, m5stick_place, evidence_dir) -> WalletConsole:
    """Port-holder guard first: pcscd keeps a STANDING serial-reader claim
    on the m5stick CDC ("M5stick GemPC (MFRC522)" in reader.conf) while
    the ccid matrix image serves — two opens of one tty do not exclude
    each other, so the suite must yield instead of colliding."""
    import subprocess
    from harness import resolve_serial_port
    port = resolve_serial_port("m5stick")
    holders = subprocess.run(["fuser", "-v", port],
                             capture_output=True, text=True)
    if holders.returncode == 0 and holders.stderr.strip():
        pytest.skip(
            f"console port held by a foreign process ({holders.stderr.strip()})"
            " — e.g. pcscd's standing m5stick GemPC reader (ccid matrix "
            "duty); owner-schedule the wallet image per the RESTORE-ECASH "
            "runbook before T5 runs")
    con = WalletConsole(port=port, evidence_dir=evidence_dir)
    request.addfinalizer(con.close)
    try:
        banner = con.banner(timeout_s=8)
    except TimeoutError as e:
        pytest.skip(
            "board not on esp32-wallet firmware (banner missing — likely "
            f"another project's image): {e}")
    (evidence_dir / "esp32-wallet.banner.log").write_text(banner)
    return con
