// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Test Helpers ──────────────────────────────────────────────────────────────

/// Builds a valid CPS file with LCW-compressed pixel data and no palette.
fn build_cps_lcw(pixels: &[u8]) -> Vec<u8> {
    let compressed = crate::lcw::compress(pixels);
    let buffer_size = pixels.len() as u32;
    // file_size = total - 2
    let total = HEADER_SIZE + compressed.len();
    let file_size = (total.saturating_sub(2)) as u16;

    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&COMPRESSION_LCW.to_le_bytes());
    buf.extend_from_slice(&buffer_size.to_le_bytes()); // u32 at offset 4
    buf.extend_from_slice(&0u16.to_le_bytes()); // palette_size = 0 at offset 8
    buf.extend_from_slice(&compressed);
    buf
}

/// Builds a valid CPS file with raw pixels (no compression, no palette).
fn build_cps_raw(pixels: &[u8]) -> Vec<u8> {
    let buffer_size = pixels.len() as u32;
    let total = HEADER_SIZE + pixels.len();
    let file_size = (total.saturating_sub(2)) as u16;

    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&COMPRESSION_NONE.to_le_bytes());
    buf.extend_from_slice(&buffer_size.to_le_bytes()); // u32 at offset 4
    buf.extend_from_slice(&0u16.to_le_bytes()); // palette_size = 0 at offset 8
    buf.extend_from_slice(pixels);
    buf
}

/// Builds a valid CPS file with LCW compression and an embedded palette.
fn build_cps_with_palette(pixels: &[u8]) -> Vec<u8> {
    let compressed = crate::lcw::compress(pixels);
    let buffer_size = pixels.len() as u32;
    let total = HEADER_SIZE + PALETTE_BYTES + compressed.len();
    let file_size = (total.saturating_sub(2)) as u16;

    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&COMPRESSION_LCW.to_le_bytes());
    buf.extend_from_slice(&buffer_size.to_le_bytes()); // u32 at offset 4
    buf.extend_from_slice(&768u16.to_le_bytes()); // palette_size = 768 at offset 8

    // Palette: 256 colors, each (i, i/2, i/3) for distinctness.
    for i in 0..256u16 {
        buf.push((i % 64) as u8);
        buf.push((i / 2 % 64) as u8);
        buf.push((i / 3 % 64) as u8);
    }

    buf.extend_from_slice(&compressed);
    buf
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a valid CPS file with LCW compression and no palette.
#[test]
fn parse_valid_no_palette() {
    let pixels = vec![0xAAu8; 64];
    let data = build_cps_lcw(&pixels);
    let cps = CpsFile::parse(&data).unwrap();
    assert_eq!(cps.header.compression, COMPRESSION_LCW);
    assert_eq!(cps.header.buffer_size, 64);
    assert_eq!(cps.header.palette_size, 0);
    assert!(cps.palette.is_none());
    assert_eq!(cps.pixels, pixels);
}

/// Parse a valid CPS file with raw (uncompressed) pixel data.
#[test]
fn parse_uncompressed() {
    let pixels = vec![0x55u8; 32];
    let data = build_cps_raw(&pixels);
    let cps = CpsFile::parse(&data).unwrap();
    assert_eq!(cps.header.compression, COMPRESSION_NONE);
    assert_eq!(cps.pixels, pixels);
}

/// Parse a CPS file with an embedded palette.
#[test]
fn parse_valid_with_palette() {
    let pixels = vec![1u8; 16];
    let data = build_cps_with_palette(&pixels);
    let cps = CpsFile::parse(&data).unwrap();
    assert!(cps.palette.is_some());
    let pal = cps.palette.unwrap();
    // Color 0: (0, 0, 0)
    assert_eq!(pal.colors[0], (0, 0, 0));
    // Color 1: (1, 0, 0) (1%64, 1/2%64, 1/3%64)
    assert_eq!(pal.colors[1], (1, 0, 0));
    assert_eq!(cps.pixels, pixels);
}

/// Palette to_rgb8 conversion scales 6-bit to 8-bit correctly.
#[test]
fn palette_to_rgb8() {
    let mut colors = [(0u8, 0u8, 0u8); 256];
    colors[0] = (63, 0, 32);
    let pal = CpsPalette { colors };
    let (r, g, b) = pal.to_rgb8(0);
    // 6-bit → 8-bit: (v << 2) | (v >> 4) for proper 0→0, 63→255 scaling.
    assert_eq!(r, (63 << 2) | (63 >> 4)); // 255
    assert_eq!(g, 0);
    assert_eq!(b, (32 << 2) | (32 >> 4)); // 130
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Input shorter than the 10-byte header is rejected.
#[test]
fn truncated_header() {
    let err = CpsFile::parse(&[0u8; 9]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { needed: 10, .. }));
}

/// Compression type other than 0 or 4 is rejected.
#[test]
fn invalid_compression() {
    let mut data = build_cps_raw(&[0; 16]);
    data[2] = 5; // compression = 5
    data[3] = 0;
    let err = CpsFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidMagic { .. }));
}

/// Buffer size field is correctly bounded by the V38 cap.
///
/// Note: buffer_size is u32; small test values are always below the
/// 262144 cap, so this verifies the field reads correctly.
#[test]
fn buffer_size_within_cap() {
    let pixels = vec![0u8; 16];
    let built = build_cps_lcw(&pixels);
    let cps = CpsFile::parse(&built).unwrap();
    assert!((cps.header.buffer_size as usize) <= MAX_BUFFER_SIZE);
}

/// Palette size that is not 0 or 768 is rejected.
#[test]
fn invalid_palette_size() {
    let mut data = build_cps_raw(&[0; 16]);
    data[8] = 100; // palette_size = 100 at offset 8 (after 4-byte buffer_size)
    data[9] = 0;
    let err = CpsFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

/// Palette declared but data is truncated.
#[test]
fn truncated_palette() {
    let mut data = vec![0u8; HEADER_SIZE + 100]; // not enough for 768 palette bytes
    data[2..4].copy_from_slice(&COMPRESSION_NONE.to_le_bytes());
    data[4..8].copy_from_slice(&16u32.to_le_bytes()); // buffer_size as u32 at offset 4
    data[8..10].copy_from_slice(&768u16.to_le_bytes()); // palette_size at offset 8
    let err = CpsFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn determinism() {
    let data = build_cps_lcw(&[42u8; 48]);
    let a = CpsFile::parse(&data).unwrap();
    let b = CpsFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ── Security Edge Cases (V38) ────────────────────────────────────────────────

/// `CpsFile::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): all-ones buffer maximises header fields, exercising
/// overflow guards, compression validation, and size checks.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = CpsFile::parse(&data);
}

/// `CpsFile::parse` on 256 bytes of `0x00` must not panic.
///
/// Why (V38): all-zero buffer exercises zero-dimension paths,
/// compression type 0, and empty payload handling.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = CpsFile::parse(&data);
}
