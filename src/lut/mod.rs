// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Red Alert Chrono Vortex lookup tables (`.lut`).
//!
//! These files are stored as 4,096 triplets with no header:
//!
//! ```text
//! [x: u8, y: u8, value: u8] × 4096
//! ```
//!
//! In the shipped Red Alert assets they appear as `HOLE0000.LUT` through
//! `HOLE0047.LUT` and drive the Chrono Vortex effect.
//!
//! The value ranges are tightly constrained:
//!
//! - `x`: 0..=63
//! - `y`: 0..=63
//! - `value`: 0..=15

use crate::error::Error;

/// Number of entries in one LUT file.
pub const LUT_ENTRY_COUNT: usize = 4096;

/// Number of bytes per LUT entry.
pub const LUT_ENTRY_SIZE: usize = 3;

/// Exact on-disk size of one LUT file.
pub const LUT_FILE_SIZE: usize = LUT_ENTRY_COUNT * LUT_ENTRY_SIZE;

/// One lookup-table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LutEntry {
    /// X coordinate or source component (0..=63).
    pub x: u8,
    /// Y coordinate or source component (0..=63).
    pub y: u8,
    /// Lookup value (0..=15).
    pub value: u8,
}

/// Parsed Chrono Vortex lookup table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LutFile {
    /// All parsed entries in on-disk order.
    pub entries: Vec<LutEntry>,
}

impl LutFile {
    /// Parses a Red Alert Chrono Vortex LUT file.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() != LUT_FILE_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: LUT_FILE_SIZE,
                context: "LUT file size",
            });
        }

        let mut entries = Vec::with_capacity(LUT_ENTRY_COUNT);
        for chunk in data.chunks_exact(LUT_ENTRY_SIZE) {
            let x = *chunk.first().unwrap_or(&0);
            let y = *chunk.get(1).unwrap_or(&0);
            let value = *chunk.get(2).unwrap_or(&0);

            if x > 63 {
                return Err(Error::InvalidSize {
                    value: x as usize,
                    limit: 63,
                    context: "LUT x component",
                });
            }
            if y > 63 {
                return Err(Error::InvalidSize {
                    value: y as usize,
                    limit: 63,
                    context: "LUT y component",
                });
            }
            if value > 15 {
                return Err(Error::InvalidSize {
                    value: value as usize,
                    limit: 15,
                    context: "LUT value component",
                });
            }

            entries.push(LutEntry { x, y, value });
        }

        Ok(Self { entries })
    }

    /// Returns the number of entries in the table.
    #[inline]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests;
