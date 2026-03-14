// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Writes a little-endian `u16` at the given offset in a buffer.
fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

/// Builds a minimal valid FNT file matching the canonical 20-byte block-offset
/// header format (EA FONT.H + Vanilla-Conquer `FontHeader`).
///
/// Creates a font with `num_chars` entries.  Only glyph `0x41` ('A') has
/// non-zero width (`glyph_w`); all others have width 0.  The 'A' glyph
/// data is filled with `0x12` (pixel 0 = color 2, pixel 1 = color 1)
/// to test 4bpp nibble extraction.
///
/// ## Layout (matches EA file layout order)
/// ```text
/// [Header]         20 bytes  (offsets 0–19)
/// [Offset table]   num_chars × u16  (at OffsetBlockOffset)
/// [Width table]    num_chars × u8   (at WidthBlockOffset)
/// [Glyph data]     variable         (at DataBlockOffset)
/// [Height table]   num_chars × u16  (at HeightOffset)
/// ```
fn build_fnt(max_height: u8, glyph_w: u8, num_chars: u16) -> Vec<u8> {
    let nc = num_chars as usize;
    // 4bpp: ceil(width / 2) bytes per row.
    let bytes_per_row = (glyph_w as usize).div_ceil(2);
    let data_rows = max_height; // Simple: all rows stored.
    let glyph_size = bytes_per_row * (data_rows as usize);

    // Block layout after 20-byte header — matches EA file ordering:
    // offsets → widths → glyph data → heights.
    let offset_table_start = 20usize;
    let offset_table_size = nc * 2;
    let width_table_start = offset_table_start + offset_table_size;
    let width_table_size = nc;
    let data_area_start = width_table_start + width_table_size;
    let height_table_start = data_area_start + glyph_size;
    let height_table_size = nc * 2;
    let total = height_table_start + height_table_size;

    let mut buf = vec![0u8; total];

    // ── Header (20 bytes) ──
    write_u16_le(&mut buf, 0, total as u16); // FontLength
    buf[2] = 0; // FontCompress (uncompressed)
    buf[3] = 5; // FontDataBlocks
    write_u16_le(&mut buf, 4, 0x0010); // InfoBlockOffset (16)
    write_u16_le(&mut buf, 6, offset_table_start as u16); // OffsetBlockOffset
    write_u16_le(&mut buf, 8, width_table_start as u16); // WidthBlockOffset
    write_u16_le(&mut buf, 10, data_area_start as u16); // DataBlockOffset
    write_u16_le(&mut buf, 12, height_table_start as u16); // HeightOffset
    write_u16_le(&mut buf, 14, 0x1012); // UnknownConst
    buf[16] = 0; // Pad
    buf[17] = (num_chars - 1) as u8; // CharCount (last char index)
    buf[18] = max_height; // MaxHeight
    buf[19] = glyph_w; // MaxWidth

    // ── Width table: set glyph 0x41 ('A') width ──
    if nc > 0x41 {
        buf[width_table_start + 0x41] = glyph_w;
    }

    // ── Offset table: set glyph 0x41 offset to data_area_start ──
    if nc > 0x41 {
        let o_pos = offset_table_start + 0x41 * 2;
        write_u16_le(&mut buf, o_pos, data_area_start as u16);
    }

    // ── Height table: set all entries. ──
    // For glyph 0x41: y_offset=0, data_rows=max_height.
    // For all others: y_offset=0, data_rows=0 (no data).
    if nc > 0x41 {
        let h_pos = height_table_start + 0x41 * 2;
        // Low byte = y_offset (0), high byte = data_rows.
        write_u16_le(&mut buf, h_pos, (data_rows as u16) << 8);
    }

    // ── Glyph data: fill with 0x12 (low nibble=2, high nibble=1). ──
    for b in buf[data_area_start..data_area_start + glyph_size].iter_mut() {
        *b = 0x12;
    }

    buf
}

// ─── Basic functionality ─────────────────────────────────────────────────────

/// Parses a well-formed FNT file and checks header fields.
#[test]
fn parse_basic() {
    let data = build_fnt(8, 6, 256);
    let fnt = FntFile::parse(&data).unwrap();
    assert_eq!(fnt.header.max_height, 8);
    assert_eq!(fnt.header.max_width, 6);
    assert_eq!(fnt.header.compress, 0);
    assert_eq!(fnt.header.data_blocks, 5);
    assert_eq!(fnt.header.num_chars, 256);
    assert_eq!(fnt.glyphs.len(), 256);
}

/// Glyph 'A' (0x41) has the correct width and non-empty data.
#[test]
fn glyph_a_populated() {
    let data = build_fnt(8, 6, 256);
    let fnt = FntFile::parse(&data).unwrap();
    let glyph = &fnt.glyphs[0x41];
    assert_eq!(glyph.code, 0x41);
    assert_eq!(glyph.width, 6);
    assert_eq!(glyph.y_offset, 0);
    assert_eq!(glyph.data_rows, 8);
    // 4bpp: ceil(6/2) = 3 bytes per row, 8 rows = 24 bytes total.
    assert_eq!(glyph.data.len(), 24);
}

/// Zero-width glyphs have empty data.
#[test]
fn zero_width_glyph() {
    let data = build_fnt(8, 6, 256);
    let fnt = FntFile::parse(&data).unwrap();
    // Glyph 0x00 has width 0.
    assert_eq!(fnt.glyphs[0].width, 0);
    assert!(fnt.glyphs[0].data.is_empty());
}

/// Pixel query returns correct 4-bit color indices for 4bpp nibble-packed data.
///
/// How: the test data is filled with 0x12. In 4bpp nibble packing (low nibble
/// first), byte 0x12 → pixel 0 = 0x2, pixel 1 = 0x1.
#[test]
fn pixel_query_4bpp() {
    let data = build_fnt(8, 6, 256);
    let fnt = FntFile::parse(&data).unwrap();
    let glyph = &fnt.glyphs[0x41];
    // Byte 0x12: low nibble = 2, high nibble = 1.
    assert_eq!(glyph.pixel(0, 0), 2); // even x → low nibble
    assert_eq!(glyph.pixel(1, 0), 1); // odd x → high nibble
    assert_eq!(glyph.pixel(2, 0), 2); // even x → low nibble of next byte
                                      // Out of bounds returns 0 (transparent).
    assert_eq!(glyph.pixel(6, 0), 0);
    assert_eq!(glyph.pixel(0, 8), 0);
}

/// Pixel query on a zero-width glyph always returns 0 (transparent).
#[test]
fn pixel_query_zero_width() {
    let data = build_fnt(8, 6, 256);
    let fnt = FntFile::parse(&data).unwrap();
    assert_eq!(fnt.glyphs[0].pixel(0, 0), 0);
}

/// Font with smaller character count (e.g. 128) parses correctly.
///
/// Why: the format supports variable character counts via the CharCount
/// header field.  This verifies non-256 counts work.
#[test]
fn variable_char_count() {
    let data = build_fnt(8, 4, 128);
    let fnt = FntFile::parse(&data).unwrap();
    assert_eq!(fnt.header.num_chars, 128);
    assert_eq!(fnt.glyphs.len(), 128);
}

// ─── Error paths ─────────────────────────────────────────────────────────────

/// Truncated file (shorter than 20-byte header) returns UnexpectedEof.
#[test]
fn truncated_header() {
    let data = [0u8; 19];
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 20,
            available: 19
        }
    ));
}

/// Non-zero compression flag returns InvalidMagic.
///
/// Why: EA LOADFONT.CPP checks compress == 0.
#[test]
fn compressed_rejected() {
    let mut data = build_fnt(8, 6, 256);
    data[2] = 1; // FontCompress = 1
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidMagic { .. }));
}

/// Wrong data-block count returns InvalidMagic.
///
/// Why: EA LOADFONT.CPP checks data_blocks == 5.
#[test]
fn wrong_data_blocks_rejected() {
    let mut data = build_fnt(8, 6, 256);
    data[3] = 4; // FontDataBlocks = 4
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidMagic { .. }));
}

/// Glyph data pointing past EOF returns UnexpectedEof.
#[test]
fn glyph_data_past_eof() {
    let mut data = build_fnt(8, 6, 256);
    // Truncate the glyph data area.
    data.truncate(data.len() - 1);
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Width table truncated returns UnexpectedEof.
#[test]
fn truncated_width_table() {
    let mut data = build_fnt(8, 6, 256);
    // Point width_block_offset past end of file.
    let len_plus_1 = (data.len() + 1) as u16;
    write_u16_le(&mut data, 8, len_plus_1);
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ─── Determinism ─────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn deterministic() {
    let data = build_fnt(8, 6, 256);
    let a = FntFile::parse(&data).unwrap();
    let b = FntFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ─── Display messages ────────────────────────────────────────────────────────

/// Error Display includes numeric context.
#[test]
fn error_display_includes_values() {
    let data = [0u8; 10];
    let err = FntFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("20"));
    assert!(msg.contains("10"));
}

// ─── Integer overflow safety ─────────────────────────────────────────────────

/// Glyph width at exactly MAX_GLYPH_WIDTH (256) with sufficient data
/// succeeds.
///
/// Why: boundary complement — width 257 would be rejected.  Verifies the
/// exact cap value is accepted.  Since glyph width is u8, max is 255
/// which is always within the cap.
#[test]
fn at_max_glyph_width_accepted() {
    let data = build_fnt(8, 255, 256);
    let fnt = FntFile::parse(&data).unwrap();
    let glyph = fnt.glyphs.get(0x41).unwrap();
    assert_eq!(glyph.width, 255);
}

/// Glyph with large width × tall height uses saturating arithmetic and
/// does not panic.
#[test]
fn glyph_size_overflow_no_panic() {
    // Build a minimal header that declares large dimensions but the file
    // is too short. Parser must not panic on overflow.
    let mut data = vec![0u8; 40];
    // Valid header skeleton.
    write_u16_le(&mut data, 0, 40); // FontLength
    data[2] = 0; // compress
    data[3] = 5; // data_blocks
    write_u16_le(&mut data, 4, 0x0010); // info block
    write_u16_le(&mut data, 6, 20); // offset table at 20
    write_u16_le(&mut data, 8, 24); // width table at 24 (nc=2, so 2 entries)
    write_u16_le(&mut data, 10, 30); // data block
    write_u16_le(&mut data, 12, 26); // height table at 26
    write_u16_le(&mut data, 14, 0x1012);
    data[16] = 0; // pad
    data[17] = 1; // CharCount raw = 1 → num_chars = 2
    data[18] = 255; // MaxHeight = 255
    data[19] = 255; // MaxWidth = 255
                    // Width table: char 0 width = 255.
    data[24] = 255;
    // Height entry char 0: y_offset=0, data_rows=255.
    write_u16_le(&mut data, 26, 0xFF00);
    // Offset for char 0 pointing to data block start.
    write_u16_le(&mut data, 20, 30);
    // Glyph data size = ceil(255/2) * 255 = 128 * 255 = 32640 — way past 40 bytes.
    let err = FntFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ─── Security adversarial tests ──────────────────────────────────────────────

/// All-0xFF input must not panic — maximum field values in every position.
///
/// Why (V38): compress=0xFF and data_blocks=0xFF hit the validation checks
/// before any table parsing.  Ensures early rejection without panic.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = [0xFF; 2048];
    let _ = FntFile::parse(&data);
}

/// All-zero input at header size must not panic.
///
/// Why (V38): compress=0 and data_blocks=0 (≠5) should be rejected by
/// the data-blocks check without accessing any table data.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = [0u8; 20];
    let _ = FntFile::parse(&data);
}

/// Offset pointing into the header area must not cause misinterpretation.
///
/// Why: a crafted file could set glyph offsets to overlap with the header.
/// The parser should still succeed (offsets are addresses within the file,
/// not restricted to the data area).
#[test]
fn adversarial_offset_inside_header() {
    let mut data = build_fnt(8, 4, 256);
    // Point glyph 0x41 offset to byte 0 (inside the header).
    let nc = 256usize;
    let _ = nc; // used only for documentation clarity
    let offset_table_start = 20;
    let o_pos = offset_table_start + 0x41 * 2;
    write_u16_le(&mut data, o_pos, 0);
    // This is valid — the parser should not panic.
    let result = FntFile::parse(&data);
    assert!(result.is_ok());
}
