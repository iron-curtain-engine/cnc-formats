// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Unit tests for SHP (KeyFrame) sprite parsing and decoding.
use super::*;

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Builds a minimal SHP binary with the given raw LCW-keyframe data.
///
/// Each frame in `frames` is stored with the LCW format code (`0x80`) in the
/// offset-table entry.  The offset table has `(frame_count + 2)` entries of
/// 8 bytes each (matching the canonical format).
///
/// The `frames` slices must already be valid LCW-compressed data or raw
/// bytes that will not be decompressed (for pure parse-level tests).
pub(super) fn build_shp(
    width: u16,
    height: u16,
    flags: u16,
    frames: &[&[u8]],
    embedded_palette: Option<&[u8]>,
) -> Vec<u8> {
    let frame_count = frames.len() as u16;
    let largest = frames.iter().map(|f| f.len()).max().unwrap_or(0) as u16;

    // Header (14 bytes).
    let mut out = Vec::new();
    let push_u16 = |v: u16, buf: &mut Vec<u8>| buf.extend_from_slice(&v.to_le_bytes());
    push_u16(frame_count, &mut out);
    push_u16(0, &mut out); // x
    push_u16(0, &mut out); // y
    push_u16(width, &mut out);
    push_u16(height, &mut out);
    push_u16(largest, &mut out);
    push_u16(flags, &mut out);

    // Offset table: (frame_count + 2) × 8 bytes.
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let palette_size = if flags & 0x0001 != 0 { 768 } else { 0 };
    let data_start = (14 + offset_table_size + palette_size) as u32;

    // Write one 8-byte entry per frame: u32(format|offset) + u16(ref) + u16(ref_fmt).
    let mut cur = data_start;
    for frame in frames {
        // LCW keyframe: format byte 0x80, low 24 bits = file offset.
        let raw = ((ShpFrameFormat::Lcw as u32) << 24) | (cur & OFFSET_MASK);
        out.extend_from_slice(&raw.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // ref_offset
        out.extend_from_slice(&0u16.to_le_bytes()); // ref_format
        cur = cur.wrapping_add(frame.len() as u32);
    }
    // EOF sentinel entry: file offset = end of all frame data.
    let eof_raw = cur & OFFSET_MASK;
    out.extend_from_slice(&eof_raw.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // Zero-padding entry.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    // Optional palette.
    if let Some(pal) = embedded_palette {
        out.extend_from_slice(pal);
    }

    // Frame data.
    for frame in frames {
        out.extend_from_slice(frame);
    }

    out
}

// ── Basic functionality ──────────────────────────────────────────────

/// Input shorter than the 14-byte header returns `UnexpectedEof`.
///
/// Why: the first operation is reading the fixed-size header; if there
/// aren't enough bytes, the parser must fail immediately.
/// Both 0-byte and 13-byte inputs are tested.
#[test]
fn test_parse_too_short() {
    assert!(matches!(
        ShpFile::parse(&[]),
        Err(Error::UnexpectedEof { .. })
    ));
    assert!(matches!(
        ShpFile::parse(&[0u8; 13]),
        Err(Error::UnexpectedEof { .. })
    ));
}

/// Parse a zero-frame SHP (header + 2 extra offset entries, no frame data).
///
/// Why: boundary test for the smallest well-formed SHP.  The parser must
/// accept it and report 0 frames with correct header fields.
#[test]
fn test_parse_zero_frames() {
    let bytes = build_shp(8, 8, 0, &[], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 0);
    assert_eq!(shp.header.width, 8);
    assert_eq!(shp.header.height, 8);
    assert!(shp.embedded_palette.is_none());
}

/// Parse a single-frame LCW SHP and verify all header fields.
///
/// Why: core happy-path test — a single 2×2 frame whose LCW data is a
/// fill command.  We assert frame count, dimensions, palette flag,
/// format code, and exact decompressed pixel content.
///
/// How: The LCW stream `[0xFE, 0x04, 0x00, 0xAA, 0x80]` fills 4 bytes
/// with 0xAA then terminates.
#[test]
fn test_parse_single_frame() {
    // LCW fill of 4 bytes of 0xAA + end marker.
    let lcw: &[u8] = &[0xFEu8, 0x04, 0x00, 0xAA, 0x80];
    let bytes = build_shp(2, 2, 0, &[lcw], None);
    let shp = ShpFile::parse(&bytes).unwrap();

    assert_eq!(shp.frame_count(), 1);
    assert_eq!(shp.header.frame_count, 1);
    assert_eq!(shp.header.width, 2);
    assert_eq!(shp.header.height, 2);
    assert!(!shp.header.has_embedded_palette());
    assert_eq!(shp.frames[0].format, ShpFrameFormat::Lcw);
    assert_eq!(shp.frames[0].data, lcw);
}

/// `pixels()` on an LCW keyframe decompresses correctly.
///
/// Why: verifies the full pipeline from raw SHP bytes through
/// `ShpFrame::pixels()` to LCW decompression.
#[test]
fn test_frame_pixels_lcw() {
    // LCW fill 4 bytes of 0xBB + end.
    let lcw: &[u8] = &[0xFE, 0x04, 0x00, 0xBB, 0x80];
    let bytes = build_shp(2, 2, 0, &[lcw], None);
    let shp = ShpFile::parse(&bytes).unwrap();

    let out = shp.frames[0].pixels(4).unwrap();
    assert_eq!(out, vec![0xBBu8; 4]);
}

/// Multi-frame SHP: each frame's data is captured correctly.
///
/// Why: verifies that the 8-byte offset-table entries assign the right
/// byte ranges to each frame, even with multiple contiguous frames.
#[test]
fn test_parse_multiple_frames() {
    let f0: &[u8] = &[0xFE, 0x04, 0x00, 0xAA, 0x80];
    let f1: &[u8] = &[0xFE, 0x04, 0x00, 0xBB, 0x80];
    let f2: &[u8] = &[0xFE, 0x04, 0x00, 0xCC, 0x80];
    let bytes = build_shp(2, 2, 0, &[f0, f1, f2], None);
    let shp = ShpFile::parse(&bytes).unwrap();

    assert_eq!(shp.frame_count(), 3);
    assert_eq!(shp.frames[0].data, f0);
    assert_eq!(shp.frames[1].data, f1);
    assert_eq!(shp.frames[2].data, f2);
    // All are LCW keyframes.
    for frame in &shp.frames {
        assert_eq!(frame.format, ShpFrameFormat::Lcw);
    }
}

/// SHP with embedded palette (flags bit 0): palette bytes are captured.
///
/// Why: some SHP files carry a 768-byte palette after the offset table.
/// The parser must extract it and expose it through `embedded_palette`,
/// and the frame data must start 768 bytes past the nominal file offset.
#[test]
fn test_parse_embedded_palette() {
    let mut pal = vec![0u8; 768];
    pal[0] = 63; // red channel of color 0 = 63
    let lcw: &[u8] = &[0xFE, 0x04, 0x00, 0x01, 0x80];
    let bytes = build_shp(2, 2, 0x0001, &[lcw], Some(&pal));
    let shp = ShpFile::parse(&bytes).unwrap();

    assert!(shp.header.has_embedded_palette());
    let ep = shp.embedded_palette.as_ref().unwrap();
    assert_eq!(ep.len(), 768);
    assert_eq!(ep[0], 63);
}

/// `frame_pixel_count()` returns `width × height`.
///
/// Why: callers use this to allocate decompression output buffers.
/// A wrong value would cause LCW decompression to over- or under-fill.
#[test]
fn test_frame_pixel_count() {
    let lcw: &[u8] = &[0xFE, 0x80, 0x01, 0x00, 0x80]; // fill 384 with 0x00
    let bytes = build_shp(16, 24, 0, &[lcw], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_pixel_count(), 384);
}

// ── Determinism ──────────────────────────────────────────────────────

/// Parsing the same SHP bytes twice yields identical results.
///
/// Why: the parser is a pure function of its input; any hidden state
/// that leaked between calls would break reproducibility.
#[test]
fn parse_is_deterministic() {
    let lcw: &[u8] = &[0xFE, 0x04, 0x00, 0xDD, 0x80];
    let bytes = build_shp(2, 2, 0, &[lcw], None);
    let a = ShpFile::parse(&bytes).unwrap();
    let b = ShpFile::parse(&bytes).unwrap();
    assert_eq!(a, b);
}

// ── decode_frames ────────────────────────────────────────────────────────────

/// `decode_frames` on a single-frame LCW SHP returns correct pixels.
///
/// Why: verifies the simplest decode path — one keyframe, no XOR-delta.
#[test]
fn decode_frames_single_lcw() {
    // LCW fill 4 bytes of 0xAA + end.
    let lcw: &[u8] = &[0xFE, 0x04, 0x00, 0xAA, 0x80];
    let bytes = build_shp(2, 2, 0, &[lcw], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    let frames = shp.decode_frames().unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0], vec![0xAAu8; 4]);
}

/// `decode_frames` on multiple LCW keyframes returns each frame independently.
///
/// Why: all-keyframe SHP files exist (e.g. static overlays with multiple
/// variants).  Each frame should decompress independently.
#[test]
fn decode_frames_multi_lcw() {
    let f0: &[u8] = &[0xFE, 0x04, 0x00, 0xAA, 0x80];
    let f1: &[u8] = &[0xFE, 0x04, 0x00, 0xBB, 0x80];
    let bytes = build_shp(2, 2, 0, &[f0, f1], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    let frames = shp.decode_frames().unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0], vec![0xAAu8; 4]);
    assert_eq!(frames[1], vec![0xBBu8; 4]);
}

/// `decode_frames` on zero-frame SHP returns empty vec.
#[test]
fn decode_frames_zero_frames() {
    let bytes = build_shp(2, 2, 0, &[], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    let frames = shp.decode_frames().unwrap();
    assert!(frames.is_empty());
}

/// `decode_frames` correctly applies an XorPrev delta chain across two frames.
///
/// Why: XorPrev (format 0x20 / KF_DELTA) frames store a Format40-encoded
/// XOR-delta that is applied directly to the previous frame's pixels.  A bug
/// in the XOR application or frame ordering would silently corrupt every
/// non-keyframe in unit animations.
///
/// How: Frame 0 is an LCW keyframe with pixels [0xAA, 0xBB, 0xCC, 0xDD].
/// Frame 1 is an XorPrev delta (Format40 repeated-XOR: 4× 0x11).
/// After XOR: frame 1 pixels = [0xAA^0x11, 0xBB^0x11, 0xCC^0x11, 0xDD^0x11]
///                            = [0xBB, 0xAA, 0xDD, 0xCC].
#[test]
fn decode_frames_xor_delta_chain() {
    // LCW literal command: 0x84 = literal 4 bytes, then 0x80 = end marker.
    let lcw_frame0: &[u8] = &[0x84, 0xAA, 0xBB, 0xCC, 0xDD, 0x80];
    // Format40 repeated-XOR: 0x00 = cmd, 0x04 = count, 0x11 = value.
    let lcw_delta1: &[u8] = &[0x00, 0x04, 0x11];

    let width: u16 = 2;
    let height: u16 = 2;
    let frame_count: u16 = 2;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    // Header (14 bytes).
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    let largest = lcw_frame0.len().max(lcw_delta1.len()) as u16;
    bytes.extend_from_slice(&largest.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // Frame 0: LCW keyframe (format 0x80) at data_start.
    let raw0 = ((ShpFrameFormat::Lcw as u32) << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // ref_offset
    bytes.extend_from_slice(&0u16.to_le_bytes()); // ref_format

    // Frame 1: XorPrev delta (format 0x20) at data_start + len(frame0).
    let frame1_offset = data_start + lcw_frame0.len() as u32;
    let raw1 = ((ShpFrameFormat::XorPrev as u32) << 24) | (frame1_offset & OFFSET_MASK);
    bytes.extend_from_slice(&raw1.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // ref_offset
    bytes.extend_from_slice(&0u16.to_le_bytes()); // ref_format

    // EOF sentinel.
    let eof_offset = frame1_offset + lcw_delta1.len() as u32;
    let raw_eof = eof_offset & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    // Zero-padding entry.
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    // Frame data.
    bytes.extend_from_slice(lcw_frame0);
    bytes.extend_from_slice(lcw_delta1);

    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 2);
    assert_eq!(shp.frames[0].format, ShpFrameFormat::Lcw);
    assert_eq!(shp.frames[1].format, ShpFrameFormat::XorPrev);

    let frames = shp.decode_frames().unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0], vec![0xAA, 0xBB, 0xCC, 0xDD]);
    assert_eq!(frames[1], vec![0xBB, 0xAA, 0xDD, 0xCC]);
}

/// `decode_frames` correctly applies XorLcw deltas against the reference keyframe,
/// not the sequential previous frame.
///
/// Why: XorLcw (0x40 / KF_KEYDELTA) stores a Format40-encoded XOR-delta against
/// the decoded reference keyframe identified by `ref_offset`.  CLOCK.SHP from
/// LORES.MIX uses this pattern: odd frames are XorLcw, all referencing the same
/// base keyframe (frame 0), so frame 3's XorLcw should XOR against frame 0, not
/// frame 2.  Using the wrong (sequential) reference frame produces visually wrong
/// pixels.
///
/// How: Frame 0 is an LCW keyframe [0xAA, 0xBB, 0xCC, 0xDD].
/// Frame 1 is XorLcw (Format40 4× 0x11, ref=frame0) → [0xBB,0xAA,0xDD,0xCC].
/// Frame 2 is XorLcw (Format40 4× 0x44, ref=frame0) → [0xEE,0xFF,0x88,0x99].
/// If frame 2 incorrectly used frame 1 as its base, it would produce [0xFF,0xEE,0x99,0x88].
#[test]
fn decode_frames_xorlcw_uses_reference_keyframe() {
    // LCW literal: 0x84 = 4 literal bytes, 0x80 = end.
    let lcw_frame0: &[u8] = &[0x84, 0xAA, 0xBB, 0xCC, 0xDD, 0x80];
    // Format40 repeated-XOR deltas: 0x00 = cmd, count, value.
    let raw_delta1: &[u8] = &[0x00, 0x04, 0x11];
    let raw_delta2: &[u8] = &[0x00, 0x04, 0x44];

    let width: u16 = 2;
    let height: u16 = 2;
    let frame_count: u16 = 3;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    // Header.
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&(lcw_frame0.len() as u16).to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // Frame 0: Lcw at data_start.
    let raw0 = ((ShpFrameFormat::Lcw as u32) << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // ref_offset (unused)
    bytes.extend_from_slice(&0u16.to_le_bytes()); // ref_format

    // Frame 1: XorLcw, ref = frame 0 (data_start).
    let frame1_off = data_start + lcw_frame0.len() as u32;
    let raw1 = ((ShpFrameFormat::XorLcw as u32) << 24) | (frame1_off & OFFSET_MASK);
    bytes.extend_from_slice(&raw1.to_le_bytes());
    bytes.extend_from_slice(&(data_start as u16).to_le_bytes()); // ref_offset = frame0 file_offset
    bytes.extend_from_slice(&(ShpFrameFormat::Lcw as u16).to_le_bytes());

    // Frame 2: XorLcw, ref = frame 0 (data_start) — NOT frame 1.
    let frame2_off = frame1_off + raw_delta1.len() as u32;
    let raw2 = ((ShpFrameFormat::XorLcw as u32) << 24) | (frame2_off & OFFSET_MASK);
    bytes.extend_from_slice(&raw2.to_le_bytes());
    bytes.extend_from_slice(&(data_start as u16).to_le_bytes()); // ref_offset = frame0 file_offset
    bytes.extend_from_slice(&(ShpFrameFormat::Lcw as u16).to_le_bytes());

    // EOF sentinel.
    let eof_off = frame2_off + raw_delta2.len() as u32;
    bytes.extend_from_slice(&(eof_off & OFFSET_MASK).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    // Zero-padding entry.
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    // Frame data.
    bytes.extend_from_slice(lcw_frame0);
    bytes.extend_from_slice(raw_delta1);
    bytes.extend_from_slice(raw_delta2);

    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 3);
    assert_eq!(shp.frames[0].format, ShpFrameFormat::Lcw);
    assert_eq!(shp.frames[1].format, ShpFrameFormat::XorLcw);
    assert_eq!(shp.frames[2].format, ShpFrameFormat::XorLcw);

    let frames = shp.decode_frames().unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0], vec![0xAA, 0xBB, 0xCC, 0xDD]);
    // Frame 1: frame0 XOR [0x11,0x11,0x11,0x11]
    assert_eq!(frames[1], vec![0xBB, 0xAA, 0xDD, 0xCC]);
    // Frame 2: frame0 XOR [0x44,0x44,0x44,0x44]  ← uses frame0 as ref, NOT frame1
    assert_eq!(frames[2], vec![0xEE, 0xFF, 0x88, 0x99]);
}

/// `apply_xor_delta` correctly handles all Format40 command types.
///
/// Why: The Format40 decoder has several command branches (small skip, small
/// XOR from stream, repeated XOR, end marker).  A bug in any branch would
/// silently corrupt the pixels it touches.
///
/// How: Frame 0 = [0x10, 0x20, 0x30, 0x40, 0x50] (1×5 LCW keyframe).
/// Frame 1 XorPrev delta applies the following Format40 commands:
///   - 0x81          → small skip 1 pixel         (pixel 0 unchanged)
///   - 0x01 0xFF     → small XOR 1 byte 0xFF       (pixel 1: 0x20^0xFF=0xDF)
///   - 0x00 0x02 0x0F→ repeated XOR 2× value 0x0F  (pixel 2: 0x3F, pixel 3: 0x4F)
///   - 0x80 0x00 0x00→ end of stream               (pixel 4 unchanged)
///
/// Expected frame 1: [0x10, 0xDF, 0x3F, 0x4F, 0x50].
#[test]
fn decode_frames_format40_command_types() {
    // LCW literal: 0x85 = 5 literal bytes, 0x80 = LCW end.
    let lcw_frame0: &[u8] = &[0x85, 0x10, 0x20, 0x30, 0x40, 0x50, 0x80];
    // Format40 delta exercising: small skip, small XOR, repeated XOR, end marker.
    let fmt40_delta: &[u8] = &[
        0x81, // small skip 1
        0x01, 0xFF, // small XOR from stream: 1 byte → pixel 1 ^= 0xFF
        0x00, 0x02, 0x0F, // repeated XOR: 2 pixels with 0x0F
        0x80, 0x00, 0x00, // end of stream
    ];

    let width: u16 = 1;
    let height: u16 = 5;
    let frame_count: u16 = 2;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&(lcw_frame0.len() as u16).to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    let raw0 = ((ShpFrameFormat::Lcw as u32) << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    let frame1_off = data_start + lcw_frame0.len() as u32;
    let raw1 = ((ShpFrameFormat::XorPrev as u32) << 24) | (frame1_off & OFFSET_MASK);
    bytes.extend_from_slice(&raw1.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    let eof_off = frame1_off + fmt40_delta.len() as u32;
    bytes.extend_from_slice(&(eof_off & OFFSET_MASK).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]); // padding entry

    bytes.extend_from_slice(lcw_frame0);
    bytes.extend_from_slice(fmt40_delta);

    let shp = ShpFile::parse(&bytes).unwrap();
    let frames = shp.decode_frames().unwrap();
    assert_eq!(frames[0], vec![0x10, 0x20, 0x30, 0x40, 0x50]);
    assert_eq!(frames[1], vec![0x10, 0xDF, 0x3F, 0x4F, 0x50]);
}
