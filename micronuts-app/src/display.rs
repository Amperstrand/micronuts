extern crate alloc;

use cashu_core_lite::token::TokenV4;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text, TextStyleBuilder},
};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};

use crate::qr::QrPayload;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 800;

const BLACK: Rgb565 = Rgb565::BLACK;
const WHITE: Rgb565 = Rgb565::WHITE;
const DARK_GRAY: Rgb565 = Rgb565::new(0x18, 0x18, 0x18);
const MID_GRAY: Rgb565 = Rgb565::new(0x40, 0x40, 0x40);
const ACCENT: Rgb565 = Rgb565::new(0x00, 0x7A, 0xCC);
const GREEN: Rgb565 = Rgb565::new(0x00, 0xCC, 0x66);
const YELLOW: Rgb565 = Rgb565::new(0xCC, 0xAA, 0x00);
const QR_BUF_SIZE: usize = Version::MAX.buffer_len();

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
            y: 60,
            w: WIDTH - 80,
            h: 110,
            label: "SCAN QR CODE",
        },
        Button {
            x: 40,
            y: 190,
            w: WIDTH - 80,
            h: 110,
            label: "IMPORT TOKEN",
        },
        Button {
            x: 40,
            y: 320,
            w: WIDTH - 80,
            h: 110,
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
        x: WIDTH as u32 - 170,
        y: 8,
        w: 160,
        h: 36,
        label: "LASER: OFF",
    }
}

pub fn draw_button<D: DrawTarget<Color = Rgb565>>(fb: &mut D, btn: &Button) {
    let rect = Rectangle::new(
        Point::new(btn.x as i32, btn.y as i32),
        Size::new(btn.w, btn.h),
    );
    rect.into_styled(PrimitiveStyle::with_fill(DARK_GRAY))
        .draw(fb)
        .ok();

    let border = Rectangle::new(
        Point::new(btn.x as i32, btn.y as i32),
        Size::new(btn.w, btn.h),
    );
    border
        .into_styled(PrimitiveStyle::with_stroke(ACCENT, 2))
        .draw(fb)
        .ok();

    let label_style = MonoTextStyle::new(&FONT_10X20, ACCENT);
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        btn.label,
        Point::new((btn.x + btn.w / 2) as i32, (btn.y + btn.h / 2 - 10) as i32),
        label_style,
        center,
    )
    .draw(fb)
    .ok();
}

pub fn draw_status_bar<D: DrawTarget<Color = Rgb565>>(fb: &mut D, right_text: &str) {
    let bar = Rectangle::new(Point::new(0, 0), Size::new(WIDTH, 44));
    bar.into_styled(PrimitiveStyle::with_fill(MID_GRAY))
        .draw(fb)
        .ok();

    let title_style = MonoTextStyle::new(&FONT_10X20, WHITE);
    Text::new("MICRONUTS", Point::new(140, 14), title_style)
        .draw(fb)
        .ok();

    let right_style = MonoTextStyle::new(&FONT_10X20, ACCENT);
    let right_len = right_text.len() as i32 * 12;
    Text::new(
        right_text,
        Point::new(WIDTH as i32 - right_len - 10, 14),
        right_style,
    )
    .draw(fb)
    .ok();
}

pub fn draw_scanning<D: DrawTarget<Color = Rgb565>>(fb: &mut D, aim_on: bool) {
    fb.clear(BLACK).ok();
    draw_status_bar(fb, "SCANNING...");

    let label_style = MonoTextStyle::new(&FONT_10X20, YELLOW);
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "Scanning for QR code...",
        Point::new(WIDTH as i32 / 2, HEIGHT as i32 / 2 - 10),
        label_style,
        center,
    )
    .draw(fb)
    .ok();

    Text::with_text_style(
        "Point the scanner at a QR code",
        Point::new(WIDTH as i32 / 2, HEIGHT as i32 / 2 + 30),
        MonoTextStyle::new(&FONT_10X20, MID_GRAY),
        center,
    )
    .draw(fb)
    .ok();

    draw_button(fb, &back_button());

    let mut aim_btn = aim_button();
    aim_btn.label = if aim_on { "LASER: ON" } else { "LASER: OFF" };
    draw_button(fb, &aim_btn);
}

pub fn render_token_info<D: DrawTarget<Color = Rgb565>>(fb: &mut D, token: &TokenV4) {
    fb.clear(Rgb565::BLACK).ok();

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "Cashu Token",
        Point::new(WIDTH as i32 / 2, 30),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();

    render_token_fields(fb, token, 80);
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
        result.push(digits[j] as char).ok();
    }

    result
}

fn format_amount(amount: u64, unit: &str) -> heapless::String<32> {
    let num_str = u64_to_string(amount);
    let mut result = heapless::String::new();
    let _ = result.push_str(&num_str);
    let _ = result.push(' ');
    let _ = result.push_str(unit);
    result
}

fn truncate_url(url: &str, max_len: usize) -> &str {
    if url.len() <= max_len {
        return url;
    }
    &url[..max_len]
}

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}

pub fn render_status<D: DrawTarget<Color = Rgb565>>(fb: &mut D, message: &str) {
    fb.clear(Rgb565::BLACK).ok();
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(WIDTH as i32 / 2, HEIGHT as i32 / 2),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_home<D: DrawTarget<Color = Rgb565>>(fb: &mut D, scanner_connected: bool) {
    fb.clear(BLACK).ok();

    let right_text = if scanner_connected {
        "GM65 OK"
    } else {
        "NO SCANNER"
    };
    draw_status_bar(fb, right_text);

    for btn in &home_buttons() {
        draw_button(fb, btn);
    }
}

pub fn render_waiting_token<D: DrawTarget<Color = Rgb565>>(fb: &mut D) {
    fb.clear(BLACK).ok();
    draw_status_bar(fb, "IMPORT TOKEN");
    draw_button(fb, &back_button());

    let label_style = MonoTextStyle::new(&FONT_10X20, YELLOW);
    let dim_style = MonoTextStyle::new(&FONT_10X20, MID_GRAY);
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "Waiting for token...",
        Point::new(WIDTH as i32 / 2, HEIGHT as i32 / 2 - 40),
        label_style,
        center,
    )
    .draw(fb)
    .ok();

    Text::with_text_style(
        "Send ImportToken via USB",
        Point::new(WIDTH as i32 / 2, HEIGHT as i32 / 2),
        dim_style,
        center,
    )
    .draw(fb)
    .ok();
}

pub fn render_error<D: DrawTarget<Color = Rgb565>>(fb: &mut D, message: &str) {
    fb.clear(Rgb565::BLACK).ok();
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::RED);
    let msg_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        "ERROR",
        Point::new(WIDTH as i32 / 2, HEIGHT as i32 / 2 - 40),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();
    Text::with_text_style(
        truncate_str(message, 40),
        Point::new(WIDTH as i32 / 2, HEIGHT as i32 / 2),
        msg_style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_scan_result<D: DrawTarget<Color = Rgb565>>(fb: &mut D, data: &[u8]) {
    fb.clear(Rgb565::BLACK).ok();

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "QR Scan Result",
        Point::new(WIDTH as i32 / 2, 30),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();

    let data_str = core::str::from_utf8(data).unwrap_or("<binary data>");
    let display_str = truncate_str(data_str, 2000);

    Text::new("Data:", Point::new(20, 80), label_style)
        .draw(fb)
        .ok();

    let chars_per_line = 48;
    let mut y = 110u32;
    let mut offset = 0;
    while offset < display_str.len() && y < HEIGHT - 30 {
        let end = core::cmp::min(offset + chars_per_line, display_str.len());
        let line = &display_str[offset..end];
        if let Ok(line_str) = core::str::from_utf8(line.as_bytes()) {
            Text::new(line_str, Point::new(20, y as i32), value_style)
                .draw(fb)
                .ok();
        }
        offset = end;
        y += 22;
    }

    let mut size_str = heapless::String::<32>::new();
    let _ = size_str.push_str(&u64_to_string(data.len() as u64));
    let _ = size_str.push_str(" bytes");
    Text::with_text_style(
        truncate_str(&size_str, 30),
        Point::new(400, (HEIGHT - 10) as i32),
        label_style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_decoded_scan<D: DrawTarget<Color = Rgb565>>(fb: &mut D, payload: &QrPayload) {
    fb.clear(BLACK).ok();

    draw_status_bar(fb, "SCAN RESULT");
    draw_button(fb, &back_button());

    let label_style = MonoTextStyle::new(&FONT_10X20, YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, WHITE);
    let ok_style = MonoTextStyle::new(&FONT_10X20, GREEN);
    let dim_style = MonoTextStyle::new(&FONT_10X20, MID_GRAY);

    let type_name = payload.type_name();
    Text::new(type_name, Point::new(20, 54), ok_style)
        .draw(fb)
        .ok();

    let raw = payload.raw_data();
    let len_label = format_u32_len(raw.len());

    let mut y = 80u32;

    Text::new("Size:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(&len_label, Point::new(120, y as i32), value_style)
        .draw(fb)
        .ok();
    y += 30;

    match payload {
        QrPayload::CashuV4 { encoded } => {
            Text::new("Type:", Point::new(20, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new("Cashu V4 Token", Point::new(120, y as i32), value_style)
                .draw(fb)
                .ok();
            y += 30;

            match cashu_core_lite::decode_token(encoded) {
                Ok(token) => {
                    render_token_fields_pretty(fb, &token, y);
                    y = y + 120;
                }
                Err(_) => {
                    Text::new("Token: decode error", Point::new(20, y as i32), dim_style)
                        .draw(fb)
                        .ok();
                    y += 30;
                }
            }
        }
        QrPayload::CashuV3 { .. } => {
            Text::new("Type:", Point::new(20, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new("Cashu V3 (legacy)", Point::new(120, y as i32), value_style)
                .draw(fb)
                .ok();
            y += 30;
        }
        QrPayload::UrFragment { parsed } => {
            let mut frag_str = heapless::String::<32>::new();
            let _ = frag_str.push_str(&format_u32_len(parsed.index as usize));
            let _ = frag_str.push('/');
            let _ = frag_str.push_str(&format_u32_len(parsed.total as usize));
            Text::new("Progress:", Point::new(20, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new(&frag_str, Point::new(160, y as i32), value_style)
                .draw(fb)
                .ok();
            y += 30;
        }
        QrPayload::PlainText(_) | QrPayload::Binary(_) => {}
    }

    Text::new("Data:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    y += 25;

    let data_str = core::str::from_utf8(raw).unwrap_or("<binary data>");
    let chars_per_line = 48;
    let mut offset = 0;
    while offset < data_str.len() && y < HEIGHT - 20 {
        let end = core::cmp::min(offset + chars_per_line, data_str.len());
        let line = &data_str[offset..end];
        Text::new(line, Point::new(20, y as i32), value_style)
            .draw(fb)
            .ok();
        offset = end;
        y += 22;
    }
}

fn format_u32_len(len: usize) -> heapless::String<16> {
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
    let _ = s.push_str(" bytes");
    s
}

fn render_token_fields<D: DrawTarget<Color = Rgb565>>(fb: &mut D, token: &TokenV4, start_y: u32) {
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);

    let mut y = start_y;

    Text::new("Mint:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(
        truncate_url(&token.mint, 30),
        Point::new(100, y as i32),
        value_style,
    )
    .draw(fb)
    .ok();
    y += 30;

    Text::new("Unit:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(&token.unit, Point::new(100, y as i32), value_style)
        .draw(fb)
        .ok();
    y += 30;

    Text::new("Amount:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    let amount_str = format_amount(token.total_amount(), &token.unit);
    Text::new(&amount_str, Point::new(120, y as i32), value_style)
        .draw(fb)
        .ok();
    y += 30;

    Text::new("Proofs:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    let proof_str = u64_to_string(token.proof_count() as u64);
    Text::new(&proof_str, Point::new(120, y as i32), value_style)
        .draw(fb)
        .ok();
}

fn render_token_fields_pretty<D: DrawTarget<Color = Rgb565>>(
    fb: &mut D,
    token: &TokenV4,
    start_y: u32,
) {
    let label_style = MonoTextStyle::new(&FONT_10X20, YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, WHITE);
    let amount_style = MonoTextStyle::new(&FONT_10X20, GREEN);

    let mut y = start_y;

    let sep = "--------------------------------------------------------";
    Text::new(
        sep,
        Point::new(20, y as i32),
        MonoTextStyle::new(&FONT_10X20, MID_GRAY),
    )
    .draw(fb)
    .ok();
    y += 22;

    Text::new("Mint:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(
        truncate_url(&token.mint, 55),
        Point::new(100, y as i32),
        value_style,
    )
    .draw(fb)
    .ok();
    y += 30;

    Text::new("Unit:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(&token.unit, Point::new(100, y as i32), value_style)
        .draw(fb)
        .ok();
    y += 35;

    let amount_str = format_amount(token.total_amount(), &token.unit);
    Text::new("Amount:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(&amount_str, Point::new(120, y as i32), amount_style)
        .draw(fb)
        .ok();
    y += 35;

    let proof_str = u64_to_string(token.proof_count() as u64);
    Text::new("Proofs:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(&proof_str, Point::new(120, y as i32), value_style)
        .draw(fb)
        .ok();
    y += 25;

    if let Some(first_token) = token.tokens.first() {
        Text::new("Keyset:", Point::new(20, y as i32), label_style)
            .draw(fb)
            .ok();
        Text::new(
            &first_token.keyset_id,
            Point::new(140, y as i32),
            value_style,
        )
        .draw(fb)
        .ok();
    }
}

pub fn render_qr_code<D: DrawTarget<Color = Rgb565>>(fb: &mut D, text: &str) -> bool {
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

    let max_scale_x = (WIDTH - 40) / total as u32;
    let max_scale_y = (HEIGHT - 80) / total as u32;
    let scale = max_scale_x.min(max_scale_y).max(1);

    let qr_pixel_w = total as u32 * scale;
    let qr_pixel_h = total as u32 * scale;
    let offset_x = (WIDTH - qr_pixel_w) / 2;
    let offset_y = 20 + (HEIGHT - qr_pixel_h - 40) / 2;

    fb.clear(BLACK).ok();

    for qr_y in 0..qr_size {
        for qr_x in 0..qr_size {
            let dark = qr.get_module(qr_x, qr_y);
            let color = if dark { BLACK } else { WHITE };

            let px = offset_x + (qr_x + border) as u32 * scale;
            let py = offset_y + (qr_y + border) as u32 * scale;

            if px + scale <= WIDTH && py + scale <= HEIGHT {
                let _ = fb.fill_solid(
                    &Rectangle::new(Point::new(px as i32, py as i32), Size::new(scale, scale)),
                    color,
                );
            }
        }
    }

    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    let label = truncate_str(text, 50);
    Text::with_text_style(
        label,
        Point::new(WIDTH as i32 / 2, (offset_y + qr_pixel_h + 10) as i32),
        style,
        center,
    )
    .draw(fb)
    .ok();

    true
}

pub fn render_qr_mirror<D: DrawTarget<Color = Rgb565>>(fb: &mut D, data: &[u8]) {
    match core::str::from_utf8(data) {
        Ok(text) if data.len() <= 200 => {
            if !render_qr_code(fb, text) {
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
