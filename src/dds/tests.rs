// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

// ── Test Helpers ──────────────────────────────────────────────────────────────

/// Builds a valid DDS file with FourCC-compressed pixel format.
///
/// Constructs the 128-byte standard header plus any pixel data provided.
fn build_dds(width: u32, height: u32, four_cc: &[u8; 4], pixel_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // Magic
    out.extend_from_slice(b"DDS ");
    // DDS_HEADER (124 bytes)
    out.extend_from_slice(&124u32.to_le_bytes()); // size
    let flags: u32 = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // pitch_or_linear_size
    out.extend_from_slice(&0u32.to_le_bytes()); // depth
    out.extend_from_slice(&0u32.to_le_bytes()); // mip_map_count
    out.extend_from_slice(&[0u8; 44]); // reserved1
                                       // DDS_PIXELFORMAT (32 bytes)
    out.extend_from_slice(&32u32.to_le_bytes()); // pf_size
    out.extend_from_slice(&DDPF_FOURCC.to_le_bytes()); // pf_flags
    out.extend_from_slice(four_cc);
    out.extend_from_slice(&0u32.to_le_bytes()); // rgb_bit_count
    out.extend_from_slice(&0u32.to_le_bytes()); // r_bitmask
    out.extend_from_slice(&0u32.to_le_bytes()); // g_bitmask
    out.extend_from_slice(&0u32.to_le_bytes()); // b_bitmask
    out.extend_from_slice(&0u32.to_le_bytes()); // a_bitmask
                                                // Caps
    out.extend_from_slice(&0x1000u32.to_le_bytes()); // caps (DDSCAPS_TEXTURE)
    out.extend_from_slice(&0u32.to_le_bytes()); // caps2
    out.extend_from_slice(&0u32.to_le_bytes()); // caps3
    out.extend_from_slice(&0u32.to_le_bytes()); // caps4
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
                                                // Pixel data
    out.extend_from_slice(pixel_data);
    assert!(out.len() >= MIN_FILE_SIZE);
    out
}

/// Builds a DDS file with uncompressed RGB pixel format (no FourCC).
#[allow(clippy::too_many_arguments)]
fn build_dds_rgb(
    width: u32,
    height: u32,
    bit_count: u32,
    r_mask: u32,
    g_mask: u32,
    b_mask: u32,
    a_mask: u32,
    pf_flags: u32,
    pixel_data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"DDS ");
    out.extend_from_slice(&124u32.to_le_bytes());
    let flags: u32 = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_PITCH;
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    let pitch = width.saturating_mul(bit_count / 8);
    out.extend_from_slice(&pitch.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // depth
    out.extend_from_slice(&0u32.to_le_bytes()); // mip_map_count
    out.extend_from_slice(&[0u8; 44]); // reserved1
                                       // Pixel format
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&pf_flags.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]); // four_cc (unused for RGB)
    out.extend_from_slice(&bit_count.to_le_bytes());
    out.extend_from_slice(&r_mask.to_le_bytes());
    out.extend_from_slice(&g_mask.to_le_bytes());
    out.extend_from_slice(&b_mask.to_le_bytes());
    out.extend_from_slice(&a_mask.to_le_bytes());
    // Caps
    out.extend_from_slice(&0x1000u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // caps2-4 + reserved2
    out.extend_from_slice(pixel_data);
    out
}

/// Builds a DDS file with a DX10 extended header.
fn build_dds_dx10(
    width: u32,
    height: u32,
    dxgi_format: u32,
    resource_dimension: u32,
    pixel_data: &[u8],
) -> Vec<u8> {
    let mut out = build_dds(width, height, b"DX10", &[]);
    // DX10 header (20 bytes) — inserted before pixel data
    out.extend_from_slice(&dxgi_format.to_le_bytes());
    out.extend_from_slice(&resource_dimension.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // misc_flag
    out.extend_from_slice(&1u32.to_le_bytes()); // array_size
    out.extend_from_slice(&0u32.to_le_bytes()); // misc_flags2
    out.extend_from_slice(pixel_data);
    out
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a valid DDS file with DXT1 compression; verify dimensions and four_cc.
#[test]
fn parse_valid_dds() {
    let pixels = [0xAAu8; 32];
    let data = build_dds(256, 128, b"DXT1", &pixels);
    let dds = DdsFile::parse(&data).unwrap();
    assert_eq!(dds.width, 256);
    assert_eq!(dds.height, 128);
    assert_eq!(dds.pixel_format.four_cc, *b"DXT1");
    assert!(dds.is_compressed());
    assert!(!dds.has_dx10());
    assert_eq!(dds.four_cc_str(), Some("DXT1"));
    assert_eq!(dds.pixel_data(), &pixels);
}

/// Parse a DDS file with a DX10 extended header.
#[test]
fn parse_dx10_header() {
    let pixels = [0xBBu8; 16];
    let dxgi_bc7 = 98; // DXGI_FORMAT_BC7_UNORM
    let dim_2d = 3; // D3D10_RESOURCE_DIMENSION_TEXTURE2D
    let data = build_dds_dx10(512, 512, dxgi_bc7, dim_2d, &pixels);
    let dds = DdsFile::parse(&data).unwrap();
    assert_eq!(dds.width, 512);
    assert_eq!(dds.height, 512);
    assert!(dds.has_dx10());
    assert!(dds.is_compressed());
    assert_eq!(dds.four_cc_str(), Some("DX10"));
    let dx10 = dds.dx10.as_ref().unwrap();
    assert_eq!(dx10.dxgi_format, dxgi_bc7);
    assert_eq!(dx10.resource_dimension, dim_2d);
    assert_eq!(dx10.array_size, 1);
    assert_eq!(dds.pixel_data(), &pixels);
}

/// Parse an uncompressed RGB DDS (DDPF_RGB, no FOURCC).
#[test]
fn parse_uncompressed_rgb() {
    let pixels = [0x11u8; 64];
    let data = build_dds_rgb(
        4,
        4,
        32,
        0x00FF0000,
        0x0000FF00,
        0x000000FF,
        0xFF000000,
        DDPF_RGB | DDPF_ALPHAPIXELS,
        &pixels,
    );
    let dds = DdsFile::parse(&data).unwrap();
    assert_eq!(dds.width, 4);
    assert_eq!(dds.height, 4);
    assert!(!dds.is_compressed());
    assert_eq!(dds.pixel_format.flags, DDPF_RGB | DDPF_ALPHAPIXELS);
    assert_eq!(dds.pixel_format.rgb_bit_count, 32);
    assert_eq!(dds.pixel_format.r_bitmask, 0x00FF0000);
    assert_eq!(dds.pixel_format.g_bitmask, 0x0000FF00);
    assert_eq!(dds.pixel_format.b_bitmask, 0x000000FF);
    assert_eq!(dds.pixel_format.a_bitmask, 0xFF000000);
    assert_eq!(dds.pixel_data(), &pixels);
}

/// Verify pixel_data() returns the correct slice.
#[test]
fn pixel_data_access() {
    let pixels: Vec<u8> = (0..100).collect();
    let data = build_dds(64, 64, b"DXT5", &pixels);
    let dds = DdsFile::parse(&data).unwrap();
    assert_eq!(dds.pixel_data().len(), 100);
    assert_eq!(dds.pixel_data(), pixels.as_slice());
}

/// Empty pixel data is valid (header-only file).
#[test]
fn parse_empty_pixel_data() {
    let data = build_dds(1, 1, b"DXT1", &[]);
    let dds = DdsFile::parse(&data).unwrap();
    assert!(dds.pixel_data().is_empty());
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Wrong magic bytes are rejected.
#[test]
fn reject_invalid_magic() {
    let mut data = build_dds(4, 4, b"DXT1", &[0; 8]);
    data[0] = b'X'; // corrupt magic
    let err = DdsFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "DDS magic"
        }
    ));
}

/// Input shorter than 128 bytes is rejected.
#[test]
fn reject_truncated_header() {
    let err = DdsFile::parse(&[0u8; 127]).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 128,
            available: 127,
        }
    ));
}

/// Header size field != 124 is rejected.
#[test]
fn reject_wrong_header_size() {
    let mut data = build_dds(4, 4, b"DXT1", &[0; 8]);
    // Patch header size at offset 4 to 100
    data[4..8].copy_from_slice(&100u32.to_le_bytes());
    let err = DdsFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "DDS header size",
            ..
        }
    ));
}

/// Pixel format size field != 32 is rejected.
#[test]
fn reject_wrong_pf_size() {
    let mut data = build_dds(4, 4, b"DXT1", &[0; 8]);
    // Pixel format size is at file offset 76 (header offset 72)
    data[76..80].copy_from_slice(&64u32.to_le_bytes());
    let err = DdsFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "DDS pixel format size",
            ..
        }
    ));
}

/// DX10 header declared but data truncated before it completes.
#[test]
fn reject_truncated_dx10() {
    // Build a valid DDS with DX10 fourcc but no DX10 header bytes
    let data = build_dds(4, 4, b"DX10", &[]);
    // This is exactly 128 bytes — needs 148 for DX10
    assert_eq!(data.len(), 128);
    let err = DdsFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 148,
            available: 128,
        }
    ));
}

// ── Security Edge Cases ──────────────────────────────────────────────────────

/// `DdsFile::parse` on 256 bytes of `0xFF` must not panic.
///
/// All-ones buffer maximises every field, exercising overflow guards
/// and validation checks.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFFu8; 256];
    let _ = DdsFile::parse(&data);
}

/// `DdsFile::parse` on 256 bytes of `0x00` must not panic.
///
/// All-zero buffer has wrong magic (`\0\0\0\0` != `DDS `), so the
/// parser should return an error without panicking.
#[test]
fn adversarial_all_zero() {
    let data = vec![0x00u8; 256];
    let result = DdsFile::parse(&data);
    assert!(result.is_err());
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn determinism() {
    let data = build_dds(128, 64, b"DXT3", &[0xCCu8; 48]);
    let a = DdsFile::parse(&data).unwrap();
    let b = DdsFile::parse(&data).unwrap();
    assert_eq!(a, b);
}
