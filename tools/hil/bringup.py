"""Bench bring-up + feasibility checks for the micronuts QR rig.

Run order (gm65 pattern): BenchLock FIRST, then the labgrid place when the
coordinator is up. Backs up the F469 image, flashes the micronuts wallet
firmware, and runs the feasibility battery:
  F1  CYD answers ID (cyd-qr firmware present)
  F2  wallet CDC ScannerStatus: connected=1
  F3  single-frame payload roundtrip (quiet cadence: render -> trigger ->
      QUIET -> poll; poll storms break the firmware's capture window)
  F4  full token e2e: mint-tool mints on-device (generate/sign/export) ->
      UR fragments on the CYD -> GM65 scans -> device reassembles + imports
      -> GetTokenInfo asserts amount/proofs

Usage: python3 bringup.py [--skip-flash] [--keep]  (--keep: no restore)
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import rig
import gm65qr
import mnscan
import urtoken
from tollgate_lab import acquire_bench_lock

COORDINATOR = "192.168.13.221:20408"
PLACE = "micronuts-qr-rig"
BACKUP_DIR = Path(__file__).parent / "results"
MINT_TOOL = Path("/home/ubuntu/.cargo-target/release/mint-tool")


def lg(args, timeout_s: int = 15):
    return subprocess.run(
        ["labgrid-client", "-x", COORDINATOR, "-p", PLACE, *args],
        capture_output=True, text=True, timeout=timeout_s,
    )


def note(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def mint_demo_token(port: str, amount: int = 21) -> str:
    """Device self-mint via mint-tool: generate -> sign -> export."""
    for cmd in (["generate", "--amount", str(amount)], ["sign"], ["export"]):
        r = subprocess.run(
            [str(MINT_TOOL), "--port", port, *cmd],
            capture_output=True, text=True, timeout=120,
        )
        if r.returncode != 0:
            raise RuntimeError(f"mint-tool {cmd[0]} failed: {r.stderr[-300:]}")
        token = re.search(r"cashuB[A-Za-z0-9_-]+", r.stdout)
        if cmd[0] == "export":
            if not token:
                raise RuntimeError(f"no token in export output: {r.stdout[-300:]}")
            return token.group(0)
    raise RuntimeError("unreachable")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-flash", action="store_true")
    ap.add_argument("--keep", action="store_true", help="skip image restore")
    args = ap.parse_args()

    BACKUP_DIR.mkdir(parents=True, exist_ok=True)

    with acquire_bench_lock("amperstrand-bench"):
        acquired = lg(["acquire"])
        note(f"place acquire rc={acquired.returncode}")

        backup = None
        cdc = None
        cyd = None
        try:
            cyd = rig.open_cyd()
            fw_id = cyd.id()
            note(f"F1 CYD: {fw_id}")

            if not args.skip_flash:
                backup = BACKUP_DIR / f"f469-backup-{time.strftime('%Y%m%d-%H%M%S')}.bin"
                note(f"backing up F469 -> {backup}")
                rig.backup_stm32(backup)
                note("backup done, flashing micronuts firmware")
                bin_path = rig.build_firmware_bin()
                rig.flash_stm32(bin_path)
                note("flash done")

            port = rig.wait_wallet_cdc()
            note(f"F2 wallet CDC: {port}")
            cdc = rig.cdc_with_retries(port)
            status = cdc.scanner_status()
            note(f"F2 ScannerStatus: {status}")
            assert status["connected"] == 1, "GM65 not connected"

            gm65qr.arm_winning_config(cyd)

            # --- F3: single-frame roundtrip (quiet cadence) ---
            r = mnscan.scan_quiet(cdc, cyd, b"micronuts-feas1")
            note(f"F3 single-frame: {r}")
            assert r["ok"], "single-frame roundtrip failed"

            # --- F4: full token e2e over UR fragments ---
            cdc.close()
            note("F4 minting demo token on device (generate/sign/export)")
            token = mint_demo_token(port, amount=21)
            note(f"F4 token: {token[:32]}... ({len(token)} chars)")
            frags = urtoken.ur_fragments(token, chunk=68)
            note(f"F4 UR: {len(frags)} fragments, frame sizes "
                 f"{min(len(f) for f in frags)}-{max(len(f) for f in frags)}")
            # Reboot to a clean slate: mint-tool's generate IMPORTS its token
            # over CDC, which would make the scan-in assertion vacuous. Only
            # the QR path may set imported_token for F4 to mean anything.
            note("F4 rebooting device for a clean slate (~80s splash)")
            rig.st_reset()
            port = rig.wait_wallet_cdc()
            cdc = rig.cdc_with_retries(port, attempts=8, settle_s=4)
            st0 = None
            for _ in range(5):
                time.sleep(2)
                st0, pl0 = cdc.token_info()
                if st0 == 0xFF:
                    break
            note(f"F4 token-info after reboot: status=0x{st0:02x} (expect 0xff)")
            assert st0 == 0xFF, f"device not clean after reboot: 0x{st0:02x} {pl0!r}"
            r = mnscan.scan_ur_token(cdc, cyd, token, chunk=68, dwell_s=3.5)
            note(f"F4 scan-in: ok={r['ok']} seen={len(r.get('fragments_seen', []))}/{r['fragments']} info={r['token_info']}")
            assert r["ok"], "UR token scan-in failed"
            assert r["token_info"]["amount"] == 21, f"amount mismatch: {r['token_info']}"
            assert r["token_info"]["proofs"] >= 1, f"no proofs: {r['token_info']}"

            note("FEASIBILITY OK — full QR token transfer works")
            return 0
        finally:
            if cyd:
                cyd.close()
            if cdc:
                cdc.close()
            if backup is not None and not args.keep:
                note("restoring pre-session F469 image")
                rig.restore_stm32(backup)
                note("restore done")
            lg(["release"])
            note("place released")


if __name__ == "__main__":
    sys.exit(main())
