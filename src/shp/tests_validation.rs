// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::tests::build_shp;
use super::*;

#[test]
fn parse_sentinel_nonzero_ref_fields_accepted() {
    // RA1 game files (e.g. from REDALERT.MIX) carry non-zero ref_offset /
    // ref_format in the EOF-sentinel entry.  These fields are not meaningful
    // for a sentinel so the parser must accept them.
    let mut bytes = build_shp(2, 2, 0, &[&[0xFE, 0x04, 0x00, 0xAB, 0x80]], None);
    let sentinel_start = 14 + OFFSET_ENTRY_SIZE; // frame_count = 1
                                                 // ref_offset (bytes 4–5 of the entry)
    bytes[sentinel_start + 4] = 0x12;
    bytes[sentinel_start + 5] = 0x34;
    // ref_format (bytes 6–7 of the entry)
    bytes[sentinel_start + 6] = 0x56;
    bytes[sentinel_start + 7] = 0x78;
    assert!(ShpFile::parse(&bytes).is_ok());
}

#[test]
fn parse_sentinel_nonzero_format_byte_accepted() {
    // RA1 files like MOUSE.SHP and EDMOUSE.SHP have a non-zero format_byte
    // in the EOF sentinel that is not a valid frame code (0x20/0x40/0x80).
    // The parser must accept benign garbage values here.
    let mut bytes = build_shp(2, 2, 0, &[&[0xFE, 0x04, 0x00, 0xAB, 0x80]], None);
    let sentinel_start = 14 + OFFSET_ENTRY_SIZE;
    // byte 3 of the entry u32 is the format_byte (bits 24-31 of the LE u32).
    bytes[sentinel_start + 3] = 0x01; // non-zero but not a valid frame code
    assert!(ShpFile::parse(&bytes).is_ok());
}

#[test]
fn parse_sentinel_with_frame_code_rejected() {
    // If the sentinel's format_byte matches a valid frame code, we probably
    // miscounted frames — reject the file.
    let mut bytes = build_shp(2, 2, 0, &[&[0xFE, 0x04, 0x00, 0xAB, 0x80]], None);
    let sentinel_start = 14 + OFFSET_ENTRY_SIZE;
    bytes[sentinel_start + 3] = 0x80; // Lcw frame code
    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidMagic { .. })));
}

#[test]
fn parse_padding_nonzero_ref_fields_accepted() {
    // Same real-world scenario for the zero-padding entry that follows the
    // EOF sentinel.
    let mut bytes = build_shp(2, 2, 0, &[&[0xFE, 0x04, 0x00, 0xAB, 0x80]], None);
    let padding_start = 14 + 2 * OFFSET_ENTRY_SIZE; // frame_count + 1
    bytes[padding_start + 4] = 0xFF;
    bytes[padding_start + 5] = 0xFF;
    bytes[padding_start + 6] = 0xFF;
    bytes[padding_start + 7] = 0xFF;
    assert!(ShpFile::parse(&bytes).is_ok());
}

#[test]
fn parse_frame_offset_before_payload_rejected() {
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    let raw0 = (0x80u32 << 24) | ((data_start - 4) & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    let raw_eof = (data_start + 4) & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0xFE, 0x04, 0x00, 0xAB]);

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

#[test]
fn parse_padding_nonzero_file_offset_accepted() {
    // Real Westwood-authored files sometimes have garbage in the padding
    // entry's offset bytes.  We must accept non-zero file_offset here.
    let mut bytes = build_shp(2, 2, 0, &[&[0xFE, 0x04, 0x00, 0xAB, 0x80]], None);
    let padding_start = 14 + 2 * OFFSET_ENTRY_SIZE;
    // Set low 24 bits of the u32 (i.e. file_offset) to a non-zero value;
    // byte 3 (format_byte) stays 0x00 — not a valid frame code.
    bytes[padding_start] = 0x01;
    assert!(ShpFile::parse(&bytes).is_ok());
}

#[test]
fn parse_padding_entry_with_frame_code_rejected() {
    // If the padding entry's format_byte matches a valid frame code we
    // interpret this as a mis-parsed header and reject the file.
    let mut bytes = build_shp(2, 2, 0, &[&[0xFE, 0x04, 0x00, 0xAB, 0x80]], None);
    let padding_start = 14 + 2 * OFFSET_ENTRY_SIZE;
    // byte 3 of the entry u32 is the format_byte (bits 24-31 of the LE u32).
    bytes[padding_start + 3] = 0x80; // Lcw frame code
    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidMagic { .. })));
}

#[test]
fn eof_error_carries_header_byte_counts() {
    let err = ShpFile::parse(&[0u8; 10]).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 14);
            assert_eq!(available, 10);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

#[test]
fn eof_error_for_truncated_offset_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&5u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 10]);
    let err = ShpFile::parse(&bytes).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 70);
            assert_eq!(available, 24);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

#[test]
fn invalid_offset_carries_position_and_bound() {
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    let raw0 = (0x80u32 << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    let raw_eof = (data_start + 100) & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
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

#[test]
fn eof_display_contains_byte_counts() {
    let err = ShpFile::parse(&[0u8; 10]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("14"), "should mention needed bytes: {msg}");
    assert!(msg.contains("10"), "should mention available bytes: {msg}");
}

#[test]
fn parse_minimum_valid_shp_is_header_plus_extra_entries() {
    let bytes = build_shp(1, 1, 0, &[], None);
    assert_eq!(bytes.len(), 30);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 0);
}

#[test]
fn parse_exactly_14_bytes_with_nonzero_frames_fails() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 12]);
    assert_eq!(bytes.len(), 14);
    assert!(matches!(
        ShpFile::parse(&bytes),
        Err(Error::UnexpectedEof { needed: 38, .. })
    ));
}

#[test]
fn parse_near_max_offset_without_panic() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0xFFu8; 24]);
    bytes.extend_from_slice(&[0u8; 4]);
    let err = ShpFile::parse(&bytes).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidOffset { .. } | Error::InvalidMagic { .. }
    ));
}

#[test]
fn parse_zero_length_frame_succeeds() {
    let bytes = build_shp(1, 1, 0, &[&[]], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_count(), 1);
    assert!(shp.frames[0].data.is_empty());
}

#[test]
fn parse_reversed_offsets_rejected() {
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    let raw0 = (0x80u32 << 24) | ((data_start + 4) & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    let raw_eof = data_start & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0u8; 8]);

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

#[test]
fn parse_truncated_palette_rejected() {
    let frame_count: u16 = 0;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());

    let off = ((14 + offset_table_size + 768) as u32) & OFFSET_MASK;
    bytes.extend_from_slice(&off.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0u8; 100]);

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

#[test]
fn pixels_xor_delta_returns_error() {
    let frame = ShpFrame {
        data: &[0xFF, 0xFF, 0xFF],
        format: ShpFrameFormat::XorPrev,
        file_offset: 0,
        ref_offset: 0,
        ref_format: 0,
    };
    let result = frame.pixels(100);
    assert!(result.is_err());
}

#[test]
fn pixels_invalid_lcw_returns_error() {
    let frame = ShpFrame {
        data: &[0xFF, 0xFF, 0xFF],
        format: ShpFrameFormat::Lcw,
        file_offset: 0,
        ref_offset: 0,
        ref_format: 0,
    };
    let result = frame.pixels(100);
    assert!(result.is_err());
}

#[test]
fn frame_pixel_count_zero_dimensions() {
    let bytes = build_shp(0, 0, 0, &[], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_pixel_count(), 0);
}

/// Degenerate SHP files with width=0 / height=0 (like SIDEBAR.SHP from
/// RA1's HIRES.MIX) must decode without error, returning one empty
/// pixel buffer per frame instead of attempting LCW decompression with
/// a zero output cap.
#[test]
fn decode_frames_zero_dimensions_returns_empty_buffers() {
    // Build a valid zero-dimension SHP with no frame data.
    let bytes = build_shp(0, 0, 0, &[], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    assert_eq!(shp.frame_pixel_count(), 0);
    let frames = shp.decode_frames().unwrap();
    assert!(frames.is_empty(), "no frames in a zero-frame SHP");
}

/// Degenerate SHP files with width=0 / height=0 but non-zero frame count
/// must still decode without crashing (pixel buffers are all empty).
#[test]
fn decode_frames_zero_dimensions_with_frames_returns_empty_buffers() {
    // The frame pixel data is 0x80 (LCW end marker) which would fail with
    // max_output=0 before the fix.  After the fix the LCW path is bypassed.
    let bytes = build_shp(0, 0, 0, &[&[0x80u8]], None);
    let shp = ShpFile::parse(&bytes).unwrap();
    // frame_count may be 0 or 1 depending on whether build_shp with one
    // zero-byte-frame produces a parseable frame; either way decode_frames
    // must not return an error.
    assert!(shp.decode_frames().is_ok());
}

#[test]
fn parse_unknown_format_code_rejected() {
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    let raw0 = (0x10u32 << 24) | (data_start & OFFSET_MASK);
    bytes.extend_from_slice(&raw0.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    let raw_eof = (data_start + 4) & OFFSET_MASK;
    bytes.extend_from_slice(&raw_eof.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0xFE, 0x04, 0x00, 0xAA]);

    let result = ShpFile::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidMagic { .. })));
}

#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = ShpFile::parse(&data);
}

#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0u8; 256];
    let _ = ShpFile::parse(&data);
}
