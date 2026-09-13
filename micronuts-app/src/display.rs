extern crate alloc;

use cashu_core_lite::token::TokenV4;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{
        rounded_rectangle::CornerRadii, Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder,
        Rectangle, RoundedRectangle,
    },
    text::{Alignment, Text, TextStyleBuilder},
};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};

use crate::qr::QrPayload;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 800;

const BLACK: Rgb888 = Rgb888::BLACK;
const WHITE: Rgb888 = Rgb888::WHITE;

const BG: Rgb888 = Rgb888::new(0x0B, 0x0F, 0x17);
const SURFACE: Rgb888 = Rgb888::new(0x18, 0x20, 0x2E);
const SURFACE_HI: Rgb888 = Rgb888::new(0x23, 0x2E, 0x40);
const STROKE: Rgb888 = Rgb888::new(0x33, 0x41, 0x58);
const TEXT: Rgb888 = Rgb888::new(0xED, 0xF2, 0xF7);
const TEXT_DIM: Rgb888 = Rgb888::new(0x8A, 0x99, 0xAE);
const ACCENT: Rgb888 = Rgb888::new(0x4C, 0xAF, 0xF0);
const ACCENT_DEEP: Rgb888 = Rgb888::new(0x11, 0x3B, 0x5E);
const GREEN: Rgb888 = Rgb888::new(0x3E, 0xD5, 0x8A);
const GREEN_DEEP: Rgb888 = Rgb888::new(0x0E, 0x3A, 0x2C);
const AMBER: Rgb888 = Rgb888::new(0xF5, 0xBF, 0x45);
const RED: Rgb888 = Rgb888::new(0xF0, 0x5E, 0x5E);
const RED_DEEP: Rgb888 = Rgb888::new(0x46, 0x18, 0x1C);

const HEADER_H: u32 = 48;
const CHAR_W: i32 = 10;
const QR_BUF_SIZE: usize = Version::MAX.buffer_len();

#[derive(Clone, Copy)]
pub struct Button {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub label: &'static str,
}

impl Button {
    pub fn hit(&self, tx: u16, ty: u16) -> bool {
        let tx = tx as u32;
        let ty = ty as u32;
        tx >= self.x && tx < self.x + self.w && ty >= self.y && ty < self.y + self.h
    }
}

pub fn home_buttons() -> [Button; 3] {
    [
        Button {
            x: 40,
            y: 100,
            w: WIDTH - 80,
            h: 120,
            label: "SCAN QR CODE",
        },
        Button {
            x: 40,
            y: 300,
            w: WIDTH - 80,
            h: 120,
            label: "IMPORT TOKEN",
        },
        Button {
            x: 40,
            y: 500,
            w: WIDTH - 80,
            h: 120,
            label: "SHOW PROOFS",
        },
    ]
}

pub fn back_button() -> Button {
    Button {
        x: 10,
        y: 8,
        w: 120,
        h: 36,
        label: "< BACK",
    }
}

pub fn aim_button() -> Button {
    Button {
        x: WIDTH - 170,
        y: 8,
        w: 160,
        h: 36,
        label: "LASER: OFF",
    }
}

// ---------------------------------------------------------------------------
// Design-system primitives
// ---------------------------------------------------------------------------

fn rect_style(fill: Option<Rgb888>, stroke: Option<(Rgb888, u32)>) -> PrimitiveStyle<Rgb888> {
    let mut b = PrimitiveStyleBuilder::new();
    if let Some(f) = fill {
        b = b.fill_color(f);
    }
    if let Some((c, w)) = stroke {
        b = b.stroke_color(c).stroke_width(w);
    }
    b.build()
}

fn card<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    fill: Rgb888,
    stroke: Rgb888,
) {
    RoundedRectangle::new(
        Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h)),
        CornerRadii::new(Size::new(12, 12)),
    )
    .into_styled(rect_style(Some(fill), Some((stroke, 1))))
    .draw(fb)
    .ok();
}

fn solid<D: DrawTarget<Color = Rgb888>>(fb: &mut D, x: u32, y: u32, w: u32, h: u32, color: Rgb888) {
    let _ = fb.fill_solid(
        &Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h)),
        color,
    );
}

fn dot<D: DrawTarget<Color = Rgb888>>(fb: &mut D, cx: i32, cy: i32, d: u32, color: Rgb888) {
    Circle::new(Point::new(cx - (d as i32) / 2, cy - (d as i32) / 2), d)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(fb)
        .ok();
}

fn text_centered<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    s: &str,
    cx: i32,
    y: i32,
    style: MonoTextStyle<'static, Rgb888>,
) {
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(s, Point::new(cx, y), style, center)
        .draw(fb)
        .ok();
}

fn text_left<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    s: &str,
    x: i32,
    y: i32,
    style: MonoTextStyle<'static, Rgb888>,
) {
    Text::new(s, Point::new(x, y), style).draw(fb).ok();
}

fn pill_width(text: &str, with_dot: bool) -> u32 {
    2 * 12 + text.len() as u32 * CHAR_W as u32 + if with_dot { 16 } else { 0 }
}

fn draw_pill<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    x: u32,
    y: u32,
    text: &str,
    fg: Rgb888,
    bg: Rgb888,
    dot_color: Option<Rgb888>,
) {
    let w = pill_width(text, dot_color.is_some());
    let h = 30u32;
    RoundedRectangle::new(
        Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h)),
        CornerRadii::new(Size::new(h / 2, h / 2)),
    )
    .into_styled(rect_style(Some(bg), Some((STROKE, 1))))
    .draw(fb)
    .ok();
    let mut tx = x + 12;
    if let Some(c) = dot_color {
        dot(fb, (tx + 4) as i32, (y + h / 2) as i32, 8, c);
        tx += 16;
    }
    text_left(
        fb,
        text,
        tx as i32,
        (y + h / 2 - 10 + 6) as i32,
        MonoTextStyle::new(&FONT_10X20, fg),
    );
}

fn draw_header<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    title: &str,
    right: Option<(&str, Rgb888)>,
    brand: bool,
) {
    solid(fb, 0, 0, WIDTH, HEADER_H, SURFACE);
    solid(fb, 0, HEADER_H - 1, WIDTH, 1, STROKE);
    if brand {
        dot(fb, 24, (HEADER_H / 2) as i32, 14, ACCENT);
        dot(fb, 24, (HEADER_H / 2) as i32, 6, BG);
        text_left(
            fb,
            title,
            44,
            (HEADER_H / 2 - 10 + 6) as i32,
            MonoTextStyle::new(&FONT_10X20, TEXT),
        );
    }
    if let Some((label, color)) = right {
        let w = pill_width(label, true);
        draw_pill(fb, WIDTH - w - 12, 9, label, color, SURFACE_HI, Some(color));
    }
}

/// Ghost pill for secondary actions (back / laser toggle).
fn draw_ghost_button<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    btn: &Button,
    fg: Rgb888,
    bg: Rgb888,
) {
    RoundedRectangle::new(
        Rectangle::new(
            Point::new(btn.x as i32, btn.y as i32),
            Size::new(btn.w, btn.h),
        ),
        CornerRadii::new(Size::new(btn.h / 2, btn.h / 2)),
    )
    .into_styled(rect_style(Some(bg), None))
    .draw(fb)
    .ok();
    text_centered(
        fb,
        btn.label,
        (btn.x + btn.w / 2) as i32,
        (btn.y + btn.h / 2 - 10 + 6) as i32,
        MonoTextStyle::new(&FONT_10X20, fg),
    );
}

// Icons -------------------------------------------------------------------

fn icon_scan<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    x: u32,
    y: u32,
    size: u32,
    t: u32,
    c: Rgb888,
) {
    let s = size as i32;
    let (px, py) = (x as i32, y as i32);
    let arm = s / 2;
    let stroke = PrimitiveStyle::with_stroke(c, t);
    let corner = |fb: &mut D, ax: i32, ay: i32, dx: i32, dy: i32| {
        Line::new(Point::new(ax, ay), Point::new(ax + dx * arm, ay))
            .into_styled(stroke)
            .draw(fb)
            .ok();
        Line::new(Point::new(ax, ay), Point::new(ax, ay + dy * arm))
            .into_styled(stroke)
            .draw(fb)
            .ok();
    };
    corner(fb, px, py, 1, 1);
    corner(fb, px + s, py, -1, 1);
    corner(fb, px, py + s, 1, -1);
    corner(fb, px + s, py + s, -1, -1);
}

fn icon_download<D: DrawTarget<Color = Rgb888>>(fb: &mut D, x: u32, y: u32, size: u32, c: Rgb888) {
    let s = size as i32;
    let (px, py) = (x as i32, y as i32);
    let shaft_w = 6i32;
    let cx = px + s / 2;
    solid(
        fb,
        (cx - shaft_w / 2) as u32,
        py as u32,
        shaft_w as u32,
        (s * 7 / 12) as u32,
        c,
    );
    Line::new(
        Point::new(cx - s / 3, py + s * 7 / 12 - s / 12),
        Point::new(cx, py + s * 7 / 12 + s / 9),
    )
    .into_styled(PrimitiveStyle::with_stroke(c, 5))
    .draw(fb)
    .ok();
    Line::new(
        Point::new(cx + s / 3, py + s * 7 / 12 - s / 12),
        Point::new(cx, py + s * 7 / 12 + s / 9),
    )
    .into_styled(PrimitiveStyle::with_stroke(c, 5))
    .draw(fb)
    .ok();
    solid(fb, px as u32, (py + s - 5) as u32, s as u32, 5, c);
}

fn icon_coins<D: DrawTarget<Color = Rgb888>>(fb: &mut D, x: u32, y: u32, size: u32, c: Rgb888) {
    let h = size / 4;
    let gap = size / 8;
    let coin = |fb: &mut D, cy: u32, inset: u32| {
        RoundedRectangle::new(
            Rectangle::new(
                Point::new((x + inset) as i32, cy as i32),
                Size::new(size - 2 * inset, h),
            ),
            CornerRadii::new(Size::new(h / 2, h / 2)),
        )
        .into_styled(PrimitiveStyle::with_fill(c))
        .draw(fb)
        .ok();
    };
    coin(fb, y + size - h, 0);
    coin(fb, y + size - 2 * h - gap, size / 8);
    coin(fb, y + size - 3 * h - 2 * gap, size / 4);
}

fn icon_alert<D: DrawTarget<Color = Rgb888>>(fb: &mut D, cx: i32, cy: i32, d: u32, c: Rgb888) {
    Circle::new(Point::new(cx - d as i32 / 2, cy - d as i32 / 2), d)
        .into_styled(PrimitiveStyle::with_stroke(c, 4))
        .draw(fb)
        .ok();
    solid(fb, (cx - 2) as u32, (cy - d as i32 / 3) as u32, 4, d / 2, c);
    dot(fb, cx, cy + d as i32 / 4, 6, c);
}

fn icon_info<D: DrawTarget<Color = Rgb888>>(fb: &mut D, cx: i32, cy: i32, d: u32, c: Rgb888) {
    Circle::new(Point::new(cx - d as i32 / 2, cy - d as i32 / 2), d)
        .into_styled(PrimitiveStyle::with_stroke(c, 4))
        .draw(fb)
        .ok();
    dot(fb, cx, cy - d as i32 / 4, 5, c);
    solid(fb, (cx - 2) as u32, (cy - d as i32 / 8) as u32, 4, d / 3, c);
}

// Hero text: built-in 5x7 pixel font, scaled --------------------------------

const HERO_0: [u8; 7] = [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E];
const HERO_DIGITS: [[u8; 7]; 10] = [
    HERO_0,
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
    [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
];

fn hero_glyph(c: char) -> Option<[u8; 7]> {
    match c {
        '0'..='9' => Some(HERO_DIGITS[c as usize - '0' as usize]),
        ' ' => Some([0; 7]),
        '.' => Some([0, 0, 0, 0, 0, 0x0C, 0x0C]),
        ',' => Some([0, 0, 0, 0, 0x0C, 0x04, 0x08]),
        ':' => Some([0, 0x0C, 0x0C, 0, 0x0C, 0x0C, 0]),
        '-' => Some([0, 0, 0, 0x1F, 0, 0, 0]),
        '!' => Some([0x04, 0x04, 0x04, 0x04, 0x04, 0, 0x04]),
        '?' => Some([0x0E, 0x11, 0x01, 0x02, 0x04, 0, 0x04]),
        'S' => Some([0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E]),
        'E' => Some([0x1F, 0x10, 0x10, 0x1F, 0x10, 0x10, 0x1F]),
        'A' => Some([0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
        'T' => Some([0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
        'M' => Some([0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
        'U' => Some([0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
        'N' => Some([0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11]),
        'R' => Some([0x0E, 0x11, 0x11, 0x1E, 0x15, 0x11, 0x11]),
        'O' => Some([0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
        'K' => Some([0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]),
        'B' => Some([0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]),
        '/' => Some([0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10]),
        _ => None,
    }
}

fn hero_width(text: &str, scale: u32) -> u32 {
    let chars = text.chars().filter(|c| hero_glyph(*c).is_some()).count();
    if chars == 0 {
        return 0;
    }
    chars as u32 * 6 * scale - scale
}

fn draw_hero<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    text: &str,
    x: i32,
    y: i32,
    scale: u32,
    color: Rgb888,
) {
    let mut cx = x;
    for c in text.chars() {
        if let Some(rows) = hero_glyph(c) {
            for (ry, row) in rows.iter().enumerate() {
                for rx in 0..5u32 {
                    if row & (1 << (4 - rx)) != 0 {
                        solid(
                            fb,
                            (cx + (rx * scale) as i32) as u32,
                            (y + (ry as u32 * scale) as i32) as u32,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
        }
        cx += (6 * scale) as i32;
    }
}

fn draw_hero_centered<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    text: &str,
    cx: i32,
    y: i32,
    scale: u32,
    color: Rgb888,
) {
    let w = hero_width(text, scale) as i32;
    draw_hero(fb, text, cx - w / 2, y, scale, color);
}

// ---------------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum MenuIcon {
    Scan,
    Download,
    Coins,
}

impl MenuIcon {
    fn draw<D: DrawTarget<Color = Rgb888>>(self, fb: &mut D, x: u32, y: u32, size: u32, c: Rgb888) {
        match self {
            MenuIcon::Scan => icon_scan(fb, x + 2, y + 2, size - 4, 3, c),
            MenuIcon::Download => icon_download(fb, x, y, size, c),
            MenuIcon::Coins => icon_coins(fb, x, y, size, c),
        }
    }
}

fn draw_menu_card<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    btn: &Button,
    icon: MenuIcon,
    label: &str,
    sub: &str,
    primary: bool,
) {
    let (fill, stroke) = if primary {
        (ACCENT_DEEP, ACCENT)
    } else {
        (SURFACE, STROKE)
    };
    card(fb, btn.x, btn.y, btn.w, btn.h, fill, stroke);
    if primary {
        solid(fb, btn.x + 1, btn.y + 14, 3, btn.h - 28, ACCENT);
    }
    let icon_box = 56;
    let icon_pad = 10;
    let ib_x = btn.x + 20;
    let ib_y = btn.y + (btn.h - icon_box) / 2;
    RoundedRectangle::new(
        Rectangle::new(
            Point::new(ib_x as i32, ib_y as i32),
            Size::new(icon_box, icon_box),
        ),
        CornerRadii::new(Size::new(12, 12)),
    )
    .into_styled(rect_style(
        Some(if primary { ACCENT } else { SURFACE_HI }),
        None,
    ))
    .draw(fb)
    .ok();
    icon.draw(
        fb,
        ib_x + icon_pad,
        ib_y + icon_pad,
        icon_box - 2 * icon_pad,
        TEXT,
    );
    let label_x = (ib_x + icon_box + 18) as i32;
    text_left(
        fb,
        label,
        label_x,
        (btn.y + btn.h / 2 - 14) as i32,
        MonoTextStyle::new(&FONT_10X20, TEXT),
    );
    text_left(
        fb,
        sub,
        label_x,
        (btn.y + btn.h / 2 + 8) as i32,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );
}

pub fn draw_button<D: DrawTarget<Color = Rgb888>>(fb: &mut D, btn: &Button) {
    draw_ghost_button(fb, btn, TEXT_DIM, SURFACE_HI);
}

pub fn draw_status_bar<D: DrawTarget<Color = Rgb888>>(fb: &mut D, right_text: &str) {
    draw_header(fb, "MICRONUTS", Some((right_text, ACCENT)), true);
}

pub fn draw_scanning<D: DrawTarget<Color = Rgb888>>(fb: &mut D, aim_on: bool) {
    fb.clear(BG).ok();
    draw_header(fb, "SCANNER", None, false);

    let vf_size = 280u32;
    let vf_x = (WIDTH - vf_size) / 2;
    let vf_y = 160u32;
    icon_scan(fb, vf_x, vf_y, vf_size, 5, ACCENT);
    RoundedRectangle::new(
        Rectangle::new(
            Point::new((vf_x + 18) as i32, (vf_y + 18) as i32),
            Size::new(vf_size - 36, vf_size - 36),
        ),
        CornerRadii::new(Size::new(14, 14)),
    )
    .into_styled(rect_style(None, Some((STROKE, 1))))
    .draw(fb)
    .ok();

    dot(fb, WIDTH as i32 / 2 - 74, 500, 10, GREEN);
    text_left(
        fb,
        "SCANNING",
        WIDTH as i32 / 2 - 56,
        494,
        MonoTextStyle::new(&FONT_10X20, GREEN),
    );
    text_centered(
        fb,
        "Point the scanner at a QR code",
        WIDTH as i32 / 2,
        560,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );

    draw_ghost_button(fb, &back_button(), TEXT_DIM, SURFACE_HI);

    let mut aim_btn = aim_button();
    aim_btn.label = if aim_on { "LASER: ON" } else { "LASER: OFF" };
    draw_ghost_button(
        fb,
        &aim_btn,
        if aim_on { AMBER } else { TEXT_DIM },
        SURFACE_HI,
    );
}

pub fn draw_scanning_progress<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    elapsed_secs: u32,
    max_secs: u32,
) {
    let remaining = max_secs.saturating_sub(elapsed_secs);
    let track_x = 40u32;
    let track_w = WIDTH - 80;
    let track_y = 700u32;
    let track_h = 10u32;
    RoundedRectangle::new(
        Rectangle::new(
            Point::new(track_x as i32, track_y as i32),
            Size::new(track_w, track_h),
        ),
        CornerRadii::new(Size::new(track_h / 2, track_h / 2)),
    )
    .into_styled(rect_style(Some(SURFACE_HI), None))
    .draw(fb)
    .ok();
    if let Some(fill_w) = track_w.checked_mul(remaining).map(|n| n / max_secs) {
        if fill_w >= track_h {
            RoundedRectangle::new(
                Rectangle::new(
                    Point::new(track_x as i32, track_y as i32),
                    Size::new(fill_w, track_h),
                ),
                CornerRadii::new(Size::new(track_h / 2, track_h / 2)),
            )
            .into_styled(rect_style(Some(GREEN), None))
            .draw(fb)
            .ok();
        }
    }

    let mut label = heapless::String::<32>::new();
    let _ = label.push_str(&format_u32_len(remaining as usize));
    let _ = label.push_str("s remaining");
    text_centered(
        fb,
        &label,
        WIDTH as i32 / 2,
        734,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );
}

pub fn draw_scanning_retry<D: DrawTarget<Color = Rgb888>>(fb: &mut D) {
    let x = 40u32;
    let w = WIDTH - 80;
    let y = 690u32;
    let h = 40u32;
    card(fb, x, y, w, h, SURFACE, AMBER);
    text_centered(
        fb,
        "RETRYING...",
        WIDTH as i32 / 2,
        (y + h / 2 - 10 + 6) as i32,
        MonoTextStyle::new(&FONT_10X20, AMBER),
    );
}

pub fn render_token_info<D: DrawTarget<Color = Rgb888>>(fb: &mut D, token: &TokenV4) {
    fb.clear(BG).ok();
    draw_header(fb, "TOKEN", None, true);

    draw_pill(
        fb,
        (WIDTH - pill_width("CASHU V4", false)) / 2,
        66,
        "CASHU V4",
        GREEN,
        GREEN_DEEP,
        None,
    );

    let amount_str = u64_to_string(token.total_amount());
    draw_hero_centered(fb, &amount_str, WIDTH as i32 / 2, 116, 7, TEXT);
    let unit_label = truncate_str(&token.unit, 8).to_uppercase();
    draw_hero_centered(fb, &unit_label, WIDTH as i32 / 2, 196, 2, TEXT_DIM);

    let mut rows: [(&str, heapless::String<48>); 4] = [
        ("MINT", heapless::String::new()),
        ("UNIT", heapless::String::new()),
        ("PROOFS", heapless::String::new()),
        ("KEYSET", heapless::String::new()),
    ];
    let _ = rows[0].1.push_str(truncate_str(&token.mint, 30));
    let _ = rows[1].1.push_str(truncate_str(&token.unit, 30));
    let _ = rows[2]
        .1
        .push_str(&u64_to_string(token.proof_count() as u64));
    if let Some(first) = token.tokens.first() {
        let _ = rows[3].1.push_str(truncate_str(&first.keyset_id, 30));
    }

    let x = 24u32;
    let w = WIDTH - 48;
    let mut y = 300u32;
    card(fb, x, y, w, 4 * 52 + 16, SURFACE, STROKE);
    y += 16;
    for (label, value) in &rows {
        text_left(
            fb,
            label,
            (x + 18) as i32,
            (y + 20) as i32,
            MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
        );
        let vw = value.len() as u32 * CHAR_W as u32;
        text_left(
            fb,
            value,
            (x + w - 18 - vw) as i32,
            (y + 20) as i32,
            MonoTextStyle::new(&FONT_10X20, TEXT),
        );
        y += 52;
    }
}

fn u64_to_string(n: u64) -> heapless::String<20> {
    let mut result = heapless::String::new();
    let mut n = n;
    let mut digits = [0u8; 20];
    let mut i = 0;

    if n == 0 {
        result.push('0').ok();
        return result;
    }

    while n > 0 {
        digits[i] = (n % 10) as u8;
        n /= 10;
        i += 1;
    }

    for j in (0..i).rev() {
        result.push(char::from(b'0' + digits[j])).ok();
    }

    result
}

fn floor_char_boundary(s: &str, max_len: usize) -> usize {
    let mut end = core::cmp::min(max_len, s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..floor_char_boundary(s, max_len)]
    }
}

fn take_line(s: &str, start: usize, max_len: usize) -> (&str, usize) {
    let mut end = floor_char_boundary(s, start + max_len);
    if end == start {
        end = start + s[start..].chars().next().map(char::len_utf8).unwrap_or(0);
    }
    (&s[start..end], end)
}

pub fn render_status<D: DrawTarget<Color = Rgb888>>(fb: &mut D, message: &str) {
    fb.clear(BG).ok();
    draw_header(fb, "MICRONUTS", None, true);
    icon_info(fb, WIDTH as i32 / 2, 300, 72, ACCENT);
    text_centered(
        fb,
        truncate_str(message, 42),
        WIDTH as i32 / 2,
        386,
        MonoTextStyle::new(&FONT_10X20, TEXT),
    );
}

pub fn render_home<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    scanner_connected: bool,
    has_last_scan: bool,
) {
    fb.clear(BG).ok();

    let (right, color) = if scanner_connected {
        ("GM65 OK", GREEN)
    } else {
        ("NO SCANNER", RED)
    };
    draw_header(fb, "MICRONUTS", Some((right, color)), true);

    text_left(
        fb,
        "WALLET",
        24,
        76,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );

    let buttons = home_buttons();
    draw_menu_card(
        fb,
        &buttons[0],
        MenuIcon::Scan,
        "SCAN QR CODE",
        "Scan a Cashu QR code",
        true,
    );

    if has_last_scan {
        draw_menu_card(
            fb,
            &buttons[1],
            MenuIcon::Scan,
            "VIEW LAST SCAN",
            "Review the captured QR",
            false,
        );
    } else {
        draw_menu_card(
            fb,
            &buttons[1],
            MenuIcon::Download,
            "IMPORT TOKEN",
            "Receive a token over USB",
            false,
        );
    }

    draw_menu_card(
        fb,
        &buttons[2],
        MenuIcon::Coins,
        "SHOW PROOFS",
        "Export proofs as QR",
        false,
    );
}

pub fn render_waiting_token<D: DrawTarget<Color = Rgb888>>(fb: &mut D) {
    fb.clear(BG).ok();
    draw_header(fb, "IMPORT", None, false);
    draw_ghost_button(fb, &back_button(), TEXT_DIM, SURFACE_HI);

    let card_y = 240u32;
    card(fb, 40, card_y, WIDTH - 80, 300, SURFACE, STROKE);
    icon_download(fb, WIDTH / 2 - 34, card_y + 48, 68, ACCENT);

    text_centered(
        fb,
        "Waiting for token...",
        WIDTH as i32 / 2,
        (card_y + 190) as i32,
        MonoTextStyle::new(&FONT_10X20, TEXT),
    );
    text_centered(
        fb,
        "Send ImportToken via USB",
        WIDTH as i32 / 2,
        (card_y + 226) as i32,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );
}

pub fn render_error<D: DrawTarget<Color = Rgb888>>(fb: &mut D, message: &str) {
    fb.clear(BG).ok();
    draw_header(fb, "MICRONUTS", None, true);

    card(fb, 24, 180, WIDTH - 48, 360, RED_DEEP, RED);
    icon_alert(fb, WIDTH as i32 / 2, 300, 84, RED);
    draw_hero_centered(fb, "ERROR", WIDTH as i32 / 2, 380, 4, RED);
    text_centered(
        fb,
        truncate_str(message, 40),
        WIDTH as i32 / 2,
        470,
        MonoTextStyle::new(&FONT_10X20, TEXT),
    );
}

pub fn render_scan_result<D: DrawTarget<Color = Rgb888>>(fb: &mut D, data: &[u8]) {
    fb.clear(BG).ok();
    draw_header(fb, "SCAN RESULT", None, false);
    draw_ghost_button(fb, &back_button(), TEXT_DIM, SURFACE_HI);

    text_left(
        fb,
        "Data:",
        24,
        76,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );

    let data_str = core::str::from_utf8(data).unwrap_or("<binary data>");
    let display_str = truncate_str(data_str, 2000);

    let chars_per_line = 44;
    let mut y = 110u32;
    let mut offset = 0;
    while offset < display_str.len() && y < HEIGHT - 60 {
        let (line, next_offset) = take_line(display_str, offset, chars_per_line);
        text_left(
            fb,
            line,
            24,
            y as i32,
            MonoTextStyle::new(&FONT_10X20, TEXT),
        );
        offset = next_offset;
        y += 24;
    }

    let mut size_str = heapless::String::<32>::new();
    let _ = size_str.push_str(&u64_to_string(data.len() as u64));
    let _ = size_str.push_str(" bytes");
    let w = pill_width(&size_str, false);
    draw_pill(
        fb,
        WIDTH - w - 16,
        HEIGHT - 48,
        &size_str,
        TEXT_DIM,
        SURFACE,
        None,
    );
}

fn type_chip(payload: &QrPayload) -> (&'static str, Rgb888, Rgb888) {
    match payload {
        QrPayload::CashuV4 { .. } => ("CASHU V4", GREEN, GREEN_DEEP),
        QrPayload::CashuV3 { .. } => ("CASHU V3", AMBER, SURFACE),
        QrPayload::UrFragment { .. } => ("UR FRAGMENT", ACCENT, ACCENT_DEEP),
        QrPayload::PlainText(_) => ("TEXT", TEXT_DIM, SURFACE),
        QrPayload::Binary(_) => ("BINARY", TEXT_DIM, SURFACE),
    }
}

pub fn render_decoded_scan<D: DrawTarget<Color = Rgb888>>(fb: &mut D, payload: &QrPayload) {
    let raw = payload.raw_data();
    if matches!(payload, QrPayload::PlainText(_) | QrPayload::Binary(_))
        && raw.len() <= 200
        && core::str::from_utf8(raw).is_ok()
    {
        render_qr_mirror(fb, raw);
        return;
    }

    fb.clear(BG).ok();
    draw_header(fb, "SCAN RESULT", None, false);
    draw_ghost_button(fb, &back_button(), TEXT_DIM, SURFACE_HI);

    let (chip_text, chip_fg, chip_bg) = type_chip(payload);
    let chip_label: heapless::String<24> = match payload {
        QrPayload::UrFragment { parsed } => {
            let mut s = heapless::String::new();
            let _ = s.push_str("UR ");
            let _ = s.push_str(&format_u32_len(parsed.index as usize));
            let _ = s.push('/');
            let _ = s.push_str(&format_u32_len(parsed.total as usize));
            s
        }
        _ => {
            let mut s = heapless::String::new();
            let _ = s.push_str(truncate_str(chip_text, 12));
            s
        }
    };
    let cw = pill_width(&chip_label, false);
    draw_pill(
        fb,
        (WIDTH - cw) / 2,
        68,
        &chip_label,
        chip_fg,
        chip_bg,
        None,
    );

    let mut y = 120u32;

    if let QrPayload::CashuV4 { encoded } = payload {
        match cashu_core_lite::decode_token(encoded) {
            Ok(token) => {
                draw_hero_centered(
                    fb,
                    &u64_to_string(token.total_amount()),
                    WIDTH as i32 / 2,
                    y as i32,
                    6,
                    GREEN,
                );
                let unit_label = truncate_str(&token.unit, 8).to_uppercase();
                draw_hero_centered(
                    fb,
                    &unit_label,
                    WIDTH as i32 / 2,
                    y as i32 + 58,
                    2,
                    TEXT_DIM,
                );
                y += 116;

                let x = 24u32;
                let w = WIDTH - 48;
                let row_h = 44u32;
                let rows = 3u32;
                card(fb, x, y, w, rows * row_h + 16, SURFACE, STROKE);
                let mut ry = y + 16;
                let mut kv = |label: &str, value: &str| {
                    text_left(
                        fb,
                        label,
                        (x + 18) as i32,
                        (ry + 16) as i32,
                        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
                    );
                    let vw = value.len() as u32 * CHAR_W as u32;
                    text_left(
                        fb,
                        truncate_str(value, 26),
                        (x + w - 18 - vw) as i32,
                        (ry + 16) as i32,
                        MonoTextStyle::new(&FONT_10X20, TEXT),
                    );
                    ry += row_h;
                };
                kv("MINT", &token.mint);
                kv("PROOFS", &u64_to_string(token.proof_count() as u64));
                if let Some(first) = token.tokens.first() {
                    kv("KEYSET", &first.keyset_id);
                }
                y += rows * row_h + 16 + 24;
            }
            Err(_) => {
                card(fb, 24, y, WIDTH - 48, 56, RED_DEEP, RED);
                text_centered(
                    fb,
                    "Token decode error",
                    WIDTH as i32 / 2,
                    (y + 28 - 10 + 6) as i32,
                    MonoTextStyle::new(&FONT_10X20, RED),
                );
                y += 56 + 24;
            }
        }
    }

    let data_str = core::str::from_utf8(raw).unwrap_or("<binary data>");
    let bytes_label = {
        let mut s = heapless::String::<24>::new();
        let _ = s.push_str(&format_u32_len(raw.len()));
        let _ = s.push_str(" BYTES");
        s
    };
    text_left(
        fb,
        "DATA",
        24,
        y as i32,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );
    let bw = bytes_label.len() as u32 * CHAR_W as u32;
    text_left(
        fb,
        &bytes_label,
        (WIDTH - 24 - bw) as i32,
        y as i32,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );
    y += 26;

    let chars_per_line = 44;
    let mut offset = 0;
    let mut lines = 0;
    let max_lines = ((HEIGHT - 40).saturating_sub(y) / 24) as usize;
    while offset < data_str.len() && lines < max_lines {
        let (line, next_offset) = take_line(data_str, offset, chars_per_line);
        text_left(
            fb,
            line,
            24,
            y as i32,
            MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
        );
        offset = next_offset;
        y += 24;
        lines += 1;
    }
    if offset < data_str.len() {
        text_left(
            fb,
            "...",
            24,
            y as i32,
            MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
        );
    }
}

pub(crate) fn format_u32_len(len: usize) -> heapless::String<16> {
    let mut s = heapless::String::new();
    if len < 10 {
        let _ = s.push((b'0' + len as u8) as char);
    } else if len < 100 {
        let _ = s.push((b'0' + (len / 10) as u8) as char);
        let _ = s.push((b'0' + (len % 10) as u8) as char);
    } else if len < 1000 {
        let _ = s.push((b'0' + (len / 100) as u8) as char);
        let _ = s.push((b'0' + ((len / 10) % 10) as u8) as char);
        let _ = s.push((b'0' + (len % 10) as u8) as char);
    } else {
        let mut n = len;
        let mut digits = [0u8; 8];
        let mut i = 0;
        while n > 0 && i < 8 {
            digits[i] = (n % 10) as u8;
            n /= 10;
            i += 1;
        }
        for j in (0..i).rev() {
            let _ = s.push(digits[j] as char);
        }
    }
    s
}

fn render_qr_screen<D: DrawTarget<Color = Rgb888>>(fb: &mut D, title: &str, text: &str) -> bool {
    let mut temp_buf = [0u8; QR_BUF_SIZE];
    let mut out_buf = [0u8; QR_BUF_SIZE];

    let qr = match QrCode::encode_text(
        text,
        &mut temp_buf,
        &mut out_buf,
        QrCodeEcc::Medium,
        Version::MIN,
        Version::MAX,
        None,
        true,
    ) {
        Ok(qr) => qr,
        Err(_) => return false,
    };

    let border = 2;
    let qr_size = qr.size();
    let total = qr_size + border * 2;

    let max_scale = ((WIDTH - 120) / total as u32)
        .min((HEIGHT - 260) / total as u32)
        .max(1);

    let qr_pixel = total as u32 * max_scale;
    let card_w = qr_pixel + 48;
    let card_h = qr_pixel + 48;
    let card_x = (WIDTH - card_w) / 2;
    let card_y = 150;

    fb.clear(BG).ok();
    draw_header(fb, title, None, false);
    draw_ghost_button(fb, &back_button(), TEXT_DIM, SURFACE_HI);

    card(fb, card_x, card_y, card_w, card_h, WHITE, STROKE);

    let offset_x = card_x + 24;
    let offset_y = card_y + 24;

    for qr_y in 0..qr_size {
        for qr_x in 0..qr_size {
            let color = if qr.get_module(qr_x, qr_y) {
                BLACK
            } else {
                WHITE
            };
            let px = offset_x + (qr_x + border) as u32 * max_scale;
            let py = offset_y + (qr_y + border) as u32 * max_scale;
            if px + max_scale <= WIDTH && py + max_scale <= HEIGHT {
                let _ = fb.fill_solid(
                    &Rectangle::new(
                        Point::new(px as i32, py as i32),
                        Size::new(max_scale, max_scale),
                    ),
                    color,
                );
            }
        }
    }

    text_centered(
        fb,
        truncate_str(text, 42),
        WIDTH as i32 / 2,
        (card_y + card_h + 30) as i32,
        MonoTextStyle::new(&FONT_10X20, TEXT_DIM),
    );

    true
}

pub fn render_qr_code<D: DrawTarget<Color = Rgb888>>(fb: &mut D, text: &str) -> bool {
    render_qr_screen(fb, "QR CODE", text)
}

pub fn render_qr_mirror<D: DrawTarget<Color = Rgb888>>(fb: &mut D, data: &[u8]) {
    match core::str::from_utf8(data) {
        Ok(text) if data.len() <= 200 => {
            if !render_qr_screen(fb, "QR MIRROR", text) {
                render_status(fb, "QR encode failed");
            }
        }
        Ok(_) => {
            render_status(fb, "Data too long for QR");
        }
        Err(_) => {
            render_status(fb, "Binary data");
        }
    }
}

/// The ShowProofs screen: the completed swap's export token as a QR
/// (#29). Returns false (and renders a status line) when there is
/// nothing to export or the QR cannot hold the token.
pub fn render_export_qr<D: DrawTarget<Color = Rgb888>>(
    fb: &mut D,
    state: &crate::state::FirmwareState,
) -> bool {
    let Some(token) = crate::command_handler::build_export_token(state) else {
        render_status(fb, "No proofs available yet");
        return false;
    };
    match cashu_core_lite::encode_token_wire(&token) {
        Ok(wire) => {
            if render_qr_screen(fb, "PROOFS", &wire) {
                true
            } else {
                render_status(fb, "QR encode failed");
                false
            }
        }
        Err(_) => {
            render_status(fb, "Token encode failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::FirmwareState;
    use alloc::vec;
    use core::convert::Infallible;
    use embedded_graphics::geometry::Size;
    use embedded_graphics::prelude::OriginDimensions;

    /// Minimal 480x800 framebuffer DrawTarget that counts pixels per
    /// color — enough to prove a QR (both dark and light modules plus a
    /// background) actually landed.
    struct CountingFb {
        w: u32,
        h: u32,
        dark: usize,
        light: usize,
        other: usize,
    }

    impl OriginDimensions for CountingFb {
        fn size(&self) -> Size {
            Size::new(self.w, self.h)
        }
    }

    impl DrawTarget for CountingFb {
        type Color = Rgb888;
        type Error = Infallible;

        fn draw_iter<T>(&mut self, item: T) -> Result<(), Self::Error>
        where
            T: IntoIterator<Item = Pixel<Rgb888>>,
        {
            for Pixel(p, c) in item {
                if p.x < 0 || p.y < 0 || p.x as u32 >= self.w || p.y as u32 >= self.h {
                    continue;
                }
                match c {
                    BLACK => self.dark += 1,
                    WHITE => self.light += 1,
                    _ => self.other += 1,
                }
            }
            Ok(())
        }

        fn clear(&mut self, color: Rgb888) -> Result<(), Self::Error> {
            match color {
                BLACK => {
                    self.dark = (self.w * self.h) as usize;
                    self.light = 0;
                }
                _ => {
                    self.light = (self.w * self.h) as usize;
                    self.dark = 0;
                }
            }
            self.other = 0;
            Ok(())
        }

        fn fill_solid(&mut self, area: &Rectangle, color: Rgb888) -> Result<(), Self::Error> {
            let Size { width, height } = area.size;
            let n = (width as usize) * (height as usize);
            match color {
                BLACK => self.dark += n,
                WHITE => self.light += n,
                _ => self.other += n,
            }
            Ok(())
        }
    }

    fn proofs_ready_state() -> FirmwareState {
        let mut state = FirmwareState::new();
        state.imported_token = Some(cashu_core_lite::TokenV4 {
            mint: alloc::string::String::from("demo://micronuts"),
            unit: alloc::string::String::from("sat"),
            memo: None,
            tokens: vec![cashu_core_lite::TokenV4Token {
                keyset_id: alloc::string::String::from("00"),
                proofs: vec![],
            }],
        });
        state.new_proofs = Some(vec![
            cashu_core_lite::Proof {
                amount: 16,
                keyset_id: alloc::string::String::from("00"),
                secret: alloc::string::String::from("7072696e742d746f6b656e2d3136"),
                c: vec![0x02; 33],
                dleq: Some(dleq()),
            },
            cashu_core_lite::Proof {
                amount: 4,
                keyset_id: alloc::string::String::from("00"),
                secret: alloc::string::String::from("7072696e742d746f6b656e2d3034"),
                c: vec![0x02; 33],
                dleq: Some(dleq()),
            },
            cashu_core_lite::Proof {
                amount: 1,
                keyset_id: alloc::string::String::from("00"),
                secret: alloc::string::String::from("7072696e742d746f6b656e2d3031"),
                c: vec![0x02; 33],
                dleq: Some(dleq()),
            },
        ]);
        state
    }

    fn dleq() -> cashu_core_lite::nuts::nut12::ProofDleq {
        cashu_core_lite::nuts::nut12::ProofDleq::new(
            cashu_core_lite::SecretKey::from_slice(&[0x11; 32]).unwrap(),
            cashu_core_lite::SecretKey::from_slice(&[0x22; 32]).unwrap(),
            cashu_core_lite::SecretKey::from_slice(&[0x33; 32]).unwrap(),
        )
    }

    #[test]
    fn export_qr_renders_for_a_real_sized_dleq_token() {
        let state = proofs_ready_state();
        let wire = cashu_core_lite::encode_token_wire(
            &crate::command_handler::build_export_token(&state).unwrap(),
        )
        .unwrap();
        // Real DLEQ-bearing tokens are ~800+ chars — far above the old
        // 200-char mirror gate; the export screen must still render.
        assert!(wire.len() > 600, "wire len {}", wire.len());

        let mut fb = CountingFb {
            w: WIDTH,
            h: HEIGHT,
            dark: 0,
            light: 0,
            other: 0,
        };
        assert!(render_export_qr(&mut fb, &state));
        assert!(fb.dark > 0 && fb.light > 0, "QR needs both module colors");
    }

    #[test]
    fn export_qr_fails_cleanly_without_proofs() {
        let state = FirmwareState::new();
        let mut fb = CountingFb {
            w: WIDTH,
            h: HEIGHT,
            dark: 0,
            light: 0,
            other: 0,
        };
        assert!(!render_export_qr(&mut fb, &state));
    }

    #[test]
    fn hero_glyphs_cover_amounts_and_sat_units() {
        for c in "0123456789 SATMURNOKB.,:-!?/".chars() {
            assert!(hero_glyph(c).is_some(), "missing hero glyph {c}");
        }
        assert!(hero_width("255", 7) > 0);
        assert!(hero_width("", 7) == 0);
    }

    #[test]
    fn u64_to_string_yields_ascii_digits() {
        // Found by the vision QA pass: `9u8 as char` is a control
        // character, so every nonzero amount rendered invisible.
        assert_eq!(u64_to_string(0).as_str(), "0");
        assert_eq!(u64_to_string(21).as_str(), "21");
        assert_eq!(u64_to_string(65535).as_str(), "65535");
        for c in u64_to_string(255).chars() {
            assert!(c.is_ascii_digit(), "non-digit {c:?} in converted amount");
        }
    }

    #[test]
    fn hero_glyphs_cover_error_title() {
        // Found by the vision QA pass: "ERROR" rendered as "RROR" — and
        // the first fix bitmap had BOTH verticals (an "8"), caught by a
        // second vision pass. Pin the shape, not just presence.
        for c in "ERROR".chars() {
            assert!(hero_glyph(c).is_some(), "missing hero glyph {c}");
        }
        assert_eq!(
            hero_glyph('E'),
            Some([0x1F, 0x10, 0x10, 0x1F, 0x10, 0x10, 0x1F])
        );
    }

    #[test]
    fn home_screen_paints_surfaces_and_keeps_hit_targets() {
        let mut fb = CountingFb {
            w: WIDTH,
            h: HEIGHT,
            dark: 0,
            light: 0,
            other: 0,
        };
        render_home(&mut fb, true, true);
        assert!(fb.other > 0, "home must paint theme colors");
        for btn in home_buttons() {
            let (cx, cy) = (btn.x + btn.w / 2, btn.y + btn.h / 2);
            assert!(btn.hit(cx as u16, cy as u16));
        }
    }
}
