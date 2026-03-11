// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! FNT bitmap font parser (`.fnt`).
//!
//! FNT files store fixed-height bitmap fonts used for in-game text rendering.
//! Each file contains up to 256 glyphs (one per byte value), each stored as
//! a column-major 1-bit-per-pixel bitmap.
//!
//! ## File Layout
//!
//! ```text
//! [FntHeader]            6 bytes
//! [char widths]          256 × u16 LE  (pixel width of each glyph)
//! [char offsets]         256 × u16 LE  (byte offset to glyph data)
//! [glyph data ...]       variable-length bitmap data
//! ```
//!
//! ## Glyph Encoding
//!
//! Each glyph is stored as `ceil(height / 8) × width` bytes.  The glyph
//! is drawn column-by-column, left to right.  Each column is stored as
//! `ceil(height / 8)` bytes, with bits packed top-to-bottom: bit 0 of
//! the first byte is the topmost pixel of that column.
//!
//! A glyph with width 0 has no pixel data (space character).
//!
//! ## References
//!
//! Format source: community documentation from the C&C Modding Wiki,
//! XCC Utilities source code, and binary analysis of game `.mix` archives.

use crate::error::Error;
use crate::read::{read_u16_le, read_u8};
use alloc::vec::Vec;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Number of character entries in a FNT file (one per byte value).
pub const FNT_CHAR_COUNT: usize = 256;

/// Size of the FNT file header in bytes.
const FNT_HEADER_SIZE: usize = 6;

/// Size of the character width table (256 × u16 = 512 bytes).
const CHAR_WIDTHS_SIZE: usize = FNT_CHAR_COUNT * 2;

/// Size of the character offset table (256 × u16 = 512 bytes).
const CHAR_OFFSETS_SIZE: usize = FNT_CHAR_COUNT * 2;

/// Minimum file size: header + width table + offset table.
const MIN_FILE_SIZE: usize = FNT_HEADER_SIZE + CHAR_WIDTHS_SIZE + CHAR_OFFSETS_SIZE;

/// V38: maximum font height in pixels.  Real-world FNT files use heights
/// of 6–16 pixels; 256 provides generous headroom.
const MAX_FONT_HEIGHT: usize = 256;

/// V38: maximum glyph width in pixels.  Same rationale as MAX_FONT_HEIGHT.
const MAX_GLYPH_WIDTH: usize = 256;

// ─── Header ──────────────────────────────────────────────────────────────────

/// Parsed FNT file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FntHeader {
    /// Total file data size (from header, informational).
    pub data_size: u16,
    /// Font height in pixels (all glyphs share the same height).
    pub height: u8,
    /// Maximum glyph width in pixels.
    pub max_width: u8,
    /// Unknown/reserved field (typically 0).
    pub unknown: u16,
}

// ─── Glyph ───────────────────────────────────────────────────────────────────

/// A single glyph from a FNT file.
///
/// The glyph's pixel data is column-major, 1 bit per pixel, packed into
/// `ceil(height / 8)` bytes per column.  Use [`FntGlyph::pixel`] to
/// query individual pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FntGlyph<'a> {
    /// Character code point (0–255).
    pub code: u8,
    /// Glyph width in pixels.
    pub width: u16,
    /// Font height in pixels (shared across all glyphs).
    pub height: u8,
    /// Raw column-major bitmap data (borrowed from input).
    pub data: &'a [u8],
}

impl FntGlyph<'_> {
    /// Returns `true` if the pixel at `(x, y)` is set (foreground).
    ///
    /// Coordinates: `x` is column (0 = left), `y` is row (0 = top).
    /// Returns `false` for out-of-bounds coordinates or zero-width glyphs.
    #[inline]
    pub fn pixel(&self, x: u16, y: u8) -> bool {
        if x >= self.width || y >= self.height || self.data.is_empty() {
            return false;
        }
        // Each column is ceil(height / 8) bytes.
        let bytes_per_col = (self.height as usize).div_ceil(8);
        let col_start = (x as usize).saturating_mul(bytes_per_col);
        let byte_idx = col_start.saturating_add(y as usize / 8);
        let bit_idx = y % 8;
        // Use .get() for defense-in-depth on the bitmap data.
        self.data
            .get(byte_idx)
            .is_some_and(|&b| (b >> bit_idx) & 1 != 0)
    }
}

// ─── Parsed File ─────────────────────────────────────────────────────────────

/// Parsed FNT bitmap font file.
///
/// Contains the font header and all 256 glyph entries.  Glyphs with
/// width 0 have empty data (space characters, unused code points).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FntFile<'a> {
    /// File header.
    pub header: FntHeader,
    /// All 256 glyph entries (indexed by character code).
    pub glyphs: Vec<FntGlyph<'a>>,
}

impl<'a> FntFile<'a> {
    /// Parses a FNT file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] if the input is truncated.
    /// - [`Error::InvalidSize`] if font height exceeds the V38 cap.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // ── Header ───────────────────────────────────────────────────────
        if data.len() < MIN_FILE_SIZE {
            return Err(Error::UnexpectedEof {
                needed: MIN_FILE_SIZE,
                available: data.len(),
            });
        }

        let data_size = read_u16_le(data, 0)?;
        let height = read_u8(data, 2)?;
        let max_width = read_u8(data, 3)?;
        let unknown = read_u16_le(data, 4)?;

        // V38: cap font height.
        if (height as usize) > MAX_FONT_HEIGHT {
            return Err(Error::InvalidSize {
                value: height as usize,
                limit: MAX_FONT_HEIGHT,
                context: "FNT font height",
            });
        }

        let header = FntHeader {
            data_size,
            height,
            max_width,
            unknown,
        };

        // ── Width + Offset Tables ────────────────────────────────────────
        let widths_start = FNT_HEADER_SIZE;
        let offsets_start = widths_start.saturating_add(CHAR_WIDTHS_SIZE);

        // The bytes_per_col for this font height.
        let bytes_per_col = (height as usize).div_ceil(8);

        // ── Glyphs ───────────────────────────────────────────────────────
        let mut glyphs = Vec::with_capacity(FNT_CHAR_COUNT);

        for i in 0..FNT_CHAR_COUNT {
            let w_pos = widths_start.saturating_add(i.saturating_mul(2));
            let glyph_width = read_u16_le(data, w_pos)?;

            // V38: cap individual glyph width.
            if (glyph_width as usize) > MAX_GLYPH_WIDTH {
                return Err(Error::InvalidSize {
                    value: glyph_width as usize,
                    limit: MAX_GLYPH_WIDTH,
                    context: "FNT glyph width",
                });
            }

            let o_pos = offsets_start.saturating_add(i.saturating_mul(2));
            let glyph_offset = read_u16_le(data, o_pos)? as usize;

            // Zero-width glyphs have no pixel data.
            if glyph_width == 0 {
                glyphs.push(FntGlyph {
                    code: i as u8,
                    width: 0,
                    height,
                    data: &[],
                });
                continue;
            }

            // Glyph data size: width × ceil(height / 8).
            let glyph_size = (glyph_width as usize).saturating_mul(bytes_per_col);

            // The glyph offset is relative to the start of the file.
            let glyph_end = glyph_offset.saturating_add(glyph_size);
            let glyph_data = data
                .get(glyph_offset..glyph_end)
                .ok_or(Error::UnexpectedEof {
                    needed: glyph_end,
                    available: data.len(),
                })?;

            glyphs.push(FntGlyph {
                code: i as u8,
                width: glyph_width,
                height,
                data: glyph_data,
            });
        }

        Ok(FntFile { header, glyphs })
    }
}

#[cfg(test)]
mod tests;
