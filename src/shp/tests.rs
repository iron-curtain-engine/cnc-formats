// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;
use alloc::string::ToString;
use alloc::vec;

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

/// Parse a zero-frame SHP (header + 1 sentinel offset, no frame data).
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

/// Parse a single-frame uncompressed SHP and verify all header fields.
///
/// Why: core happy-path test — a single 8×8 frame whose pixel data is
/// a sequential ramp.  We assert frame count, dimensions, palette flag,
/// compression flag, and exact pixel content.
#[test]
fn test_parse_single_frame() {
    let pixels: Vec<u8> = (0u8..64).collect(); // 8×8
    let bytes = build_shp(8, 8, 0, &[&pixels], None);
    let shp = ShpFile::parse(&bytes).unwrap();

    assert_eq!(shp.frame_count(), 1);
    assert_eq!(shp.header.frame_count, 1);
    assert_eq!(shp.header.width, 8);
    assert_eq!(shp.header.height, 8);
    assert!(!shp.header.has_embedded_palette());
    assert!(shp.frames[0].is_uncompressed);
    assert_eq!(shp.frames[0].data, pixels);
}

/// `pixels()` on an uncompressed frame returns raw bytes unchanged.
///
/// Why: when the uncompressed flag is set, no LCW decompression should
/// occur; the frame data is returned as-is.  This tests that code path.
#[test]
fn test_frame_pixels_uncompressed() {
    let pixels: Vec<u8> = (0u8..16).collect(); // 4×4
    let bytes = build_shp(4, 4, 0, &[&pixels], None);
    let shp = ShpFile::parse(&bytes).unwrap();

    let out = shp.frames[0].pixels(16).unwrap();
    assert_eq!(out, pixels);
}

/// Multi-frame SHP: each frame's pixel content is captured correctly.
///
/// Why: verifies that the offset table assigns the right byte ranges
/// to each frame, even when multiple frames share contiguous storage.
#[test]
fn test_parse_multiple_frames() {
    let f0: Vec<u8> = vec![0xAAu8; 16];
    let f1: Vec<u8> = vec![0xBBu8; 16];
    let f2: Vec<u8> = vec![0xCCu8; 16];
    let bytes = build_shp(4, 4, 0, &[&f0, &f1, &f2], None);
    let shp = ShpFile::parse(&bytes).unwrap();

    assert_eq!(shp.frame_count(), 3);
    assert_eq!(shp.frames[0].data, f0);
    assert_eq!(shp.frames[1].data, f1);
    assert_eq!(shp.frames[2].data, f2);
}

/// SHP with embedded palette (flags bit 0): palette bytes are captured.
///
/// Why: some SHP files carry a 768-byte palette after the offset table.
/// The parser must extract it and expose it through `embedded_palette`.
#[test]
fn test_parse_embedded_palette() {
    let mut pal = vec![0u8; 768];
    pal[0] = 63; // red channel of color 0 = 63
    let pixels: Vec<u8> = vec![0u8; 4];
    let bytes = build_shp(2, 2, 0x0001, &[&pixels], Some(&pal));
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
    let bytes = build_shp(16, 24, 0, &[&vec![0u8; 384]], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_pixel_count(), 384);
}

/// LCW-compressed frame: `pixels()` decompresses correctly.
///
/// Why: exercises the full pipeline from raw SHP bytes through
/// `ShpFrame::pixels()` to LCW decompression.  The LCW stream is a
/// single long-fill command (`0xFE, 4, 0, 0xAB`) + end marker (`0x80`).
///
/// How: the SHP is built manually without the uncompressed flag so the
/// offset table entry lacks `OFFSET_UNCOMPRESSED_FLAG`.
#[test]
fn test_compressed_frame_pixels() {
    // Build a small LCW stream that decompresses to 4 bytes of 0xAB.
    // 0xFE = long fill, count=4, value=0xAB, 0x80 = end
    let lcw_data: Vec<u8> = vec![0xFEu8, 0x04, 0x00, 0xAB, 0x80];

    // Build SHP manually without the uncompressed flag.
    // We'll construct the byte stream directly.
    let frame_count: u16 = 1;
    let width: u16 = 2;
    let height: u16 = 2;
    let flags: u16 = 0;
    let offset_table_size = (frame_count as usize + 1) * 4;
    let data_start = 14 + offset_table_size;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&(lcw_data.len() as u16).to_le_bytes()); // largest
    bytes.extend_from_slice(&flags.to_le_bytes());

    // Offset table: offset[0] = data_start (no flag), offset[1] = sentinel
    let off0 = data_start as u32; // compressed: no OFFSET_UNCOMPRESSED_FLAG
    let off1 = (data_start + lcw_data.len()) as u32;
    bytes.extend_from_slice(&off0.to_le_bytes());
    bytes.extend_from_slice(&off1.to_le_bytes());

    // Frame data
    bytes.extend_from_slice(&lcw_data);

    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 1);
    assert!(!shp.frames[0].is_uncompressed);

    let pixels = shp.frames[0].pixels(4).unwrap();
    assert_eq!(pixels, vec![0xABu8; 4]);
}

/// Frame offsets pointing outside file data → `InvalidOffset`.
///
/// Why: without bounds validation, a malformed offset table would
/// cause an out-of-bounds slice access.
///
/// How: a 1-frame SHP is built manually with a sentinel offset 9999
/// bytes past the start, but only 4 bytes of actual data are appended.
#[test]
fn test_parse_invalid_offset() {
    // Manually build a 1-frame SHP with a sentinel offset past end of data.
    let frame_count: u16 = 1;
    let width: u16 = 2;
    let height: u16 = 2;
    let offset_table_size = 2 * 4; // (1+1) offsets
    let data_start = 14 + offset_table_size;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // Frame 0 starts at data_start (uncompressed), sentinel at data_start + 9999
    let off0 = (data_start as u32) | super::OFFSET_UNCOMPRESSED_FLAG;
    let off1 = ((data_start + 9999) as u32) | super::OFFSET_UNCOMPRESSED_FLAG;
    bytes.extend_from_slice(&off0.to_le_bytes());
    bytes.extend_from_slice(&off1.to_le_bytes());

    // Only append 4 bytes of actual data (far less than 9999)
    bytes.extend_from_slice(&[0u8; 4]);

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
/// Why: with 5 frames the parser needs `14 + 6×4 = 38` bytes.  A 24-byte
/// input should report `needed = 38`.
#[test]
fn eof_error_for_truncated_offset_table() {
    // Header says 5 frames → offset table needs 14 + (5+1)*4 = 38 bytes.
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
            assert_eq!(needed, 38, "14 header + 24 offset table bytes");
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
    let offset_table_size = 2 * 4;
    let data_start = 14 + offset_table_size;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&2u16.to_le_bytes()); // width
    bytes.extend_from_slice(&2u16.to_le_bytes()); // height
    bytes.extend_from_slice(&4u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    let off0 = (data_start as u32) | super::OFFSET_UNCOMPRESSED_FLAG;
    let off1 = ((data_start + 100) as u32) | super::OFFSET_UNCOMPRESSED_FLAG;
    bytes.extend_from_slice(&off0.to_le_bytes());
    bytes.extend_from_slice(&off1.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 2]); // only 2 data bytes, not 100

    let err = ShpFile::parse(&bytes).unwrap_err();
    match err {
        Error::InvalidOffset { offset, bound } => {
            assert_eq!(offset, data_start + 100);
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
    let pixels: Vec<u8> = (0u8..16).collect();
    let bytes = build_shp(4, 4, 0, &[&pixels], None);
    let a = ShpFile::parse(&bytes).unwrap();
    let b = ShpFile::parse(&bytes).unwrap();
    assert_eq!(a, b);
}

// ── Boundary tests ──────────────────────────────────────────────────

/// Minimum valid SHP is exactly 18 bytes: 14-byte header + 4-byte sentinel.
///
/// Why: boundary test for the smallest file accepted by the parser.
/// Verifying the byte count catches off-by-one errors in the offset
/// table calculation.
#[test]
fn parse_minimum_valid_shp_is_header_plus_sentinel() {
    let bytes = build_shp(1, 1, 0, &[], None);
    // Minimum valid = 14 header + 4 sentinel offset = 18 bytes
    assert_eq!(bytes.len(), 18);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 0);
}

/// 14 bytes is enough for the header but not the offset table.
///
/// Why: with `frame_count = 1`, the parser needs `14 + 2×4 = 22` bytes.
/// Exactly 14 bytes should trigger `UnexpectedEof { needed: 22 }`.
#[test]
fn parse_exactly_14_bytes_with_nonzero_frames_fails() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes()); // frame_count = 1
    bytes.extend_from_slice(&[0u8; 12]); // rest of header
    assert_eq!(bytes.len(), 14);
    assert!(matches!(
        ShpFile::parse(&bytes),
        Err(Error::UnexpectedEof { needed: 22, .. })
    ));
}

// ── Integer overflow safety ──────────────────────────────────────────

/// Offset entries near `u32::MAX` (after flag masking) are rejected.
///
/// Why (V38): masking off the uncompressed flag from `0xFFFF_FFFF`
/// yields `0x7FFF_FFFF`, which far exceeds any real file length.
/// The parser must return `InvalidOffset`, not panic.
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
                                                  // After masking OFFSET_UNCOMPRESSED_FLAG, these resolve to 0x7FFF_FFFF.
    bytes.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // off0
    bytes.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // sentinel
    bytes.extend_from_slice(&[0u8; 4]); // tiny data
    let err = ShpFile::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

// ── Security: edge-case tests ────────────────────────────────────────

/// `start == end` (zero-length frame) produces an empty data slice.
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
/// How: offset[0] is set 4 bytes past the sentinel, causing `start > end`.
#[test]
fn parse_reversed_offsets_rejected() {
    let frame_count: u16 = 1;
    let offset_table_size = 2 * 4;
    let data_start = 14 + offset_table_size;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&2u16.to_le_bytes()); // width
    bytes.extend_from_slice(&2u16.to_le_bytes()); // height
    bytes.extend_from_slice(&0u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags

    // off0 > sentinel (after masking) → start > end
    let off0 = ((data_start + 4) as u32) | OFFSET_UNCOMPRESSED_FLAG;
    let sentinel = (data_start as u32) | OFFSET_UNCOMPRESSED_FLAG;
    bytes.extend_from_slice(&off0.to_le_bytes());
    bytes.extend_from_slice(&sentinel.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]); // data

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
    let offset_table_size = 4; // 1 sentinel × 4 bytes

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // x
    bytes.extend_from_slice(&0u16.to_le_bytes()); // y
    bytes.extend_from_slice(&1u16.to_le_bytes()); // width
    bytes.extend_from_slice(&1u16.to_le_bytes()); // height
    bytes.extend_from_slice(&0u16.to_le_bytes()); // largest
    bytes.extend_from_slice(&1u16.to_le_bytes()); // flags = has palette

    // Sentinel offset
    let off = (14 + offset_table_size + 768) as u32;
    bytes.extend_from_slice(&(off | OFFSET_UNCOMPRESSED_FLAG).to_le_bytes());

    // Only 100 palette bytes (need 768)
    bytes.extend_from_slice(&[0u8; 100]);

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// `pixels()` on a compressed frame with invalid LCW data returns an error.
///
/// Why: corrupt compressed data must not cause a panic; the callers
/// get an `Err` they can display or recover from.
#[test]
fn pixels_invalid_lcw_returns_error() {
    // Build a frame manually with is_uncompressed = false and bad data
    let frame = ShpFrame {
        data: &[0xFF, 0xFF, 0xFF], // truncated LCW command
        is_uncompressed: false,
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
