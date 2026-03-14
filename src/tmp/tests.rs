// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ─── TD test helpers ─────────────────────────────────────────────────────────

/// Builds a minimal valid Tiberian Dawn TMP file matching `IControl_Type`.
///
/// Constructs an icon set with `count` tiles, each `iw × ih` pixels.
/// The map data (at offset `map_off`) contains `count` sequential byte
/// values.  Tile pixels are filled with the tile index as a byte value.
///
/// Layout:
/// ```text
/// [0..32]       IControl_Type header (32 bytes)
/// [32..32+map]  Map data (count bytes)
/// [icons_off..] Icon pixel data (count × iw × ih bytes)
/// ```
fn build_td_tmp(iw: u16, ih: u16, count: u16) -> Vec<u8> {
    let icon_area = (iw as usize) * (ih as usize);
    let map_start = TD_HEADER_SIZE; // 32
    let map_size = count as usize;
    let icons_start = map_start + map_size;
    let total = icons_start + (count as usize) * icon_area;
    let mut buf = vec![0u8; total];

    // Header: IControl_Type (32 bytes).
    buf[0..2].copy_from_slice(&iw.to_le_bytes()); // icon_width
    buf[2..4].copy_from_slice(&ih.to_le_bytes()); // icon_height
    buf[4..6].copy_from_slice(&count.to_le_bytes()); // count
    buf[6..8].copy_from_slice(&count.to_le_bytes()); // allocated = count
    buf[8..12].copy_from_slice(&(total as u32).to_le_bytes()); // size
    buf[12..16].copy_from_slice(&(icons_start as u32).to_le_bytes()); // icons_offset
    buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // palettes_offset
    buf[20..24].copy_from_slice(&0u32.to_le_bytes()); // remaps_offset
    buf[24..28].copy_from_slice(&0u32.to_le_bytes()); // trans_flag_offset
    buf[28..32].copy_from_slice(&(map_start as u32).to_le_bytes()); // map_offset

    // Map data: sequential indices (or 0 if no tiles).
    for i in 0..map_size {
        buf[map_start + i] = i as u8;
    }

    // Tile pixel data: each tile filled with its index value.
    for t in 0..count as usize {
        let start = icons_start + t * icon_area;
        for p in 0..icon_area {
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

/// Parses a well-formed TD TMP with 2 distinct 24×24 tiles.
#[test]
fn td_parse_basic() {
    let data = build_td_tmp(24, 24, 2);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.icon_width, 24);
    assert_eq!(tmp.header.icon_height, 24);
    assert_eq!(tmp.header.count, 2);
    assert_eq!(tmp.tiles.len(), 2);
    assert_eq!(tmp.map_data.len(), 2);
    // First tile should be filled with 0x00.
    assert!(tmp.tiles[0].pixels.iter().all(|&b| b == 0));
    // Second tile should be filled with 0x01.
    assert!(tmp.tiles[1].pixels.iter().all(|&b| b == 1));
}

/// Parses a TD TMP with zero tiles.
///
/// Files with zero count are valid (empty template).  No map data or
/// tile data follows.
#[test]
fn td_parse_zero_tiles() {
    let data = build_td_tmp(24, 24, 0);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.count, 0);
    assert_eq!(tmp.tiles.len(), 0);
    assert!(tmp.map_data.is_empty());
}

/// Parses a TD TMP with custom (non-24) tile dimensions.
///
/// The parser must accept arbitrary icon sizes, not just 24×24.
#[test]
fn td_parse_custom_tile_size() {
    let data = build_td_tmp(16, 16, 1);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.header.icon_width, 16);
    assert_eq!(tmp.header.icon_height, 16);
    assert_eq!(tmp.tiles[0].pixels.len(), 256);
}

// ─── TD error paths ──────────────────────────────────────────────────────────

/// Input shorter than the TD header (32 bytes) returns UnexpectedEof.
#[test]
fn td_truncated_header() {
    let data = [0u8; 31];
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 32,
            available: 31
        }
    ));
}

/// Truncated map data returns UnexpectedEof.
#[test]
fn td_truncated_icon_map() {
    // 2 tiles with map_offset pointing to offset 32, needs 2 bytes.
    // Supply only 33 bytes (just 1 map byte instead of 2).
    let mut data = vec![0u8; 33];
    data[0..2].copy_from_slice(&24u16.to_le_bytes()); // icon_width
    data[2..4].copy_from_slice(&24u16.to_le_bytes()); // icon_height
    data[4..6].copy_from_slice(&2u16.to_le_bytes()); // count=2
    data[6..8].copy_from_slice(&2u16.to_le_bytes()); // allocated
    data[8..12].copy_from_slice(&0u32.to_le_bytes()); // size
    data[12..16].copy_from_slice(&0u32.to_le_bytes()); // icons_offset
    data[28..32].copy_from_slice(&32u32.to_le_bytes()); // map_offset=32
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Truncated tile data returns UnexpectedEof.
#[test]
fn td_truncated_tile_data() {
    let mut data = build_td_tmp(24, 24, 1);
    data.truncate(data.len() - 1);
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Zero icon dimensions with non-zero count returns InvalidSize.
///
/// Prevents division by zero and zero-area allocations.
#[test]
fn td_zero_tile_dimensions() {
    let mut data = build_td_tmp(24, 24, 1);
    // Set icon_width = 0.
    data[0..2].copy_from_slice(&0u16.to_le_bytes());
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { value: 0, .. }));
}

// ─── TD determinism ──────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn td_deterministic() {
    let data = build_td_tmp(24, 24, 4);
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
    let data = build_td_tmp(1, 1, 4096);
    let tmp = TdTmpFile::parse(&data).unwrap();
    assert_eq!(tmp.tiles.len(), 4096);
}

/// Tile count one past the V38 cap (4097) is rejected.
#[test]
fn td_over_max_tile_count_rejected() {
    let mut data = vec![0u8; TD_HEADER_SIZE];
    data[0..2].copy_from_slice(&1u16.to_le_bytes()); // icon_width=1
    data[2..4].copy_from_slice(&1u16.to_le_bytes()); // icon_height=1
    data[4..6].copy_from_slice(&4097u16.to_le_bytes()); // count=4097
    data[6..8].copy_from_slice(&4097u16.to_le_bytes()); // allocated
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

/// Icon dimensions that would overflow `icon_width * icon_height` use
/// saturating arithmetic and return an appropriate error instead of panicking.
#[test]
fn td_grid_overflow_no_panic() {
    let mut data = vec![0u8; TD_HEADER_SIZE];
    data[0..2].copy_from_slice(&0xFFFFu16.to_le_bytes()); // icon_width
    data[2..4].copy_from_slice(&0xFFFFu16.to_le_bytes()); // icon_height
    data[4..6].copy_from_slice(&1u16.to_le_bytes()); // count=1
    let err = TdTmpFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
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
    assert!(msg.contains("32"));
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
    let data = [0u8; 32];
    let _ = TdTmpFile::parse(&data);
}

/// All-zero RA input (header + some zero offset bytes) must not panic.
///
/// Why (V38): zero image_width, image_height, tile_width, tile_height
/// exercises the zero-dimension path including division-by-zero guards.
#[test]
fn ra_adversarial_all_zero_no_panic() {
    let data = [0u8; 256];
    let _ = RaTmpFile::parse(&data);
}

// ─── RA integer overflow safety ──────────────────────────────────────────────

/// RA header with near-`u32::MAX` dimensions must not panic.
///
/// Why: `image_width * image_height` or `cols * rows` could overflow
/// without saturating arithmetic.  The parser must reject gracefully.
#[test]
fn ra_dimension_overflow_no_panic() {
    let mut data = vec![0u8; 256];
    // image_width = 0xFFFF_FFFF, image_height = 0xFFFF_FFFF
    data[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    data[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    // tile_width = 1, tile_height = 1 (to maximise cols * rows)
    data[8..12].copy_from_slice(&1u32.to_le_bytes());
    data[12..16].copy_from_slice(&1u32.to_le_bytes());
    let err = RaTmpFile::parse(&data);
    assert!(err.is_err());
}

// ── TD TMP encoder round-trip tests ─────────────────────────────────

/// Encoding 2 tiles (24x24) then parsing the result recovers the original pixels.
///
/// Why: the encoder must produce a valid `IControl_Type` layout that the
/// parser can read back.  Two tiles exercise the multi-tile offset and
/// map-data paths.
#[test]
fn encode_td_tmp_round_trip() {
    let tile0 = vec![0xAAu8; 24 * 24];
    let tile1 = vec![0x55u8; 24 * 24];
    let tiles: Vec<&[u8]> = vec![&tile0, &tile1];

    let encoded = encode_td_tmp(&tiles, 24, 24).unwrap();
    let parsed = TdTmpFile::parse(&encoded).unwrap();

    assert_eq!(parsed.header.icon_width, 24);
    assert_eq!(parsed.header.icon_height, 24);
    assert_eq!(parsed.header.count, 2);
    assert_eq!(parsed.tiles.len(), 2);
    assert_eq!(parsed.tiles[0].pixels, &tile0[..]);
    assert_eq!(parsed.tiles[1].pixels, &tile1[..]);
}

/// Encoding a single tile then parsing recovers the original pixels.
///
/// Why: a single-tile file is the minimum non-empty case.  The map data
/// has exactly one entry and the icon offset table starts immediately.
#[test]
fn encode_td_tmp_single_tile() {
    let tile = vec![0x42u8; 24 * 24];
    let tiles: Vec<&[u8]> = vec![&tile];

    let encoded = encode_td_tmp(&tiles, 24, 24).unwrap();
    let parsed = TdTmpFile::parse(&encoded).unwrap();

    assert_eq!(parsed.header.count, 1);
    assert_eq!(parsed.tiles.len(), 1);
    assert_eq!(parsed.tiles[0].pixels, &tile[..]);
}

/// Encoding zero tiles produces a valid empty TD TMP file.
///
/// Why: an empty icon set (count=0) is valid — the parser must accept
/// the header-only output without tile or map data.
#[test]
fn encode_td_tmp_empty() {
    let tiles: Vec<&[u8]> = vec![];

    let encoded = encode_td_tmp(&tiles, 24, 24).unwrap();
    let parsed = TdTmpFile::parse(&encoded).unwrap();

    assert_eq!(parsed.header.count, 0);
    assert_eq!(parsed.tiles.len(), 0);
    assert!(parsed.map_data.is_empty());
}
