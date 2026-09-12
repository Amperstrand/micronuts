"""GM65 sustained-load degradation study (#92) — root-cause session.

Phases (all against the live wallet firmware over CDC):
  baseline    fresh-render health check after a module power-cycle
  load:unique sustained fresh-content renders at a fixed duty — the
              degradation trigger; records the onset curve
  heal        mid-session ScannerHeal (0x13: crate deep_sleep_reboot +
              re-init + policy restart) followed by a health re-measure
  load:same   SAME content re-rendered every cycle — discriminates
              decode-engine load (degrades only on unique) from
              optical/screen-refresh effects (degrades on both)

Artifacts: results/soak-<ts>/{summary.md, data.json}
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import rig
import gm65qr
from tollgate_lab import acquire_bench_lock

RESULTS = Path(__file__).parent / "results"
CMD_SCANNER_HEAL = 0x13
ALNUM = gm65qr.ALNUM


def note(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def one_scan(cdc, cyd, payload: bytes, budget_s: float = 8.0) -> dict:
    """Render + trigger + poll until the scan returns (storm cadence)."""
    cyd.show_qr(payload)
    cdc.trigger()
    t0 = time.monotonic()
    while time.monotonic() - t0 < budget_s:
        st, pl = cdc.read_data()
        if st == rig.STATUS_OK and pl and len(pl) >= 2:
            got = gm65qr.sanitize_scan(pl[1:])
            return {"ok": got == gm65qr.sanitize_scan(payload),
                    "latency_s": round(time.monotonic() - t0, 2)}
        time.sleep(0.4)
    return {"ok": False, "latency_s": None}


def health(cdc, cyd, n: int, seq_base: int) -> list[dict]:
    out = []
    for i in range(n):
        payload = gm65qr.unique_payload(seq_base + i, 14)
        r = one_scan(cdc, cyd, payload)
        out.append(r)
        note(f"  health[{i}] ok={r['ok']} lat={r['latency_s']}")
        time.sleep(1.0)
    return out


def load(cdc, cyd, duty_s: float, max_s: float, mode: str, seq_base: int) -> list[dict]:
    """Sustained renders; stop early after `streak_fail` consecutive misses."""
    fixed = b"fixed-payload-constant-content"
    out = []
    fails = 0
    t0 = time.monotonic()
    i = 0
    while time.monotonic() - t0 < max_s:
        payload = fixed if mode == "same" else gm65qr.unique_payload(seq_base + i, 14)
        r = one_scan(cdc, cyd, payload, budget_s=duty_s)
        r.update({"t": round(time.monotonic() - t0, 1), "i": i, "mode": mode})
        out.append(r)
        fails = fails + 1 if not r["ok"] else 0
        i += 1
        if fails >= 6:
            note(f"  load[{mode}] stopping: 6 consecutive failures at t={r['t']}s")
            break
    ok = sum(1 for r in out if r["ok"])
    note(f"  load[{mode}]: {ok}/{len(out)} scans in {round(time.monotonic()-t0)}s duty={duty_s}s")
    return out


def main() -> int:
    with acquire_bench_lock("amperstrand-bench"):
        port = rig.wait_wallet_cdc()
        cdc = rig.cdc_with_retries(port, attempts=8, settle_s=4)
        cyd = rig.open_cyd()
        gm65qr.arm_winning_config(cyd)

        data: dict = {"started": time.strftime("%Y-%m-%dT%H:%M:%S")}

        note("phase: baseline health (post power-cycle)")
        data["baseline"] = health(cdc, cyd, n=5, seq_base=1000)

        note("phase: load unique @2s duty (max 6 min)")
        data["load_unique"] = load(cdc, cyd, duty_s=2.0, max_s=360, mode="unique", seq_base=2000)

        note("phase: heal (ScannerHeal 0x13) + re-measure")
        st, pl = cdc.send_recv(CMD_SCANNER_HEAL, timeout=15.0)
        note(f"  heal status=0x{st:02x}")
        time.sleep(8)
        data["post_heal"] = health(cdc, cyd, n=5, seq_base=3000)

        note("phase: load same-content re-render @2s (discriminator, max 3 min)")
        data["load_same"] = load(cdc, cyd, duty_s=2.0, max_s=180, mode="same", seq_base=4000)

        note("phase: final health")
        data["final"] = health(cdc, cyd, n=3, seq_base=5000)

        outdir = RESULTS / f"soak-{time.strftime('%Y%m%d-%H%M%S')}"
        outdir.mkdir(parents=True, exist_ok=True)
        (outdir / "data.json").write_text(json.dumps(data, indent=1))
        summary = [f"# GM65 soak — {data['started']}", ""]
        for phase in ("baseline", "load_unique", "post_heal", "load_same", "final"):
            rows = data[phase]
            if not rows:
                continue
            ok = sum(1 for r in rows if r.get("ok"))
            lats = [r["latency_s"] for r in rows if r.get("latency_s")]
            summary.append(
                f"- **{phase}**: {ok}/{len(rows)} ok"
                + (f", lat p50≈{sorted(lats)[len(lats)//2]}s max {max(lats)}s" if lats else "")
            )
        (outdir / "summary.md").write_text("\n".join(summary) + "\n")
        note(f"artifacts: {outdir}")
        cyd.close()
        cdc.close()
        return 0


if __name__ == "__main__":
    sys.exit(main())
