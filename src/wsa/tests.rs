// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Builds a minimal valid WSA file with the given number of frames.
///
/// Each frame's compressed data is a short byte sequence `[0x80, frame_idx]`
/// (arbitrary — we only test parsing here, not LCW decompression).
/// If `has_loop` is true, the last offset entry points to a dummy loop delta.
fn build_wsa(num_frames: u16, width: u16, height: u16, has_loop: bool) -> Vec<u8> {
    let frame_payload_size = 2usize; // 2 bytes per frame as dummy data
    let num_offsets = (num_frames as usize) + 2;
    let offsets_size = num_offsets * 4;
    let header_and_offsets = 14 + offsets_size;

    // Total frame data: num_frames frames of frame_payload_size each.
    // If has_loop, add one more for the loop delta.
    let data_payload =
        (num_frames as usize) * frame_payload_size + if has_loop { frame_payload_size } else { 0 };
    let total = header_and_offsets + data_payload;
    let mut buf = vec![0u8; total];

    // ── Header (14 bytes) ──
    buf[0..2].copy_from_slice(&num_frames.to_le_bytes());
    buf[2..4].copy_from_slice(&0u16.to_le_bytes()); // x
    buf[4..6].copy_from_slice(&0u16.to_le_bytes()); // y
    buf[6..8].copy_from_slice(&width.to_le_bytes());
    buf[8..10].copy_from_slice(&height.to_le_bytes());
    // largest_frame_size (u16) at offset 10 — dummy value.
    buf[10..12].copy_from_slice(&(frame_payload_size as u16).to_le_bytes());
    // flags (u16) at offset 12 — 0 = no embedded palette.
    buf[12..14].copy_from_slice(&0u16.to_le_bytes());

    // ── Offset table ──
    // Offsets are relative to the data area (after header + offset table).
    let offset_table_start = 14;
    for i in 0..=num_frames as usize {
        let rel_offset = i * frame_payload_size;
        let pos = offset_table_start + i * 4;
        buf[pos..pos + 4].copy_from_slice(&(rel_offset as u32).to_le_bytes());
    }

    // Loop delta offset.
    let loop_entry_pos = offset_table_start + (num_frames as usize + 1) * 4;
    if has_loop {
        let loop_rel = (num_frames as usize) * frame_payload_size;
        buf[loop_entry_pos..loop_entry_pos + 4].copy_from_slice(&(loop_rel as u32).to_le_bytes());
    } else {
        buf[loop_entry_pos..loop_entry_pos + 4].copy_from_slice(&0u32.to_le_bytes());
    }

    // ── Frame data ──
    for i in 0..num_frames as usize {
        let abs = header_and_offsets + i * frame_payload_size;
        buf[abs] = 0x80; // dummy
        buf[abs + 1] = i as u8;
    }
    if has_loop {
        let abs = header_and_offsets + (num_frames as usize) * frame_payload_size;
        buf[abs] = 0x80;
        buf[abs + 1] = 0xFF; // loop marker
    }

    buf
}

// ─── Basic functionality ─────────────────────────────────────────────────────

/// Parses a well-formed WSA with 3 frames and no loop.
#[test]
fn parse_basic() {
    let data = build_wsa(3, 320, 200, false);
    let wsa = WsaFile::parse(&data).unwrap();
    assert_eq!(wsa.header.num_frames, 3);
    assert_eq!(wsa.header.width, 320);
    assert_eq!(wsa.header.height, 200);
    assert_eq!(wsa.header.largest_frame_size, 2);
    assert_eq!(wsa.header.flags, 0);
    assert!(!wsa.header.has_embedded_palette());
    assert_eq!(wsa.frames.len(), 3);
    assert!(!wsa.has_loop_frame);
    // Each frame has 2 bytes of dummy data.
    assert_eq!(wsa.frames[0].data.len(), 2);
    assert_eq!(wsa.frames[0].data[0], 0x80);
    assert_eq!(wsa.frames[1].data[1], 1);
}

/// Parses a WSA with a loop frame.
#[test]
fn parse_with_loop() {
    let data = build_wsa(2, 64, 64, true);
    let wsa = WsaFile::parse(&data).unwrap();
    assert!(wsa.has_loop_frame);
    assert_eq!(wsa.frames.len(), 2);
}

/// Zero-frame WSA is accepted (valid but degenerate).
#[test]
fn parse_zero_frames() {
    let data = build_wsa(0, 320, 200, false);
    let wsa = WsaFile::parse(&data).unwrap();
    assert_eq!(wsa.header.num_frames, 0);
    assert_eq!(wsa.frames.len(), 0);
    assert!(!wsa.has_loop_frame);
}

/// Frame index is correctly assigned.
#[test]
fn frame_indices() {
    let data = build_wsa(4, 16, 16, false);
    let wsa = WsaFile::parse(&data).unwrap();
    for (i, f) in wsa.frames.iter().enumerate() {
        assert_eq!(f.index, i);
    }
}

// ─── Error paths ─────────────────────────────────────────────────────────────

/// Truncated header returns UnexpectedEof.
#[test]
fn truncated_header() {
    let data = [0u8; 13];
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 14,
            available: 13
        }
    ));
}

/// Truncated offset table returns UnexpectedEof.
#[test]
fn truncated_offset_table() {
    let mut data = build_wsa(5, 320, 200, false);
    // Need 14 + (5+2)*4 = 42 bytes for header+offsets; truncate to 30.
    data.truncate(30);
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Frame data truncated returns UnexpectedEof.
#[test]
fn truncated_frame_data() {
    let mut data = build_wsa(3, 320, 200, false);
    data.truncate(data.len() - 1);
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Frame count over V38 cap returns InvalidSize.
#[test]
fn over_max_frame_count() {
    // Can't construct a real file with 8193 frames — just test the header.
    let mut data = vec![0u8; 14];
    data[0..2].copy_from_slice(&8193u16.to_le_bytes());
    data[6..8].copy_from_slice(&1u16.to_le_bytes()); // width=1
    data[8..10].copy_from_slice(&1u16.to_le_bytes()); // height=1
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 8193,
            limit: 8192,
            ..
        }
    ));
}

/// Frame area over V38 cap returns InvalidSize.
#[test]
fn over_max_frame_area() {
    let mut data = vec![0u8; 14];
    data[0..2].copy_from_slice(&1u16.to_le_bytes()); // num_frames=1
    data[6..8].copy_from_slice(&0xFFFFu16.to_le_bytes()); // width=65535
    data[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes()); // height=65535
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

// ─── Determinism ─────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn deterministic() {
    let data = build_wsa(5, 320, 200, true);
    let a = WsaFile::parse(&data).unwrap();
    let b = WsaFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ─── Display messages ────────────────────────────────────────────────────────

/// Error Display output includes numeric context.
#[test]
fn error_display_includes_values() {
    let data = [0u8; 10];
    let err = WsaFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("14"));
    assert!(msg.contains("10"));
}

// ─── Integer overflow safety ─────────────────────────────────────────────────

/// WSA file with exactly MAX_FRAME_COUNT (8192) frames succeeds.
///
/// Why: boundary complement to `over_max_frame_count` (8193 rejected).
/// Verifies that the exact cap value is accepted.
#[test]
fn at_max_frame_count_accepted() {
    let data = build_wsa(8192, 1, 1, false);
    let wsa = WsaFile::parse(&data).unwrap();
    assert_eq!(wsa.header.num_frames, 8192);
}

/// Offset table size that would overflow uses saturating arithmetic and
/// returns an error instead of panicking.
#[test]
fn offsets_overflow_no_panic() {
    // Header claiming 0xFFFF frames needs (65535+2)*4 = 262148 bytes of
    // offsets — way more than 14 bytes of available data.
    let mut data = vec![0u8; 14];
    data[0..2].copy_from_slice(&0xFFFFu16.to_le_bytes());
    data[6..8].copy_from_slice(&1u16.to_le_bytes());
    data[8..10].copy_from_slice(&1u16.to_le_bytes());
    // Should be rejected either by MAX_FRAME_COUNT (8192) or by EOF check.
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize { .. } | Error::UnexpectedEof { .. }
    ));
}

// ─── Security adversarial tests ──────────────────────────────────────────────

/// All-0xFF input must not panic — exercises maximum field values.
///
/// Why: every parsed u16/u32 field becomes 0xFFFF/0xFFFFFFFF, the worst-case
/// for overflow.  The parser must reject gracefully.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = [0xFF; 256];
    let _ = WsaFile::parse(&data);
}

/// All-zero input must not panic.
///
/// Why: zero frame count, zero dimensions — must not trigger division by
/// zero or empty-slice panics.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = [0u8; 256];
    let _ = WsaFile::parse(&data);
}

/// Offset table with self-referencing offsets must not cause infinite loops.
///
/// Why: a crafted file where offset[0] == offset[1] creates a zero-length
/// frame, which is valid.  But offset[n] pointing backward could confuse
/// naïve parsers.
#[test]
fn adversarial_self_referencing_offsets() {
    let mut data = build_wsa(2, 8, 8, false);
    // Set all offsets to the same value — zero-length frames.
    let offset_val = (WSA_HEADER_SIZE + (2 + 2) * 4) as u32;
    for i in 0..4 {
        let pos = WSA_HEADER_SIZE + i * 4;
        data[pos..pos + 4].copy_from_slice(&offset_val.to_le_bytes());
    }
    let _ = WsaFile::parse(&data);
}

// ── decode_frames ────────────────────────────────────────────────────────────

/// Builds a WSA file with real LCW-compressed XOR-delta frames.
///
/// Frame 0: LCW fill of `pixel_count` bytes with value `0xAA`.
/// Frame 1: LCW fill of `pixel_count` bytes with XOR-delta `0x11` (so
///   frame 1 result = `0xAA ^ 0x11 = 0xBB`).
fn build_wsa_lcw(width: u16, height: u16) -> Vec<u8> {
    let pixel_count = (width as usize) * (height as usize);
    // LCW fill command: 0xFE, count_lo, count_hi, value, 0x80 (end).
    let lcw_frame0 = [
        0xFEu8,
        pixel_count as u8,
        (pixel_count >> 8) as u8,
        0xAA,
        0x80,
    ];
    let lcw_frame1 = [
        0xFEu8,
        pixel_count as u8,
        (pixel_count >> 8) as u8,
        0x11,
        0x80,
    ];

    let num_frames: u16 = 2;
    let num_offsets = (num_frames as usize) + 2;
    let offsets_size = num_offsets * 4;
    let header_and_offsets = 14 + offsets_size;

    let total = header_and_offsets + lcw_frame0.len() + lcw_frame1.len();
    let mut buf = vec![0u8; total];

    // Header.
    buf[0..2].copy_from_slice(&num_frames.to_le_bytes());
    buf[6..8].copy_from_slice(&width.to_le_bytes());
    buf[8..10].copy_from_slice(&height.to_le_bytes());

    // Offset table (relative to data base = header_and_offsets).
    let off0 = 0u32;
    let off1 = lcw_frame0.len() as u32;
    let off_sentinel = off1 + lcw_frame1.len() as u32;
    let ot = 14;
    buf[ot..ot + 4].copy_from_slice(&off0.to_le_bytes());
    buf[ot + 4..ot + 8].copy_from_slice(&off1.to_le_bytes());
    buf[ot + 8..ot + 12].copy_from_slice(&off_sentinel.to_le_bytes());
    // Loop offset = 0 (no loop).

    // Frame data.
    buf[header_and_offsets..header_and_offsets + lcw_frame0.len()].copy_from_slice(&lcw_frame0);
    buf[header_and_offsets + lcw_frame0.len()..].copy_from_slice(&lcw_frame1);

    buf
}

/// `decode_frames` decodes XOR-delta chain correctly.
///
/// Why: WSA frames are XOR-deltas from a zero canvas.  Frame 0 fills
/// with 0xAA, frame 1's delta of 0x11 XOR'd onto 0xAA gives 0xBB.
#[test]
fn decode_frames_xor_delta_chain() {
    let data = build_wsa_lcw(2, 2);
    let wsa = WsaFile::parse(&data).unwrap();
    let frames = wsa.decode_frames().unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0], vec![0xAAu8; 4]);
    assert_eq!(frames[1], vec![0xBBu8; 4]);
}

/// `decode_frames` on a zero-frame WSA returns empty vec.
#[test]
fn decode_frames_empty() {
    let data = build_wsa(0, 4, 4, false);
    let wsa = WsaFile::parse(&data).unwrap();
    let frames = wsa.decode_frames().unwrap();
    assert!(frames.is_empty());
}

/// WSA with a loop frame has `has_loop_frame` set and loop delta data is present.
///
/// Why: the loop-back delta (at offsets[num_frames]..offsets[num_frames+1])
/// allows seamless animation looping.  If `offsets[num_frames+1] != 0` the
/// parser must report `has_loop_frame = true`.  A bug that misread the
/// sentinel offset or off-by-one in the offset table index would silently
/// drop loop support.
///
/// How: builds a 1-frame WSA with real LCW data and an additional loop delta
/// occupying space after the normal frame.  Verifies `has_loop_frame` is
/// true, the normal frame decodes correctly, and the file parses without
/// error despite the extra data region.
#[test]
fn parse_loop_frame_present() {
    let width: u16 = 2;
    let height: u16 = 2;
    let pixel_count = (width as usize) * (height as usize);

    // LCW fill command: 0xFE, count_lo, count_hi, value, 0x80 (end).
    let lcw_frame0 = [
        0xFEu8,
        pixel_count as u8,
        (pixel_count >> 8) as u8,
        0xAA,
        0x80,
    ];
    // Loop delta: LCW fill with 0x55 (XOR'd onto frame 0 would give 0xFF).
    let lcw_loop = [
        0xFEu8,
        pixel_count as u8,
        (pixel_count >> 8) as u8,
        0x55,
        0x80,
    ];

    let num_frames: u16 = 1;
    let num_offsets = (num_frames as usize) + 2;
    let offsets_size = num_offsets * 4;
    let header_and_offsets = WSA_HEADER_SIZE + offsets_size;
    let total = header_and_offsets + lcw_frame0.len() + lcw_loop.len();
    let mut buf = vec![0u8; total];

    // Header (14 bytes).
    buf[0..2].copy_from_slice(&num_frames.to_le_bytes());
    buf[6..8].copy_from_slice(&width.to_le_bytes());
    buf[8..10].copy_from_slice(&height.to_le_bytes());

    // Offset table (relative to data base = header_and_offsets).
    // offsets[0] = 0 (frame 0 start)
    // offsets[1] = len(frame0) (loop delta start, also bounds frame 0)
    // offsets[2] = len(frame0) + len(loop) (end-of-data sentinel, non-zero → has loop)
    let ot = WSA_HEADER_SIZE;
    let off0 = 0u32;
    let off1 = lcw_frame0.len() as u32;
    let off2 = off1 + lcw_loop.len() as u32;
    buf[ot..ot + 4].copy_from_slice(&off0.to_le_bytes());
    buf[ot + 4..ot + 8].copy_from_slice(&off1.to_le_bytes());
    buf[ot + 8..ot + 12].copy_from_slice(&off2.to_le_bytes());

    // Frame data.
    buf[header_and_offsets..header_and_offsets + lcw_frame0.len()].copy_from_slice(&lcw_frame0);
    buf[header_and_offsets + lcw_frame0.len()..].copy_from_slice(&lcw_loop);

    let wsa = WsaFile::parse(&buf).unwrap();
    assert!(wsa.has_loop_frame, "loop frame sentinel is non-zero");
    assert_eq!(wsa.frames.len(), 1);

    // Verify the normal frame decodes correctly.
    let decoded = wsa.decode_frames().unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0], vec![0xAAu8; pixel_count]);
}

// ── WSA encoder round-trip tests ────────────────────────────────────

/// Encoding 2 frames then parsing and decoding recovers the original pixels.
///
/// Why: the encoder must produce valid LCW-compressed XOR-deltas that the
/// parser can read and `decode_frames` can decompress.  Two frames exercise
/// the delta-chain: frame 0 is XOR'd against a zero canvas, frame 1 is
/// XOR'd against frame 0.
#[test]
fn encode_frames_round_trip() {
    let frame0 = vec![0xAAu8; 4 * 4];
    let mut frame1 = vec![0xBBu8; 4 * 4];
    // Make the frames distinct to exercise a non-trivial delta.
    frame1[0] = 0xCC;

    let frames: Vec<&[u8]> = vec![&frame0, &frame1];
    let encoded = encode_frames(&frames, 4, 4).unwrap();

    let wsa = WsaFile::parse(&encoded).unwrap();
    assert_eq!(wsa.header.num_frames, 2);
    assert_eq!(wsa.header.width, 4);
    assert_eq!(wsa.header.height, 4);

    let decoded = wsa.decode_frames().unwrap();
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0], frame0);
    assert_eq!(decoded[1], frame1);
}

/// Encoding a single frame then parsing and decoding recovers the pixels.
///
/// Why: a single frame is the minimum non-empty WSA.  Its delta is simply
/// XOR against the zero canvas (i.e., the raw pixel data itself).
#[test]
fn encode_frames_single_frame() {
    let frame = vec![0x42u8; 4 * 4];
    let frames: Vec<&[u8]> = vec![&frame];

    let encoded = encode_frames(&frames, 4, 4).unwrap();

    let wsa = WsaFile::parse(&encoded).unwrap();
    assert_eq!(wsa.header.num_frames, 1);

    let decoded = wsa.decode_frames().unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0], frame);
}

/// Encoding zero frames produces a valid empty WSA file.
///
/// Why: a zero-frame WSA is degenerate but valid.  The encoder must emit
/// a header and offset table that the parser accepts.
#[test]
fn encode_frames_empty() {
    let frames: Vec<&[u8]> = vec![];

    let encoded = encode_frames(&frames, 4, 4).unwrap();

    let wsa = WsaFile::parse(&encoded).unwrap();
    assert_eq!(wsa.header.num_frames, 0);
    assert_eq!(wsa.frames.len(), 0);
}
