"""T5 — micronuts esp32-wallet (MFRC522) as the second reader counterpart.

The m5stick runs esp32-wallet firmware with feature `nfc-mfrc522`
(PR #108): the `nfc` verb does one ISO-14443-3 activation (REQA →
select → HLTA) and prints the tag's ATQA/UID. A phone HCE tag in the
field answers at 14443-3 level, so phone-vs-device tap detection is:

    nfc: tag ATQA=..UID=02..   (phone HCE UID class)

Board policy: the m5stick is a shared bench board. When it serves
another project's image (e.g. the ccid GemPC matrix), the banner
fixture times out and the whole suite skips with that reason — never
reflash over a live sibling experiment (RESTORE-ECASH runbook v2
doctrine). When the board is on wallet fw, the full flow runs:
console health → wallet `receive` of a rig-minted token → `nfc` tap
detection with the phone staged.
"""

import re

import pytest

PHONE_HCE_UID_CLASS = re.compile(r"UID=[0-9A-F]{2}")


def test_console_banner_and_help(wallet_console):
    out = wallet_console.cmd("help")
    assert "connect" in out and "balance" in out, out
    assert "nfc" in out, f"firmware lacks the nfc verb (feature nfc-mfrc522)"


def test_wallet_console_health(wallet_console):
    out = wallet_console.cmd("heap")
    assert re.search(r"free: \d+ B", out), out
    out = wallet_console.cmd("balance")
    assert re.search(r"balance: \d+ sats", out), out


def test_nfc_verb_sees_phone_hce(wallet_console, evidence_dir):
    """Phone staged on the reader antenna (bench geometry): the `nfc`
    verb must answer — tag lines prove coupling, `no tag in field` is a
    loud skip (phone not on the coil), and an MFRC522 init error is a
    hard fail (hardware regression PR #108 guards against)."""
    out = wallet_console.cmd("nfc", timeout_s=30)
    (evidence_dir / "t5_nfc_verb.log").write_text(out)
    if "nfc init error" in out or "nfc i2c error" in out:
        pytest.fail(f"MFRC522 frontend broken: {out}")
    if "no tag in field" in out:
        pytest.skip("no tag coupled — phone not on the m5stick coil; "
                    "place it and rerun (bench geometry)")
    m = PHONE_HCE_UID_CLASS.search(out)
    assert m, out
    assert "ATQA=" in out, out


def test_wallet_receive_via_console(wallet_console, mint_url, evidence_dir):
    """Full money leg on the SECOND device target: mint a token with the
    nucula rig payer at the bench mint, `receive` it on the esp32-wallet,
    balance grows. Requires the wallet fw to be built with the same mint
    (MICRONUTS_MINT_URL) and bench WiFi.

    Console caveat (bench-characterized 2026-10-22): wallet ops execute
    and persist correctly, but the REPL dies after any multi-KB line
    crosses the UART (token in OR token print out) — so the authoritative
    assert is the BALANCE, read via `status` after a console reopen if
    the reply was lost."""
    out = wallet_console.cmd("connect", timeout_s=60)
    if "connect error" in out:
        pytest.skip(f"wallet cannot reach its baked mint (wifi/mint "
                    f"mismatch): {out!r}")
    out = wallet_console.cmd("status")
    m = re.search(r"balance: (\d+) sats", out)
    assert m, out
    before = int(m.group(1))

    import os
    import sys
    from pathlib import Path
    nucula_root = os.environ.get("HIL_NUCULA_ROOT")
    if not nucula_root:
        pytest.skip("HIL_NUCULA_ROOT unset — rig payer checkout for "
                    "host-side minting not provided")
    sys.path.insert(0, str(Path(nucula_root) / "tests" / "hil" / "phone"))
    from harness import mint_token_via_rig  # nucula harness payer
    token = mint_token_via_rig(Path(nucula_root), mint_url, sats=21)

    out = wallet_console.cmd(f"receive {token}", timeout_s=180)
    (evidence_dir / "t5_receive.log").write_text(out)

    # authoritative read: balance via status; if the multi-KB exchange
    # killed the REPL (known fw issue), reopen (RTS reboot) and re-read —
    # NVS persistence makes the balance the durable truth.
    out = wallet_console.cmd("status")
    if "balance:" not in out:
        from harness import resolve_serial_port, WalletConsole
        wallet_console.close()
        con2 = WalletConsole(port=resolve_serial_port("m5stick"))
        try:
            con2.banner(40)
            import time as _t
            _t.sleep(4); con2._pump(); con2.buf = ""
            out = con2.cmd("status")
        finally:
            con2.close()
    (evidence_dir / "t5_balance_after.log").write_text(out)
    m = re.search(r"balance: (\d+) sats", out)
    assert m, f"no balance readable after receive:\n{out}"
    assert int(m.group(1)) > before, (
        f"balance did not grow after receive ({before} -> {m.group(1)})")
    m = re.search(r"balance: (\d+) sats", wallet_console.cmd("balance"))
    assert m and int(m.group(1)) > before, "balance did not grow after receive"
