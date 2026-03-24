#!/usr/bin/env python3
"""
Boot Splash Preview Renderer for Micronuts

Simulates the boot splash animation on the host and generates:
  - Individual PNG frames for each variant
  - An animated GIF showing all variants cycling
  - A composite screenshot suitable for README embedding

This replicates the firmware rendering logic in Python so developers
can preview what the splash looks like without flashing hardware.

Usage:
    python3 scripts/render_preview.py

Output: firmware/assets/preview/
"""

import os
import struct
import sys

from PIL import Image, ImageDraw, ImageFont

# ---------------------------------------------------------------------------
# Configuration — must match firmware constants
# ---------------------------------------------------------------------------

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS_DIR = os.path.join(REPO_ROOT, "firmware", "assets", "generated")
OUT_DIR = os.path.join(REPO_ROOT, "firmware", "assets", "preview")

WIDTH = 800
HEIGHT = 480
BG_COLOR = (0, 0, 0)

TILE_GAP = 2
VARIANT_DURATION_FRAMES = 90  # 3 seconds at 30 FPS

# Tile catalog (matches generate_assets.py output)
TILE_FILES = [
    ("tiny",   "nut-tiny.png"),
    ("small",  "nut-small.png"),
    ("medium", "nut-medium.png"),
    ("large",  "nut-large.png"),
]

# Variant configs (must match firmware VARIANTS array)
VARIANTS = [
    {
        "name": "A — Dense Drift",
        "tile_index": 0,  # tiny
        "speed": 3,
        "brick_stagger": False,
        "odd_row_speed_delta": 0,
        "row_phase_step": 0,
        "extra_gap": 0,
    },
    {
        "name": "B — Brick Scroll",
        "tile_index": 2,  # medium
        "speed": 2,
        "brick_stagger": True,
        "odd_row_speed_delta": 1,
        "row_phase_step": 0,
        "extra_gap": 4,
    },
    {
        "name": "C — Big Wave",
        "tile_index": 3,  # large
        "speed": 1,
        "brick_stagger": False,
        "odd_row_speed_delta": 0,
        "row_phase_step": 8,
        "extra_gap": 6,
    },
]


# ---------------------------------------------------------------------------
# Rendering engine (mirrors firmware logic)
# ---------------------------------------------------------------------------

def load_tiles():
    """Load tile images from generated assets."""
    tiles = []
    for name, filename in TILE_FILES:
        path = os.path.join(ASSETS_DIR, filename)
        if not os.path.exists(path):
            print(f"WARNING: tile {path} not found, run generate_assets.py first")
            sys.exit(1)
        img = Image.open(path).convert("RGBA")
        tiles.append({"name": name, "image": img, "w": img.width, "h": img.height})
    return tiles


def render_frame(tiles, variant_cfg, row_offsets, frame_num):
    """Render a single animation frame, returns PIL Image."""
    canvas = Image.new("RGB", (WIDTH, HEIGHT), BG_COLOR)

    tile_idx = variant_cfg["tile_index"]
    if tile_idx >= len(tiles):
        tile_idx = len(tiles) - 1
    tile = tiles[tile_idx]
    tile_img = tile["image"]
    tw, th = tile["w"], tile["h"]

    gap = TILE_GAP + variant_cfg["extra_gap"]
    pitch = tw + gap
    row_pitch = th + gap

    if pitch == 0 or row_pitch == 0:
        return canvas

    num_rows = (HEIGHT + row_pitch - 1) // row_pitch

    # Update row offsets
    for row in range(min(num_rows, len(row_offsets))):
        direction = 1 if row % 2 == 0 else -1
        speed = variant_cfg["speed"]
        if row % 2 != 0:
            speed += variant_cfg["odd_row_speed_delta"]
        row_offsets[row] += direction * speed
        row_offsets[row] %= pitch

    # Render tiles row by row with seamless wrapping
    for row in range(num_rows):
        row_y = row * row_pitch
        if row_y >= HEIGHT:
            break

        base_offset = row_offsets[row] if row < len(row_offsets) else 0

        stagger = (pitch // 2) if (variant_cfg["brick_stagger"] and row % 2 != 0) else 0
        phase = variant_cfg["row_phase_step"] * row

        total_offset = base_offset + stagger + phase

        # Normalize offset to [0, pitch) for seamless wrapping
        norm = total_offset % pitch

        # Start one tile before the left edge, iterate rightward
        tile_x = norm - pitch
        while tile_x < WIDTH:
            if tile_x + tw > 0 and row_y + th > 0 and row_y < HEIGHT:
                canvas.paste(tile_img, (tile_x, row_y), tile_img)
            tile_x += pitch

    return canvas


def add_label(img, text, position="bottom-right"):
    """Add a small text label to the image."""
    draw = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 14)
    except (IOError, OSError):
        font = ImageFont.load_default()

    bbox = draw.textbbox((0, 0), text, font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]

    if position == "bottom-right":
        x, y = WIDTH - tw - 10, HEIGHT - th - 10
    elif position == "bottom-left":
        x, y = 10, HEIGHT - th - 10
    else:
        x, y = 10, 10

    # Semi-transparent background
    draw.rectangle([x - 4, y - 2, x + tw + 4, y + th + 2], fill=(0, 0, 0))
    draw.text((x, y), text, fill=(128, 128, 128), font=font)
    return img


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    tiles = load_tiles()

    print(f"Rendering boot splash preview ({WIDTH}x{HEIGHT})")
    print(f"Output: {OUT_DIR}")
    print()

    all_frames = []
    composite_frames = []

    for vi, vcfg in enumerate(VARIANTS):
        print(f"Variant {vi}: {vcfg['name']}")

        row_offsets = [0] * 30

        # Render 90 frames (3 seconds at 30 FPS)
        variant_frames = []
        for f in range(VARIANT_DURATION_FRAMES):
            frame = render_frame(tiles, vcfg, row_offsets, f)
            variant_frames.append(frame)

        # Save key frames
        for key_frame in [0, 30, 60, 89]:
            out_path = os.path.join(OUT_DIR, f"variant_{vi}_frame_{key_frame:03d}.png")
            img = variant_frames[key_frame].copy()
            add_label(img, f"Variant {chr(65 + vi)} | Frame {key_frame}")
            img.save(out_path)

        # Use frame 45 (mid-animation) as the composite representative
        composite_frames.append(variant_frames[45].copy())

        # Sample every 3rd frame for GIF (reduce size)
        for i in range(0, len(variant_frames), 3):
            all_frames.append(variant_frames[i].copy())

        print(f"  Saved {len(variant_frames)} frames, key frames exported")

    # Create animated GIF
    gif_path = os.path.join(OUT_DIR, "boot-splash-preview.gif")
    if all_frames:
        all_frames[0].save(
            gif_path,
            save_all=True,
            append_images=all_frames[1:],
            duration=100,  # 100ms per sampled frame
            loop=0,
        )
        print(f"\nAnimated GIF: {gif_path} ({len(all_frames)} frames)")

    # Create composite screenshot (all 3 variants side by side)
    margin = 10
    comp_w = WIDTH * 3 + margin * 4
    comp_h = HEIGHT + margin * 2 + 30
    composite = Image.new("RGB", (comp_w, comp_h), (20, 20, 20))
    draw = ImageDraw.Draw(composite)

    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf", 18)
    except (IOError, OSError):
        font = ImageFont.load_default()

    for i, (frame, vcfg) in enumerate(zip(composite_frames, VARIANTS)):
        x = margin + i * (WIDTH + margin)
        y = 30
        composite.paste(frame, (x, y))
        label = f"Variant {chr(65 + i)}: {vcfg['name'].split('—')[1].strip() if '—' in vcfg['name'] else vcfg['name']}"
        draw.text((x, 6), label, fill=(200, 200, 200), font=font)

    comp_path = os.path.join(OUT_DIR, "boot-splash-composite.png")
    composite.save(comp_path)
    print(f"Composite: {comp_path} ({comp_w}x{comp_h})")

    # Create a single screenshot for README (just variant A, mid-animation)
    readme_path = os.path.join(OUT_DIR, "boot-splash-screenshot.png")
    screenshot = composite_frames[0].copy()
    add_label(screenshot, "Micronuts Boot Splash — Variant A", "top-left")
    screenshot.save(readme_path)
    print(f"README screenshot: {readme_path}")

    print("\nDone! Preview assets generated.")


if __name__ == "__main__":
    main()
