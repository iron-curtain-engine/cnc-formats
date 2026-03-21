// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! TMP terrain tile parser (`.tmp`).
//!
//! TMP files store terrain tiles (also called "templates" or "icon sets") used
//! to paint the isometric map.  Each tile is a rectangular block of
//! palette-indexed pixels, typically 24×24 pixels.
//!
//! ## Variants
//!
//! Two main variants exist, distinguished by their origin game:
//!
//! - **Tiberian Dawn (TD):** compact format with a flat icon array.  An icon
//!   map selects which tile images appear at each grid position.
//! - **Red Alert (RA):** richer format with a per-tile offset table that
//!   allows individual tiles to be absent (offset = 0 → transparent/empty).
//!
//! Both variants share the concept of a rectangular grid of tiles, but differ
//! in header layout and tile addressing.
//!
//! ## TD Layout (`IControl_Type`)
//!
//! ```text
//! [TdTmpHeader]       32 bytes  (IControl_Type: 4×i16 + 6×i32)
//! [map data]           count bytes  (at Map offset)
//! [tile pixel data]    count × (icon_width × icon_height) bytes  (at Icons offset)
//! ```
//!
//! ## RA Layout
//!
//! ```text
//! [RaTmpHeader]       16 bytes
//! [offset table]      grid_cols × grid_rows × u32   (0 = empty tile)
//! [tile data ...]     each non-empty tile: 576 bytes (24 × 24)
//! ```
//!
//! ## Relationship to `binary-codecs.md`
//!
//! `binary-codecs.md` documents two TMP representations:
//!
//! 1. **IFF-chunked source format** — ICON / SINF / SSET / TRNS / MAP /
//!    RPAL / RTBL chunks, parsed by `Load_Icon_Set()` during development.
//! 2. **`IControl_Type` flat-binary format** — a single contiguous allocation
//!    with self-referencing long offsets (Icons, Palettes, Remaps, TransFlag,
//!    Map, ColorMap), documented in the "In-Memory Control Structure" section.
//!
//! The game's build pipeline pre-processes IFF source files into
//! `IControl_Type` blobs for the shipped MIX archives.  This parser targets
//! the `IControl_Type` flat representation — the form that community tools
//! (XCC Utilities, ModEnc) document and that appears inside MIX entries.
//! Both representations are canonical; no design divergence exists.
//!
//! ## References
//!
//! Format source: community documentation from the C&C Modding Wiki,
//! XCC Utilities source code, and binary analysis of game `.mix` archives.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Default tile pixel width used by both TD and RA variants.
pub const DEFAULT_TILE_WIDTH: u32 = 24;

/// Default tile pixel height used by both TD and RA variants.
pub const DEFAULT_TILE_HEIGHT: u32 = 24;

/// Byte size of one standard 24×24 tile.
pub const TILE_BYTES_24X24: usize = 24 * 24;

/// V38: maximum number of tiles per TMP file.  Terrain templates never
/// exceed a few hundred tiles in practice; 4096 provides ample headroom
/// while capping the offset-table allocation to ~16 KB.
const MAX_TILE_COUNT: usize = 4096;

/// V38: maximum allowed tile pixel area.  Prevents degenerate tile
/// dimensions (e.g. `tile_w = 65535, tile_h = 65535`) from causing
/// enormous allocations.  1 MB of pixels per tile is far beyond anything
/// used by original game files.
const MAX_TILE_AREA: usize = 1024 * 1024;

// ─── TD Variant ──────────────────────────────────────────────────────────────

/// Size of the Tiberian Dawn TMP header in bytes.
///
/// Matches the `IControl_Type` struct from `tiberiandawn/tile.h`:
/// 4 × `int16_t` (8 bytes) + 6 × `int32_t` (24 bytes) = 32 bytes.
const TD_HEADER_SIZE: usize = 32;

/// Parsed header for a **Tiberian Dawn** TMP file.
///
/// Layout matches the `IControl_Type` struct from the original game source
/// (`tiberiandawn/tile.h`).  The header describes an icon set: a collection
/// of `count` tile images, each `icon_width × icon_height` pixels.
///
/// Grid dimensions (how tiles are arranged in a template) are NOT stored in
/// this header — they come from the game's template database.  The `Map`
/// offset points to a mapping table that the game uses with externally-known
/// grid dimensions.
///
/// ```text
/// Offset  Type     Field           Description
/// 0       i16      Width           Icon pixel width (typically 24)
/// 2       i16      Height          Icon pixel height (typically 24)
/// 4       i16      Count           Number of icons in the set
/// 6       i16      Allocated       Allocated icon slots
/// 8       i32      Size            Total data size
/// 12      i32      Icons           Offset to icon pixel data
/// 16      i32      Palettes        Offset to palette data
/// 20      i32      Remaps          Offset to remap tables
/// 24      i32      TransFlag       Offset to transparency flags
/// 28      i32      Map             Offset to icon map
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdTmpHeader {
    /// Width of each icon in pixels (typically 24).
    pub icon_width: u16,
    /// Height of each icon in pixels (typically 24).
    pub icon_height: u16,
    /// Number of icons (tile images) in the set.
    pub count: u16,
    /// Allocated icon slots (often == `count`).
    pub allocated: u16,
    /// Total data size of the icon set blob.
    pub size: u32,
    /// Byte offset to icon pixel data within the blob.
    pub icons_offset: u32,
    /// Byte offset to palette data (0 if unused).
    pub palettes_offset: u32,
    /// Byte offset to remap tables (0 if unused).
    pub remaps_offset: u32,
    /// Byte offset to transparency flag data (0 if unused).
    pub trans_flag_offset: u32,
    /// Byte offset to icon map data (0 if unused).
    pub map_offset: u32,
}

/// A single tile image from a Tiberian Dawn TMP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdTmpTile<'input> {
    /// Sequential index of this tile in the icon set.
    pub index: u8,
    /// Raw palette-indexed pixel data (`icon_width × icon_height` bytes).
    pub pixels: &'input [u8],
}

/// Parsed Tiberian Dawn TMP file.
///
/// The `map_data` contains the raw icon-map bytes from the `Map` offset
/// in the header.  Its length is `count` bytes (one per icon slot).
/// The game uses this with externally-known grid dimensions to map
/// grid positions to icon images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdTmpFile<'input> {
    /// File header (matches `IControl_Type`).
    pub header: TdTmpHeader,
    /// Raw icon-map data: `count` bytes from the `Map` offset.
    /// Each byte maps an icon slot to a display position.
    /// Empty if `map_offset` is 0.
    pub map_data: &'input [u8],
    /// The distinct tile images stored in the file.
    pub tiles: Vec<TdTmpTile<'input>>,
}

impl<'input> TdTmpFile<'input> {
    /// Parses a Tiberian Dawn TMP file from a raw byte slice.
    ///
    /// # Layout
    ///
    /// The 32-byte `IControl_Type` header is followed by data sections at
    /// the offsets specified by the header fields.  Icon pixel data starts
    /// at `icons_offset`, and the icon map starts at `map_offset`.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, zero dimensions, or tile counts
    /// that would exceed the V38 safety cap.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── Header (32 bytes: 4 × i16 + 6 × i32) ────────────────────────
        if data.len() < TD_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: TD_HEADER_SIZE,
                available: data.len(),
            });
        }

        let icon_width = read_u16_le(data, 0)?;
        let icon_height = read_u16_le(data, 2)?;
        let count = read_u16_le(data, 4)?;
        let allocated = read_u16_le(data, 6)?;
        let size = read_u32_le(data, 8)?;
        let icons_offset = read_u32_le(data, 12)?;
        let palettes_offset = read_u32_le(data, 16)?;
        let remaps_offset = read_u32_le(data, 20)?;
        let trans_flag_offset = read_u32_le(data, 24)?;
        let map_offset = read_u32_le(data, 28)?;

        // V38: validate icon dimensions are non-zero when count > 0.
        if count > 0 && (icon_width == 0 || icon_height == 0) {
            return Err(Error::InvalidSize {
                value: 0,
                limit: 1,
                context: "TD TMP icon dimensions must be non-zero",
            });
        }

        // V38: cap tile count.
        if (count as usize) > MAX_TILE_COUNT {
            return Err(Error::InvalidSize {
                value: count as usize,
                limit: MAX_TILE_COUNT,
                context: "TD TMP tile count",
            });
        }

        // V38: cap tile pixel area.
        let icon_area = (icon_width as usize).saturating_mul(icon_height as usize);
        if icon_area > MAX_TILE_AREA {
            return Err(Error::InvalidSize {
                value: icon_area,
                limit: MAX_TILE_AREA,
                context: "TD TMP tile area",
            });
        }

        let header = TdTmpHeader {
            icon_width,
            icon_height,
            count,
            allocated,
            size,
            icons_offset,
            palettes_offset,
            remaps_offset,
            trans_flag_offset,
            map_offset,
        };

        // ── Map Data ─────────────────────────────────────────────────────
        // The Map offset points to an array of `count` bytes.  Each byte
        // maps an icon slot to a display position in the template grid.
        // Grid dimensions are external (from the game's template database).
        let map_data = if map_offset > 0 {
            let ms = map_offset as usize;
            let me = ms.saturating_add(count as usize);
            data.get(ms..me).ok_or(Error::UnexpectedEof {
                needed: me,
                available: data.len(),
            })?
        } else {
            &[] as &[u8]
        };

        // ── Tile Data ────────────────────────────────────────────────────
        // Use icons_offset if non-zero, otherwise tiles start right after
        // the header.
        let tiles_start = if icons_offset > 0 {
            icons_offset as usize
        } else {
            TD_HEADER_SIZE
        };

        let tc = count as usize;
        let mut tiles = Vec::with_capacity(tc);
        for i in 0..tc {
            let tile_start = tiles_start.saturating_add(i.saturating_mul(icon_area));
            let tile_end = tile_start.saturating_add(icon_area);
            let pixels = data.get(tile_start..tile_end).ok_or(Error::UnexpectedEof {
                needed: tile_end,
                available: data.len(),
            })?;
            tiles.push(TdTmpTile {
                index: i as u8,
                pixels,
            });
        }

        Ok(TdTmpFile {
            header,
            map_data,
            tiles,
        })
    }
}

// ─── RA Variant ──────────────────────────────────────────────────────────────

/// Size of the Red Alert TMP header in bytes.
const RA_HEADER_SIZE: usize = 16;

/// Parsed header for a **Red Alert** TMP file.
///
/// The image dimensions are in pixels; the tile dimensions are fixed at
/// 24×24.  The number of tiles in the grid is
/// `(image_width / tile_width) × (image_height / tile_height)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaTmpHeader {
    /// Total image width in pixels.
    pub image_width: u32,
    /// Total image height in pixels.
    pub image_height: u32,
    /// Tile width in pixels (always 24 in vanilla game files).
    pub tile_width: u32,
    /// Tile height in pixels (always 24 in vanilla game files).
    pub tile_height: u32,
}

impl RaTmpHeader {
    /// Number of tile columns: `image_width / tile_width`.
    #[inline]
    pub fn cols(&self) -> u32 {
        if self.tile_width == 0 {
            return 0;
        }
        self.image_width / self.tile_width
    }

    /// Number of tile rows: `image_height / tile_height`.
    #[inline]
    pub fn rows(&self) -> u32 {
        if self.tile_height == 0 {
            return 0;
        }
        self.image_height / self.tile_height
    }
}

/// A single tile from a Red Alert TMP file.
///
/// Tiles with offset 0 in the offset table are transparent/empty and are
/// represented by `None` in the tile vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaTmpTile<'input> {
    /// Column index in the tile grid.
    pub col: u32,
    /// Row index in the tile grid.
    pub row: u32,
    /// Raw palette-indexed pixel data (`tile_width × tile_height` bytes).
    pub pixels: &'input [u8],
}

/// Parsed Red Alert TMP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaTmpFile<'input> {
    /// File header.
    pub header: RaTmpHeader,
    /// Tiles in grid order (row-major).  `None` entries are empty/transparent.
    pub tiles: Vec<Option<RaTmpTile<'input>>>,
}

impl<'input> RaTmpFile<'input> {
    /// Parses a Red Alert TMP file from a raw byte slice.
    ///
    /// # Layout
    ///
    /// The 16-byte header is followed by an offset table of `cols × rows`
    /// little-endian `u32` values.  Each non-zero offset points to a tile's
    /// pixel data within the file.  Zero offsets indicate empty/transparent
    /// tiles.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, zero tile dimensions, or tile
    /// counts that exceed the V38 safety cap.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── Header ───────────────────────────────────────────────────────
        if data.len() < RA_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: RA_HEADER_SIZE,
                available: data.len(),
            });
        }

        let image_width = read_u32_le(data, 0)?;
        let image_height = read_u32_le(data, 4)?;
        let tile_width = read_u32_le(data, 8)?;
        let tile_height = read_u32_le(data, 12)?;

        let header = RaTmpHeader {
            image_width,
            image_height,
            tile_width,
            tile_height,
        };

        // V38: tile dimensions must be non-zero to avoid division by zero.
        if tile_width == 0 || tile_height == 0 {
            return Err(Error::InvalidSize {
                value: 0,
                limit: 1,
                context: "RA TMP tile dimensions must be non-zero",
            });
        }

        let cols = image_width / tile_width;
        let rows = image_height / tile_height;
        let grid_count = (cols as usize).saturating_mul(rows as usize);

        // V38: cap total tile count.
        if grid_count > MAX_TILE_COUNT {
            return Err(Error::InvalidSize {
                value: grid_count,
                limit: MAX_TILE_COUNT,
                context: "RA TMP grid tile count",
            });
        }

        // V38: cap tile pixel area.
        let tile_area = (tile_width as usize).saturating_mul(tile_height as usize);
        if tile_area > MAX_TILE_AREA {
            return Err(Error::InvalidSize {
                value: tile_area,
                limit: MAX_TILE_AREA,
                context: "RA TMP tile area",
            });
        }

        // ── Offset Table ─────────────────────────────────────────────────
        let offsets_start = RA_HEADER_SIZE;
        let offsets_size = grid_count.saturating_mul(4);
        let offsets_end = offsets_start.saturating_add(offsets_size);
        if offsets_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: offsets_end,
                available: data.len(),
            });
        }

        // ── Tiles ────────────────────────────────────────────────────────
        let mut tiles = Vec::with_capacity(grid_count);
        for i in 0..grid_count {
            let offset_pos = offsets_start.saturating_add(i.saturating_mul(4));
            let tile_offset = read_u32_le(data, offset_pos)? as usize;

            if tile_offset == 0 {
                // Empty / transparent tile.
                tiles.push(None);
                continue;
            }

            let tile_end = tile_offset.saturating_add(tile_area);
            let pixels = data
                .get(tile_offset..tile_end)
                .ok_or(Error::UnexpectedEof {
                    needed: tile_end,
                    available: data.len(),
                })?;

            let col = (i % (cols as usize)) as u32;
            let row = (i / (cols as usize)) as u32;

            tiles.push(Some(RaTmpTile { col, row, pixels }));
        }

        Ok(RaTmpFile { header, tiles })
    }
}

// ── TMP TD Encoder ───────────────────────────────────────────────────────────
//
// Builds a valid Tiberian Dawn TMP file from palette-indexed tile pixels.
// The layout matches `IControl_Type` — the simplest terrain tile format in
// the C&C franchise.

/// Encodes palette-indexed tiles into a complete TD TMP file.
///
/// Each tile in `tiles` must be exactly `tile_width × tile_height` bytes of
/// palette-indexed pixel data.  The output is a valid TD TMP file that
/// [`TdTmpFile::parse`] can round-trip.
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

/// Tiberian Sun / Red Alert 2 isometric terrain tiles.
pub mod ts;
pub use ts::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_ts;
