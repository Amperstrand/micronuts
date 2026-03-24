# Boot Splash Animation

Retro-style boot splash screen for Micronuts, featuring the official [Cashu nut logo](https://cashu.space/) in a tiled grid with alternating row-scrolling animation.

## Overview

At boot, the display fills with a repeating grid of Cashu nut logos. Every other row scrolls in the opposite direction, creating a retro parallax / raster-style visual. Three distinct variants cycle every 3 seconds. Touch anywhere to exit the splash and continue into the normal firmware UI.

## Logo Source

The official Cashu nut logo is vendored from the [cashubtc/docs.cashu.space](https://github.com/cashubtc/docs.cashu.space) repository:

- **Source file**: `public/homepage/nut-logo.png`
- **Vendored copy**: `firmware/assets/nut-logo-original.png` (707×809 RGBA)
- **License**: Used with attribution per the Cashu project

The logo is **not modified** in any way — no recoloring, no anti-aliasing, no redesign. All resizing is done offline using **nearest-neighbor** interpolation to preserve pixel crispness.

## Asset Pipeline

### Generated tile sizes

From the original 707×809 logo, four tile masters are generated at different sizes:

| Name   | Size    | Use case |
|--------|---------|----------|
| tiny   | 21×24   | Dense grid, Variant A |
| small  | 35×40   | Available for custom variants |
| medium | 56×64   | Brick layout, Variant B |
| large  | 84×96   | Large logos, Variant C |

### How assets are generated

1. **`scripts/generate_assets.py`** reads `firmware/assets/nut-logo-original.png`
2. Resizes to each target height using **nearest-neighbor only** (no smoothing)
3. Composites the transparent logo onto black background
4. Converts to **RGB565** format (16-bit, little-endian)
5. Writes:
   - PNG masters → `firmware/assets/generated/nut-{size}.png`
   - RGB565 binary → `firmware/assets/generated/nut-{size}.rgb565`
   - Rust source → `firmware/src/boot_splash_assets.rs`

### Regenerating assets

```bash
pip install Pillow
python3 scripts/generate_assets.py
```

The generated `boot_splash_assets.rs` is checked into the repo so that **normal firmware builds do not require Python or internet access**.

## Variants

### Variant A — Dense Drift
- **Tile**: tiny (21×24)
- **Speed**: 3 px/frame
- **Layout**: Regular grid, tight spacing
- **Feel**: Fast retro wallpaper, dense texture

### Variant B — Brick Scroll
- **Tile**: medium (56×64)
- **Speed**: 2 px/frame (odd rows +1 px/frame extra)
- **Layout**: Brick stagger (odd rows offset by half a tile)
- **Feel**: Arcade title-card, brick-wall pattern

### Variant C — Big Wave
- **Tile**: large (84×96)
- **Speed**: 1 px/frame
- **Layout**: Regular grid, 8px per-row phase offset
- **Feel**: Slow, gentle wave motion, more breathing room

All variants share:
- Alternating row directions (even→right, odd→left)
- Seamless wraparound scrolling
- Black background, pixel-perfect 1:1 rendering
- ~30 FPS frame pacing

## Configuration

### Changing variant parameters

Edit the `VARIANTS` array in `firmware/src/boot_splash.rs`:

```rust
VariantConfig {
    tile_index: 0,        // Index into TILE_CATALOG (0=tiny, 1=small, 2=medium, 3=large)
    speed: 3,             // Pixels per frame (base speed for even rows)
    brick_stagger: false,  // Half-tile offset on odd rows
    odd_row_speed_delta: 0, // Extra speed for odd rows
    row_phase_step: 0,     // Per-row phase offset in pixels (wave effect)
    extra_gap: 0,          // Additional gap between tiles (added to base 2px)
}
```

### Changing timing

- `VARIANT_DURATION_FRAMES`: Frames per variant (default: 90 = 3 seconds at 30 FPS)
- `TILE_GAP`: Base gap between tiles (default: 2 pixels)
- Frame pacing: `delay.delay_ms(33u32)` in `main.rs` (~30 FPS)

### Adding a new tile size

1. Edit `TILE_CATALOG` in `scripts/generate_assets.py`
2. Re-run `python3 scripts/generate_assets.py`
3. Reference the new tile by index in your variant config

### Adding a new variant

1. Increase `NUM_VARIANTS` in `boot_splash.rs`
2. Add a new `VariantConfig` to the `VARIANTS` array
3. Add the corresponding font glyph in `render_variant_indicator`

## Touch Interaction

- **Tap anywhere**: Exit splash immediately, continue boot
- **Touch failure**: If the FT6X06 controller doesn't initialize, the splash still runs with timeout-only exit (no brick)
- **Auto-exit**: After 2 full variant cycles (~18 seconds), the splash exits automatically

## Architecture

```
firmware/src/
├── boot_splash.rs        # Animation engine (render_frame, blit_tile, state management)
├── boot_splash_assets.rs # Generated RGB565 tile data (const arrays)
├── main.rs               # Integration point (splash loop before main loop)
└── lib.rs                # Module declarations

firmware/assets/
├── nut-logo-original.png     # Vendored official Cashu nut logo
├── generated/
│   ├── nut-tiny.png          # 21×24 tile master
│   ├── nut-small.png         # 35×40 tile master
│   ├── nut-medium.png        # 56×64 tile master
│   ├── nut-large.png         # 84×96 tile master
│   └── *.rgb565              # Binary RGB565 data (intermediate)
└── preview/
    ├── boot-splash-preview.gif       # Animated preview
    ├── boot-splash-composite.png     # All variants side by side
    ├── boot-splash-screenshot.png    # Single screenshot for README
    └── variant_*_frame_*.png         # Key frames per variant

scripts/
├── generate_assets.py    # Offline tile master generation
└── render_preview.py     # Host-side preview renderer
```

## Display Adaptation

The splash is parameterized by `(width, height)` passed to `render_frame()`. To adapt for a different display:

1. Generate tile sizes appropriate for the new resolution
2. Adjust `tile_index` in each variant's config
3. The rendering loop automatically adapts to fill any display size

## Preview

To generate preview images locally:

```bash
pip install Pillow
python3 scripts/generate_assets.py  # if not already done
python3 scripts/render_preview.py
```

Output goes to `firmware/assets/preview/`. The CI workflow also generates these as artifacts on every push that touches splash-related files.
