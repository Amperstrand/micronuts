"""Physical QR loop: REAL testnut-minted ecash → CYD QR → GM65 laser → F469.

The capstone hardware test (money taxonomy: testnut FakeWallet test
money). Flow:
  1. host mints a live token from testnut over HTTPS (engine example)
  2. F469 rebooted to a clean slate (no imported token)
  3. token UR-fragmented onto the CYD, GM65 scans each frame into the
     device, firmware reassembles + imports
  4. GetTokenInfo asserts the amount + proof count

Same safety pattern as bringup.py: BenchLock FIRST, labgrid place,
image backup/restore in finally (--keep to skip restore).
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
EXAMPLE = Path(
    "/home/ubuntu/.cargo-target/debug/examples/print_live_token"
)


def lg(args, timeout_s: int = 15):
    return subprocess.run(
        ["labgrid-client", "-x", COORDINATOR, "-p", PLACE, *args],
        capture_output=True, text=True, timeout=timeout_s,
    )


def note(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def mint_live_token(amount: int, send: int) -> str:
    """Run the engine example against testnut over HTTPS."""
    env = {
        **__import__("os").environ,
        "TESTNUT_AMOUNT": str(amount),
        "TESTNUT_SEND": str(send),
    }
    r = subprocess.run(
        [str(EXAMPLE)], capture_output=True, text=True, timeout=120, env=env,
    )
    if r.returncode != 0:
        raise RuntimeError(f"live mint failed: {r.stderr[-400:]}")
    token = re.search(r"cashuB[A-Za-z0-9_-]+", r.stdout)
    if not token:
        raise RuntimeError(f"no token in output: {r.stdout[-300:]}")
    return token.group(0)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-flash", action="store_true")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--fund", type=int, default=47)
    ap.add_argument("--send", type=int, default=21)
    args = ap.parse_args()

    BACKUP_DIR.mkdir(parents=True, exist_ok=True)

    with acquire_bench_lock("amperstrand-bench"):
        from tollgate_lab import ensure_run_headroom

        hygiene = ensure_run_headroom()
        if hygiene.acted:
            note(f"disk hygiene: {hygiene.actions}")

        acquired = lg(["acquire"])
        note(f"place acquire rc={acquired.returncode}")

        backup = None
        cdc = None
        cyd = None
        try:
            # --- L1: live token from the real mint over HTTPS ---
            note(f"L1 minting {args.send} sats from testnut (fund {args.fund})")
            token = mint_live_token(args.fund, args.send)
            note(f"L1 token: {token[:40]}... ({len(token)} chars)")

            cyd = rig.open_cyd()
            try:
                fw_id = cyd.id()
            except Exception:
                # Shared bench CYD may run foreign firmware (wifi debris on
                # the console) — reflash cyd-qr (gm65-scanner owns it).
                note("CYD not answering ID — flashing cyd-qr firmware")
                cyd.close(); cyd = None
                GM65_RIG = Path("/home/ubuntu/src/gm65-scanner/tools/hil/rig.py")
                sys.path.insert(0, str(GM65_RIG.parent))
                gm_rig = __import__("rig")
                gm_rig.flash_cyd(gm_rig.build_cyd_elf())
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

            # --- L2: clean slate (reboot clears any imported token) ---
            note("L2 rebooting device for a clean slate (~80s splash)")
            rig.st_reset()
            port = rig.wait_wallet_cdc()
            cdc = rig.cdc_with_retries(port, attempts=8, settle_s=4)
            status = cdc.scanner_status()
            note(f"F2 ScannerStatus: {status}")
            assert status["connected"] == 1, "GM65 not connected"

            st0 = None
            for _ in range(5):
                time.sleep(2)
                st0, pl0 = cdc.token_info()
                if st0 == 0xFF:
                    break
            note(f"L2 token-info after reboot: status=0x{st0:02x} (expect 0xff)")
            assert st0 == 0xFF, f"device not clean after reboot: 0x{st0:02x} {pl0!r}"

            # --- L3: physical scan-in over UR fragments ---
            gm65qr.arm_winning_config(cyd)
            frags = urtoken.ur_fragments(token, chunk=68)
            note(f"L3 UR: {len(frags)} fragments, frame sizes "
                 f"{min(len(f) for f in frags)}-{max(len(f) for f in frags)}")
            r = mnscan.scan_ur_token(cdc, cyd, token, chunk=68, dwell_s=3.5)
            note(f"L3 scan-in: ok={r['ok']} seen={len(r.get('fragments_seen', []))}/{r['fragments']} "
                 f"info={r['token_info']}")
            assert r["ok"], "UR token scan-in failed"

            # --- L4: the device holds the REAL ecash ---
            info = r["token_info"]
            assert info["amount"] == args.send, f"amount mismatch: {info}"
            assert info["proofs"] >= 1, f"no proofs: {info}"

            note(f"PHYSICAL LOOP OK — {args.send} sats of testnut ecash "
                 f"({info['proofs']} proofs) scanned by laser into the F469")
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
