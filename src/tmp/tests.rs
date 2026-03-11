// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ─── TD test helpers ─────────────────────────────────────────────────────────

/// Builds a minimal valid Tiberian Dawn TMP file.
///
/// Constructs a `grid_w × grid_h` grid with `tile_count` tiles, each
/// `tile_w × tile_h` pixels.  The icon map maps each cell to
/// `cell_index % tile_count`.  Tile pixels are filled with the tile index
/// as a byte value.
fn build_td_tmp(grid_w: u16, grid_h: u16, tile_count: u16, tile_w: u16, tile_h: u16) -> Vec<u8> {
    let grid_size = (grid_w as usize) * (grid_h as usize);
    let tile_area = (tile_w as usize) * (tile_h as usize);
    let map_end = 20 + grid_size;
    let total = map_end + (tile_count as usize) * tile_area;
    let mut buf = vec![0u8; total];

    // Header: width, height, tile_count, allocated, tile_w, tile_h,
    // file_size (u32), image_start (u32).
    buf[0..2].copy_from_slice(&grid_w.to_le_bytes());
    buf[2..4].copy_from_slice(&grid_h.to_le_bytes());
    buf[4..6].copy_from_slice(&tile_count.to_le_bytes());
    buf[6..8].copy_from_slice(&tile_count.to_le_bytes()); // allocated = tile_count
    buf[8..10].copy_from_slice(&tile_w.to_le_bytes());
    buf[10..12].copy_from_slice(&tile_h.to_le_bytes());
    buf[12..16].copy_from_slice(&(total as u32).to_le_bytes());
    buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // image_start = 0

    // Icon map: sequential tile indices (or 0 if no tiles).
    for i in 0..grid_size {
        buf[20 + i] = if tile_count == 0 {
            0
        } else {
            (i % (tile_count as usize)) as u8
        };
    }

    // Tile pixel data: each tile filled with its index value.
    for t in 0..tile_count as usize {
        let start = map_end + t * tile_area;
        for p in 0..tile_area {
            buf[start + p] = t as u8;
        }
    }

    buf
}

// ─── RA test helpers ─────────────────────────────────────────────────────────

/// Builds a minimal valid Red Alert TMP file.
///
/// Constructs a grid of `cols × rows` tiles, each `tw × th` pixels.
/// `empty_indices` lists grid positions (0-based) that should have offset 0
/// (empty/transparent).  Non-empty tile pixels are filled with
/// `(grid_index & 0xFF)`.
fn build_ra_tmp(image_w: u32, image_h: u32, tw: u32, th: u32, empty_indices: &[usize]) -> Vec<u8> {
    let cols = image_w / tw;
    let rows = image_h / th;
    let grid_count = (cols * rows) as usize;
    let tile_area = (tw * th) as usize;

    // Count non-empty tiles to size the buffer.
    let non_empty = grid_count - empty_indices.len();
    let offsets_size = grid_count * 4;
    let header_and_offsets = 16 + offsets_size;
    let total = header_and_offsets + non_empty * tile_area;
    let mut buf = vec![0u8; total];

    // Header.
    buf[0..4].copy_from_slice(&image_w.to_le_bytes());
    buf[4..8].copy_from_slice(&image_h.to_le_bytes());
    buf[8..12].copy_from_slice(&tw.to_le_bytes());
    buf[12..16].copy_from_slice(&th.to_le_bytes());

    // Offset table and tile data.
    let mut data_pos = header_and_offsets;
    for i in 0..grid_count {
        let offset_pos = 16 + i * 4;
        if empty_indices.contains(&i) {
            // Offset 0 = empty tile.
            buf[offset_pos..offset_pos + 4].copy_from_slice(&0u32.to_le_bytes());
        } else {
            buf[offset_pos..offset_pos + 4].copy_from_slice(&(data_pos as u32).to_le_bytes());
            // Fill tile pixels.
            for p in 0..tile_area {
                buf[data_pos + p] = (i & 0xFF) as u8;
            }
            data_pos += tile_area;
        }
    }

    buf
}

// ─── TD basic functionality ──────────────────────────────────────────────────

/// Parses a well-formed TD TMP with a 2×2 grid and 2 distinct tiles.
#[test]
fn td_parse_basic() {
    let data = build_td_tmp(2, 2, 2, 24, 24);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.width, 2);
    assert_eq!(tmp.header.height, 2);
    assert_eq!(tmp.header.tile_count, 2);
    assert_eq!(tmp.header.tile_w, 24);
    assert_eq!(tmp.header.tile_h, 24);
    assert_eq!(tmp.tiles.len(), 2);
    assert_eq!(tmp.icon_map.len(), 4);
    // First tile should be filled with 0x00.
    assert!(tmp.tiles[0].pixels.iter().all(|&b| b == 0));
    // Second tile should be filled with 0x01.
    assert!(tmp.tiles[1].pixels.iter().all(|&b| b == 1));
}

/// Parses a TD TMP with zero tiles and a 1×1 grid.
///
/// Files with zero tile_count are valid (empty template).  The icon map
/// still exists (1 byte), but no tile data follows.
#[test]
fn td_parse_zero_tiles() {
    let data = build_td_tmp(1, 1, 0, 24, 24);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.tile_count, 0);
    assert_eq!(tmp.tiles.len(), 0);
    assert_eq!(tmp.icon_map.len(), 1);
}

/// Parses a TD TMP with custom (non-24) tile dimensions.
///
/// The parser must accept arbitrary tile sizes, not just 24×24.
#[test]
fn td_parse_custom_tile_size() {
    let data = build_td_tmp(1, 1, 1, 16, 16);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.tile_w, 16);
    assert_eq!(tmp.header.tile_h, 16);
    assert_eq!(tmp.tiles[0].pixels.len(), 256);
}

// ─── TD error paths ──────────────────────────────────────────────────────────

/// Input shorter than the TD header (20 bytes) returns UnexpectedEof.
#[test]
fn td_truncated_header() {
    let data = [0u8; 19];
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 20,
            available: 19
        }
    ));
}

/// Truncated icon map returns UnexpectedEof.
#[test]
fn td_truncated_icon_map() {
    // 2×2 grid needs 4 bytes of icon map after 20-byte header = 24 bytes.
    let data = build_td_tmp(2, 2, 1, 24, 24);
    let short = &data[..22]; // header + only 2 map bytes
    let err = TdTmpFile::parse(short).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Truncated tile data returns UnexpectedEof.
#[test]
fn td_truncated_tile_data() {
    let mut data = build_td_tmp(1, 1, 1, 24, 24);
    data.truncate(data.len() - 1);
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Zero tile dimensions with non-zero tile_count returns InvalidSize.
///
/// Prevents division by zero and zero-area allocations.
#[test]
fn td_zero_tile_dimensions() {
    let mut data = build_td_tmp(1, 1, 1, 24, 24);
    // Set tile_w = 0.
    data[8..10].copy_from_slice(&0u16.to_le_bytes());
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { value: 0, .. }));
}

// ─── TD determinism ──────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn td_deterministic() {
    let data = build_td_tmp(3, 2, 4, 24, 24);
    let a = TdTmpFile::parse(&data).unwrap();
    let b = TdTmpFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ─── TD V38 boundary tests ──────────────────────────────────────────────────

/// Tile count at the V38 cap (4096) is accepted.
///
/// A file with 4096 1×1 tiles is small enough to fit in memory even on
/// constrained systems.
#[test]
fn td_max_tile_count_accepted() {
    let data = build_td_tmp(1, 1, 4096, 1, 1);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.tiles.len(), 4096);
}

/// Tile count one past the V38 cap (4097) is rejected.
#[test]
fn td_over_max_tile_count_rejected() {
    // We can't use build_td_tmp for 4097 tiles easily — just build a
    // minimal header that claims 4097 tiles.
    let mut data = vec![0u8; 20 + 1]; // header + 1 icon-map byte
    data[0..2].copy_from_slice(&1u16.to_le_bytes()); // width=1
    data[2..4].copy_from_slice(&1u16.to_le_bytes()); // height=1
    data[4..6].copy_from_slice(&4097u16.to_le_bytes()); // tile_count=4097
    data[6..8].copy_from_slice(&4097u16.to_le_bytes()); // allocated
    data[8..10].copy_from_slice(&1u16.to_le_bytes()); // tile_w=1
    data[10..12].copy_from_slice(&1u16.to_le_bytes()); // tile_h=1
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 4097,
            limit: 4096,
            ..
        }
    ));
}

// ─── TD integer overflow safety ──────────────────────────────────────────────

/// Grid dimensions that would overflow `width * height` use saturating
/// arithmetic and return an appropriate error instead of panicking.
#[test]
fn td_grid_overflow_no_panic() {
    let mut data = vec![0u8; 20];
    data[0..2].copy_from_slice(&0xFFFFu16.to_le_bytes()); // width
    data[2..4].copy_from_slice(&0xFFFFu16.to_le_bytes()); // height
    data[4..6].copy_from_slice(&0u16.to_le_bytes()); // tile_count=0
    data[8..10].copy_from_slice(&24u16.to_le_bytes());
    data[10..12].copy_from_slice(&24u16.to_le_bytes());
    // Icon map would need 0xFFFF * 0xFFFF bytes — way more than we have.
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ─── RA basic functionality ──────────────────────────────────────────────────

/// Parses a well-formed RA TMP: 2×2 tiles, all non-empty.
#[test]
fn ra_parse_basic() {
    let data = build_ra_tmp(48, 48, 24, 24, &[]);
    let tmp = RaTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.image_width, 48);
    assert_eq!(tmp.header.image_height, 48);
    assert_eq!(tmp.header.tile_width, 24);
    assert_eq!(tmp.header.tile_height, 24);
    assert_eq!(tmp.tiles.len(), 4);
    // All tiles should be Some.
    assert!(tmp.tiles.iter().all(|t| t.is_some()));
    // First tile pixels filled with 0.
    assert!(tmp.tiles[0]
        .as_ref()
        .unwrap()
        .pixels
        .iter()
        .all(|&b| b == 0));
}

/// Parses an RA TMP with empty tiles (offset = 0).
///
/// Empty tiles are used for irregularly-shaped templates where some grid
/// cells are transparent.
#[test]
fn ra_parse_with_empty_tiles() {
    let data = build_ra_tmp(48, 48, 24, 24, &[1, 3]);
    let tmp = RaTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.tiles.len(), 4);
    assert!(tmp.tiles[0].is_some());
    assert!(tmp.tiles[1].is_none()); // empty
    assert!(tmp.tiles[2].is_some());
    assert!(tmp.tiles[3].is_none()); // empty
}

/// Grid position (col, row) is correctly assigned to each tile.
#[test]
fn ra_tile_grid_positions() {
    let data = build_ra_tmp(72, 48, 24, 24, &[]);
    let tmp = RaTmpFile::parse(&data).unwrap();
    // 3 cols × 2 rows = 6 tiles.
    assert_eq!(tmp.tiles.len(), 6);
    let t0 = tmp.tiles[0].as_ref().unwrap();
    assert_eq!((t0.col, t0.row), (0, 0));
    let t2 = tmp.tiles[2].as_ref().unwrap();
    assert_eq!((t2.col, t2.row), (2, 0));
    let t3 = tmp.tiles[3].as_ref().unwrap();
    assert_eq!((t3.col, t3.row), (0, 1));
}

/// Header accessors `cols()` and `rows()` return correct values.
#[test]
fn ra_header_cols_rows() {
    let header = RaTmpHeader {
        image_width: 72,
        image_height: 48,
        tile_width: 24,
        tile_height: 24,
    };
    assert_eq!(header.cols(), 3);
    assert_eq!(header.rows(), 2);
}

/// `cols()` / `rows()` return 0 when tile dimensions are 0 (no division
/// by zero).
#[test]
fn ra_header_zero_tile_dims() {
    let header = RaTmpHeader {
        image_width: 48,
        image_height: 48,
        tile_width: 0,
        tile_height: 0,
    };
    assert_eq!(header.cols(), 0);
    assert_eq!(header.rows(), 0);
}

// ─── RA error paths ──────────────────────────────────────────────────────────

/// Input shorter than the RA header (16 bytes) returns UnexpectedEof.
#[test]
fn ra_truncated_header() {
    let data = [0u8; 15];
    let err = RaTmpFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 16,
            available: 15
        }
    ));
}

/// Zero tile dimensions in RA format returns InvalidSize.
#[test]
fn ra_zero_tile_dimensions() {
    let mut data = vec![0u8; 16];
    data[0..4].copy_from_slice(&48u32.to_le_bytes()); // image_width
    data[4..8].copy_from_slice(&48u32.to_le_bytes()); // image_height
    data[8..12].copy_from_slice(&0u32.to_le_bytes()); // tile_width = 0
    data[12..16].copy_from_slice(&24u32.to_le_bytes());
    let err = RaTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { value: 0, .. }));
}

/// Truncated offset table returns UnexpectedEof.
#[test]
fn ra_truncated_offset_table() {
    // 2×2 tiles = 4 offsets = 16 bytes; header = 16 bytes; total needed = 32.
    let mut data = vec![0u8; 28]; // 4 bytes short
    data[0..4].copy_from_slice(&48u32.to_le_bytes());
    data[4..8].copy_from_slice(&48u32.to_le_bytes());
    data[8..12].copy_from_slice(&24u32.to_le_bytes());
    data[12..16].copy_from_slice(&24u32.to_le_bytes());
    let err = RaTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Tile offset pointing past end of data returns UnexpectedEof.
#[test]
fn ra_invalid_tile_offset() {
    // Build a valid file, then corrupt a tile offset to point past EOF.
    let mut data = build_ra_tmp(24, 24, 24, 24, &[]);
    // The offset table starts at byte 16; overwrite first offset to 0xFFFF.
    data[16..20].copy_from_slice(&0xFFFFu32.to_le_bytes());
    let err = RaTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ─── RA determinism ──────────────────────────────────────────────────────────

/// Parsing the same RA TMP input twice yields identical results.
#[test]
fn ra_deterministic() {
    let data = build_ra_tmp(72, 48, 24, 24, &[2]);
    let a = RaTmpFile::parse(&data).unwrap();
    let b = RaTmpFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ─── RA V38 boundary tests ──────────────────────────────────────────────────

/// Grid count at the V38 cap (4096) is accepted (64×64 grid).
#[test]
fn ra_max_grid_count_accepted() {
    // 64 cols × 64 rows = 4096 tiles.  Use 1×1 tiles to keep data small.
    let data = build_ra_tmp(64, 64, 1, 1, &[]);
    let tmp = RaTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.tiles.len(), 4096);
}

/// Grid count over the V38 cap is rejected.
#[test]
fn ra_over_max_grid_count_rejected() {
    // Header claiming 65×64 = 4160 tiles with 1×1 dims.
    let mut data = vec![0u8; 16];
    data[0..4].copy_from_slice(&65u32.to_le_bytes());
    data[4..8].copy_from_slice(&64u32.to_le_bytes());
    data[8..12].copy_from_slice(&1u32.to_le_bytes());
    data[12..16].copy_from_slice(&1u32.to_le_bytes());
    let err = RaTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { limit: 4096, .. }));
}

// ─── RA Display messages ─────────────────────────────────────────────────────

/// Error Display output includes numeric context.
#[test]
fn ra_error_display_includes_values() {
    let data = [0u8; 10];
    let err = RaTmpFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("16"));
    assert!(msg.contains("10"));
}

/// TD error Display output includes numeric context.
#[test]
fn td_error_display_includes_values() {
    let data = [0u8; 5];
    let err = TdTmpFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("20"));
    assert!(msg.contains("5"));
}

// ─── Security adversarial tests ──────────────────────────────────────────────

/// All-0xFF input must not panic on TD parser.
///
/// Why: maximum field values for width, height, tile_count, tile_w, tile_h
/// would trigger overflow in size calculations without saturating arithmetic.
#[test]
fn td_adversarial_all_ff_no_panic() {
    let data = [0xFF; 256];
    let _ = TdTmpFile::parse(&data);
}

/// All-0xFF input must not panic on RA parser.
#[test]
fn ra_adversarial_all_ff_no_panic() {
    let data = [0xFF; 256];
    let _ = RaTmpFile::parse(&data);
}

/// RA file with image dimensions that aren't multiples of tile dimensions.
///
/// Why: `cols = image_width / tile_width` is integer division.  A remainder
/// is acceptable — the parser should not round up and create phantom tiles.
#[test]
fn ra_non_divisible_dimensions() {
    // 50×50 image with 24×24 tiles → 2×2 = 4 grid tiles (50/24 = 2).
    let data = build_ra_tmp(50, 50, 24, 24, &[]);
    let tmp = RaTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.cols(), 2);
    assert_eq!(tmp.header.rows(), 2);
    assert_eq!(tmp.tiles.len(), 4);
}

/// All-zero TD input (exactly header size) must not panic.
#[test]
fn td_adversarial_all_zero_no_panic() {
    let data = [0u8; 20];
    let _ = TdTmpFile::parse(&data);
}
