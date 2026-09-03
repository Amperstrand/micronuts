//! FSP service-datagram segmentation codec.
//!
//! The FMP frame cap (2048 B on WiFi/UDP transports, 768 B on the ESP32-D0WD
//! L2CAP build) is smaller than real mint replies — a single-keyset `get_keys`
//! is ~5.4 KB and a multi-keyset reply ~21 KB (micronuts #47 item 4 sizing).
//! Service envelopes above the cap are split across multiple datagrams with a
//! 2-byte header — the same pattern as the ESP-NOW `espnow_frag` codec — and
//! reassembled strictly in order; any gap or interleave drops the message and
//! the RPC layer above retries.
//!
//! This module is written against `core` only so it can be lifted into the
//! no_std wallet/sidecar side unchanged.

/// Wire bytes prepended to every segment: `[msg_id][flags]`.
pub const SEGMENT_HEADER_LEN: usize = 2;

/// Set in `flags` on the final segment of a message.
pub const LAST_FLAG: u8 = 0x80;

/// Upper bound on segments per message (7-bit index space). The sender-side
/// [`for_each_segment`] asserts this; a frame that needs more segments at the
/// chosen mtu is a caller error.
pub const MAX_SEGMENTS: usize = 128;

/// Errors produced by [`Reassembler::push`]. The in-flight message is always
/// dropped (state reset) when an error is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragError {
    /// Segment shorter than the 2-byte header.
    TruncatedHeader,
    /// Strict in-order violation (gap or mid-stream start); message dropped.
    OutOfOrder,
    /// Reassembled frame exceeds the reassembler buffer.
    BufferOverflow,
}

/// Number of segments [`for_each_segment`] emits for `frame_len` payload bytes.
pub fn segment_count(frame_len: usize, mtu: usize) -> usize {
    debug_assert!(mtu > SEGMENT_HEADER_LEN);
    let cap = mtu - SEGMENT_HEADER_LEN;
    frame_len.div_ceil(cap).max(1)
}

/// Splits `frame` into wire segments, invoking `emit(header, payload)` once
/// per segment where `header` is the 2-byte `[msg_id][flags]` prefix.
///
/// An empty frame still emits one (header-only, last-flagged) segment.
/// `mtu` is the datagram size budget including the header; it must exceed
/// [`SEGMENT_HEADER_LEN`].
pub fn for_each_segment<F: FnMut(&[u8; SEGMENT_HEADER_LEN], &[u8])>(
    frame: &[u8],
    mtu: usize,
    msg_id: u8,
    mut emit: F,
) {
    assert!(mtu > SEGMENT_HEADER_LEN, "mtu must exceed segment header");
    let cap = mtu - SEGMENT_HEADER_LEN;
    let total = segment_count(frame.len(), mtu);
    assert!(
        total <= MAX_SEGMENTS,
        "frame of {} bytes needs {} segments at mtu {mtu}; max is {MAX_SEGMENTS}",
        frame.len(),
        total
    );
    let mut offset = 0usize;
    for idx in 0..total {
        let end = (offset + cap).min(frame.len());
        let last = idx + 1 == total;
        let flags = if last {
            LAST_FLAG | idx as u8
        } else {
            idx as u8
        };
        let header = [msg_id, flags];
        emit(&header, &frame[offset..end]);
        offset = end;
    }
}

/// In-order reassembler for received segments.
///
/// Semantics (mirroring the ESP-NOW `espnow_frag` codec): strictly in-order
/// per `msg_id`. A segment with index 0 always starts a fresh message — if a
/// different message was in flight it is dropped. Any other gap, interleave,
/// or oversized result drops the in-flight message and returns an error.
#[derive(Debug)]
pub struct Reassembler<const N: usize> {
    buf: [u8; N],
    len: usize,
    next_idx: u8,
    msg_id: u8,
    active: bool,
}

impl<const N: usize> Reassembler<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0u8; N],
            len: 0,
            next_idx: 0,
            msg_id: 0,
            active: false,
        }
    }

    /// Feeds one wire segment. Returns the completed frame on the final
    /// segment, `None` while more segments are expected, or an error (with
    /// the in-flight message dropped).
    pub fn push(&mut self, segment: &[u8]) -> Result<Option<&[u8]>, FragError> {
        if segment.len() < SEGMENT_HEADER_LEN {
            return Err(FragError::TruncatedHeader);
        }
        let msg_id = segment[0];
        let flags = segment[1];
        let payload = &segment[SEGMENT_HEADER_LEN..];
        let last = flags & LAST_FLAG != 0;
        let idx = flags & !LAST_FLAG;

        let in_order = self.active && msg_id == self.msg_id && idx == self.next_idx;
        if !in_order {
            // Idle, interleave, or gap. Only a fresh index-0 segment may
            // start a message; anything else drops and reports.
            if idx != 0 {
                self.active = false;
                return Err(FragError::OutOfOrder);
            }
            self.active = true;
            self.msg_id = msg_id;
            self.next_idx = 0;
            self.len = 0;
        }

        if self.len + payload.len() > N {
            self.active = false;
            return Err(FragError::BufferOverflow);
        }
        self.buf[self.len..self.len + payload.len()].copy_from_slice(payload);
        self.len += payload.len();
        self.next_idx = self.next_idx.wrapping_add(1);

        if last {
            let len = self.len;
            self.active = false;
            return Ok(Some(&self.buf[..len]));
        }
        Ok(None)
    }
}

impl<const N: usize> Default for Reassembler<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(msg_id: u8, idx: u8, last: bool, payload: &[u8]) -> Vec<u8> {
        let flags = if last { LAST_FLAG | idx } else { idx };
        let mut out = Vec::with_capacity(SEGMENT_HEADER_LEN + payload.len());
        out.push(msg_id);
        out.push(flags);
        out.extend_from_slice(payload);
        out
    }

    fn collect(frame: &[u8], mtu: usize, msg_id: u8) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for_each_segment(frame, mtu, msg_id, |header, payload| {
            let mut seg = Vec::with_capacity(SEGMENT_HEADER_LEN + payload.len());
            seg.extend_from_slice(header);
            seg.extend_from_slice(payload);
            out.push(seg);
        });
        out
    }

    #[test]
    fn small_frame_is_single_last_flagged_segment() {
        let segs = collect(b"hello", 2048, 7);
        assert_eq!(segs.len(), 1);
        assert_eq!(&segs[0][..2], &[7, LAST_FLAG]);
        assert_eq!(&segs[0][2..], b"hello");
    }

    #[test]
    fn empty_frame_emits_one_header_only_segment() {
        let segs = collect(b"", 2048, 1);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], vec![1, LAST_FLAG]);
    }

    #[test]
    fn exact_multiple_of_payload_cap_has_no_empty_tail() {
        let cap = 2048 - SEGMENT_HEADER_LEN;
        let frame = vec![0xA5u8; cap * 3];
        let segs = collect(&frame, 2048, 9);
        assert_eq!(segs.len(), 3);
        assert!(segs.iter().all(|s| s.len() <= 2048));
        assert_eq!(segs[2].len(), SEGMENT_HEADER_LEN + cap);
        assert_eq!(segs[2][1], LAST_FLAG | 2);
    }

    #[test]
    fn round_trip_over_caps() {
        for mtu in [2048usize, 768, 64, 3] {
            for len in [0usize, 1, mtu - SEGMENT_HEADER_LEN, mtu * 5, 5462] {
                if segment_count(len, mtu) > MAX_SEGMENTS {
                    continue;
                }
                let frame: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
                let segs = collect(&frame, mtu, 42);
                assert!(segs.iter().all(|s| s.len() <= mtu));
                assert_eq!(segment_count(len, mtu), segs.len());
                let mut re = Reassembler::<65536>::new();
                let mut completed = None;
                for (i, seg) in segs.iter().enumerate() {
                    let out = re.push(seg).expect("in-order stream must not error");
                    if let Some(f) = out {
                        assert_eq!(i + 1, segs.len(), "completion only on last segment");
                        completed = Some(f.to_vec());
                    }
                }
                assert_eq!(completed.expect("frame must complete"), frame);
            }
        }
    }

    #[test]
    fn reassembler_rejects_truncated_header() {
        let mut re = Reassembler::<64>::new();
        assert_eq!(re.push(&[0x01]), Err(FragError::TruncatedHeader));
    }

    #[test]
    fn sender_asserts_segment_count_bound() {
        // A frame needing more than MAX_SEGMENTS at the chosen mtu is a
        // caller error (7-bit index space cannot address more).
        let frame = vec![0u8; 5462];
        let result = std::panic::catch_unwind(|| {
            let mut sink = 0usize;
            for_each_segment(&frame, 3, 1, |_, p| sink += p.len());
            sink
        });
        assert!(result.is_err(), "mtu 3 for 5462 bytes must assert");
    }

    #[test]
    fn gap_drops_message_and_requires_fresh_start() {
        let mut re = Reassembler::<256>::new();
        assert_eq!(re.push(&segment(5, 0, false, b"ab")), Ok(None));
        // index jumps 0 -> 2: in-flight message dropped, error reported.
        assert_eq!(
            re.push(&segment(5, 2, false, b"cd")),
            Err(FragError::OutOfOrder)
        );
        // a mid-stream (non-zero) start is also rejected while idle.
        assert_eq!(
            re.push(&segment(5, 1, true, b"ef")),
            Err(FragError::OutOfOrder)
        );
        // only index 0 starts a new message.
        assert_eq!(re.push(&segment(5, 0, true, b"ok")), Ok(Some(&b"ok"[..])));
    }

    #[test]
    fn interleave_from_other_message_drops_in_flight() {
        let mut re = Reassembler::<256>::new();
        assert_eq!(re.push(&segment(1, 0, false, b"aa")), Ok(None));
        // a different msg_id arriving mid-message: index 0 restarts cleanly
        assert_eq!(re.push(&segment(2, 0, false, b"bb")), Ok(None));
        assert_eq!(re.push(&segment(2, 1, true, b"cc")), Ok(Some(&b"bbcc"[..])));
    }

    #[test]
    fn interleave_nonzero_from_other_message_is_out_of_order() {
        let mut re = Reassembler::<256>::new();
        assert_eq!(re.push(&segment(1, 0, false, b"aa")), Ok(None));
        assert_eq!(
            re.push(&segment(2, 3, true, b"bb")),
            Err(FragError::OutOfOrder)
        );
    }

    #[test]
    fn overflow_drops_message() {
        let mut re = Reassembler::<4>::new();
        assert_eq!(re.push(&segment(1, 0, false, b"abc")), Ok(None));
        assert_eq!(
            re.push(&segment(1, 1, true, b"de")),
            Err(FragError::BufferOverflow)
        );
        // recovered by a fresh message
        assert_eq!(re.push(&segment(1, 0, true, b"ok")), Ok(Some(&b"ok"[..])));
    }

    #[test]
    fn max_index_segment_is_accepted() {
        let mut re = Reassembler::<1024>::new();
        for idx in 0..MAX_SEGMENTS {
            let last = idx == MAX_SEGMENTS - 1;
            let seg = segment(3, idx as u8, last, &[idx as u8; 2]);
            let out = re.push(&seg).expect("127 segments must reassemble");
            if last {
                assert_eq!(out.map(|f| f.len()), Some(MAX_SEGMENTS * 2));
            }
        }
    }
}
