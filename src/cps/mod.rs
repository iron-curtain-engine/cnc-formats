// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! CPS (Compressed Screen Picture) parser (`.cps`).
//!
//! CPS files store a single full-screen palette-indexed image, optionally
//! with an embedded 6-bit VGA palette.  They are used in Tiberian Dawn,
//! Red Alert 1, and Dune II for title screens, loading screens, and menu
//! backgrounds.
//!
//! ## Layout
//!
//! ```text
//! [Header]          10 bytes
//! [Palette]         768 bytes (only if palette_size == 768)
//! [Image Data]      LCW-compressed or raw pixel data
//! ```
//!
//! ## References
//!
//! Format source: XCC Utilities, CnC-Tools documentation, ModEnc wiki.

use crate::error::Error;
use crate::read::{read_u16_le, read_u8};

// ── Constants ─────────────────────────────────────────────────────────────────

/// CPS file header size in bytes.
const HEADER_SIZE: usize = 10;

/// V38: maximum uncompressed buffer size.  320×200 = 64000 is typical;
/// 262144 (256 KB) provides ample headroom while preventing abuse.
const MAX_BUFFER_SIZE: usize = 262_144;

/// Palette size in bytes when embedded (256 colors × 3 bytes).
const PALETTE_BYTES: usize = 768;

/// Standard CPS image width in pixels.
pub const CPS_WIDTH: u16 = 320;

/// Standard CPS image height in pixels.
pub const CPS_HEIGHT: u16 = 200;

/// Compression identifier: LCW (Format80).
pub const COMPRESSION_LCW: u16 = 4;

/// Compression identifier: no compression (raw pixels).
pub const COMPRESSION_NONE: u16 = 0;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Parsed CPS file header.
///
/// ```text
/// Offset  Size  Field
/// 0       u16   file_size      (total file size minus 2)
/// 2       u16   compression    (0 = raw, 4 = LCW)
/// 4       u16   buffer_size    (uncompressed pixel byte count)
/// 6       u16   palette_size   (0 or 768)
/// 8       u16   unknown
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpsHeader {
    /// Total file size minus 2 (as stored in the header).
    pub file_size: u16,
    /// Compression type: 0 = raw, 4 = LCW.
    pub compression: u16,
    /// Uncompressed image size in bytes (typically 64000).
    pub buffer_size: u16,
    /// Embedded palette size: 0 (no palette) or 768 (256 × RGB).
    pub palette_size: u16,
}

/// An embedded 6-bit VGA palette from a CPS file.
///
/// Each color component is in the range 0–63.  Use `to_rgb8` on individual
/// entries to convert to 8-bit range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpsPalette {
    /// 256 RGB color entries, each component 0–63.
    pub colors: [(u8, u8, u8); 256],
}

impl CpsPalette {
    /// Converts a single palette entry to 8-bit RGB by shifting left 2 bits.
    #[inline]
    pub fn to_rgb8(&self, index: u8) -> (u8, u8, u8) {
        let (r, g, b) = self.colors[index as usize];
        (r << 2 | r >> 4, g << 2 | g >> 4, b << 2 | b >> 4)
    }
}

/// Parsed CPS (Compressed Screen Picture) file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpsFile {
    /// File header.
    pub header: CpsHeader,
    /// Embedded palette, if present.
    pub palette: Option<CpsPalette>,
    /// Decompressed palette-indexed pixel data.
    pub pixels: Vec<u8>,
}

impl CpsFile {
    /// Parses a CPS file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, invalid compression type,
    /// buffer size exceeding V38 caps, or invalid palette size.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // ── Header (10 bytes) ────────────────────────────────────────────
        if data.len() < HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            });
        }

        let file_size = read_u16_le(data, 0)?;
        let compression = read_u16_le(data, 2)?;
        let buffer_size = read_u16_le(data, 4)?;
        let palette_size = read_u16_le(data, 6)?;

        // V38: validate compression type.
        if compression != COMPRESSION_NONE && compression != COMPRESSION_LCW {
            return Err(Error::InvalidMagic {
                context: "CPS compression type (expected 0 or 4)",
            });
        }

        // V38: cap uncompressed buffer size.
        if (buffer_size as usize) > MAX_BUFFER_SIZE {
            return Err(Error::InvalidSize {
                value: buffer_size as usize,
                limit: MAX_BUFFER_SIZE,
                context: "CPS buffer size",
            });
        }

        // V38: palette_size must be 0 or exactly 768.
        if palette_size != 0 && palette_size as usize != PALETTE_BYTES {
            return Err(Error::InvalidSize {
                value: palette_size as usize,
                limit: PALETTE_BYTES,
                context: "CPS palette size (must be 0 or 768)",
            });
        }

        let header = CpsHeader {
            file_size,
            compression,
            buffer_size,
            palette_size,
        };

        let mut offset = HEADER_SIZE;

        // ── Palette (optional, 768 bytes) ────────────────────────────────
        let palette = if palette_size as usize == PALETTE_BYTES {
            let pal_end = offset.saturating_add(PALETTE_BYTES);
            if pal_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: pal_end,
                    available: data.len(),
                });
            }

            let mut colors = [(0u8, 0u8, 0u8); 256];
            for (i, color) in colors.iter_mut().enumerate() {
                let base = offset.saturating_add(i.saturating_mul(3));
                let r = read_u8(data, base)?;
                let g = read_u8(data, base.saturating_add(1))?;
                let b = read_u8(data, base.saturating_add(2))?;
                *color = (r, g, b);
            }
            offset = pal_end;
            Some(CpsPalette { colors })
        } else {
            None
        };

        // ── Image data ───────────────────────────────────────────────────
        let image_data = data.get(offset..).ok_or(Error::UnexpectedEof {
            needed: offset.saturating_add(1),
            available: data.len(),
        })?;

        let pixels = if compression == COMPRESSION_LCW {
            crate::lcw::decompress(image_data, buffer_size as usize)?
        } else {
            // Raw uncompressed: copy buffer_size bytes.
            let raw = image_data
                .get(..buffer_size as usize)
                .ok_or(Error::UnexpectedEof {
                    needed: offset.saturating_add(buffer_size as usize),
                    available: data.len(),
                })?;
            raw.to_vec()
        };

        Ok(CpsFile {
            header,
            palette,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests;
