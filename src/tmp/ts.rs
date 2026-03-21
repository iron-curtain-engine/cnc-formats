// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Tiberian Sun / Red Alert 2 isometric TMP terrain tile parser.
//!
//! TS/RA2 use isometric (diamond-shaped) terrain tiles with additional
//! height data, terrain classification, and optional extra overlay graphics
//! for cliffs, ramps, and buildings.
//!
//! ## Layout
//!
//! ```text
//! [Header]           16 bytes  (tile_width, tile_height, tiles_x, tiles_y)
//! [Offset Table]     tiles_x × tiles_y × 4 bytes  (u32 offsets, 0 = empty)
//! [Tile Data ...]    per non-empty tile:
//!   [Tile Header]    52 bytes  (extra dims, flags, height, terrain, radar)
//!   [Iso Pixels]     tile_width × tile_height / 2 bytes
//!   [Extra Pixels]   extra_width × extra_height bytes  (if flags & 0x01)
//!   [Z Data]         tile_width × tile_height / 2 bytes  (if flags & 0x02)
//! ```
//!
//! ## Standard Dimensions
//!
//! - Tiberian Sun: 48 × 24 pixels per tile
//! - Red Alert 2:  60 × 30 pixels per tile

use crate::error::Error;
use crate::read::{read_u32_le, read_u8};

use super::{MAX_TILE_AREA, MAX_TILE_COUNT};

// ── Constants ─────────────────────────────────────────────────────────────────

/// TS/RA2 isometric tile header size in bytes.
const TS_TILE_HEADER_SIZE: usize = 52;

/// RA header reuse: the file header is 16 bytes, same layout as RA TMP.
const TS_FILE_HEADER_SIZE: usize = 16;

/// Standard Tiberian Sun tile width.
pub const TS_TILE_WIDTH: u32 = 48;

/// Standard Tiberian Sun tile height.
pub const TS_TILE_HEIGHT: u32 = 24;

/// Standard Red Alert 2 tile width.
pub const RA2_TILE_WIDTH: u32 = 60;

/// Standard Red Alert 2 tile height.
pub const RA2_TILE_HEIGHT: u32 = 30;

/// Tile flag: has extra overlay pixel data (cliffs, ramps).
pub const FLAG_HAS_EXTRA: u32 = 0x01;

/// Tile flag: has per-pixel Z (height) data.
pub const FLAG_HAS_Z_DATA: u32 = 0x02;

/// V38: maximum extra overlay area (width × height).
const MAX_EXTRA_AREA: usize = 256 * 256;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Parsed header for a TS/RA2 isometric TMP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsTmpHeader {
    /// Width of each isometric tile in pixels.
    pub tile_width: u32,
    /// Height of each isometric tile in pixels.
    pub tile_height: u32,
    /// Number of tile columns in the grid.
    pub tiles_x: u32,
    /// Number of tile rows in the grid.
    pub tiles_y: u32,
}

/// Radar minimap colour pair for a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRadarColor {
    /// Left pixel colour (R, G, B).
    pub left: (u8, u8, u8),
    /// Right pixel colour (R, G, B).
    pub right: (u8, u8, u8),
}

/// Per-tile metadata from the 52-byte tile header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsTileHeader {
    /// X offset for extra overlay graphics.
    pub x_extra: i32,
    /// Y offset for extra overlay graphics.
    pub y_extra: i32,
    /// Pixel width of extra overlay data.
    pub extra_width: u32,
    /// Pixel height of extra overlay data.
    pub extra_height: u32,
    /// Tile flags (bit 0 = has extra, bit 1 = has Z data).
    pub flags: u32,
    /// Terrain elevation value.
    pub height: u8,
    /// Terrain classification index.
    pub terrain_type: u8,
    /// Ramp/slope type (0 = flat).
    pub ramp_type: u8,
    /// Radar minimap colours.
    pub radar_color: TileRadarColor,
}

/// A single isometric tile from a TS/RA2 TMP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsTmpTile<'input> {
    /// Column index in the tile grid.
    pub col: u32,
    /// Row index in the tile grid.
    pub row: u32,
    /// Per-tile metadata.
    pub header: TsTileHeader,
    /// Diamond-shaped palette-indexed pixels (`tile_width × tile_height / 2` bytes).
    pub iso_pixels: &'input [u8],
    /// Extra overlay pixels (`extra_width × extra_height` bytes), if present.
    pub extra_pixels: Option<&'input [u8]>,
    /// Per-iso-pixel height/Z values, if present.
    pub z_data: Option<&'input [u8]>,
}

/// Parsed Tiberian Sun / Red Alert 2 isometric TMP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsTmpFile<'input> {
    /// File header.
    pub header: TsTmpHeader,
    /// Tiles in grid order (row-major). `None` entries are empty/transparent.
    pub tiles: Vec<Option<TsTmpTile<'input>>>,
}

impl<'input> TsTmpFile<'input> {
    /// Parses a TS/RA2 isometric TMP file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, zero tile dimensions, grid counts
    /// exceeding V38 caps, or insufficient data for declared tiles.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── File Header (16 bytes) ───────────────────────────────────────
        if data.len() < TS_FILE_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: TS_FILE_HEADER_SIZE,
                available: data.len(),
            });
        }

        let tile_width = read_u32_le(data, 0)?;
        let tile_height = read_u32_le(data, 4)?;
        let tiles_x = read_u32_le(data, 8)?;
        let tiles_y = read_u32_le(data, 12)?;

        // V38: tile dimensions must be non-zero and even (iso diamond).
        if tile_width == 0 || tile_height == 0 {
            return Err(Error::InvalidSize {
                value: 0,
                limit: 1,
                context: "TS TMP tile dimensions must be non-zero",
            });
        }

        let grid_count = (tiles_x as usize).saturating_mul(tiles_y as usize);
        if grid_count > MAX_TILE_COUNT {
            return Err(Error::InvalidSize {
                value: grid_count,
                limit: MAX_TILE_COUNT,
                context: "TS TMP grid tile count",
            });
        }

        // Iso pixel count = tile_width × tile_height / 2.
        let iso_pixel_size = (tile_width as usize).saturating_mul(tile_height as usize) / 2;
        if iso_pixel_size > MAX_TILE_AREA {
            return Err(Error::InvalidSize {
                value: iso_pixel_size,
                limit: MAX_TILE_AREA,
                context: "TS TMP iso pixel area",
            });
        }

        let header = TsTmpHeader {
            tile_width,
            tile_height,
            tiles_x,
            tiles_y,
        };

        // ── Offset Table ─────────────────────────────────────────────────
        let offsets_start = TS_FILE_HEADER_SIZE;
        let offsets_size = grid_count.saturating_mul(4);
        let offsets_end = offsets_start.saturating_add(offsets_size);
        if offsets_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: offsets_end,
                available: data.len(),
            });
        }

        // ── Tiles ────────────────────────────────────────────────────────
        let cols = tiles_x as usize;
        let mut tiles = Vec::with_capacity(grid_count);

        for i in 0..grid_count {
            let offset_pos = offsets_start.saturating_add(i.saturating_mul(4));
            let tile_offset = read_u32_le(data, offset_pos)? as usize;

            if tile_offset == 0 {
                tiles.push(None);
                continue;
            }

            // ── Tile Header (52 bytes) ───────────────────────────────────
            let th_end = tile_offset.saturating_add(TS_TILE_HEADER_SIZE);
            if th_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: th_end,
                    available: data.len(),
                });
            }

            let x_extra = read_u32_le(data, tile_offset)? as i32;
            let y_extra = read_u32_le(data, tile_offset.saturating_add(4))? as i32;
            let extra_width = read_u32_le(data, tile_offset.saturating_add(8))?;
            let extra_height = read_u32_le(data, tile_offset.saturating_add(12))?;
            let flags = read_u32_le(data, tile_offset.saturating_add(16))?;
            let height_val = read_u8(data, tile_offset.saturating_add(20))?;
            let terrain_type = read_u8(data, tile_offset.saturating_add(21))?;
            let ramp_type = read_u8(data, tile_offset.saturating_add(22))?;
            let radar_rl = read_u8(data, tile_offset.saturating_add(23))?;
            let radar_gl = read_u8(data, tile_offset.saturating_add(24))?;
            let radar_bl = read_u8(data, tile_offset.saturating_add(25))?;
            let radar_rr = read_u8(data, tile_offset.saturating_add(26))?;
            let radar_gr = read_u8(data, tile_offset.saturating_add(27))?;
            let radar_br = read_u8(data, tile_offset.saturating_add(28))?;

            let tile_header = TsTileHeader {
                x_extra,
                y_extra,
                extra_width,
                extra_height,
                flags,
                height: height_val,
                terrain_type,
                ramp_type,
                radar_color: TileRadarColor {
                    left: (radar_rl, radar_gl, radar_bl),
                    right: (radar_rr, radar_gr, radar_br),
                },
            };

            // ── Iso Pixels ───────────────────────────────────────────────
            let iso_start = th_end;
            let iso_end = iso_start.saturating_add(iso_pixel_size);
            let iso_pixels = data.get(iso_start..iso_end).ok_or(Error::UnexpectedEof {
                needed: iso_end,
                available: data.len(),
            })?;

            let mut cursor = iso_end;

            // ── Extra Pixels (optional) ──────────────────────────────────
            let extra_pixels =
                if (flags & FLAG_HAS_EXTRA) != 0 && extra_width > 0 && extra_height > 0 {
                    let extra_area = (extra_width as usize).saturating_mul(extra_height as usize);
                    if extra_area > MAX_EXTRA_AREA {
                        return Err(Error::InvalidSize {
                            value: extra_area,
                            limit: MAX_EXTRA_AREA,
                            context: "TS TMP extra overlay area",
                        });
                    }
                    let extra_end = cursor.saturating_add(extra_area);
                    let extra = data.get(cursor..extra_end).ok_or(Error::UnexpectedEof {
                        needed: extra_end,
                        available: data.len(),
                    })?;
                    cursor = extra_end;
                    Some(extra)
                } else {
                    None
                };

            // ── Z Data (optional) ────────────────────────────────────────
            let z_data = if (flags & FLAG_HAS_Z_DATA) != 0 {
                let z_end = cursor.saturating_add(iso_pixel_size);
                let z = data.get(cursor..z_end).ok_or(Error::UnexpectedEof {
                    needed: z_end,
                    available: data.len(),
                })?;
                Some(z)
            } else {
                None
            };

            let col = if cols > 0 { (i % cols) as u32 } else { 0 };
            let row = if cols > 0 { (i / cols) as u32 } else { 0 };

            tiles.push(Some(TsTmpTile {
                col,
                row,
                header: tile_header,
                iso_pixels,
                extra_pixels,
                z_data,
            }));
        }

        Ok(TsTmpFile { header, tiles })
    }
}
