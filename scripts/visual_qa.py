#!/usr/bin/env python3
"""Objective visual QA for micronuts screen captures (480x800).

Run after regenerating captures with the shotui example:

    cargo run -p micronuts-app --example shotui --features std
    python3 scripts/visual_qa.py target/shots

Checks per screenshot:
  - edge_bleed:  no content within 3px of left/right/bottom edges
                 below the header zone (catches text/rect overflow)
  - bright_ink:  >=0.4% of pixels are bright text/icons on the dark bg
  - structure:   >=5% of pixels differ from the background (cards,
                 bars, QR card present)
Screen-specific:
  - home cards:  three card bands share identical x-extent & width
  - scanning:    green progress-bar fill present at the track
  - qr screens:  >=300px contiguous white run + black modules with
                 25-60% density (scannable QR card)

Exit code: 0 all checks pass, 1 otherwise (report lists failures).
"""
import sys
import numpy as np
from PIL import Image

W, H = 480, 800
HEADER = 60

BG = (0x0B, 0x0F, 0x17)


def near(a, b, tol=14):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def load(p):
    return np.asarray(Image.open(p).convert("RGB")).astype(int)


def edge_bleed(img):
    region = img[HEADER:, :, :]
    left = region[:, :3].reshape(-1, 3)
    right = region[:, -3:].reshape(-1, 3)
    bottom = img[-3:, :, :].reshape(-1, 3)
    bad = 0
    for strip in (left, right, bottom):
        for px in strip:
            if not near(px, BG):
                bad += 1
                break
    return bad == 0


def bright_ink(img):
    lum = img.mean(axis=2)
    return float((lum > 90).mean()) >= 0.004


def structure(img):
    lum = img.mean(axis=2)
    bg_lum = np.median(lum)
    return float((np.abs(lum - bg_lum) > 6).mean()) >= 0.05


def home_cards(img):
    bgc = np.array(BG)
    rows = []
    for cy in (160, 360, 560):
        row = img[cy]
        mask = ~np.all(np.abs(row - bgc) <= 14, axis=1)
        if not mask.any():
            return False, "no card at y=%d" % cy
        xs = np.where(mask)[0]
        rows.append((int(xs.min()), int(xs.max())))
    xs0 = {r[0] for r in rows}
    xs1 = {r[1] for r in rows}
    return len(xs0) == 1 and len(xs1) == 1, str(rows)


def scan_bar(img):
    band = img[700:712, 40:440]
    green = np.all(np.abs(band - np.array([0x3E, 0xD5, 0x8A])) <= 30, axis=2)
    return int(green.sum()) > 0


def qr_card(img):
    lum = img.mean(axis=2)
    white_rows = 0
    best = 0
    for y in range(150, 700, 7):
        run = 0
        mx = 0
        for x in range(0, W):
            if lum[y, x] > 220:
                run += 1
                mx = max(mx, run)
            else:
                run = 0
        best = max(best, mx)
        if mx >= 300:
            white_rows += 1
    if best < 300 or white_rows < 20:
        return False, "white run best=%d rows=%d" % (best, white_rows)
    card = img[170:660, 40:440]
    black = np.all(card < 60, axis=2)
    white = np.all(card > 200, axis=2)
    total = black.sum() + white.sum()
    if total == 0:
        return False, "no modules"
    density = black.sum() / total
    return 0.25 <= density <= 0.60, "density=%.2f" % density


CHECKS = {
    "01_home.png": [edge_bleed, bright_ink, structure, lambda i: home_cards(i)[0]],
    "02_home_last_scan.png": [edge_bleed, bright_ink, structure, lambda i: home_cards(i)[0]],
    "03_scanning.png": [edge_bleed, bright_ink, structure, scan_bar],
    "04_scanning_retry.png": [edge_bleed, bright_ink, structure],
    "05_scan_result_cashu.png": [edge_bleed, bright_ink, structure],
    "06_scan_result_text.png": [edge_bleed, bright_ink, structure, qr_card],
    "07_waiting_token.png": [edge_bleed, bright_ink, structure],
    "08_token_info.png": [edge_bleed, bright_ink, structure],
    "09_export_qr.png": [edge_bleed, bright_ink, structure, qr_card],
    "10_error.png": [edge_bleed, bright_ink, structure],
    "11_status.png": [edge_bleed, bright_ink, structure],
}

NAMES = {
    edge_bleed: "edge_bleed",
    bright_ink: "bright_ink",
    structure: "structure",
    scan_bar: "scan_bar",
    qr_card: "qr_card",
    home_cards: "home_cards",
}


def main(d):
    failures = 0
    for name, checks in CHECKS.items():
        img = load("%s/%s" % (d, name))
        for c in checks:
            ok = c(img)
            label = NAMES.get(c, getattr(c, "__name__", str(c)))
            if not ok:
                print("FAIL %s: %s" % (name, label))
                failures += 1
    print("%d failures across %d screenshots" % (failures, len(CHECKS)))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/shots-after")
