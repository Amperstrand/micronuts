//! Retro boot splash animation system for Micronuts.
//!
//! Displays a tiled grid of the official Cashu nut logo with alternating
//! row-scrolling directions, cycling through multiple visual variants.
//!
//! # Architecture
//!
//! The splash treats the display as horizontal rows of repeated logo tiles.
//! Each row has:
//! - a chosen tile asset (from `boot_splash_assets`)
//! - its own horizontal pixel offset (integer only)
//! - a scroll direction (left or right, alternating by row index)
//! - an optional brick-style half-tile stagger
//!
//! Seamless wraparound is achieved by rendering enough tiles to cover the
//! display width plus one extra tile, then using modulo arithmetic on the
//! row offset.
//!
//! # Variants
//!
//! Three built-in variants cycle every 3 seconds:
//!
//! - **Variant A (Dense Drift)**: tiny tiles, tight spacing, fast opposing drift
//! - **Variant B (Brick Scroll)**: medium tiles, brick-layout stagger, mixed speeds
//! - **Variant C (Big Wave)**: large tiles, slow motion with per-row phase offset
//!
//! # Touch to exit
//!
//! Any touch event exits the splash immediately. Horizontal swipes optionally
//! switch variants manually.
//!
//! # Display-size strategy
//!
//! The module is parameterized by `(width, height)` so it can be adapted to
//! other display sizes. Tile assets are selected from a pre-generated catalog.

use crate::boot_splash_assets::{TileAsset, TILE_CATALOG};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How long each variant is shown (in frames at ~30 FPS).
const VARIANT_DURATION_FRAMES: u32 = 90; // 3 seconds × 30 FPS

/// Number of animation variants.
const NUM_VARIANTS: usize = 3;

/// Background color (RGB565 black).
const BG_COLOR: u16 = 0x0000;

/// Spacing between tiles (pixels) — kept small for dense feel.
const TILE_GAP: u16 = 2;

// ---------------------------------------------------------------------------
// Variant configuration
// ---------------------------------------------------------------------------

/// Parameters that define one visual variant.
#[derive(Clone, Copy)]
struct VariantConfig {
    /// Index into TILE_CATALOG for the tile to use.
    tile_index: usize,
    /// Base scroll speed in pixels per frame (positive = rightward for even rows).
    speed: i16,
    /// Whether to stagger odd rows by half a tile pitch (brick layout).
    brick_stagger: bool,
    /// Extra speed delta applied to odd rows (makes adjacent rows differ).
    odd_row_speed_delta: i16,
    /// Per-row phase offset in pixels (wave effect).
    row_phase_step: i16,
    /// Extra gap between tiles (added to TILE_GAP).
    extra_gap: u16,
}

/// The three built-in variants.
const VARIANTS: [VariantConfig; NUM_VARIANTS] = [
    // Variant A — Dense Drift
    // Tiny tiles, tight spacing, fast constant opposing row motion
    VariantConfig {
        tile_index: 0, // tiny
        speed: 3,
        brick_stagger: false,
        odd_row_speed_delta: 0,
        row_phase_step: 0,
        extra_gap: 0,
    },
    // Variant B — Brick Scroll
    // Medium tiles, brick-layout stagger, opposing directions, slightly
    // different speeds between adjacent rows
    VariantConfig {
        tile_index: 2, // medium
        speed: 2,
        brick_stagger: true,
        odd_row_speed_delta: 1,
        row_phase_step: 0,
        extra_gap: 4,
    },
    // Variant C — Big Wave
    // Large tiles, slow motion, per-row phase offset for wave feel
    VariantConfig {
        tile_index: 3, // large
        speed: 1,
        brick_stagger: false,
        odd_row_speed_delta: 0,
        row_phase_step: 8,
        extra_gap: 6,
    },
];

// ---------------------------------------------------------------------------
// Splash state
// ---------------------------------------------------------------------------

/// Mutable state for the running splash animation.
pub struct SplashState {
    /// Current variant index (0..NUM_VARIANTS).
    variant: usize,
    /// Frame counter within current variant.
    frame: u32,
    /// Global frame counter (monotonic, for phase calculations).
    pub global_frame: u32,
    /// Per-row scroll accumulators (enough for up to 30 rows).
    row_offsets: [i32; 30],
}

impl SplashState {
    pub fn new() -> Self {
        Self {
            variant: 0,
            frame: 0,
            global_frame: 0,
            row_offsets: [0i32; 30],
        }
    }

    /// Advance to the next variant (wraps around).
    pub fn next_variant(&mut self) {
        self.variant = (self.variant + 1) % NUM_VARIANTS;
        self.frame = 0;
        // Reset row offsets for clean transition
        self.row_offsets = [0i32; 30];
    }

    /// Advance to the previous variant (wraps around).
    pub fn prev_variant(&mut self) {
        self.variant = if self.variant == 0 {
            NUM_VARIANTS - 1
        } else {
            self.variant - 1
        };
        self.frame = 0;
        self.row_offsets = [0i32; 30];
    }
}

// ---------------------------------------------------------------------------
// Core rendering
// ---------------------------------------------------------------------------

/// Render one frame of the boot splash directly into a raw u16 framebuffer.
///
/// `fb` must be a slice of at least `width * height` u16 values in row-major
/// order (matches the LTDC framebuffer layout).
///
/// Returns `true` if the variant just cycled (so caller can detect transitions).
pub fn render_frame(
    fb: &mut [u16],
    width: u32,
    height: u32,
    state: &mut SplashState,
) -> bool {
    let cfg = &VARIANTS[state.variant];
    let cat_len = TILE_CATALOG.len();
    let tile: &TileAsset = if cfg.tile_index < cat_len {
        &TILE_CATALOG[cfg.tile_index]
    } else {
        &TILE_CATALOG[cat_len - 1]
    };

    let tw = tile.width as u32;
    let th = tile.height as u32;
    let gap = (TILE_GAP + cfg.extra_gap) as u32;
    let pitch = tw + gap; // horizontal distance between tile origins
    let row_pitch = th + gap; // vertical distance between tile origins

    if pitch == 0 || row_pitch == 0 {
        return false;
    }

    let num_rows = ((height + row_pitch - 1) / row_pitch) as usize;
    let tiles_per_row = ((width + pitch - 1) / pitch) + 2; // +2 for seamless wrap

    // --- Update row offsets ---
    for row in 0..num_rows.min(state.row_offsets.len()) {
        let dir: i32 = if row % 2 == 0 { 1 } else { -1 };
        let speed = cfg.speed as i32
            + if row % 2 != 0 {
                cfg.odd_row_speed_delta as i32
            } else {
                0
            };
        state.row_offsets[row] += dir * speed;

        // Keep offset bounded to avoid overflow over long runs
        let wrap = pitch as i32;
        if wrap > 0 {
            state.row_offsets[row] = state.row_offsets[row].rem_euclid(wrap);
        }
    }

    // --- Clear framebuffer ---
    for px in fb[..(width * height) as usize].iter_mut() {
        *px = BG_COLOR;
    }

    // --- Render tiled rows ---
    for row in 0..num_rows {
        let row_y = (row as u32) * row_pitch;
        if row_y >= height {
            break;
        }

        let base_offset = if row < state.row_offsets.len() {
            state.row_offsets[row]
        } else {
            0
        };

        // Brick stagger: shift odd rows by half a pitch
        let stagger = if cfg.brick_stagger && row % 2 != 0 {
            pitch as i32 / 2
        } else {
            0
        };

        // Phase offset (wave effect)
        let phase = cfg.row_phase_step as i32 * row as i32;

        let total_offset = base_offset + stagger + phase;

        // Render tiles for this row
        for col_idx in 0..tiles_per_row {
            let tile_x_base = (col_idx as i32) * (pitch as i32) - total_offset;

            // Wrap into visible range
            let wrap = pitch as i32;
            let tile_x = if wrap > 0 {
                let mut x = tile_x_base % wrap;
                if x < -(tw as i32) {
                    x += wrap * (((-(x + tw as i32)) / wrap) + 1);
                }
                // Shift to cover the full width
                x - pitch as i32
            } else {
                tile_x_base
            };

            // Blit tile pixels
            blit_tile(fb, width, height, tile, tile_x, row_y as i32);
        }
    }

    // --- Render variant indicator (tiny overlay in bottom-right corner) ---
    render_variant_indicator(fb, width, height, state.variant);

    // --- Advance frame ---
    state.frame += 1;
    state.global_frame += 1;

    let cycled = state.frame >= VARIANT_DURATION_FRAMES;
    if cycled {
        state.next_variant();
    }

    cycled
}

/// Blit a single tile onto the framebuffer at integer position (tx, ty).
/// Clips to framebuffer bounds. No scaling.
#[inline]
fn blit_tile(
    fb: &mut [u16],
    fb_w: u32,
    fb_h: u32,
    tile: &TileAsset,
    tx: i32,
    ty: i32,
) {
    let tw = tile.width as i32;
    let th = tile.height as i32;

    // Clip source rectangle
    let src_x0 = if tx < 0 { -tx } else { 0 };
    let src_y0 = if ty < 0 { -ty } else { 0 };
    let dst_x0 = (tx + src_x0).max(0) as u32;
    let dst_y0 = (ty + src_y0).max(0) as u32;
    let copy_w = (tw - src_x0).min(fb_w as i32 - dst_x0 as i32);
    let copy_h = (th - src_y0).min(fb_h as i32 - dst_y0 as i32);

    if copy_w <= 0 || copy_h <= 0 {
        return;
    }

    let copy_w = copy_w as u32;
    let copy_h = copy_h as u32;
    let src_x0 = src_x0 as u32;
    let src_y0 = src_y0 as u32;

    for row in 0..copy_h {
        let src_row_start = ((src_y0 + row) * tile.width as u32 + src_x0) as usize;
        let dst_row_start = ((dst_y0 + row) * fb_w + dst_x0) as usize;
        let len = copy_w as usize;

        // Direct memcpy-style copy for the hot path
        fb[dst_row_start..dst_row_start + len]
            .copy_from_slice(&tile.data[src_row_start..src_row_start + len]);
    }
}

/// Render a tiny variant indicator ("A", "B", "C") in the bottom-right corner.
/// Uses a minimal 5x7 pixel font baked in as bitmaps.
fn render_variant_indicator(fb: &mut [u16], width: u32, height: u32, variant: usize) {
    // 5x7 bitmap font for 'A', 'B', 'C' — each is 5 columns × 7 rows
    const FONT_A: [u8; 7] = [
        0b01110,
        0b10001,
        0b10001,
        0b11111,
        0b10001,
        0b10001,
        0b10001,
    ];
    const FONT_B: [u8; 7] = [
        0b11110,
        0b10001,
        0b10001,
        0b11110,
        0b10001,
        0b10001,
        0b11110,
    ];
    const FONT_C: [u8; 7] = [
        0b01110,
        0b10001,
        0b10000,
        0b10000,
        0b10000,
        0b10001,
        0b01110,
    ];

    let glyph: &[u8; 7] = match variant {
        0 => &FONT_A,
        1 => &FONT_B,
        _ => &FONT_C,
    };

    let scale = 2u32; // 2x scale for visibility
    let gw = 5 * scale;
    let gh = 7 * scale;
    let margin = 8u32;
    let ox = width.saturating_sub(gw + margin);
    let oy = height.saturating_sub(gh + margin);

    // Dimmed white for subtlety
    let color: u16 = 0x8410; // RGB565 ~mid-gray

    for row in 0..7u32 {
        let bits = glyph[row as usize];
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) != 0 {
                // Draw a scale×scale block
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = ox + col * scale + dx;
                        let py = oy + row * scale + dy;
                        if px < width && py < height {
                            fb[(py * width + px) as usize] = color;
                        }
                    }
                }
            }
        }
    }
}

/// Check if a touch event represents a horizontal swipe.
/// Returns Some(true) for swipe-right, Some(false) for swipe-left, None for tap.
pub fn classify_touch(x1: i32, _y1: i32, x2: i32, _y2: i32) -> Option<bool> {
    let dx = x2 - x1;
    if dx.abs() > 50 {
        Some(dx > 0)
    } else {
        None
    }
}
