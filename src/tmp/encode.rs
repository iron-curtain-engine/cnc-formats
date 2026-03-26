// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! TD TMP encoder: builds valid Tiberian Dawn terrain tile files.
//!
//! Split from `mod.rs` to keep that file under the 600-line cap.
//! Provides [`encode_td_tmp`] (public production function) and
//! `build_td_test_tmp` (crate-internal test helper).

use super::TD_HEADER_SIZE;
use crate::error::Error;

/// Encodes palette-indexed tiles into a complete TD TMP file.
///
/// Each tile in `tiles` must be exactly `tile_width × tile_height` bytes of
/// palette-indexed pixel data.  The output is a valid TD TMP file that
/// [`super::TdTmpFile::parse`] can round-trip.
///
/// # Errors
///
/// Returns [`Error::InvalidSize`] if any tile has the wrong pixel count.
pub fn encode_td_tmp(tiles: &[&[u8]], tile_width: u16, tile_height: u16) -> Result<Vec<u8>, Error> {
    let tile_area = (tile_width as usize).saturating_mul(tile_height as usize);
    for tile in tiles {
        if tile.len() != tile_area {
            return Err(Error::InvalidSize {
                value: tile.len(),
                limit: tile_area,
                context: "TD TMP tile pixel count mismatch",
            });
        }
    }

    let count = tiles.len() as u16;
    let map_start = TD_HEADER_SIZE;
    let map_size = count as usize;
    let icons_start = map_start + map_size;
    let total = icons_start + (count as usize) * tile_area;

    // Buffer is pre-sized to `total` — all writes below are within bounds
    // by construction.  The `buf_len` binding avoids borrow-checker conflicts
    // between `.get_mut()` and error-context calls.
    let mut buf = vec![0u8; total];
    let buf_len = buf.len();

    // IControl_Type header (32 bytes).
    let eof = |needed| Error::UnexpectedEof {
        needed,
        available: buf_len,
    };

    buf.get_mut(0..2)
        .ok_or(eof(2))?
        .copy_from_slice(&tile_width.to_le_bytes());
    buf.get_mut(2..4)
        .ok_or(eof(4))?
        .copy_from_slice(&tile_height.to_le_bytes());
    buf.get_mut(4..6)
        .ok_or(eof(6))?
        .copy_from_slice(&count.to_le_bytes());
    buf.get_mut(6..8)
        .ok_or(eof(8))?
        .copy_from_slice(&count.to_le_bytes());
    buf.get_mut(8..12)
        .ok_or(eof(12))?
        .copy_from_slice(&(total as u32).to_le_bytes());
    buf.get_mut(12..16)
        .ok_or(eof(16))?
        .copy_from_slice(&(icons_start as u32).to_le_bytes());
    // palettes_offset, remaps_offset, trans_flag_offset: 0 (already zeroed).
    buf.get_mut(28..32)
        .ok_or(eof(32))?
        .copy_from_slice(&(map_start as u32).to_le_bytes());

    // Map data: sequential tile indices.
    for i in 0..map_size {
        if let Some(slot) = buf.get_mut(map_start + i) {
            *slot = i as u8;
        }
    }

    // Tile pixel data.
    for (t, tile) in tiles.iter().enumerate() {
        let start = icons_start + t * tile_area;
        if let Some(dest) = buf.get_mut(start..start + tile_area) {
            dest.copy_from_slice(tile);
        }
    }

    Ok(buf)
}

/// Builds a minimal 4×4, 2-tile TD TMP for cross-module testing.
#[cfg(all(test, feature = "convert"))]
pub(crate) fn build_td_test_tmp() -> Vec<u8> {
    let iw: u16 = 4;
    let ih: u16 = 4;
    let count: u16 = 2;
    let icon_area = (iw as usize) * (ih as usize);
    let map_start = TD_HEADER_SIZE;
    let map_size = count as usize;
    let icons_start = map_start + map_size;
    let total = icons_start + (count as usize) * icon_area;
    let mut buf = vec![0u8; total];

    // IControl_Type header (32 bytes).
    buf[0..2].copy_from_slice(&iw.to_le_bytes());
    buf[2..4].copy_from_slice(&ih.to_le_bytes());
    buf[4..6].copy_from_slice(&count.to_le_bytes());
    buf[6..8].copy_from_slice(&count.to_le_bytes());
    buf[8..12].copy_from_slice(&(total as u32).to_le_bytes());
    buf[12..16].copy_from_slice(&(icons_start as u32).to_le_bytes());
    buf[16..20].copy_from_slice(&0u32.to_le_bytes());
    buf[20..24].copy_from_slice(&0u32.to_le_bytes());
    buf[24..28].copy_from_slice(&0u32.to_le_bytes());
    buf[28..32].copy_from_slice(&(map_start as u32).to_le_bytes());

    // Map data: sequential indices.
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
