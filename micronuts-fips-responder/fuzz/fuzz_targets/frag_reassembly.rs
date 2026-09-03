#![no_main]
//! Fuzz the FSP service-datagram segmentation codec (`frag`).
//!
//! Invariants:
//! 1. Roundtrip: any frame that fits (buffer + MAX_SEGMENTS) reassembles
//!    byte-identically after `for_each_segment` at any mtu.
//! 2. No panics on arbitrary adversarial segment streams (order, gap,
//!    duplicate msg_id, cap edges) — every push yields Ok or Err.

use libfuzzer_sys::fuzz_target;
use micronuts_fips_responder::frag::{for_each_segment, segment_count, Reassembler, MAX_SEGMENTS};

const BUF: usize = 2048;

fuzz_target!(|data: &[u8]| {
    // 1. Roundtrip at an mtu derived from the first byte (3..=130 keeps
    //    small-mtu multi-segment paths hot).
    let mtu = 3 + (data.first().copied().unwrap_or(0) % 128) as usize;
    let frame = &data[data.len().min(1)..];
    if frame.len() <= BUF && segment_count(frame.len(), mtu) <= MAX_SEGMENTS {
        let mut re = Reassembler::<BUF>::new();
        for_each_segment(frame, mtu, 0x5A, |header, payload| {
            let mut seg = Vec::with_capacity(2 + payload.len());
            seg.extend_from_slice(header);
            seg.extend_from_slice(payload);
            match re.push(&seg) {
                Ok(None) => {}
                Ok(Some(completed)) => assert_eq!(completed, frame, "roundtrip must be stable"),
                Err(e) => panic!("clean segment stream must not error: {e:?}"),
            }
        });
    }

    // 2. Adversarial stream: interpret input as [len:1][seg bytes...] records;
    //    every push must return Ok/Err without panicking.
    let mut re = Reassembler::<BUF>::new();
    let mut rest = data;
    while let Some(&l) = rest.first() {
        let (seg, tail) = rest[1..].split_at((l as usize).min(rest.len() - 1));
        let _ = re.push(seg);
        rest = tail;
    }
});
