// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Test Helpers ──────────────────────────────────────────────────────────────

/// Builds a minimal valid TS SHP file with uncompressed frames.
///
/// Creates a `width × height` canvas with `num_frames` frames, each covering
/// the full canvas with pixel value `fill`.
fn build_shp_ts_uncompressed(width: u16, height: u16, num_frames: u16, fill: u8) -> Vec<u8> {
    let area = width as usize * height as usize;
    let headers_size = FILE_HEADER_SIZE + num_frames as usize * FRAME_HEADER_SIZE;
    let total = headers_size + num_frames as usize * area;
    let mut buf = Vec::with_capacity(total);

    // File header.
    buf.extend_from_slice(&0u16.to_le_bytes()); // zero marker
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&num_frames.to_le_bytes());

    // Frame headers.
    for i in 0..num_frames as usize {
        let offset = (headers_size + i * area) as u32;
        buf.extend_from_slice(&0u16.to_le_bytes()); // x
        buf.extend_from_slice(&0u16.to_le_bytes()); // y
        buf.extend_from_slice(&width.to_le_bytes()); // cx
        buf.extend_from_slice(&height.to_le_bytes()); // cy
        buf.push(0); // compression = none
        buf.extend_from_slice(&[0u8; 3]); // padding
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown1
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown2
        buf.extend_from_slice(&offset.to_le_bytes()); // file_offset
    }

    // Frame pixel data.
    for _ in 0..num_frames as usize {
        buf.resize(buf.len() + area, fill);
    }

    buf
}

/// Builds a TS SHP file with scanline-RLE compressed frames.
///
/// Creates a 4×2 canvas with 1 frame. Each scanline has two literal pixels
/// (values `0xAA`, `0xBB`) followed by a 2-pixel transparent run.
fn build_shp_ts_rle() -> Vec<u8> {
    let width: u16 = 4;
    let height: u16 = 2;
    let num_frames: u16 = 1;
    let headers_size = FILE_HEADER_SIZE + FRAME_HEADER_SIZE;

    // Build RLE data for each scanline.
    // Scanline: u16 length, then RLE bytes.
    // Literal 0xAA, literal 0xBB, transparent run of 2 (0x00, 0x02).
    let scanline: Vec<u8> = vec![
        6, 0, // u16 length = 6 (2 for length field + 4 bytes of RLE data)
        0xAA, 0xBB, 0x00, 0x02,
    ];
    let rle_data_len = scanline.len() * height as usize;
    let total = headers_size + rle_data_len;

    let mut buf = Vec::with_capacity(total);

    // File header.
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&num_frames.to_le_bytes());

    // Frame header.
    buf.extend_from_slice(&0u16.to_le_bytes()); // x
    buf.extend_from_slice(&0u16.to_le_bytes()); // y
    buf.extend_from_slice(&width.to_le_bytes()); // cx
    buf.extend_from_slice(&height.to_le_bytes()); // cy
    buf.push(1); // compression = scanline RLE
    buf.extend_from_slice(&[0u8; 3]);
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(headers_size as u32).to_le_bytes()); // file_offset

    // RLE data.
    for _ in 0..height {
        buf.extend_from_slice(&scanline);
    }

    buf
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a valid TS SHP file with uncompressed frames.
#[test]
fn parse_valid_uncompressed() {
    let data = build_shp_ts_uncompressed(4, 4, 2, 0xAA);
    let shp = ShpTsFile::parse(&data).unwrap();
    assert_eq!(shp.header.width, 4);
    assert_eq!(shp.header.height, 4);
    assert_eq!(shp.header.num_frames, 2);
    assert_eq!(shp.frames.len(), 2);
}

/// Decode uncompressed frame pixels.
#[test]
fn decode_uncompressed_pixels() {
    let data = build_shp_ts_uncompressed(4, 4, 1, 0x55);
    let shp = ShpTsFile::parse(&data).unwrap();
    let pixels = shp.frames[0].pixels().unwrap();
    assert_eq!(pixels.len(), 16);
    assert!(pixels.iter().all(|&p| p == 0x55));
}

/// Decode scanline-RLE compressed frame pixels.
#[test]
fn decode_rle_pixels() {
    let data = build_shp_ts_rle();
    let shp = ShpTsFile::parse(&data).unwrap();
    let pixels = shp.frames[0].pixels().unwrap();
    assert_eq!(pixels.len(), 8); // 4×2
                                 // Each scanline: 0xAA, 0xBB, 0, 0
    assert_eq!(pixels[0], 0xAA);
    assert_eq!(pixels[1], 0xBB);
    assert_eq!(pixels[2], 0);
    assert_eq!(pixels[3], 0);
    assert_eq!(pixels[4], 0xAA);
    assert_eq!(pixels[5], 0xBB);
}

/// Empty frame (cx=0 or cy=0) produces empty pixel buffer.
#[test]
fn empty_frame_pixels() {
    let mut data = build_shp_ts_uncompressed(4, 4, 1, 0xAA);
    // Set cx=0 in the frame header.
    let cx_offset = FILE_HEADER_SIZE + 4;
    data[cx_offset] = 0;
    data[cx_offset + 1] = 0;
    let shp = ShpTsFile::parse(&data).unwrap();
    let pixels = shp.frames[0].pixels().unwrap();
    assert!(pixels.is_empty());
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Input shorter than 8 bytes is rejected.
#[test]
fn truncated_header() {
    let err = ShpTsFile::parse(&[0u8; 7]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { needed: 8, .. }));
}

/// First u16 != 0 is rejected as invalid magic.
#[test]
fn first_word_nonzero_rejected() {
    let mut data = build_shp_ts_uncompressed(4, 4, 1, 0);
    data[0] = 1; // first u16 = 1
    let err = ShpTsFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidMagic { .. }));
}

/// Frame count exceeding V38 cap is rejected.
#[test]
fn too_many_frames() {
    let mut data = build_shp_ts_uncompressed(4, 4, 1, 0);
    let frames = (MAX_FRAMES as u16).saturating_add(1);
    data[6..8].copy_from_slice(&frames.to_le_bytes());
    let err = ShpTsFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "SHP TS frame count",
            ..
        }
    ));
}

/// Not enough data for declared frame headers.
#[test]
fn truncated_frame_headers() {
    let mut data = vec![0u8; FILE_HEADER_SIZE + 10]; // not enough for 1 frame header
    data[6..8].copy_from_slice(&1u16.to_le_bytes()); // 1 frame
    let err = ShpTsFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn determinism() {
    let data = build_shp_ts_uncompressed(4, 4, 2, 0xBB);
    let a = ShpTsFile::parse(&data).unwrap();
    let b = ShpTsFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ── Security Edge Cases (V38) ────────────────────────────────────────────────

/// `ShpTsFile::parse` on 256 bytes of `0xFF` must not panic.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = ShpTsFile::parse(&data);
}

/// `ShpTsFile::parse` on 256 bytes of `0x00` must not panic.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = ShpTsFile::parse(&data);
}
