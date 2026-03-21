// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! FNT bitmap font parser (`.fnt`).
//!
//! FNT files store fixed-height bitmap fonts used for in-game text rendering
//! in Tiberian Dawn and Red Alert.  Each file contains a variable number of
//! glyphs (up to 256), each stored as **4bpp nibble-packed row-major** data.
//!
//! ## File Layout
//!
//! The header uses a block-offset design: five logical blocks (info, offsets,
//! widths, data, heights) are located by `u16` pointers stored in the header.
//! This matches EA's `FontHeader` struct from `TIBERIANDAWN/WIN32LIB/FONT.H`
//! and the `Buffer_Print` renderer in Vanilla-Conquer's `common/font.cpp`.
//!
//! ```text
//! [FntHeader]            20 bytes  (block-offset header)
//! [info block]           4 bytes   (at InfoBlockOffset, font metrics)
//! [offset table]         num_chars × u16 LE (at OffsetBlockOffset)
//! [width table]          num_chars × u8     (at WidthBlockOffset)
//! [glyph data ...]       variable           (at DataBlockOffset)
//! [height table]         num_chars × u16 LE (at HeightOffset)
//! ```
//!
//! ## Glyph Encoding
//!
//! Each glyph is stored as `ceil(width / 2) × data_rows` bytes.  Pixels are
//! packed two per byte (4 bits each), row-major, low nibble first.  Color
//! index 0 is transparent; indices 1–15 map through a color translation table
//! (not stored in the FNT file — the game supplies it at render time).
//!
//! A glyph with width 0 has no pixel data (space character).
//!
//! ## Height Table
//!
//! Each entry is a `u16` where the low byte is the Y-offset (vertical
//! position of the glyph's first row within the character cell) and the
//! high byte is the number of data rows actually stored.  This allows
//! glyphs to omit leading/trailing transparent rows.
//!
//! ## References
//!
//! - EA FONT.H / FONT.CPP / SET_FONT.CPP / LOADFONT.CPP
//!   (`CnC_Remastered_Collection/TIBERIANDAWN/WIN32LIB/`)
//! - Vanilla-Conquer `common/font.cpp` (decompiled `FontHeader` + `Buffer_Print`)
//! - TXTPRNT.ASM (confirms 4bpp nibble-packed rendering)

use crate::error::Error;
use crate::read::{read_u16_le, read_u8};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Size of the FNT file header in bytes (block-offset header).
const FNT_HEADER_SIZE: usize = 20;

/// Expected number of data blocks in a valid FNT file.
/// EA's LOADFONT.CPP checks `FontDataBlocks == 5`.
const EXPECTED_DATA_BLOCKS: u8 = 5;

/// V38: maximum number of characters per font file.
const MAX_CHAR_COUNT: usize = 256;

/// V38: maximum font height in pixels.  Real-world FNT files use heights
/// of 6–16 pixels; 256 provides generous headroom.
const MAX_FONT_HEIGHT: usize = 256;

/// V38: maximum glyph width in pixels.  Same rationale as MAX_FONT_HEIGHT.
const MAX_GLYPH_WIDTH: usize = 256;

// ─── Header ──────────────────────────────────────────────────────────────────

/// Parsed FNT file header (20 bytes).
///
/// Matches EA's `FontHeader` struct from Vanilla-Conquer `common/font.cpp`.
/// The five block-offset fields locate the logical sections within the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FntHeader {
    /// Total font data length in bytes (from file, informational).
    pub font_length: u16,
    /// Compression flag (0 = uncompressed; only 0 is supported).
    pub compress: u8,
    /// Number of data blocks (must be 5 for TD/RA fonts).
    pub data_blocks: u8,
    /// Byte offset to the info block (typically 0x0010 = 16).
    pub info_block_offset: u16,
    /// Byte offset to the per-character offset table (typically 0x0014 = 20).
    pub offset_block_offset: u16,
    /// Byte offset to the per-character width table.
    pub width_block_offset: u16,
    /// Byte offset to the glyph data section.
    pub data_block_offset: u16,
    /// Byte offset to the per-character height table.
    pub height_offset: u16,
    /// Unknown constant (0x1012 or 0x1011 in game files).
    pub unknown_const: u16,
    /// Number of characters (= raw field value + 1; raw field = last char index).
    pub num_chars: u16,
    /// Maximum glyph height across all characters.
    pub max_height: u8,
    /// Maximum glyph width across all characters.
    pub max_width: u8,
}

// ─── Glyph ───────────────────────────────────────────────────────────────────

/// A single glyph from a FNT file.
///
/// Glyph pixel data is 4bpp nibble-packed, row-major.  Two pixels per byte,
/// low nibble first.  Use [`FntGlyph::pixel`] to query individual pixel
/// color indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FntGlyph<'input> {
    /// Character code point (0–255).
    pub code: u8,
    /// Glyph width in pixels.
    pub width: u8,
    /// Y-offset of first data row within the character cell.
    pub y_offset: u8,
    /// Number of pixel rows actually stored in the glyph data.
    pub data_rows: u8,
    /// Raw 4bpp nibble-packed row-major bitmap data (borrowed from input).
    /// Size: `ceil(width / 2) × data_rows` bytes.
    pub data: &'input [u8],
}

impl FntGlyph<'_> {
    /// Returns the 4-bit color index at pixel `(x, y)` within the glyph's
    /// local coordinate space (relative to `y_offset`).
    ///
    /// Coordinates: `x` is column (0 = left), `y` is row (0 = top of data).
    /// Returns 0 (transparent) for out-of-bounds coordinates, zero-width
    /// glyphs, or zero-row glyphs.
    ///
    /// The returned value is a 4-bit palette index (0–15).  Color 0 is
    /// always transparent; indices 1–15 are mapped through a color
    /// translation table supplied by the renderer.
    #[inline]
    pub fn pixel(&self, x: u8, y: u8) -> u8 {
        if x >= self.width || y >= self.data_rows || self.data.is_empty() {
            return 0;
        }
        // Each row is ceil(width / 2) bytes.  Two pixels per byte,
        // low nibble = left pixel, high nibble = right pixel.
        let bytes_per_row = (self.width as usize).div_ceil(2);
        let byte_idx = (y as usize)
            .saturating_mul(bytes_per_row)
            .saturating_add(x as usize / 2);
        let byte_val = match self.data.get(byte_idx) {
            Some(&b) => b,
            None => return 0,
        };
        // Low nibble = even x, high nibble = odd x.
        if x % 2 == 0 {
            byte_val & 0x0F
        } else {
            (byte_val >> 4) & 0x0F
        }
    }
}

// ─── Parsed File ─────────────────────────────────────────────────────────────

/// Parsed FNT bitmap font file.
///
/// Contains the font header and all glyph entries.  Glyphs with width 0
/// have empty data (space characters, unused code points).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FntFile<'input> {
    /// File header.
    pub header: FntHeader,
    /// Glyph entries (indexed by character code, length = `header.num_chars`).
    pub glyphs: Vec<FntGlyph<'input>>,
}

impl<'input> FntFile<'input> {
    /// Parses a FNT file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] if the input is truncated.
    /// - [`Error::InvalidSize`] if font height, glyph width, or character
    ///   count exceed V38 caps.
    /// - [`Error::InvalidMagic`] if the compression flag is non-zero or
    ///   the data-block count is not 5.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── Header (20 bytes) ────────────────────────────────────────────
        if data.len() < FNT_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: FNT_HEADER_SIZE,
                available: data.len(),
            });
        }

        let font_length = read_u16_le(data, 0)?;
        let compress = read_u8(data, 2)?;
        let data_blocks = read_u8(data, 3)?;
        let info_block_offset = read_u16_le(data, 4)?;
        let offset_block_offset = read_u16_le(data, 6)?;
        let width_block_offset = read_u16_le(data, 8)?;
        let data_block_offset = read_u16_le(data, 10)?;
        let height_offset = read_u16_le(data, 12)?;
        let unknown_const = read_u16_le(data, 14)?;
        // Byte 16 is padding, byte 17 is char_count_raw (last character index).
        let _pad = read_u8(data, 16)?;
        let char_count_raw = read_u8(data, 17)?;
        let max_height = read_u8(data, 18)?;
        let max_width = read_u8(data, 19)?;

        // Number of characters = last_char_index + 1.
        let num_chars = (char_count_raw as u16).saturating_add(1);

        // ── Validation ───────────────────────────────────────────────────
        // EA's LOADFONT.CPP requires compress == 0 and data_blocks == 5.
        if compress != 0 {
            return Err(Error::InvalidMagic {
                context: "FNT compression flag (expected 0)",
            });
        }
        if data_blocks != EXPECTED_DATA_BLOCKS {
            return Err(Error::InvalidMagic {
                context: "FNT data blocks (expected 5)",
            });
        }

        // V38: cap character count.
        if (num_chars as usize) > MAX_CHAR_COUNT {
            return Err(Error::InvalidSize {
                value: num_chars as usize,
                limit: MAX_CHAR_COUNT,
                context: "FNT character count",
            });
        }

        // V38: cap font height.
        if (max_height as usize) > MAX_FONT_HEIGHT {
            return Err(Error::InvalidSize {
                value: max_height as usize,
                limit: MAX_FONT_HEIGHT,
                context: "FNT font height",
            });
        }

        let header = FntHeader {
            font_length,
            compress,
            data_blocks,
            info_block_offset,
            offset_block_offset,
            width_block_offset,
            data_block_offset,
            height_offset,
            unknown_const,
            num_chars,
            max_height,
            max_width,
        };

        // ── Table bounds checks ──────────────────────────────────────────
        let nc = num_chars as usize;

        // Width table: num_chars × u8 at width_block_offset.
        let wb_start = width_block_offset as usize;
        let wb_end = wb_start.saturating_add(nc);
        if wb_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: wb_end,
                available: data.len(),
            });
        }

        // Offset table: num_chars × u16 at offset_block_offset.
        let ob_start = offset_block_offset as usize;
        let ob_end = ob_start.saturating_add(nc.saturating_mul(2));
        if ob_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: ob_end,
                available: data.len(),
            });
        }

        // Height table: num_chars × u16 at height_offset.
        let hb_start = height_offset as usize;
        let hb_end = hb_start.saturating_add(nc.saturating_mul(2));
        if hb_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: hb_end,
                available: data.len(),
            });
        }

        // ── Glyphs ──────────────────────────────────────────────────────
        let mut glyphs = Vec::with_capacity(nc);

        for i in 0..nc {
            // Width: u8 at width_block_offset + i.
            let glyph_width = read_u8(data, wb_start.saturating_add(i))?;

            // V38: cap individual glyph width.
            if (glyph_width as usize) > MAX_GLYPH_WIDTH {
                return Err(Error::InvalidSize {
                    value: glyph_width as usize,
                    limit: MAX_GLYPH_WIDTH,
                    context: "FNT glyph width",
                });
            }

            // Height entry: u16 at height_offset + i*2.
            // Low byte = y_offset, high byte = data_rows.
            let h_pos = hb_start.saturating_add(i.saturating_mul(2));
            let height_entry = read_u16_le(data, h_pos)?;
            let y_offset = (height_entry & 0xFF) as u8;
            let data_rows = ((height_entry >> 8) & 0xFF) as u8;

            // Glyph data offset: u16 at offset_block_offset + i*2.
            let o_pos = ob_start.saturating_add(i.saturating_mul(2));
            let glyph_data_offset = read_u16_le(data, o_pos)? as usize;

            // Zero-width or zero-row glyphs have no pixel data.
            if glyph_width == 0 || data_rows == 0 {
                glyphs.push(FntGlyph {
                    code: i as u8,
                    width: glyph_width,
                    y_offset,
                    data_rows,
                    data: &[],
                });
                continue;
            }

            // 4bpp: each row is ceil(width / 2) bytes.
            let bytes_per_row = (glyph_width as usize).div_ceil(2);
            let glyph_size = bytes_per_row.saturating_mul(data_rows as usize);

            let glyph_end = glyph_data_offset.saturating_add(glyph_size);
            let glyph_data =
                data.get(glyph_data_offset..glyph_end)
                    .ok_or(Error::UnexpectedEof {
                        needed: glyph_end,
                        available: data.len(),
                    })?;

            glyphs.push(FntGlyph {
                code: i as u8,
                width: glyph_width,
                y_offset,
                data_rows,
                data: glyph_data,
            });
        }

        Ok(FntFile { header, glyphs })
    }
}

#[cfg(test)]
mod tests;
