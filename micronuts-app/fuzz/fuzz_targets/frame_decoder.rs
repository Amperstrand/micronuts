#![no_main]
//! Fuzz the USB CDC FrameDecoder state machine (the #55 surface).
//!
//! Invariants:
//! 1. No panics on any byte sequence (the #55 bug class: invalid command
//!    bytes / length fields mid-stream).
//! 2. Chunking-invariance: whether the same wire bytes arrive as one
//!    buffer, byte-by-byte, or split at an arbitrary point, the first
//!    decoded frame (if any) is identical — chunk boundaries can never
//!    create, destroy, or alter a frame.

use libfuzzer_sys::fuzz_target;
use micronuts_app::protocol::FrameDecoder;

fuzz_target!(|data: &[u8]| {
    let mut whole = FrameDecoder::new();
    let f_whole = whole.decode(data);

    let mut per_byte = FrameDecoder::new();
    let mut f_byte: Option<micronuts_app::protocol::Frame> = None;
    for &b in data {
        if f_byte.is_none() {
            f_byte = per_byte.decode(&[b]);
        } else {
            let _ = per_byte.decode(&[b]);
        }
    }

    let k = data.len() / 3;
    let mut split = FrameDecoder::new();
    let mut f_split: Option<micronuts_app::protocol::Frame> = None;
    for chunk in [&data[..k], &data[k..]] {
        if f_split.is_none() {
            f_split = split.decode(chunk);
        } else {
            let _ = split.decode(chunk);
        }
    }

    let key = |f: &Option<micronuts_app::protocol::Frame>| {
        f.as_ref()
            .map(|fr| (fr.command, fr.length, fr.payload[..fr.length as usize].to_vec()))
    };
    assert_eq!(key(&f_whole), key(&f_byte), "byte-by-byte must match whole");
    assert_eq!(key(&f_whole), key(&f_split), "split feed must match whole");
});
