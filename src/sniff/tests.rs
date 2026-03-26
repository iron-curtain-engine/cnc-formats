// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Unit tests for format sniffing probes.

use super::*;

/// VQA files are detected by FORM/WVQA magic.
#[test]
fn sniff_vqa() {
    let mut data = vec![0u8; 64];
    data[..4].copy_from_slice(b"FORM");
    data[8..12].copy_from_slice(b"WVQA");
    assert_eq!(sniff_format(&data), Some("vqa"));
}

/// BIG archives are detected by BIGF/BIG4 magic.
#[test]
fn sniff_big() {
    let mut data = vec![0u8; 32];
    data[..4].copy_from_slice(b"BIGF");
    assert_eq!(sniff_format(&data), Some("big"));
}

/// PAL files are detected by exact size (768) and value range (0–63).
#[test]
fn sniff_pal() {
    let data = vec![32u8; 768];
    assert_eq!(sniff_format(&data), Some("pal"));
}

/// ENG-family string tables are detected from their offset table layout.
#[test]
fn sniff_eng() {
    let data = [6u8, 0, 6, 0, 7, 0, 0, b'A', 0];
    assert_eq!(sniff_format(&data), Some("eng"));
}

/// Segmented DIP files are detected as installer data rather than generic blobs.
#[test]
fn sniff_segmented_dip() {
    let data = [
        0x02, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x3C,
        0x3C, 0x01, 0x80, 0x00, 0x00, 0x0B, 0x80,
    ];
    assert_eq!(sniff_format(&data), Some("dip"));
}

/// Chrono Vortex LUT files are detected by their exact size and bounds.
#[test]
fn sniff_lut() {
    let mut data = Vec::with_capacity(crate::lut::LUT_FILE_SIZE);
    for i in 0..crate::lut::LUT_ENTRY_COUNT {
        data.push((i % 64) as u8);
        data.push(((i / 64) % 64) as u8);
        data.push(((i / 256) % 16) as u8);
    }
    assert_eq!(sniff_format(&data), Some("lut"));
}

/// VQP files are detected by exact packed-table size.
#[test]
fn sniff_vqp() {
    let mut data = Vec::new();
    data.extend_from_slice(&1u32.to_le_bytes());
    data.resize(4 + crate::vqp::VQP_TABLE_SIZE, 0);
    assert_eq!(sniff_format(&data), Some("vqp"));
}

/// PAL with out-of-range values (>63) is not detected.
#[test]
fn sniff_pal_out_of_range_rejected() {
    let mut data = vec![32u8; 768];
    data[0] = 200;
    assert_ne!(sniff_format(&data), Some("pal"));
}

/// INI text is detected by ASCII content and `[` bracket.
#[test]
fn sniff_ini() {
    let data = b"[General]\nSpeed=5\n";
    assert_eq!(sniff_format(data), Some("ini"));
}

/// Binary data without brackets is not detected as INI.
#[test]
fn sniff_not_ini() {
    let data = vec![0xFFu8; 100];
    assert_ne!(sniff_format(&data), Some("ini"));
}

/// Empty data returns None.
#[test]
fn sniff_empty() {
    assert_eq!(sniff_format(&[]), None);
}

/// Very short data returns None (not a panic).
#[test]
fn sniff_short() {
    assert_eq!(sniff_format(&[0x42]), None);
}

/// VXL files are detected by "Voxel Animation\0" magic.
#[test]
fn sniff_vxl() {
    let mut data = vec![0u8; 1024];
    data[..16].copy_from_slice(b"Voxel Animation\0");
    assert_eq!(sniff_format(&data), Some("vxl"));
}

/// CSF files are detected by " FSC" magic.
#[test]
fn sniff_csf() {
    let mut data = vec![0u8; 64];
    data[..4].copy_from_slice(b" FSC");
    assert_eq!(sniff_format(&data), Some("csf"));
}

/// SHP detection uses structural heuristics.
#[cfg(feature = "convert")]
#[test]
fn sniff_shp_from_parsed_file() {
    // Build a minimal valid SHP: 1 frame, 2×2 pixels, LCW-compressed.
    let shp_bytes = crate::shp::build_test_shp_helper(2, 2, 0xAA);
    assert_eq!(sniff_format(&shp_bytes), Some("shp"));
}

/// VOC files are detected by "Creative Voice File\x1a" magic.
#[test]
fn sniff_voc() {
    let mut data = vec![0u8; 32];
    data[..20].copy_from_slice(b"Creative Voice File\x1a");
    data[20..22].copy_from_slice(&26u16.to_le_bytes());
    assert_eq!(sniff_format(&data), Some("voc"));
}

/// DDS files are detected by "DDS " magic + header size 124.
#[test]
fn sniff_dds() {
    let mut data = vec![0u8; 128];
    data[..4].copy_from_slice(b"DDS ");
    data[4..8].copy_from_slice(&124u32.to_le_bytes());
    assert_eq!(sniff_format(&data), Some("dds"));
}

/// APT files are detected by "Apt\0" magic.
#[test]
fn sniff_apt() {
    let mut data = vec![0u8; 16];
    data[..4].copy_from_slice(b"Apt\0");
    assert_eq!(sniff_format(&data), Some("apt"));
}

/// SAGE map files are detected by "CkMp" magic.
#[test]
fn sniff_map_sage() {
    let mut data = vec![0u8; 16];
    data[..4].copy_from_slice(b"CkMp");
    assert_eq!(sniff_format(&data), Some("map_sage"));
}

/// JPEG files are detected by FF D8 FF magic.
#[test]
fn sniff_jpg() {
    let mut data = vec![0u8; 32];
    data[0] = 0xFF;
    data[1] = 0xD8;
    data[2] = 0xFF;
    data[3] = 0xE0; // JFIF APP0 marker
    assert_eq!(sniff_format(&data), Some("jpg"));
}

/// JPEG: 2 bytes (FF D8 without third FF) is not detected.
#[test]
fn sniff_jpg_too_short() {
    assert_eq!(sniff_format(&[0xFF, 0xD8]), None);
}

/// JPEG: FF D8 followed by non-FF is not detected.
#[test]
fn sniff_jpg_invalid_third_byte() {
    assert_eq!(sniff_format(&[0xFF, 0xD8, 0x00, 0x00]), None);
}

/// JPEG: single byte is not detected.
#[test]
fn sniff_jpg_single_byte() {
    assert_eq!(sniff_format(&[0xFF]), None);
}

/// TGA files with v2.0 footer are detected.
#[test]
fn sniff_tga_footer() {
    // Build a buffer large enough that the footer doesn't overlap the header,
    // and fill with non-zero pixel data to avoid triggering other sniffers.
    let mut data = vec![0x42u8; 128];
    // Write 18-byte TGA header.
    data[0] = 0; // id_length
    data[1] = 0; // no color map
    data[2] = 2; // true-color
    data[3..8].copy_from_slice(&[0; 5]); // color map spec
    data[8..12].copy_from_slice(&[0; 4]); // origin
    data[12..14].copy_from_slice(&4u16.to_le_bytes()); // width
    data[14..16].copy_from_slice(&4u16.to_le_bytes()); // height
    data[16] = 24; // pixel_depth
    data[17] = 0x20; // top-to-bottom
                     // Append TGA 2.0 footer at the very end.
    let footer_start = data.len() - 26;
    data[footer_start..footer_start + 4].copy_from_slice(&0u32.to_le_bytes());
    data[footer_start + 4..footer_start + 8].copy_from_slice(&0u32.to_le_bytes());
    data[footer_start + 8..].copy_from_slice(b"TRUEVISION-XFILE.\0");
    assert_eq!(sniff_format(&data), Some("tga"));
}
