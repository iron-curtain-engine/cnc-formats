// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Tiberian Dawn / Red Alert 1 terrain grid parser (`.bin`).
//!
//! BIN files store a flat 2D grid of terrain template references used by
//! TD (64×64) and RA1 (up to 128×128) maps.  Each cell is 2 bytes:
//! a template type and an icon index within that template.
//!
//! ## File Layout
//!
//! ```text
//! No header — raw cells:
//! [Cell 0]   template_type: u8, template_icon: u8
//! [Cell 1]   ...
//! ...
//! [Cell N-1]
//! ```
//!
//! ## References
//!
//! Format source: XCC Utilities documentation, CnC-Tools wiki.

use crate::error::Error;
use crate::read::read_u8;

/// Bytes per cell entry in the BIN grid.
const CELL_SIZE: usize = 2;

/// Maximum supported dimension (width or height) for a BIN terrain grid.
///
/// TD maps are 64×64 and RA1 maps are 128×128.  A generous upper bound of
/// 256 prevents accidental multi-gigabyte allocations from corrupt input
/// while still covering all known game variants.
const MAX_DIMENSION: usize = 256;

/// A single terrain cell in the map grid.
///
/// Each cell references a terrain template by type number and selects one
/// sub-tile (icon) within that template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinCell {
    /// Terrain template number.
    pub template_type: u8,
    /// Sub-tile index within the template.
    pub template_icon: u8,
}

/// A parsed BIN terrain grid.
///
/// The grid stores `width × height` cells in row-major order.  Use
/// [`BinMap::cell`] for coordinate-based access or [`BinMap::cells`] for
/// the flat slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinMap {
    width: usize,
    height: usize,
    cells: Vec<BinCell>,
}

impl BinMap {
    /// Parses a terrain grid from a raw byte slice with explicit dimensions.
    ///
    /// The caller specifies the grid `width` and `height` because BIN files
    /// contain no header — TD uses 64×64 and RA1 uses 128×128.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidSize`] if either dimension is zero, exceeds
    ///   `MAX_DIMENSION`, or if the product overflows.
    /// - [`Error::InvalidSize`] if `data.len()` does not equal
    ///   `width * height * 2`.
    pub fn parse(data: &[u8], width: usize, height: usize) -> Result<Self, Error> {
        // Reject zero dimensions.
        if width == 0 {
            return Err(Error::InvalidSize {
                value: 0,
                limit: MAX_DIMENSION,
                context: "BIN terrain grid width",
            });
        }
        if height == 0 {
            return Err(Error::InvalidSize {
                value: 0,
                limit: MAX_DIMENSION,
                context: "BIN terrain grid height",
            });
        }

        // Reject dimensions that exceed the safety cap.
        if width > MAX_DIMENSION {
            return Err(Error::InvalidSize {
                value: width,
                limit: MAX_DIMENSION,
                context: "BIN terrain grid width",
            });
        }
        if height > MAX_DIMENSION {
            return Err(Error::InvalidSize {
                value: height,
                limit: MAX_DIMENSION,
                context: "BIN terrain grid height",
            });
        }

        // Compute total cell count with overflow protection.
        let cell_count = width.checked_mul(height).ok_or(Error::InvalidSize {
            value: width,
            limit: MAX_DIMENSION,
            context: "BIN terrain grid dimension overflow",
        })?;

        let expected_size = cell_count.saturating_mul(CELL_SIZE);

        if data.len() != expected_size {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: expected_size,
                context: "BIN terrain grid",
            });
        }

        // Parse cells using safe read helpers.
        let mut cells = Vec::with_capacity(cell_count);
        for i in 0..cell_count {
            let offset = i.saturating_mul(CELL_SIZE);
            let template_type = read_u8(data, offset)?;
            let template_icon = read_u8(data, offset.saturating_add(1))?;
            cells.push(BinCell {
                template_type,
                template_icon,
            });
        }

        Ok(Self {
            width,
            height,
            cells,
        })
    }

    /// Returns the grid width (number of columns).
    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the grid height (number of rows).
    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Returns all cells as a flat slice in row-major order.
    #[inline]
    pub fn cells(&self) -> &[BinCell] {
        &self.cells
    }

    /// Returns the cell at grid coordinates `(x, y)`, or `None` if
    /// the coordinates are out of bounds.
    ///
    /// `x` is the column (0-based) and `y` is the row (0-based).
    #[inline]
    pub fn cell(&self, x: usize, y: usize) -> Option<&BinCell> {
        if x < self.width && y < self.height {
            self.cells.get(y * self.width + x)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests;
