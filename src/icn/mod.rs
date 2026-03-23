// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Dune II icon/tile graphics parser (`.icn` + `ICON.MAP`).
//!
//! ICN files contain raw palette-indexed tile graphics.  Each tile is a
//! fixed-size block of pixels (typically 16x16 = 256 bytes).  A companion
//! ICON.MAP file provides index-to-tile mappings for tilesets.
//!
//! ## ICN Layout
//!
//! ```text
//! [Tile 0]   tile_width * tile_height bytes (palette indices)
//! [Tile 1]   ...
//! ```

use crate::error::Error;
use crate::read::read_u16_le;

/// Maximum allowed value for tile width or height.
const MAX_TILE_DIMENSION: usize = 64;

/// Maximum number of tiles an ICN file may contain.
const MAX_TILES: usize = 65_536;

/// Maximum number of entries an ICON.MAP file may contain.
const MAX_MAP_ENTRIES: usize = 65_536;

/// A parsed ICN tile graphics file.
///
/// Borrows the original data slice and provides indexed access to
/// individual tiles.  Each tile is a contiguous run of
/// `tile_width * tile_height` palette-indexed bytes.
#[derive(Debug)]
pub struct IcnFile<'input> {
    tile_width: usize,
    tile_height: usize,
    tile_count: usize,
    data: &'input [u8],
}

impl<'input> IcnFile<'input> {
    /// Parses an ICN file from raw bytes.
    ///
    /// The caller specifies the tile dimensions because the file itself
    /// contains no header.  Standard Dune II tiles are 16x16 pixels.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidSize`] if:
    /// - `tile_width` or `tile_height` is zero or exceeds `MAX_TILE_DIMENSION`.
    /// - `data.len()` is not an exact multiple of the tile size.
    /// - The computed tile count exceeds `MAX_TILES`.
    pub fn parse(data: &'input [u8], tile_width: usize, tile_height: usize) -> Result<Self, Error> {
        // Validate tile dimensions.
        if tile_width == 0 || tile_width > MAX_TILE_DIMENSION {
            return Err(Error::InvalidSize {
                value: tile_width,
                limit: MAX_TILE_DIMENSION,
                context: "ICN tile size",
            });
        }
        if tile_height == 0 || tile_height > MAX_TILE_DIMENSION {
            return Err(Error::InvalidSize {
                value: tile_height,
                limit: MAX_TILE_DIMENSION,
                context: "ICN tile size",
            });
        }

        // Compute tile_size with overflow protection.
        let tile_size = tile_width
            .checked_mul(tile_height)
            .ok_or(Error::InvalidSize {
                value: usize::MAX,
                limit: MAX_TILE_DIMENSION.saturating_mul(MAX_TILE_DIMENSION),
                context: "ICN tile size",
            })?;

        // Data length must be an exact multiple of tile_size.
        if data.len() % tile_size != 0 {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: tile_size,
                context: "ICN tile data",
            });
        }

        let tile_count = data.len() / tile_size;

        if tile_count > MAX_TILES {
            return Err(Error::InvalidSize {
                value: tile_count,
                limit: MAX_TILES,
                context: "ICN tile data",
            });
        }

        Ok(Self {
            tile_width,
            tile_height,
            tile_count,
            data,
        })
    }

    /// Returns the tile width in pixels.
    pub fn tile_width(&self) -> usize {
        self.tile_width
    }

    /// Returns the tile height in pixels.
    pub fn tile_height(&self) -> usize {
        self.tile_height
    }

    /// Returns the number of tiles in the file.
    pub fn tile_count(&self) -> usize {
        self.tile_count
    }

    /// Returns the raw pixel data for the tile at `index`, or `None` if
    /// the index is out of range.
    ///
    /// The returned slice contains `tile_width * tile_height` bytes of
    /// palette-indexed pixel data.
    pub fn tile(&self, index: usize) -> Option<&'input [u8]> {
        let tile_size = self.tile_width.checked_mul(self.tile_height)?;
        let start = index.checked_mul(tile_size)?;
        self.data.get(start..start.checked_add(tile_size)?)
    }
}

/// A parsed ICON.MAP index table.
///
/// Each entry is a `u16` that maps a tileset index to an ICN tile number.
#[derive(Debug)]
pub struct IcnMap {
    entries: Vec<u16>,
}

impl IcnMap {
    /// Parses an ICON.MAP file from raw bytes.
    ///
    /// The file is a flat array of little-endian `u16` values with no
    /// header.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidSize`] if:
    /// - `data.len()` is odd (not a whole number of `u16` entries).
    /// - The entry count exceeds `MAX_MAP_ENTRIES`.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() % 2 != 0 {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: 2,
                context: "ICON.MAP entries",
            });
        }

        let entry_count = data.len() / 2;

        if entry_count > MAX_MAP_ENTRIES {
            return Err(Error::InvalidSize {
                value: entry_count,
                limit: MAX_MAP_ENTRIES,
                context: "ICON.MAP entries",
            });
        }

        let mut entries = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let offset = i.checked_mul(2).ok_or(Error::InvalidSize {
                value: usize::MAX,
                limit: MAX_MAP_ENTRIES,
                context: "ICON.MAP entries",
            })?;
            entries.push(read_u16_le(data, offset)?);
        }

        Ok(Self { entries })
    }

    /// Returns all map entries as a slice.
    pub fn entries(&self) -> &[u16] {
        &self.entries
    }

    /// Returns the number of entries in the map.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the tile index at the given position, or `None` if out of
    /// range.
    pub fn get(&self, index: usize) -> Option<u16> {
        self.entries.get(index).copied()
    }
}

#[cfg(test)]
mod tests;
