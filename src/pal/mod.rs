// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! PAL palette parser (`.pal`).
//!
//! A `.pal` file is the simplest format in the C&C asset library: exactly
//! 768 raw bytes with no header, no footer, no magic number.
//!
//! ## Layout
//!
//! ```text
//! 768 bytes = 256 entries × 3 bytes (R, G, B)
//! ```
//!
//! ## 6-bit VGA Color Range
//!
//! Each R/G/B component is in the **6-bit VGA range (0–63)**, matching the
//! original VGA DAC register width.  To convert to the modern 8-bit (0–255)
//! range, shift each component left by 2 bits (`value << 2`).
//!
//! ## References
//!
//! Implemented from binary analysis of game `.pal` files — the format is
//! 768 raw bytes with no structure to reverse-engineer.  Cross-reference:
//! the original game defines the layout in `PALETTE.H` / `LOADPAL.CPP`.

use crate::error::Error;

/// Number of colors in a C&C palette.
pub const PALETTE_SIZE: usize = 256;

/// Raw byte size of a `.pal` file on disk.
pub const PALETTE_BYTES: usize = PALETTE_SIZE * 3;

/// A single RGB color entry in the **6-bit VGA range** (components 0–63).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PalColor {
    /// Red channel (0–63).
    pub r: u8,
    /// Green channel (0–63).
    pub g: u8,
    /// Blue channel (0–63).
    pub b: u8,
}

impl PalColor {
    /// Converts this 6-bit color to 8-bit by shifting each component left 2.
    ///
    /// This matches the conversion performed by the original engine when
    /// programming the VGA DAC:
    /// ```c
    /// buffer[i] = palette[i] << 2;  // 6-bit (0–63) → 8-bit (0–252)
    /// ```
    ///
    /// The `& 0x3F` mask reproduces VGA DAC hardware behaviour: the DAC
    /// ignores the top two bits of each component, so values > 63 in a
    /// modded or corrupt PAL file are silently truncated rather than
    /// causing arithmetic overflow on the left shift.
    #[inline]
    pub fn to_rgb8(self) -> [u8; 3] {
        [
            (self.r & 0x3F) << 2,
            (self.g & 0x3F) << 2,
            (self.b & 0x3F) << 2,
        ]
    }
}

/// A parsed 256-color C&C palette.
///
/// All color values are stored in the canonical **6-bit VGA range** (0–63).
/// Use [`PalColor::to_rgb8`] or [`Palette::to_rgb8_array`] for display-ready values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// The 256 color entries in 6-bit VGA range.
    pub colors: [PalColor; PALETTE_SIZE],
}

impl Palette {
    /// Parses a `.pal` file from a raw byte slice.
    ///
    /// The slice must be exactly [`PALETTE_BYTES`] (768) bytes long.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if `data` is shorter than 768 bytes.
    /// Returns [`Error::InvalidSize`] if `data` is longer than 768 bytes.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() < PALETTE_BYTES {
            return Err(Error::UnexpectedEof {
                needed: PALETTE_BYTES,
                available: data.len(),
            });
        }
        if data.len() > PALETTE_BYTES {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: PALETTE_BYTES,
                context: "PAL file size",
            });
        }
        let mut colors = [PalColor { r: 0, g: 0, b: 0 }; PALETTE_SIZE];
        for (i, color) in colors.iter_mut().enumerate() {
            // Safe: upfront check guarantees 768 bytes; .get() is
            // defense-in-depth against future changes to the check.
            let base = i * 3;
            let triple = data.get(base..base + 3).ok_or(Error::UnexpectedEof {
                needed: base + 3,
                available: data.len(),
            })?;
            // Safe via .get(): triple is a 3-byte slice from .get(base..base+3).
            color.r = triple.first().copied().unwrap_or(0);
            color.g = triple.get(1).copied().unwrap_or(0);
            color.b = triple.get(2).copied().unwrap_or(0);
        }
        Ok(Palette { colors })
    }

    /// Returns all 256 colors converted to 8-bit RGB triples.
    ///
    /// Each `[u8; 3]` is `[R, G, B]` in the range 0–252 (multiples of 4).
    /// Useful for building a lookup table once, then indexing by palette
    /// index when rendering SHP frame pixels.
    pub fn to_rgb8_array(&self) -> [[u8; 3]; PALETTE_SIZE] {
        let mut out = [[0u8; 3]; PALETTE_SIZE];
        for (i, c) in self.colors.iter().enumerate() {
            if let Some(slot) = out.get_mut(i) {
                *slot = c.to_rgb8();
            }
        }
        out
    }
    /// Encodes this palette back into the raw 768-byte VGA PAL format.
    ///
    /// The output is a valid `.pal` file that [`Palette::parse`] can
    /// round-trip.  Each R/G/B component is written in the original 6-bit
    /// VGA range (0–63).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PALETTE_BYTES);
        for c in &self.colors {
            out.push(c.r);
            out.push(c.g);
            out.push(c.b);
        }
        out
    }

    /// Creates a palette from 8-bit RGB data (256 × 3 bytes, range 0–255).
    ///
    /// Converts from modern 8-bit range to 6-bit VGA range by shifting right
    /// 2 bits (`value >> 2`).  This is the inverse of [`PalColor::to_rgb8`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if `rgb8` is shorter than 768 bytes.
    pub fn from_rgb8(rgb8: &[u8]) -> Result<Self, Error> {
        if rgb8.len() < PALETTE_BYTES {
            return Err(Error::UnexpectedEof {
                needed: PALETTE_BYTES,
                available: rgb8.len(),
            });
        }
        let mut colors = [PalColor { r: 0, g: 0, b: 0 }; PALETTE_SIZE];
        for (i, color) in colors.iter_mut().enumerate() {
            let base = i * 3;
            let triple = rgb8.get(base..base + 3).ok_or(Error::UnexpectedEof {
                needed: base + 3,
                available: rgb8.len(),
            })?;
            color.r = triple.first().copied().unwrap_or(0) >> 2;
            color.g = triple.get(1).copied().unwrap_or(0) >> 2;
            color.b = triple.get(2).copied().unwrap_or(0) >> 2;
        }
        Ok(Palette { colors })
    }
}

#[cfg(test)]
mod tests;
