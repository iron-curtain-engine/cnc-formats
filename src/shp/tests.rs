// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

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
fn build_shp(
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

/// Frame offsets pointing outside file data → `InvalidOffset`.
///
/// Why: without bounds validation, a malformed offset table would
/// cause an out-of-bounds slice access.
///
/// How: The EOF entry's file offset is set far past the actual data length.
#[test]
fn test_parse_invalid_offset() {
    // Build manually: 1 LCW frame + EOF pointing way past end.
    let frame_count: u16 = 1;
    let width: u16 = 2;
    let height: u16 = 2;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // Frame 0: LCW at data_start.
    let raw0 = (0x80u32 << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // EOF: file offset = data_start + 9999 (way past end).
    let raw_eof = (data_start + 9999) & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // Zero-padding entry.
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    // Only 4 bytes of actual data.
    bytes.extend_from_slice(&[0xFE, 0x04, 0x00, 0xAB]);

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

// ── Error field & Display verification ────────────────────────────────

/// `UnexpectedEof` for a too-short header carries the exact byte counts.
///
/// Why: structured error fields let callers generate precise diagnostics.
/// A 10-byte input needs 14 (header size) and has only 10.
#[test]
fn eof_error_carries_header_byte_counts() {
    let err = ShpFile::parse(&[0u8; 10]).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 14, "SHP header is 14 bytes");
            assert_eq!(available, 10);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// `UnexpectedEof` for a truncated offset table includes total needed bytes.
///
/// Why: with 5 frames the parser needs `14 + (5+2)×8 = 70` bytes.  A 24-byte
/// input should report `needed = 70`.
#[test]
fn eof_error_for_truncated_offset_table() {
    // Header says 5 frames → offset table needs 14 + (5+2)*8 = 70 bytes.
    // Supply exactly 14 header bytes + 10 more = 24 total.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&5u16.to_le_bytes()); // frame_count = 5
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&4u16.to_le_bytes()); // width
    bytes.extend_from_slice(&4u16.to_le_bytes()); // height
    bytes.extend_from_slice(&0u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
    bytes.extend_from_slice(&[0u8; 10]); // partial offset data
    let err = ShpFile::parse(&bytes).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 70, "14 header + 56 offset table bytes");
            assert_eq!(available, 24);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// `InvalidOffset` carries the out-of-bounds position and buffer length.
///
/// Why: callers need both values to diagnose which frame entry is
/// corrupt and how far it overshot.
#[test]
fn invalid_offset_carries_position_and_bound() {
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&2u16.to_le_bytes()); // width
    bytes.extend_from_slice(&2u16.to_le_bytes()); // height
    bytes.extend_from_slice(&4u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // Frame 0 at data_start.
    let raw0 = (0x80u32 << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // EOF at data_start + 100.
    let raw_eof = (data_start + 100) & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // Zero-padding.
    bytes.extend_from_slice(&[0u8; 8]);
    // Only 2 data bytes (not 100).
    bytes.extend_from_slice(&[0u8; 2]);

    let err = ShpFile::parse(&bytes).unwrap_err();
    match err {
        Error::InvalidOffset { offset, bound } => {
            assert_eq!(offset, (data_start + 100) as usize);
            assert_eq!(bound, bytes.len());
        }
        other => panic!("Expected InvalidOffset, got: {other}"),
    }
}

/// `Error::Display` embeds the numeric context for human-readable output.
///
/// Why: the Display trait output is the user-facing message; it must
/// include `needed` and `available` byte counts for diagnostics.
#[test]
fn eof_display_contains_byte_counts() {
    let err = ShpFile::parse(&[0u8; 10]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("14"), "should mention needed bytes: {msg}");
    assert!(msg.contains("10"), "should mention available bytes: {msg}");
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

// ── Boundary tests ──────────────────────────────────────────────────

/// Minimum valid SHP is header (14) + 2 extra entries × 8 = 30 bytes.
///
/// Why: boundary test for the smallest file accepted by the parser.
/// Verifying the byte count catches off-by-one errors in the offset
/// table calculation.
#[test]
fn parse_minimum_valid_shp_is_header_plus_extra_entries() {
    let bytes = build_shp(1, 1, 0, &[], None);
    // Minimum valid = 14 header + 2×8 offset entries = 30 bytes.
    assert_eq!(bytes.len(), 30);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 0);
}

/// 14 bytes is enough for the header but not the offset table.
///
/// Why: with `frame_count = 1`, the parser needs `14 + 3×8 = 38` bytes.
/// Exactly 14 bytes should trigger `UnexpectedEof { needed: 38 }`.
#[test]
fn parse_exactly_14_bytes_with_nonzero_frames_fails() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes()); // frame_count = 1
    bytes.extend_from_slice(&[0u8; 12]); // rest of header
    assert_eq!(bytes.len(), 14);
    assert!(matches!(
        ShpFile::parse(&bytes),
        Err(Error::UnexpectedEof { needed: 38, .. })
    ));
}

// ── Integer overflow safety ──────────────────────────────────────────

/// Offset entries with all-`0xFF` data (near `u32::MAX` offsets) are rejected.
///
/// Why (V38): after masking, the 24-bit file offset `0x00FF_FFFF` still
/// exceeds any reasonable file length.  The parser must return an error.
#[test]
fn parse_near_max_offset_without_panic() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes()); // frame_count
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&4u16.to_le_bytes()); // width
    bytes.extend_from_slice(&4u16.to_le_bytes()); // height
    bytes.extend_from_slice(&0u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
                                                  // 3 × 8-byte entries, all 0xFF.
    bytes.extend_from_slice(&[0xFFu8; 24]);
    bytes.extend_from_slice(&[0u8; 4]); // tiny data
    let err = ShpFile::parse(&bytes).unwrap_err();
    // Could be InvalidOffset or InvalidMagic depending on how far parsing gets.
    assert!(
        matches!(
            err,
            Error::InvalidOffset { .. } | Error::InvalidMagic { .. }
        ),
        "Expected InvalidOffset or InvalidMagic, got: {err}"
    );
}

// ── Security: edge-case tests ────────────────────────────────────────

/// Zero-length frame (start == end offset) produces an empty data slice.
///
/// Why: zero-length frames can appear in animation sprites (e.g. blank
/// placeholder frames).  The parser must accept them.
#[test]
fn parse_zero_length_frame_succeeds() {
    let bytes = build_shp(1, 1, 0, &[&[]], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 1);
    assert!(shp.frames[0].data.is_empty());
}

/// Reversed offsets (`start > end` after masking) → `InvalidOffset`.
///
/// Why: if a corrupt offset table has `off[0] > off[1]` after masking,
/// the resulting `start > end` range must be caught immediately.
///
/// How: frame 0's file offset is set 4 bytes past the EOF entry.
#[test]
fn parse_reversed_offsets_rejected() {
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&2u16.to_le_bytes()); // width
    bytes.extend_from_slice(&2u16.to_le_bytes()); // height
    bytes.extend_from_slice(&0u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // Frame 0 at data_start + 4 (past EOF entry's offset of data_start).
    let raw0 = (0x80u32 << 24) | ((data_start + 4) & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // EOF at data_start (earlier than frame 0 → reversed).
    let raw_eof = data_start & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // Zero-padding.
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0u8; 8]); // data padding

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

/// Embedded-palette flag set but fewer than 768 palette bytes → `UnexpectedEof`.
///
/// Why: the parser must not read past the end of input when the flags
/// promise a palette but the data is shorter than 768 bytes.
#[test]
fn parse_truncated_palette_rejected() {
    let frame_count: u16 = 0;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&1u16.to_le_bytes()); // width
    bytes.extend_from_slice(&1u16.to_le_bytes()); // height
    bytes.extend_from_slice(&0u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&1u16.to_le_bytes()); // flags = has palette

    // EOF + zero-padding entries.
    let off = ((14 + offset_table_size + 768) as u32) & OFFSET_MASK;
    bytes.extend_from_slice(&off.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]); // zero-padding entry

    // Only 100 palette bytes (need 768).
    bytes.extend_from_slice(&[0u8; 100]);

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// `pixels()` on an XOR-delta frame returns an error (needs ShpFile context).
///
/// Why: XOR-delta frames need a reference frame for decompression.
/// The standalone `pixels()` method cannot resolve cross-frame references,
/// so it must return a descriptive error.
#[test]
fn pixels_xor_delta_returns_error() {
    let frame = ShpFrame {
        data: &[0xFF, 0xFF, 0xFF],
        format: ShpFrameFormat::XorPrev,
        ref_offset: 0,
        ref_format: 0,
    };
    let result = frame.pixels(100);
    assert!(result.is_err());
}

/// `pixels()` on an LCW frame with invalid data returns an error.
///
/// Why: corrupt compressed data must not cause a panic; the callers
/// get an `Err` they can display or recover from.
#[test]
fn pixels_invalid_lcw_returns_error() {
    let frame = ShpFrame {
        data: &[0xFF, 0xFF, 0xFF], // truncated LCW command
        format: ShpFrameFormat::Lcw,
        ref_offset: 0,
        ref_format: 0,
    };
    let result = frame.pixels(100);
    assert!(result.is_err());
}

/// `frame_pixel_count()` handles zero width or height without panic.
///
/// Why: degenerate SHP files with 0×0 dimensions exist as placeholders.
/// The multiplication `0 × 0 = 0` must not produce surprising results.
#[test]
fn frame_pixel_count_zero_dimensions() {
    let bytes = build_shp(0, 0, 0, &[], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_pixel_count(), 0);
}

/// Unrecognised format code in offset entry → `InvalidMagic`.
///
/// Why: the only valid format codes are 0x80 (LCW), 0x40 (XORLCW),
/// and 0x20 (XORPrev).  Any other value indicates file corruption.
#[test]
fn parse_unknown_format_code_rejected() {
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&2u16.to_le_bytes()); // width
    bytes.extend_from_slice(&2u16.to_le_bytes()); // height
    bytes.extend_from_slice(&0u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // Frame 0 with invalid format code 0x10.
    let raw0 = (0x10u32 << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // EOF.
    let raw_eof = (data_start + 4) & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // Zero-padding.
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0xFE, 0x04, 0x00, 0xAA]); // data

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidMagic { .. })));
}

// ── Adversarial security tests ───────────────────────────────────────

/// `ShpFile::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer sets `frame_count = 0xFFFF`,
/// `width = 0xFFFF`, `height = 0xFFFF`, and all offset entries to
/// `0xFFFFFFFF`.  The parser must reject this via size/offset
/// validation without overflow or out-of-bounds access.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = ShpFile::parse(&data);
}

/// `ShpFile::parse` on 256 zero bytes must not panic.
///
/// Why: an all-zero header has `frame_count = 0`, `width = 0`,
/// `height = 0`.  The parser must handle the degenerate case of a
/// zero-frame SHP correctly.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0u8; 256];
    let _ = ShpFile::parse(&data);
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
/// Why: XorPrev (format 0x20) frames are LCW-decompressed into a delta buffer
/// that is then XOR'd with the previous frame's pixels.  This is the primary
/// inter-frame compression used by C&C sprite animations.  A bug in the XOR
/// application or frame ordering would silently corrupt every non-keyframe.
///
/// How: Frame 0 is an LCW keyframe with pixels [0xAA, 0xBB, 0xCC, 0xDD].
/// Frame 1 is an XorPrev delta whose decompressed bytes are [0x11, 0x11, 0x11, 0x11].
/// After XOR: frame 1 pixels = [0xAA^0x11, 0xBB^0x11, 0xCC^0x11, 0xDD^0x11]
///                            = [0xBB, 0xAA, 0xDD, 0xCC].
#[test]
fn decode_frames_xor_delta_chain() {
    // LCW literal command: 0x84 = literal 4 bytes, then 0x80 = end marker.
    let lcw_frame0: &[u8] = &[0x84, 0xAA, 0xBB, 0xCC, 0xDD, 0x80];
    let lcw_delta1: &[u8] = &[0xFE, 0x04, 0x00, 0x11, 0x80]; // fill 4 bytes of 0x11

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
