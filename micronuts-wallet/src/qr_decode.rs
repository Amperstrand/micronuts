//! QR decoding for camera frames (browser). The GM65 scanner module
//! decodes QR on-chip, so software decoding exists only for the camera
//! path — encode/decode round-trips are unit-tested on native.

/// Decode the first QR code found in a luminance buffer (0 = black,
/// 255 = white), row-major, `width * height` pixels.
pub fn decode_luminance(width: usize, height: usize, luma: &[u8]) -> Option<String> {
    if luma.len() < width * height || width == 0 || height == 0 {
        return None;
    }
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| luma[y * width + x]);
    for grid in prepared.detect_grids() {
        if let Ok((_meta, content)) = grid.decode() {
            return Some(content);
        }
    }
    None
}

/// RGBA variant used by the camera loop (BT.601 integer luma).
pub fn decode_rgba(width: usize, height: usize, rgba: &[u8]) -> Option<String> {
    if rgba.len() < width * height * 4 {
        return None;
    }
    let luma: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            ((u32::from(px[0]) * 299 + u32::from(px[1]) * 587 + u32::from(px[2]) * 114) / 1000)
                as u8
        })
        .collect();
    decode_luminance(width, height, &luma)
}

/// Encode `text` as a QR luminance grid (dark modules = 0), mirroring the
/// on-screen renderer's colors — the decode round-trip test bed.
pub fn encode_luminance(text: &str, scale: usize, quiet_zone: usize) -> Option<(usize, Vec<u8>)> {
    use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
    let mut out = [0u8; Version::MAX.buffer_len()];
    let mut temp = [0u8; Version::MAX.buffer_len()];
    let code = QrCode::encode_text(
        text,
        &mut temp,
        &mut out,
        QrCodeEcc::Medium,
        Version::MIN,
        Version::MAX,
        None,
        true,
    )
    .ok()?;
    let modules = code.size() as usize;
    let dim = (modules + quiet_zone * 2) * scale;
    let mut luma = vec![255u8; dim * dim];
    for y in 0..dim {
        for x in 0..dim {
            let mx = (x / scale) as i32 - quiet_zone as i32;
            let my = (y / scale) as i32 - quiet_zone as i32;
            let dark = mx >= 0
                && my >= 0
                && (mx as usize) < modules
                && (my as usize) < modules
                && code.get_module(mx, my);
            if dark {
                luma[y * dim + x] = 0;
            }
        }
    }
    Some((dim, luma))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(text: &str) {
        let (dim, luma) = encode_luminance(text, 4, 4).expect("encode");
        let decoded = decode_luminance(dim, dim, &luma).expect("decode");
        assert_eq!(decoded, text);
    }

    #[test]
    fn roundtrip_short_text() {
        roundtrip("hello");
    }

    #[test]
    fn roundtrip_cashu_shaped_token() {
        let token = format!(
            "cashuB{}",
            "pGFtdWh0dHA6Ly8xMjcuMC4wLjE6MzAzMHVpc2F0bQ"
                .chars()
                .cycle()
                .take(90)
                .collect::<String>()
        );
        roundtrip(&token);
    }

    #[test]
    fn roundtrip_via_rgba_path() {
        let text = "rgba path 42";
        let (dim, luma) = encode_luminance(text, 4, 4).expect("encode");
        let rgba: Vec<u8> = luma.iter().flat_map(|&v| [v, v, v, 255]).collect();
        assert_eq!(decode_rgba(dim, dim, &rgba).as_deref(), Some(text));
    }

    #[test]
    fn corrupted_grid_never_panics() {
        let (dim, luma) = encode_luminance("corrupt me", 4, 4).expect("encode");
        let mut corrupted = luma;
        for (i, byte) in corrupted.iter_mut().enumerate() {
            if i % 2 == 0 {
                *byte = 255 - *byte;
            }
        }
        let _ = decode_luminance(dim, dim, &corrupted);
    }

    #[test]
    fn undersized_inputs_return_none() {
        assert_eq!(decode_luminance(0, 0, &[]), None);
        assert_eq!(decode_luminance(4, 4, &[0; 4]), None);
        assert_eq!(decode_rgba(2, 2, &[0; 4]), None);
    }
}
