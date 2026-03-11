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
//! ## TD Layout
//!
//! ```text
//! [TdTmpHeader]       20 bytes
//! [icon index]         width × height bytes  (maps grid position → tile ID)
//! [tile data ...]      tile_count × (tile_w × tile_h) bytes
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
const TD_HEADER_SIZE: usize = 20;

/// Parsed header for a **Tiberian Dawn** TMP file.
///
/// The header describes a grid of tiles.  `width` × `height` gives the grid
/// dimensions (in tiles, not pixels).  `tile_count` is the number of distinct
/// tile images stored in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdTmpHeader {
    /// Grid width in tiles.
    pub width: u16,
    /// Grid height in tiles.
    pub height: u16,
    /// Number of distinct tile images stored in the file.
    pub tile_count: u16,
    /// Allocated tile slots (often == `tile_count`, used by the game engine).
    pub allocated: u16,
    /// Tile pixel width (typically 24).
    pub tile_w: u16,
    /// Tile pixel height (typically 24).
    pub tile_h: u16,
    /// File data size from header (informational, may not match actual size).
    pub file_size: u32,
    /// Image data offset — where tile pixel data begins.
    pub image_start: u32,
    /// Palette data offset (0 if no embedded palette).
    pub palette_start: u32,
    /// Flags field.
    pub flags: u32,
}

/// A single tile image from a Tiberian Dawn TMP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdTmpTile<'a> {
    /// Index of this tile in the icon map.
    pub index: u8,
    /// Raw palette-indexed pixel data (tile_w × tile_h bytes).
    pub pixels: &'a [u8],
}

/// Parsed Tiberian Dawn TMP file.
///
/// The `icon_map` maps each grid position (row-major, `width × height`) to a
/// tile image index.  Actual tile image data is in `tiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdTmpFile<'a> {
    /// File header.
    pub header: TdTmpHeader,
    /// Icon map: one byte per grid cell, mapping grid position to tile index.
    pub icon_map: &'a [u8],
    /// The distinct tile images stored in the file.
    pub tiles: Vec<TdTmpTile<'a>>,
}

impl<'a> TdTmpFile<'a> {
    /// Parses a Tiberian Dawn TMP file from a raw byte slice.
    ///
    /// # Layout
    ///
    /// The header (20 bytes) is followed by an icon map of `width × height`
    /// bytes, then `tile_count` tile images each of `tile_w × tile_h` bytes.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, zero dimensions, or tile counts
    /// that would exceed the V38 safety cap.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // ── Header ───────────────────────────────────────────────────────
        if data.len() < TD_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: TD_HEADER_SIZE,
                available: data.len(),
            });
        }

        let width = read_u16_le(data, 0)?;
        let height = read_u16_le(data, 2)?;
        let tile_count = read_u16_le(data, 4)?;
        let allocated = read_u16_le(data, 6)?;
        let tile_w = read_u16_le(data, 8)?;
        let tile_h = read_u16_le(data, 10)?;
        let file_size = read_u32_le(data, 12)?;
        let image_start = read_u32_le(data, 16)?;

        // TD header has additional fields for palette and flags, but many
        // files only have the core 20 bytes.  We default the rest to zero.
        let palette_start = 0u32;
        let flags = 0u32;

        // V38: validate dimensions are non-zero when tile_count > 0.
        if tile_count > 0 && (tile_w == 0 || tile_h == 0) {
            return Err(Error::InvalidSize {
                value: 0,
                limit: 1,
                context: "TD TMP tile dimensions must be non-zero",
            });
        }

        // V38: cap tile count.
        if (tile_count as usize) > MAX_TILE_COUNT {
            return Err(Error::InvalidSize {
                value: tile_count as usize,
                limit: MAX_TILE_COUNT,
                context: "TD TMP tile count",
            });
        }

        // V38: cap tile pixel area.
        let tile_area = (tile_w as usize).saturating_mul(tile_h as usize);
        if tile_area > MAX_TILE_AREA {
            return Err(Error::InvalidSize {
                value: tile_area,
                limit: MAX_TILE_AREA,
                context: "TD TMP tile area",
            });
        }

        let header = TdTmpHeader {
            width,
            height,
            tile_count,
            allocated,
            tile_w,
            tile_h,
            file_size,
            image_start,
            palette_start,
            flags,
        };

        // ── Icon Map ─────────────────────────────────────────────────────
        let grid_size = (width as usize).saturating_mul(height as usize);
        let map_start = TD_HEADER_SIZE;
        let map_end = map_start.saturating_add(grid_size);
        let icon_map = data.get(map_start..map_end).ok_or(Error::UnexpectedEof {
            needed: map_end,
            available: data.len(),
        })?;

        // ── Tile Data ────────────────────────────────────────────────────
        // Use image_start if non-zero, otherwise tiles immediately follow
        // the icon map.
        let tiles_offset = if image_start > 0 {
            image_start as usize
        } else {
            map_end
        };

        let tc = tile_count as usize;
        let mut tiles = Vec::with_capacity(tc);
        for i in 0..tc {
            let tile_start = tiles_offset.saturating_add(i.saturating_mul(tile_area));
            let tile_end = tile_start.saturating_add(tile_area);
            let pixels = data.get(tile_start..tile_end).ok_or(Error::UnexpectedEof {
                needed: tile_end,
                available: data.len(),
            })?;
            // The icon map index for this tile is its sequential position.
            tiles.push(TdTmpTile {
                index: i as u8,
                pixels,
            });
        }

        Ok(TdTmpFile {
            header,
            icon_map,
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
pub struct RaTmpTile<'a> {
    /// Column index in the tile grid.
    pub col: u32,
    /// Row index in the tile grid.
    pub row: u32,
    /// Raw palette-indexed pixel data (`tile_width × tile_height` bytes).
    pub pixels: &'a [u8],
}

/// Parsed Red Alert TMP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaTmpFile<'a> {
    /// File header.
    pub header: RaTmpHeader,
    /// Tiles in grid order (row-major).  `None` entries are empty/transparent.
    pub tiles: Vec<Option<RaTmpTile<'a>>>,
}

impl<'a> RaTmpFile<'a> {
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
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
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

#[cfg(test)]
mod tests;
