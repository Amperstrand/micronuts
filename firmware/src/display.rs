use cashu_core_lite::token::TokenV4;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Alignment, Text, TextStyleBuilder},
};
use stm32f469i_disc::hal::ltdc::LtdcFramebuffer;

pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 480;

pub fn render_token_info(fb: &mut LtdcFramebuffer<u16>, token: &TokenV4) {
    fb.clear(Rgb565::BLACK).ok();

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style("Cashu Token", Point::new(400, 30), title_style, center_text)
        .draw(fb)
        .ok();

    let y_sep = 50;
    let mint_label_y = y_sep + 40;

    Text::new("Mint:", Point::new(20, mint_label_y), label_style)
        .draw(fb)
        .ok();
    Text::new(
        truncate_url(&token.mint, 60),
        Point::new(100, mint_label_y),
        value_style,
    )
    .draw(fb)
    .ok();

    let unit_y = mint_label_y + 40;
    Text::new("Unit:", Point::new(20, unit_y), label_style)
        .draw(fb)
        .ok();
    Text::new(&token.unit, Point::new(100, unit_y), value_style)
        .draw(fb)
        .ok();

    let amount_y = unit_y + 40;
    Text::new("Amount:", Point::new(20, amount_y), label_style)
        .draw(fb)
        .ok();

    let amount_str = format_amount(token.total_amount(), &token.unit);
    Text::new(&amount_str, Point::new(120, amount_y), value_style)
        .draw(fb)
        .ok();

    let proof_y = amount_y + 40;
    Text::new("Proofs:", Point::new(20, proof_y), label_style)
        .draw(fb)
        .ok();

    let proof_str = u64_to_string(token.proof_count() as u64);
    Text::new(&proof_str, Point::new(120, proof_y), value_style)
        .draw(fb)
        .ok();

    if let Some(memo) = &token.memo {
        let memo_y = proof_y + 40;
        Text::new("Memo:", Point::new(20, memo_y), label_style)
            .draw(fb)
            .ok();
        Text::new(truncate_str(memo, 50), Point::new(100, memo_y), value_style)
            .draw(fb)
            .ok();
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

pub fn render_status(fb: &mut LtdcFramebuffer<u16>, message: &str) {
    fb.clear(Rgb565::BLACK).ok();
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(400, 240),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_error(fb: &mut LtdcFramebuffer<u16>, message: &str) {
    fb.clear(Rgb565::BLACK).ok();
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::RED);
    let msg_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style("ERROR", Point::new(400, 200), title_style, center_text)
        .draw(fb)
        .ok();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(400, 240),
        msg_style,
        center_text,
    )
    .draw(fb)
    .ok();
}
