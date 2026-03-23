// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

// ── Helpers ────────────────────────────────────────────────────────────

/// Builds a synthetic ICN file with `tile_count` tiles of size
/// `tile_w * tile_h`.  Each byte is `(tile_index + byte_index) & 0xFF`
/// so that every tile has a distinct, verifiable pattern.
fn build_icn(tile_w: usize, tile_h: usize, tile_count: usize) -> Vec<u8> {
    let tile_size = tile_w * tile_h;
    let mut out = Vec::with_capacity(tile_count * tile_size);
    for i in 0..tile_count {
        for j in 0..tile_size {
            out.push(((i + j) & 0xFF) as u8);
        }
    }
    out
}

/// Builds a synthetic ICON.MAP file from a slice of `u16` entries.
fn build_icon_map(entries: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * 2);
    for &e in entries {
        out.extend_from_slice(&e.to_le_bytes());
    }
    out
}

// ── IcnFile parsing ───────────────────────────────────────────────────

/// Three 16x16 tiles parse correctly and report the right tile count.
///
/// Why: validates the happy path for the standard Dune II tile size.
#[test]
fn parse_valid_icn() {
    let data = build_icn(16, 16, 3);
    let icn = IcnFile::parse(&data, 16, 16).unwrap();
    assert_eq!(icn.tile_count(), 3);
    assert_eq!(icn.tile_width(), 16);
    assert_eq!(icn.tile_height(), 16);
}

/// `tile()` returns the correct pixel data for each tile.
///
/// Why: verifies that tile indexing computes the right byte offsets and
/// that the returned slice matches the builder's pattern.
#[test]
fn tile_access() {
    let data = build_icn(16, 16, 3);
    let icn = IcnFile::parse(&data, 16, 16).unwrap();

    for tile_idx in 0..3 {
        let pixels = icn.tile(tile_idx).expect("tile should exist");
        assert_eq!(pixels.len(), 256);
        // Verify the first and last bytes match the build pattern.
        assert_eq!(pixels[0], ((tile_idx) & 0xFF) as u8);
        assert_eq!(pixels[255], ((tile_idx + 255) & 0xFF) as u8);
    }
}

/// `tile()` returns `None` for an out-of-bounds index.
///
/// Why: callers must get `None` (not a panic) for invalid indices.
#[test]
fn tile_out_of_bounds() {
    let data = build_icn(16, 16, 2);
    let icn = IcnFile::parse(&data, 16, 16).unwrap();

    assert!(icn.tile(2).is_none());
    assert!(icn.tile(usize::MAX).is_none());
}

/// Data whose length is not a multiple of tile_size is rejected.
///
/// Why: a partial tile at the end indicates a truncated or corrupt file.
#[test]
fn reject_non_multiple() {
    let mut data = build_icn(16, 16, 2);
    data.push(0xAA); // one extra byte
    let err = IcnFile::parse(&data, 16, 16).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ICN tile data",
            ..
        }
    ));
}

/// Zero width is rejected.
///
/// Why: a zero dimension makes the tile size zero, which would cause
/// division by zero when computing tile count.
#[test]
fn reject_zero_width() {
    let err = IcnFile::parse(&[], 0, 16).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ICN tile size",
            ..
        }
    ));
}

/// Zero height is rejected.
///
/// Why: same as `reject_zero_width` but for the other dimension.
#[test]
fn reject_zero_height() {
    let err = IcnFile::parse(&[], 16, 0).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ICN tile size",
            ..
        }
    ));
}

/// Dimensions exceeding MAX_TILE_DIMENSION are rejected.
///
/// Why: guards against absurd dimensions that could cause large
/// allocations or overflow.
#[test]
fn reject_too_large_dimension() {
    let err = IcnFile::parse(&[], 65, 16).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ICN tile size",
            ..
        }
    ));

    let err = IcnFile::parse(&[], 16, 65).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ICN tile size",
            ..
        }
    ));
}

/// An empty data slice with valid dimensions parses as zero tiles.
///
/// Why: zero tiles is a valid degenerate case (0 / tile_size == 0).
#[test]
fn parse_empty_data_zero_tiles() {
    let icn = IcnFile::parse(&[], 16, 16).unwrap();
    assert_eq!(icn.tile_count(), 0);
    assert!(icn.tile(0).is_none());
}

// ── IcnMap parsing ────────────────────────────────────────────────────

/// Five ICON.MAP entries parse correctly and report the right values.
///
/// Why: validates the happy path for a small map file.
#[test]
fn parse_valid_icon_map() {
    let entries = [0u16, 1, 42, 1000, 65535];
    let data = build_icon_map(&entries);
    let map = IcnMap::parse(&data).unwrap();

    assert_eq!(map.len(), 5);
    assert!(!map.is_empty());
    assert_eq!(map.entries(), &entries);
}

/// `get()` returns the correct entry for valid indices and `None` for
/// out-of-bounds indices.
///
/// Why: verifies both the valid and invalid paths of indexed access.
#[test]
fn icon_map_get() {
    let entries = [10u16, 20, 30];
    let data = build_icon_map(&entries);
    let map = IcnMap::parse(&data).unwrap();

    assert_eq!(map.get(0), Some(10));
    assert_eq!(map.get(1), Some(20));
    assert_eq!(map.get(2), Some(30));
    assert_eq!(map.get(3), None);
    assert_eq!(map.get(usize::MAX), None);
}

/// An ICON.MAP file with an odd byte count is rejected.
///
/// Why: each entry is 2 bytes; an odd total means the file is truncated.
#[test]
fn reject_odd_map_size() {
    let data = vec![0u8; 5]; // 5 bytes = 2 entries + 1 trailing byte
    let err = IcnMap::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ICON.MAP entries",
            ..
        }
    ));
}

/// An empty ICON.MAP file parses as zero entries.
///
/// Why: an empty map is a valid degenerate case.
#[test]
fn parse_empty_icon_map() {
    let map = IcnMap::parse(&[]).unwrap();
    assert_eq!(map.len(), 0);
    assert!(map.is_empty());
    assert_eq!(map.get(0), None);
}

// ── Adversarial / robustness tests ────────────────────────────────────

/// All-`0xFF` ICN data does not panic.
///
/// Why: `0xFF` is a valid palette index; the parser must not treat any
/// byte value as special.
#[test]
fn adversarial_icn_all_ff() {
    let data = vec![0xFFu8; 256 * 4]; // 4 tiles of 16x16
    let icn = IcnFile::parse(&data, 16, 16).unwrap();
    assert_eq!(icn.tile_count(), 4);
    let tile = icn.tile(0).unwrap();
    assert!(tile.iter().all(|&b| b == 0xFF));
}

/// All-zero ICN data at 0x0 tile size is rejected (zero dimension).
///
/// Why: 0x0 dimensions must be caught before division by zero.
#[test]
fn adversarial_icn_all_zero() {
    let err = IcnFile::parse(&[0u8; 256], 0, 0).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ICN tile size",
            ..
        }
    ));
}
