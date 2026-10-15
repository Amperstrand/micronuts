"""Signed, reservation-capable labgrid place holding for the micronuts HIL
scripts (bench canon: conwrt-bench docs/labgrid-coordination.md, "Identify
and release your holds"; pattern reference ccid-firmware-rs).

Two behaviors:
- sign_identity(): every hold this process makes shows up in
  `labgrid-client who` as free text (LG_USERNAME override) — who runs
  what, from which tree.
- acquire()/release(): plain acquire by default; `wait=True` queues via
  a coordinator reservation instead of failing when the rig is busy
  (owner-locked allocation — the current holder's release hands the
  place to the queue head, not to the fastest poller).
"""
import os
import re
import subprocess
import time
from pathlib import Path

_REPO_ROOT = Path(__file__).resolve().parent.parent.parent


def sign_identity(label: str) -> None:
    if os.environ.get("LG_USERNAME"):
        return
    try:
        sha = subprocess.run(
            ["git", "rev-parse", "--short=7", "HEAD"],
            capture_output=True, text=True, timeout=10, cwd=_REPO_ROOT,
        ).stdout.strip()
    except Exception:
        sha = "nogit"
    os.environ["LG_USERNAME"] = f"micronuts-hil~{label}~sha={sha}"


def _client(coordinator: str, *args: str, timeout_s: float = 15):
    return subprocess.run(
        ["labgrid-client", "-x", coordinator, *args],
        capture_output=True, text=True, timeout=timeout_s,
    )


def acquire(place: str, coordinator: str, wait: bool = False,
            priority: int = 0, timeout_min: int = 120):
    """Acquire `place`. Returns the reservation token when the hold came
    through a reservation (release() needs it), else None. Raises
    RuntimeError on refusal."""
    acquired = _client(coordinator, "-p", place, "acquire")
    if acquired.returncode == 0:
        return None
    stderr = acquired.stderr.strip()
    if not wait:
        raise RuntimeError(
            f"place {place} busy ({stderr or 'see labgrid-client who'}); "
            "pass --wait-place to queue politely"
        )
    r = _client(coordinator, "reserve", "--wait", "--prio", str(priority),
                f"name={place}", timeout_s=timeout_min * 60)
    if r.returncode != 0:
        raise RuntimeError(
            f"reservation for {place} failed: {(r.stderr or r.stdout).strip()[:200]}"
        )
    m = re.search(r"^\s*token:\s*(\S+)", r.stdout, re.M)
    if not m:
        raise RuntimeError(f"no reservation token in output: {r.stdout!r}")
    token = m.group(1)
    alloc = _client(coordinator, "-p", f"+{token}", "acquire")
    if alloc.returncode != 0:
        _client(coordinator, "cancel-reservation", token)
        raise RuntimeError(
            f"could not acquire allocated {place}: {alloc.stderr.strip()[:200]}"
        )
    print(f"[{time.strftime('%H:%M:%S')}] place {place} acquired via reservation {token}",
          flush=True)
    return token


def release(place: str, coordinator: str, token: str | None) -> None:
    _client(coordinator, "-p", f"+{token}" if token else place, "release")
    if token:
        _client(coordinator, "cancel-reservation", token)
