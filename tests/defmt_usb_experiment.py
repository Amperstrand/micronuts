#!/usr/bin/env python3
"""defmt-rtt vs USB experiment runner.

Tests whether defmt-rtt blocking mode (not probe-rs itself) causes
USB enumeration failures on STM32F469I-Discovery.

Usage:
    sudo python3 tests/defmt_usb_experiment.py <test_name> [count]

Test names:
    A  - Baseline: no probe-rs, defmt info (proven working)
    B  - probe-rs + no-defmt binary (minimal USB-only firmware)
    C  - probe-rs + DEFMT_LOG=off
    D  - probe-rs + DEFMT_RTT_BUFFER_SIZE=8192
    E  - probe-rs + disable-blocking-mode feature
    F  - probe-rs + DEFMT_LOG=error

Requires: st-flash, probe-rs, pyserial, arm-none-eabi-objcopy
"""

import glob
import os
import re
import serial
import serial.tools.list_ports_common
import subprocess
import sys
import time
import json
import signal
from datetime import datetime, timezone

BAUD = 115200
TIMEOUT = 3.0
CMD_COUNT = int(sys.argv[2]) if len(sys.argv) > 2 else 50
WALLET_VID = 0x16c0
WALLET_PID = 0x27dd

def run(cmd, timeout=120, check=True):
    r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=timeout)
    if check and r.returncode != 0:
        print(f"  CMD FAILED: {cmd}")
        print(f"  stdout: {r.stdout[-500:]}")
        print(f"  stderr: {r.stderr[-500:]}")
    return r

def find_wallet_port():
    for f in glob.glob("/dev/ttyACM*"):
        try:
            info = serial.tools.list_ports_common.ListPortInfo(f)
            if info.vid == WALLET_VID and info.pid == WALLET_PID:
                return f
        except Exception:
            pass
    return None

def wait_for_wallet_port(max_wait=30):
    for i in range(max_wait):
        port = find_wallet_port()
        if port:
            return port
        time.sleep(1)
    return None

def send_command(ser, cmd):
    frame = bytes([cmd, 0x00, 0x00])
    start = time.monotonic()
    ser.write(frame)
    ser.flush()
    header = ser.read(3)
    elapsed = time.monotonic() - start
    if len(header) < 3:
        return None, f"timeout ({elapsed:.2f}s)", elapsed
    status = header[0]
    length = (header[1] << 8) | header[2]
    payload = b""
    if length > 0:
        payload = ser.read(length)
        if len(payload) < length:
            return None, f"incomplete_payload ({len(payload)}/{length})", elapsed
    return {"status": status, "length": length, "payload": payload}, None, elapsed

def run_usb_test(port, count):
    try:
        ser = serial.Serial(port=port, baudrate=BAUD, timeout=TIMEOUT, dsrdtr=False, rtscts=False)
    except Exception as e:
        return {"pass": False, "error": f"Cannot open port: {e}"}

    time.sleep(0.1)
    successes = 0
    failures = 0
    first_error = None
    timings = []

    for i in range(count):
        resp, err, elapsed = send_command(ser, 0x10)
        timings.append(round(elapsed * 1000, 2))
        if resp is None:
            failures += 1
            if first_error is None:
                first_error = f"cmd#{i}: {err}"
        else:
            successes += 1

    ser.close()

    timings.sort()
    passed = failures == 0
    return {
        "pass": passed,
        "total": count,
        "successes": successes,
        "failures": failures,
        "first_error": first_error,
        "median_ms": timings[len(timings)//2] if timings else 0,
        "max_ms": timings[-1] if timings else 0,
    }

def kill_probe_rs():
    run("pkill -9 probe-rs 2>/dev/null; sleep 2", check=False)

def flash_firmware(bin_path):
    kill_probe_rs()
    r = run(f'st-flash --connect-under-reset write "{bin_path}" 0x08000000', timeout=30)
    if r.returncode != 0:
        return False
    r = run("st-flash --connect-under-reset reset", timeout=10)
    return r.returncode == 0

def build_firmware(extra_env=None, extra_features=None, example=None, no_defmt=False):
    env_parts = []
    if extra_env:
        env_parts.append(extra_env)
    env_str = " ".join(env_parts)

    target = "thumbv7em-none-eabihf"
    if no_defmt:
        pkg = "--bin usb-test-no-defmt"
    elif example:
        pkg = f"--bin {example}"
    else:
        pkg = "-p firmware"

    features = ""
    if extra_features:
        features = f'--no-default-features --features "{extra_features}"'
    elif no_defmt:
        features = ""

    cmd = f"{env_str} cargo build {pkg} --release --target {target} {features}"
    print(f"  Building: {cmd[:120]}...")
    r = run(cmd, timeout=300)
    if r.returncode != 0:
        return None
    elf = f"target/{target}/release/firmware"
    if no_defmt:
        elf = f"target/{target}/release/usb-test-no-defmt"
    bin_path = f"{elf}.bin"
    r = run(f'arm-none-eabi-objcopy -O binary "{elf}" "{bin_path}"', timeout=30)
    if r.returncode != 0:
        return None
    return bin_path

def start_probe_rs(elf_path):
    proc = subprocess.Popen(
        ["probe-rs", "run", "--chip", "STM32F469NIHx", elf_path],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    time.sleep(5)
    return proc

def stop_probe_rs(proc):
    if proc:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
    kill_probe_rs()

def run_test(test_name):
    print(f"\n{'='*60}")
    print(f"  Test {test_name}: {TESTS[test_name]['desc']}")
    print(f"  Commands: {CMD_COUNT}, Timeout: {TIMEOUT}s")
    print(f"  Time: {datetime.now(timezone.utc).isoformat()}")
    print(f"{'='*60}")

    cfg = TESTS[test_name]
    use_probe_rs = cfg.get("probe_rs", False)
    no_defmt = cfg.get("no_defmt", False)
    example = cfg.get("example", None)
    extra_env = cfg.get("env", None)
    extra_features = cfg.get("features", None)

    print("\n[1/4] Building firmware...")
    bin_path = build_firmware(
        extra_env=extra_env,
        extra_features=extra_features,
        example=example,
        no_defmt=no_defmt,
    )
    if bin_path is None:
        print(f"  BUILD FAILED")
        return {"test": test_name, "pass": False, "error": "build_failed"}

    print(f"  Binary: {bin_path}")

    print("\n[2/4] Flashing firmware...")
    if not flash_firmware(bin_path):
        print(f"  FLASH FAILED")
        return {"test": test_name, "pass": False, "error": "flash_failed"}

    print("\n[3/4] Waiting for boot...")
    if use_probe_rs:
        elf = f"target/thumbv7em-none-eabihf/release/firmware"
        if no_defmt:
            elf = f"target/thumbv7em-none-eabihf/release/usb-test-no-defmt"
        print("  Starting probe-rs...")
        probe_proc = start_probe_rs(elf)
        time.sleep(10)
    else:
        probe_proc = None
        time.sleep(18)

    print("\n[4/4] Running USB test...")
    port = find_wallet_port()
    if not port:
        port = wait_for_wallet_port(max_wait=15)

    if not port:
        if probe_proc:
            stop_probe_rs(probe_proc)
        print(f"  WALLET PORT NOT FOUND")
        return {"test": test_name, "pass": False, "error": "port_not_found", "probe_rs": use_probe_rs}

    print(f"  Port: {port}")
    result = run_usb_test(port, CMD_COUNT)
    result["test"] = test_name
    result["probe_rs"] = use_probe_rs
    result["desc"] = cfg["desc"]

    if probe_proc:
        stop_probe_rs(probe_proc)

    status = "PASS" if result["pass"] else "FAIL"
    print(f"\n  Result: {status}")
    print(f"  Successes: {result['successes']}/{result['total']}")
    print(f"  Median: {result['median_ms']}ms, Max: {result['max_ms']}ms")
    if result["first_error"]:
        print(f"  First error: {result['first_error']}")

    return result

TESTS = {
    "A": {
        "desc": "Baseline: no probe-rs, defmt info, 1024-byte buffer",
        "probe_rs": False,
    },
    "C": {
        "desc": "probe-rs + DEFMT_LOG=off (no logging output)",
        "probe_rs": True,
        "env": "DEFMT_LOG=off",
    },
    "D": {
        "desc": "probe-rs + DEFMT_RTT_BUFFER_SIZE=8192 (large buffer)",
        "probe_rs": True,
        "env": "DEFMT_RTT_BUFFER_SIZE=8192",
    },
    "E": {
        "desc": "probe-rs + disable-blocking-mode (non-blocking RTT writes)",
        "probe_rs": True,
        "features": "defmt,rtt-nonblocking",
    },
    "F": {
        "desc": "probe-rs + DEFMT_LOG=error (minimal logging)",
        "probe_rs": True,
        "env": "DEFMT_LOG=error",
    },
    "B": {
        "desc": "probe-rs + no-defmt binary (minimal USB-only firmware)",
        "probe_rs": True,
        "no_defmt": True,
    },
}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(f"Usage: sudo {sys.argv[0]} <test_name> [count]")
        print(f"Tests: {', '.join(sorted(TESTS.keys()))}")
        sys.exit(1)

    test_name = sys.argv[1].upper()
    if test_name not in TESTS:
        print(f"Unknown test: {test_name}. Valid: {', '.join(sorted(TESTS.keys()))}")
        sys.exit(1)

    os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    result = run_test(test_name)

    results_dir = "tests/results"
    os.makedirs(results_dir, exist_ok=True)
    ts = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    outfile = f"{results_dir}/defmt_experiment_{test_name}_{ts}.json"
    with open(outfile, "w") as f:
        json.dump(result, f, indent=2)
    print(f"\n  Saved: {outfile}")

    sys.exit(0 if result["pass"] else 1)
