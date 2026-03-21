// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Test Helpers ──────────────────────────────────────────────────────────────

/// Builds a minimal valid TS TMP file with the given grid and tile properties.
///
/// Creates a `tiles_x × tiles_y` grid.  The first tile has extras and z data
/// (flags=0x03), the second tile is plain (flags=0), remaining slots are empty.
fn build_ts_tmp(tile_width: u32, tile_height: u32, tiles_x: u32, tiles_y: u32) -> Vec<u8> {
    let grid_count = (tiles_x * tiles_y) as usize;
    let iso_size = (tile_width as usize) * (tile_height as usize) / 2;
    let extra_w: u32 = 4;
    let extra_h: u32 = 2;
    let extra_size = (extra_w * extra_h) as usize;

    // Calculate offsets.
    let header_size = 16;
    let offset_table_size = grid_count * 4;
    let tile0_offset = header_size + offset_table_size;
    // Tile 0: header(52) + iso + extra + z
    let tile0_size = 52 + iso_size + extra_size + iso_size;
    let tile1_offset = tile0_offset + tile0_size;
    // Tile 1: header(52) + iso (no extras, no z)
    let tile1_size = 52 + iso_size;
    let total = tile1_offset + tile1_size;

    let mut buf = vec![0u8; total];

    // File header.
    buf[0..4].copy_from_slice(&tile_width.to_le_bytes());
    buf[4..8].copy_from_slice(&tile_height.to_le_bytes());
    buf[8..12].copy_from_slice(&tiles_x.to_le_bytes());
    buf[12..16].copy_from_slice(&tiles_y.to_le_bytes());

    // Offset table: tile 0 and tile 1 present, rest empty.
    let off_base = header_size;
    if grid_count >= 1 {
        buf[off_base..off_base + 4].copy_from_slice(&(tile0_offset as u32).to_le_bytes());
    }
    if grid_count >= 2 {
        buf[off_base + 4..off_base + 8].copy_from_slice(&(tile1_offset as u32).to_le_bytes());
    }
    // Remaining offsets stay 0 (empty tiles).

    // ── Tile 0: has extra + z data ───────────────────────────────────────
    let t0 = tile0_offset;
    // x_extra, y_extra
    buf[t0..t0 + 4].copy_from_slice(&10i32.to_le_bytes());
    buf[t0 + 4..t0 + 8].copy_from_slice(&5i32.to_le_bytes());
    // extra_width, extra_height
    buf[t0 + 8..t0 + 12].copy_from_slice(&extra_w.to_le_bytes());
    buf[t0 + 12..t0 + 16].copy_from_slice(&extra_h.to_le_bytes());
    // flags = HAS_EXTRA | HAS_Z_DATA
    buf[t0 + 16..t0 + 20].copy_from_slice(&0x03u32.to_le_bytes());
    // height=5, terrain=1, ramp=0
    buf[t0 + 20] = 5;
    buf[t0 + 21] = 1;
    buf[t0 + 22] = 0;
    // radar colors
    buf[t0 + 23] = 100; // red left
    buf[t0 + 24] = 150; // green left
    buf[t0 + 25] = 200; // blue left
    buf[t0 + 26] = 50; // red right
    buf[t0 + 27] = 75; // green right
    buf[t0 + 28] = 100; // blue right

    // Iso pixels: fill with 0xAA.
    let iso_start = t0 + 52;
    for byte in buf[iso_start..iso_start + iso_size].iter_mut() {
        *byte = 0xAA;
    }
    // Extra pixels: fill with 0xBB.
    let extra_start = iso_start + iso_size;
    for byte in buf[extra_start..extra_start + extra_size].iter_mut() {
        *byte = 0xBB;
    }
    // Z data: fill with height value 3.
    let z_start = extra_start + extra_size;
    for byte in buf[z_start..z_start + iso_size].iter_mut() {
        *byte = 3;
    }

    // ── Tile 1: plain (no extras) ────────────────────────────────────────
    let t1 = tile1_offset;
    // flags = 0 (no extras, no z)
    // Iso pixels: fill with 0xCC.
    let iso1_start = t1 + 52;
    for byte in buf[iso1_start..iso1_start + iso_size].iter_mut() {
        *byte = 0xCC;
    }

    buf
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a valid TS TMP file and verify header fields.
#[test]
fn parse_ts_valid() {
    let data = build_ts_tmp(8, 4, 2, 1);
    let tmp = TsTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.tile_width, 8);
    assert_eq!(tmp.header.tile_height, 4);
    assert_eq!(tmp.header.tiles_x, 2);
    assert_eq!(tmp.header.tiles_y, 1);
    assert_eq!(tmp.tiles.len(), 2);
}

/// Tile with extras and z data has all three data sections.
#[test]
fn parse_ts_tile_with_extras() {
    let data = build_ts_tmp(8, 4, 2, 1);
    let tmp = TsTmpFile::parse(&data).unwrap();
    let tile0 = tmp.tiles[0].as_ref().unwrap();
    assert_eq!(tile0.header.flags, 0x03);
    assert_eq!(tile0.header.height, 5);
    assert_eq!(tile0.header.terrain_type, 1);
    assert_eq!(tile0.header.radar_color.left, (100, 150, 200));
    assert_eq!(tile0.header.radar_color.right, (50, 75, 100));
    // Iso pixels: 8*4/2 = 16 bytes of 0xAA.
    assert_eq!(tile0.iso_pixels.len(), 16);
    assert!(tile0.iso_pixels.iter().all(|&b| b == 0xAA));
    // Extra pixels: 4*2 = 8 bytes of 0xBB.
    let extra = tile0.extra_pixels.unwrap();
    assert_eq!(extra.len(), 8);
    assert!(extra.iter().all(|&b| b == 0xBB));
    // Z data: 16 bytes of value 3.
    let z = tile0.z_data.unwrap();
    assert_eq!(z.len(), 16);
    assert!(z.iter().all(|&b| b == 3));
}

/// Tile without extras has None for extra_pixels and z_data.
#[test]
fn parse_ts_plain_tile() {
    let data = build_ts_tmp(8, 4, 2, 1);
    let tmp = TsTmpFile::parse(&data).unwrap();
    let tile1 = tmp.tiles[1].as_ref().unwrap();
    assert_eq!(tile1.header.flags, 0);
    assert!(tile1.extra_pixels.is_none());
    assert!(tile1.z_data.is_none());
    assert!(tile1.iso_pixels.iter().all(|&b| b == 0xCC));
}

/// Empty tile slots (offset=0) yield None in the tiles vector.
#[test]
fn parse_ts_empty_tile() {
    let data = build_ts_tmp(8, 4, 3, 1); // 3 tiles, only first 2 have data
    let tmp = TsTmpFile::parse(&data).unwrap();
    assert!(tmp.tiles[2].is_none());
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Input shorter than 16 bytes is rejected.
#[test]
fn parse_ts_truncated_header() {
    let err = TsTmpFile::parse(&[0u8; 15]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { needed: 16, .. }));
}

/// Zero tile dimensions are rejected.
#[test]
fn parse_ts_zero_tile_dimensions() {
    let mut data = build_ts_tmp(8, 4, 1, 1);
    // Set tile_width = 0.
    data[0..4].copy_from_slice(&0u32.to_le_bytes());
    let err = TsTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { value: 0, .. }));
}

/// Grid count exceeding V38 cap is rejected.
#[test]
fn parse_ts_too_many_tiles() {
    let mut data = vec![0u8; 16];
    data[0..4].copy_from_slice(&8u32.to_le_bytes()); // tile_width
    data[4..8].copy_from_slice(&4u32.to_le_bytes()); // tile_height
    data[8..12].copy_from_slice(&4097u32.to_le_bytes()); // tiles_x
    data[12..16].copy_from_slice(&1u32.to_le_bytes()); // tiles_y
    let err = TsTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

/// Tile offset pointing near end of file causes truncation error.
#[test]
fn parse_ts_truncated_tile() {
    let mut data = build_ts_tmp(8, 4, 1, 1);
    // Truncate to cut off tile data.
    data.truncate(20 + 4 + 30); // header + 1 offset + partial tile
    let err = TsTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn parse_ts_determinism() {
    let data = build_ts_tmp(8, 4, 2, 1);
    let a = TsTmpFile::parse(&data).unwrap();
    let b = TsTmpFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ── Security Edge Cases (V38) ────────────────────────────────────────────────

/// `TsTmpFile::parse` on 256 bytes of `0xFF` must not panic.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = TsTmpFile::parse(&data);
}

/// `TsTmpFile::parse` on 256 bytes of `0x00` must not panic.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = TsTmpFile::parse(&data);
}
