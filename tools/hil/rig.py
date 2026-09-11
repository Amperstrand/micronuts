"""Micronuts QR rig: CYD QR source + GM65/F469 wallet, host plumbing.

Adapted from gm65-scanner's tools/hil/rig.py (their README blesses the
copy). Topology (bench, 2026-09-11):

    CYD (ESP32-2432S028, ST7796) --CH340 ttyUSB--> host serial line protocol
    CYD screen  <--points at-->  GM65 module --USART6--> STM32F469I-DISCO
    F469 user USB --CDC 16c0:27dd "Micronuts Cashu Hardware Wallet"--> host

The F469 board is SHARED with gm65-scanner sessions: flash sessions MUST
backup the 2 MiB image first and restore it afterwards (backup_stm32 /
restore_stm32). Safety: fips-lab boards.toml is the flash gate.
"""

from __future__ import annotations

import json
import os
import subprocess
import time
import tomllib
from pathlib import Path

import serial

# Shared CYD QR-source client (tollgate-lab owns it — same module gm65's
# harness imports; 2026-09-11 DRY extraction).
from tollgate_lab.cyd_qr import QR_WINNING_CAP, CydQrClient

REPO_ROOT = Path(__file__).resolve().parents[2]
BOARDS_TOML = REPO_ROOT.parent / "fips-lab" / "fips_lab" / "boards.toml"

CYD_REGISTRY_KEY = "cyd-ch340"
CYD_ID_PATH = "pci-0000:02:00.0-usb-0:1:1.0-port0"
CYD_VIDPID = (0x1A86, 0x7523)

STM32_REGISTRY_KEY = "stm32f469i-disco"
STLINK_SERIAL = "066FFF515786534867184152"

WALLET_CDC_VIDPID = (0x16C0, 0x27DD)  # micronuts wallet firmware
WALLET_CDC_SERIAL = "F4691"
WALLET_CDC_PRODUCT = "Micronuts Cashu Hardware Wallet"

STM32_FLASH_SIZE = 0x200000  # 2 MiB (STM32F469NI)
STM32_FLASH_BASE = 0x08000000

# Micronuts CDC protocol (micronuts-app/src/protocol.rs)
CMD_IMPORT_TOKEN = 0x01
CMD_GET_TOKEN_INFO = 0x02
CMD_GET_BLINDED = 0x03
CMD_SEND_SIGNATURES = 0x04
CMD_GET_PROOFS = 0x05
CMD_SCANNER_STATUS = 0x10
CMD_SCANNER_TRIGGER = 0x11
CMD_SCANNER_DATA = 0x12
STATUS_OK = 0x00
STATUS_NO_DATA = 0x12

SCAN_TYPE_PLAIN = 0x00
SCAN_TYPE_CASHU_V4 = 0x01
SCAN_TYPE_CASHU_V3 = 0x02
SCAN_TYPE_UR = 0x03
SCAN_TYPE_BINARY = 0x04


class RigError(RuntimeError):
    pass


def require_board(key: str, op: str) -> None:
    path = Path(os.environ.get("MICRONUTS_BOARDS_TOML", BOARDS_TOML))
    with open(path, "rb") as f:
        data = tomllib.load(f)
    spec = data.get("boards", {}).get(key)
    if spec is None:
        raise RigError(f"board {key!r} is not in {path} — refusing {op}")
    if op not in spec.get("ops", []):
        raise RigError(f"board {key!r} does not permit {op!r} (allowed: {spec.get('ops')})")


def cyd_port() -> Path:
    by_path = Path("/dev/serial/by-path") / CYD_ID_PATH
    if not by_path.exists():
        raise RigError(
            f"CYD CH340 not attached (expected /dev/serial/by-path/{CYD_ID_PATH})"
        )
    return by_path.resolve()


def find_serial_by_id(id_substring: str, timeout: float = 15.0):
    """Match /dev/serial/by-id entries on a substring (pyserial's .product is
    udev-dependent and often None for these CDCs)."""
    import glob as _glob

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        matches = _glob.glob(f"/dev/serial/by-id/*{id_substring}*")
        if matches:
            return matches[0]
        time.sleep(0.5)
    return None


def wait_wallet_cdc(timeout: float = 150.0) -> str:
    """The wallet shares VID:PID and serial F4691 with the gm65 sync
    firmware — identify by the by-id PRODUCT string, never VID:PID alone.
    Budget ~60s: USB only comes up after the boot splash (measured wall
    time, HARDWARE-TEST-RESULTS-20260903 + 41a5f7e correction)."""
    port = find_serial_by_id("Micronuts", timeout=timeout)
    if not port:
        raise RigError("micronuts wallet CDC (product 'Micronuts') not attached")
    return port


def cdc_with_retries(port: str, attempts: int = 10, settle_s: float = 3.0):
    last = None
    for _ in range(attempts):
        try:
            cdc = MicronutsCdc(port)
            cdc.scanner_status()
            return cdc
        except (RigError, serial.SerialException) as e:
            last = e
            time.sleep(settle_s)
    raise RigError(f"wallet CDC on {port} never answered ScannerStatus: {last}")




class MicronutsCdc:
    """3-byte framed CDC client for the micronuts wallet firmware
    ([cmd, len_hi, len_lo] / [status, len_hi, len_lo]) — same wire shape as
    the gm65-scanner firmware, different command set."""

    def __init__(self, port: str, timeout: float = 4.0):
        self.ser = serial.Serial(port, 115200, timeout=timeout)

    def close(self):
        self.ser.close()

    def drain(self):
        self.ser.reset_input_buffer()
        time.sleep(0.2)
        old = self.ser.timeout
        self.ser.timeout = 0.2
        while self.ser.read(256):
            pass
        self.ser.timeout = old

    def send_recv(self, cmd: int, payload: bytes = b"", timeout: float = 4.0):
        frame = bytes([cmd, (len(payload) >> 8) & 0xFF, len(payload) & 0xFF]) + payload
        self.ser.write(frame)
        self.ser.flush()
        old = self.ser.timeout
        self.ser.timeout = timeout
        resp = self.ser.read(3)
        body = b""
        if len(resp) == 3:
            length = (resp[1] << 8) | resp[2]
            body = self.ser.read(length) if length else b""
        self.ser.timeout = old
        if len(resp) < 3:
            return None, b""
        return resp[0], body

    def scanner_status(self):
        status, payload = self.send_recv(CMD_SCANNER_STATUS)
        if status != STATUS_OK or len(payload) < 3:
            raise RigError(
                f"ScannerStatus failed: status={status} payload={payload!r}"
            )
        return {"connected": payload[0], "data_ready": payload[1], "model": payload[2]}

    def trigger(self):
        status, _ = self.send_recv(CMD_SCANNER_TRIGGER, timeout=8.0)
        return status

    def read_data(self):
        return self.send_recv(CMD_SCANNER_DATA, timeout=4.0)

    def import_token(self, token_bytes: bytes):
        return self.send_recv(CMD_IMPORT_TOKEN, token_bytes, timeout=10.0)

    def token_info(self):
        return self.send_recv(CMD_GET_TOKEN_INFO, timeout=10.0)


def parse_token_info(payload: bytes) -> dict:
    """GetTokenInfo payload: mint-len || mint || unit-len || unit ||
    amount(u64 LE) || proofs(u32 LE)."""
    off = 0
    mint_len = payload[off]
    off += 1
    mint = payload[off : off + mint_len].decode()
    off += mint_len
    unit_len = payload[off]
    off += 1
    unit = payload[off : off + unit_len].decode()
    off += unit_len
    amount = int.from_bytes(payload[off : off + 8], "big")
    off += 8
    proofs = int.from_bytes(payload[off : off + 4], "big")
    return {"mint": mint, "unit": unit, "amount": amount, "proofs": proofs}


def _kill_port_users(port: Path):
    subprocess.run(["pkill", "-f", f"raw_logger.py {port}"], capture_output=True)
    time.sleep(0.3)
    subprocess.run(["fuser", "-k", str(port)], capture_output=True)
    time.sleep(0.7)


def build_firmware_bin() -> Path:
    """Build the wallet firmware and return the flashable .bin (the machine
    shares one target-dir across projects via ~/.cargo/config.toml)."""
    manifest = REPO_ROOT / "firmware" / "Cargo.toml"
    result = subprocess.run(
        [
            "cargo", "build", "--release", "--target", "thumbv7em-none-eabihf",
            "--manifest-path", str(manifest), "--message-format=json",
        ],
        capture_output=True, text=True, timeout=1800, cwd=REPO_ROOT,
    )
    if result.returncode != 0:
        raise RigError(f"cargo build failed: {result.stderr[-800:]}")
    elf = None
    for line in result.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") == "compiler-artifact" and msg.get("executable"):
            if msg.get("target", {}).get("name") == "firmware":
                elf = Path(msg["executable"])
    if elf is None:
        raise RigError("no compiler-artifact for firmware")
    bin_path = Path("/tmp/micronuts-firmware.bin")
    r = subprocess.run(
        ["arm-none-eabi-objcopy", "-O", "binary", str(elf), str(bin_path)],
        capture_output=True, timeout=60,
    )
    if r.returncode != 0:
        raise RigError("objcopy failed")
    return bin_path


def _st_flash(*args, timeout: int = 300) -> subprocess.CompletedProcess:
    result = subprocess.run(
        ["st-flash", "--connect-under-reset", *args],
        capture_output=True, text=True, timeout=timeout,
    )
    if result.returncode != 0:
        raise RigError(f"st-flash {' '.join(args)} failed: {result.stderr[-500:]}")
    return result


def backup_stm32(dest: Path) -> Path:
    require_board(STM32_REGISTRY_KEY, "flash")
    _st_flash("read", str(dest), hex(STM32_FLASH_BASE), hex(STM32_FLASH_SIZE), timeout=600)
    return dest


def flash_stm32(bin_path: Path) -> None:
    require_board(STM32_REGISTRY_KEY, "flash")
    subprocess.run(["pkill", "-9", "st-flash"], capture_output=True)
    time.sleep(1)
    _st_flash("write", str(bin_path), hex(STM32_FLASH_BASE))
    # st-flash write alone leaves the target wedged on this board (USB dead
    # until an explicit reset — gm65 bench lesson 2026-09-08/09)
    _st_flash("reset", timeout=60)


def restore_stm32(backup: Path) -> None:
    flash_stm32(backup)


def st_reset() -> None:
    _st_flash("reset", timeout=60)


def open_cyd() -> CydQrClient:
    return CydQrClient(str(cyd_port()))
