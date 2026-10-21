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

from harness import AcquiredPlace, LabgridBusy, WalletConsole

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
    con = WalletConsole(evidence_dir=evidence_dir)
    request.addfinalizer(con.close)
    banner = con.banner(timeout_s=8)
    (evidence_dir / "esp32-wallet.banner.log").write_text(banner)
    return con
