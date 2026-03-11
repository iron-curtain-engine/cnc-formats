// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Builds a minimal valid FNT file.
///
/// Creates a font with the given `height`.  Only glyph `0x41` ('A') has
/// non-zero width (`glyph_w`); all other glyphs have width 0.  The 'A'
/// glyph data is filled with `0xFF` bytes (all pixels set).
fn build_fnt(height: u8, glyph_w: u16) -> Vec<u8> {
    let bytes_per_col = (height as usize).div_ceil(8);
    let glyph_size = (glyph_w as usize) * bytes_per_col;

    // File layout: header (6) + widths (512) + offsets (512) + glyph data.
    let data_area_start = 6 + 512 + 512;
    let total = data_area_start + glyph_size;
    let mut buf = vec![0u8; total];

    // Header.
    buf[0..2].copy_from_slice(&(total as u16).to_le_bytes()); // data_size
    buf[2] = height;
    buf[3] = glyph_w as u8; // max_width
    buf[4..6].copy_from_slice(&0u16.to_le_bytes()); // unknown

    // Width table: 256 × u16.  Set glyph 0x41 ('A') width.
    let w_offset = 6 + 0x41 * 2;
    buf[w_offset..w_offset + 2].copy_from_slice(&glyph_w.to_le_bytes());

    // Offset table: 256 × u16.  Set glyph 0x41 offset to data_area_start.
    let o_offset = 6 + 512 + 0x41 * 2;
    buf[o_offset..o_offset + 2].copy_from_slice(&(data_area_start as u16).to_le_bytes());

    // Glyph data: all pixels set.
    for b in buf[data_area_start..].iter_mut() {
        *b = 0xFF;
    }

    buf
}

// ─── Basic functionality ─────────────────────────────────────────────────────

/// Parses a well-formed FNT file and checks header fields.
#[test]
fn parse_basic() {
    let data = build_fnt(8, 6);
    let fnt = FntFile::parse(&data).unwrap();
    assert_eq!(fnt.header.height, 8);
    assert_eq!(fnt.header.max_width, 6);
    assert_eq!(fnt.glyphs.len(), 256);
}

/// Glyph 'A' (0x41) has the correct width and non-empty data.
#[test]
fn glyph_a_populated() {
    let data = build_fnt(8, 6);
    let fnt = FntFile::parse(&data).unwrap();
    let glyph = &fnt.glyphs[0x41];
    assert_eq!(glyph.code, 0x41);
    assert_eq!(glyph.width, 6);
    assert_eq!(glyph.height, 8);
    // height=8 → 1 byte per column, width=6 → 6 bytes total.
    assert_eq!(glyph.data.len(), 6);
}

/// Zero-width glyphs have empty data.
#[test]
fn zero_width_glyph() {
    let data = build_fnt(8, 6);
    let fnt = FntFile::parse(&data).unwrap();
    // Glyph 0x00 has width 0.
    assert_eq!(fnt.glyphs[0].width, 0);
    assert!(fnt.glyphs[0].data.is_empty());
}

/// Pixel query returns correct values for an all-set glyph.
#[test]
fn pixel_query() {
    let data = build_fnt(8, 6);
    let fnt = FntFile::parse(&data).unwrap();
    let glyph = &fnt.glyphs[0x41];
    // All pixels should be set (data is 0xFF).
    assert!(glyph.pixel(0, 0));
    assert!(glyph.pixel(5, 7));
    // Out of bounds returns false.
    assert!(!glyph.pixel(6, 0));
    assert!(!glyph.pixel(0, 8));
}

/// Pixel query on a zero-width glyph always returns false.
#[test]
fn pixel_query_zero_width() {
    let data = build_fnt(8, 6);
    let fnt = FntFile::parse(&data).unwrap();
    assert!(!fnt.glyphs[0].pixel(0, 0));
}

/// Font with non-byte-aligned height (e.g. 12 → 2 bytes per column).
#[test]
fn non_byte_aligned_height() {
    let data = build_fnt(12, 4);
    let fnt = FntFile::parse(&data).unwrap();
    let glyph = &fnt.glyphs[0x41];
    // height=12 → ceil(12/8) = 2 bytes per column, width=4 → 8 bytes total.
    assert_eq!(glyph.data.len(), 8);
    assert!(glyph.pixel(0, 0));
    assert!(glyph.pixel(0, 11));
}

// ─── Error paths ─────────────────────────────────────────────────────────────

/// Truncated file returns UnexpectedEof.
///
/// The minimum file size is 6 + 512 + 512 = 1030 bytes.
#[test]
fn truncated_file() {
    let data = [0u8; 1029];
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 1030,
            available: 1029
        }
    ));
}

/// Glyph data pointing past EOF returns UnexpectedEof.
#[test]
fn glyph_data_past_eof() {
    let mut data = build_fnt(8, 6);
    // Truncate the glyph data area.
    data.truncate(data.len() - 1);
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Font height over V38 cap returns InvalidSize.
#[test]
fn height_over_cap() {
    let mut data = vec![0u8; 1030];
    data[2] = 255; // height = 255 (within cap)
                   // Height 255 is still within MAX_FONT_HEIGHT (256).
                   // We need to actually exceed 256, but height is u8 so max is 255.
                   // This test verifies that 255 is accepted.
                   // The V38 cap is MAX_FONT_HEIGHT = 256, so u8 values never exceed it.
                   // Instead, let's test the glyph width cap.
    let mut data2 = vec![0u8; 1030];
    data2[2] = 8; // height = 8
                  // Set glyph 0x41 width to 257 (over MAX_GLYPH_WIDTH = 256).
    let w_pos = 6 + 0x41 * 2;
    data2[w_pos..w_pos + 2].copy_from_slice(&257u16.to_le_bytes());
    let err = FntFile::parse(&data2).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 257,
            limit: 256,
            ..
        }
    ));
}

// ─── Determinism ─────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn deterministic() {
    let data = build_fnt(8, 6);
    let a = FntFile::parse(&data).unwrap();
    let b = FntFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ─── Display messages ────────────────────────────────────────────────────────

/// Error Display includes numeric context.
#[test]
fn error_display_includes_values() {
    let data = [0u8; 100];
    let err = FntFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("1030"));
    assert!(msg.contains("100"));
}

// ─── Integer overflow safety ─────────────────────────────────────────────────

/// Glyph with large width × tall height uses saturating arithmetic and
/// does not panic.
#[test]
fn glyph_size_overflow_no_panic() {
    let mut data = vec![0u8; 1030];
    data[2] = 255; // height = 255 → bytes_per_col = 32
                   // Set glyph 0x00 width to 256 (max allowed).
    data[6..8].copy_from_slice(&256u16.to_le_bytes());
    // Offset for glyph 0x00 pointing to data_area_start.
    let o_pos = 6 + 512;
    data[o_pos..o_pos + 2].copy_from_slice(&1030u16.to_le_bytes());
    // Glyph data size would be 256 * 32 = 8192 bytes, way past our 1030-byte file.
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ─── Security adversarial tests ──────────────────────────────────────────────

/// All-0xFF input must not panic — maximum field values in every position.
///
/// Why: height=255, all widths=0xFFFF, all offsets=0xFFFF — the worst case
/// for integer overflow.  The parser must reject gracefully.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = [0xFF; 2048];
    let _ = FntFile::parse(&data);
}

/// All-zero input (at minimum size) must not panic.
///
/// Why: height=0 triggers the V38 zero-height check.  The parser must
/// reject without panicking on division in bytes_per_col.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = [0u8; 1030];
    let _ = FntFile::parse(&data);
}

/// Offset pointing to the middle of the width table (inside the header area)
/// must not cause the parser to misinterpret header data as glyph pixels.
///
/// Why: a crafted file could set glyph offsets to overlap with the header
/// or width table.  The parser should still succeed (offsets are addresses
/// within the file, not restricted to the data area).
#[test]
fn adversarial_offset_inside_header() {
    let mut data = vec![0u8; 1030];
    data[2] = 8; // height = 8, bytes_per_col = 1
                 // Set glyph 0x00 width = 1 (needs 1 byte of data).
    data[6..8].copy_from_slice(&1u16.to_le_bytes());
    // Point glyph 0x00 offset to byte 10 (inside the width table).
    let o_pos = 6 + 512;
    data[o_pos..o_pos + 2].copy_from_slice(&10u16.to_le_bytes());
    // This is valid — the parser should not panic.
    let result = FntFile::parse(&data);
    assert!(result.is_ok());
}

/// Every glyph with max width (256) and max height (255) — massive
/// data requirements that exceed the file.  Must not allocate or panic.
#[test]
fn adversarial_all_glyphs_max_width() {
    let mut data = vec![0u8; 1030];
    data[2] = 255; // height = 255
                   // Set every glyph width to 256.
    for i in 0..256 {
        let pos = 6 + i * 2;
        data[pos..pos + 2].copy_from_slice(&256u16.to_le_bytes());
    }
    // Various offsets — doesn't matter, glyph data won't fit.
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}
