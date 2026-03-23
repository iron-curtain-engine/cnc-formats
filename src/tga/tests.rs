// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Builds a minimal TGA file with no image ID, no color map, and the
/// given pixel data appended directly after the 18-byte header.
fn build_tga(
    width: u16,
    height: u16,
    image_type: u8,
    pixel_depth: u8,
    pixel_data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0); // id_length
    out.push(0); // color_map_type
    out.push(image_type);
    out.extend_from_slice(&0u16.to_le_bytes()); // color_map_first
    out.extend_from_slice(&0u16.to_le_bytes()); // color_map_length
    out.push(0); // color_map_entry_size
    out.extend_from_slice(&0u16.to_le_bytes()); // x_origin
    out.extend_from_slice(&0u16.to_le_bytes()); // y_origin
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.push(pixel_depth);
    out.push(0); // image_descriptor
    out.extend_from_slice(pixel_data);
    out
}

/// Appends a valid TGA 2.0 footer (26 bytes) to the given buffer.
fn build_tga_with_footer(base: &mut Vec<u8>) {
    base.extend_from_slice(&0u32.to_le_bytes()); // extension_offset
    base.extend_from_slice(&0u32.to_le_bytes()); // developer_offset
    base.extend_from_slice(b"TRUEVISION-XFILE.\0");
}

// ── Parsing valid files ──────────────────────────────────────────────────────

/// A 4x4 24-bit true-color image parses correctly.
///
/// Why: exercises the most common TGA variant (uncompressed true-color)
/// and verifies that dimensions, pixel depth, and data length are correct.
#[test]
fn parse_valid_truecolor() {
    let pixels = vec![0xABu8; 4 * 4 * 3]; // 4×4 × 3 bytes per pixel
    let data = build_tga(4, 4, 2, 24, &pixels);
    let tga = TgaFile::parse(&data).unwrap();

    assert_eq!(tga.header.width, 4);
    assert_eq!(tga.header.height, 4);
    assert_eq!(tga.header.pixel_depth, 24);
    assert_eq!(tga.header.image_type, TgaImageType::TrueColor);
    assert_eq!(tga.image_data().len(), 48);
    assert!(!tga.is_rle());
    assert!(!tga.has_color_map());
    assert!(!tga.has_footer());
}

/// A 2x2 8-bit grayscale image parses correctly.
///
/// Why: grayscale is the simplest pixel format (1 byte per pixel).
#[test]
fn parse_valid_grayscale() {
    let pixels = vec![128u8; 2 * 2]; // 2×2 × 1 byte per pixel
    let data = build_tga(2, 2, 3, 8, &pixels);
    let tga = TgaFile::parse(&data).unwrap();

    assert_eq!(tga.header.width, 2);
    assert_eq!(tga.header.height, 2);
    assert_eq!(tga.header.pixel_depth, 8);
    assert_eq!(tga.header.image_type, TgaImageType::Grayscale);
    assert_eq!(tga.image_data().len(), 4);
}

/// A color-mapped image with a 4-entry 24-bit palette parses correctly.
///
/// Why: validates that the color map region is sliced out properly and
/// pixel data follows immediately after it.
#[test]
fn parse_with_color_map() {
    let mut out = Vec::new();
    out.push(0); // id_length
    out.push(1); // color_map_type = has color map
    out.push(1); // image_type = color-mapped
    out.extend_from_slice(&0u16.to_le_bytes()); // color_map_first
    out.extend_from_slice(&4u16.to_le_bytes()); // color_map_length = 4 entries
    out.push(24); // color_map_entry_size = 24 bits
    out.extend_from_slice(&0u16.to_le_bytes()); // x_origin
    out.extend_from_slice(&0u16.to_le_bytes()); // y_origin
    out.extend_from_slice(&2u16.to_le_bytes()); // width
    out.extend_from_slice(&2u16.to_le_bytes()); // height
    out.push(8); // pixel_depth = 8 (index into color map)
    out.push(0); // image_descriptor

    // Color map: 4 entries × 3 bytes = 12 bytes
    let cmap = vec![0xCCu8; 12];
    out.extend_from_slice(&cmap);

    // Pixel data: 2×2 × 1 byte = 4 bytes
    let pixels = vec![0, 1, 2, 3];
    out.extend_from_slice(&pixels);

    let tga = TgaFile::parse(&out).unwrap();
    assert!(tga.has_color_map());
    assert_eq!(tga.color_map().len(), 12);
    assert_eq!(tga.image_data().len(), 4);
    assert_eq!(tga.header.color_map_length, 4);
    assert_eq!(tga.header.color_map_entry_size, 24);
}

/// An image with a non-empty image ID field parses correctly.
///
/// Why: the image ID is variable-length (0–255 bytes) and sits between
/// the header and color map.  This test ensures the offset arithmetic
/// accounts for it.
#[test]
fn parse_with_image_id() {
    let mut out = Vec::new();
    out.push(5); // id_length = 5 bytes
    out.push(0); // color_map_type
    out.push(3); // image_type = grayscale
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // width = 1
    out.extend_from_slice(&1u16.to_le_bytes()); // height = 1
    out.push(8); // pixel_depth = 8
    out.push(0); // image_descriptor

    // Image ID: 5 bytes
    out.extend_from_slice(b"HELLO");

    // Pixel data: 1×1 × 1 = 1 byte
    out.push(0x42);

    let tga = TgaFile::parse(&out).unwrap();
    assert_eq!(tga.image_id(), b"HELLO");
    assert_eq!(tga.image_data().len(), 1);
    assert_eq!(tga.image_data()[0], 0x42);
}

/// A file with a valid TGA 2.0 footer is detected.
///
/// Why: the footer is identified by its 18-byte signature at the end of
/// the file.  This verifies footer detection and offset extraction.
#[test]
fn parse_with_footer() {
    let pixels = vec![0u8; 4]; // 2×2 × 1 byte
    let mut data = build_tga(2, 2, 3, 8, &pixels);
    build_tga_with_footer(&mut data);

    let tga = TgaFile::parse(&data).unwrap();
    assert!(tga.has_footer());
    let footer = tga.footer.as_ref().unwrap();
    assert_eq!(footer.extension_offset, 0);
    assert_eq!(footer.developer_offset, 0);
}

/// RLE true-color type is correctly identified.
///
/// Why: RLE images (type >= 9) have variable-length compressed data,
/// so the parser must not reject them for having fewer bytes than the
/// uncompressed size would require.
#[test]
fn parse_rle_type() {
    // RLE data is shorter than the full uncompressed pixel count;
    // the parser must accept it without size validation.
    let rle_data = vec![0x00, 0xFF, 0x00, 0xFF]; // dummy RLE stream
    let data = build_tga(4, 4, 10, 24, &rle_data);
    let tga = TgaFile::parse(&data).unwrap();

    assert!(tga.is_rle());
    assert_eq!(tga.header.image_type, TgaImageType::RleTrueColor);
    assert_eq!(tga.image_data().len(), 4);
}

// ── Descriptor bit tests ─────────────────────────────────────────────────────

/// The top-to-bottom bit in the image descriptor is parsed correctly.
///
/// Why: bit 5 of image_descriptor controls row ordering.  Getting this
/// wrong flips the image vertically.
#[test]
fn is_top_to_bottom() {
    let mut out = Vec::new();
    out.push(0); // id_length
    out.push(0); // color_map_type
    out.push(3); // image_type = grayscale
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // width
    out.extend_from_slice(&1u16.to_le_bytes()); // height
    out.push(8); // pixel_depth
    out.push(0x20); // image_descriptor: bit 5 = top-to-bottom

    out.push(0x00); // 1 pixel

    let tga = TgaFile::parse(&out).unwrap();
    assert!(tga.is_top_to_bottom());
    assert!(!tga.is_right_to_left());
    assert_eq!(tga.alpha_depth(), 0);
}

/// Right-to-left and alpha depth bits are parsed correctly.
///
/// Why: bit 4 controls column ordering, bits 0–3 carry alpha depth.
#[test]
fn is_right_to_left_and_alpha() {
    let mut out = Vec::new();
    out.push(0);
    out.push(0);
    out.push(2); // true-color
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.push(32); // pixel_depth = 32 (BGRA)
    out.push(0x18); // bit 4 (right-to-left) + 8 alpha bits

    out.extend_from_slice(&[0u8; 4]); // 1 pixel × 4 bytes

    let tga = TgaFile::parse(&out).unwrap();
    assert!(tga.is_right_to_left());
    assert!(!tga.is_top_to_bottom());
    assert_eq!(tga.alpha_depth(), 8);
}

// ── Error cases ──────────────────────────────────────────────────────────────

/// Input shorter than 18 bytes is rejected.
///
/// Why: the TGA header is always 18 bytes.  Anything shorter cannot
/// possibly contain a valid header.
#[test]
fn reject_truncated_header() {
    let data = vec![0u8; 17];
    let err = TgaFile::parse(&data).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 18);
            assert_eq!(available, 17);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// An unrecognised image type value is rejected.
///
/// Why: only types 0, 1, 2, 3, 9, 10, 11 are valid.  Type 5 (for
/// example) must produce InvalidMagic.
#[test]
fn reject_invalid_image_type() {
    let data = build_tga(1, 1, 5, 8, &[0]);
    let err = TgaFile::parse(&data).unwrap_err();
    assert!(
        matches!(err, Error::InvalidMagic { context } if context == "TGA image type"),
        "Expected InvalidMagic for TGA image type, got: {err}",
    );
}

/// Pixel data shorter than width × height × bpp is rejected.
///
/// Why: for uncompressed images the parser must verify that enough
/// pixel data is present.  A truncated file must not silently succeed.
#[test]
fn reject_truncated_image_data() {
    // 4×4 24-bit needs 48 bytes of pixel data, but we only provide 10.
    let pixels = vec![0u8; 10];
    let data = build_tga(4, 4, 2, 24, &pixels);
    let err = TgaFile::parse(&data).unwrap_err();
    assert!(
        matches!(err, Error::UnexpectedEof { .. }),
        "Expected UnexpectedEof, got: {err}",
    );
}

/// Truncated image ID is rejected.
///
/// Why: if id_length claims N bytes but fewer than N remain after the
/// header, the parser must error rather than read out of bounds.
#[test]
fn reject_truncated_image_id() {
    let mut out = Vec::new();
    out.push(10); // id_length = 10
    out.push(0);
    out.push(3);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.push(8);
    out.push(0);
    // Only 3 bytes of image ID (less than 10).
    out.extend_from_slice(&[0x41, 0x42, 0x43]);

    let err = TgaFile::parse(&out).unwrap_err();
    assert!(
        matches!(err, Error::UnexpectedEof { .. }),
        "Expected UnexpectedEof, got: {err}",
    );
}

/// Empty input (0 bytes) is rejected cleanly.
///
/// Why: degenerate case — the parser must not panic on empty input.
#[test]
fn reject_empty_input() {
    let err = TgaFile::parse(&[]).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 18);
            assert_eq!(available, 0);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

// ── Adversarial / fuzz-style tests ───────────────────────────────────────────

/// 18 bytes of 0xFF must not panic.
///
/// Why (V38): all-`0xFF` exercises the worst-case field values.
/// `image_type = 0xFF` is invalid and should produce `InvalidMagic`.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFFu8; 18];
    let result = TgaFile::parse(&data);
    // 0xFF is not a valid image type, so we expect InvalidMagic.
    assert!(result.is_err());
    // The important thing is: no panic.
}

/// 18+ bytes of 0x00 must not panic.
///
/// Why (V38): all-zero input sets image_type=0 (NoImage) and all
/// dimensions to zero.  This exercises the zero-size image path.
#[test]
fn adversarial_all_zero() {
    let data = vec![0x00u8; 64];
    let result = TgaFile::parse(&data);
    // image_type 0 = NoImage, width=0, height=0.  Should parse OK.
    assert!(result.is_ok());
    let tga = result.unwrap();
    assert_eq!(tga.header.image_type, TgaImageType::NoImage);
    assert_eq!(tga.header.width, 0);
    assert_eq!(tga.header.height, 0);
    assert_eq!(tga.image_data().len(), 0);
}

/// A 32-bit true-color image parses correctly.
///
/// Why: 32-bit BGRA is common in Generals TGA textures and exercises
/// the 4-bytes-per-pixel path.
#[test]
fn parse_valid_32bit_truecolor() {
    let pixels = vec![0xDDu8; 2 * 2 * 4]; // 2×2 × 4 bytes
    let data = build_tga(2, 2, 2, 32, &pixels);
    let tga = TgaFile::parse(&data).unwrap();

    assert_eq!(tga.header.width, 2);
    assert_eq!(tga.header.height, 2);
    assert_eq!(tga.header.pixel_depth, 32);
    assert_eq!(tga.image_data().len(), 16);
}

/// A NoImage type with zero dimensions and no pixel data parses.
///
/// Why: type 0 is a valid TGA image type that carries no pixel data.
#[test]
fn parse_no_image_type() {
    let data = build_tga(0, 0, 0, 0, &[]);
    let tga = TgaFile::parse(&data).unwrap();
    assert_eq!(tga.header.image_type, TgaImageType::NoImage);
    assert_eq!(tga.image_data().len(), 0);
}
