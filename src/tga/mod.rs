// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! TGA (Truevision TGA 2.0) image parser for Generals textures.
//!
//! Parses the 18-byte header, optional image ID, optional color map,
//! and provides zero-copy access to the image data region.  The optional
//! TGA 2.0 footer (last 26 bytes) is detected by its signature.
//!
//! ## Layout
//!
//! ```text
//! [Header]         18 bytes
//! [Image ID]       id_length bytes (optional)
//! [Color Map]      color_map_length × ceil(color_map_entry_size / 8) bytes
//! [Image Data]     width × height × ceil(pixel_depth / 8) bytes (or RLE)
//! ```
//!
//! ## References
//!
//! Format source: Truevision TGA 2.0 specification, public domain.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le, read_u8};

// ── Constants ─────────────────────────────────────────────────────────────────

/// TGA file header size in bytes.
const HEADER_SIZE: usize = 18;

/// TGA 2.0 footer size in bytes.
const FOOTER_SIZE: usize = 26;

/// TGA 2.0 footer signature (18 bytes including the NUL terminator).
const FOOTER_SIGNATURE: &[u8; 18] = b"TRUEVISION-XFILE.\0";

/// Maximum allowed image dimension (width or height).
const MAX_DIMENSION: u32 = 65_535;

// ── Types ─────────────────────────────────────────────────────────────────────

/// TGA image type identifier.
///
/// Covers both uncompressed and RLE-compressed variants of the three
/// data organisations defined by the TGA specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TgaImageType {
    /// No image data present (type 0).
    NoImage,
    /// Uncompressed color-mapped image (type 1).
    ColorMapped,
    /// Uncompressed true-color image (type 2).
    TrueColor,
    /// Uncompressed grayscale image (type 3).
    Grayscale,
    /// RLE-compressed color-mapped image (type 9).
    RleColorMapped,
    /// RLE-compressed true-color image (type 10).
    RleTrueColor,
    /// RLE-compressed grayscale image (type 11).
    RleGrayscale,
}

impl TgaImageType {
    /// Converts a raw `u8` image type field to its enum variant.
    fn from_u8(value: u8) -> Result<Self, Error> {
        match value {
            0 => Ok(TgaImageType::NoImage),
            1 => Ok(TgaImageType::ColorMapped),
            2 => Ok(TgaImageType::TrueColor),
            3 => Ok(TgaImageType::Grayscale),
            9 => Ok(TgaImageType::RleColorMapped),
            10 => Ok(TgaImageType::RleTrueColor),
            11 => Ok(TgaImageType::RleGrayscale),
            _ => Err(Error::InvalidMagic {
                context: "TGA image type",
            }),
        }
    }
}

/// Parsed TGA file header (18 bytes).
///
/// ```text
/// Offset  Size  Field
/// 0       u8    id_length
/// 1       u8    color_map_type
/// 2       u8    image_type
/// 3       u16   color_map_first
/// 5       u16   color_map_length
/// 7       u8    color_map_entry_size
/// 8       u16   x_origin
/// 10      u16   y_origin
/// 12      u16   width
/// 14      u16   height
/// 16      u8    pixel_depth
/// 17      u8    image_descriptor
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgaHeader {
    /// Length of the image ID field in bytes (0–255).
    pub id_length: u8,
    /// Color map type: 0 = no color map, 1 = has color map.
    pub color_map_type: u8,
    /// Image data type (uncompressed, RLE, color-mapped, etc.).
    pub image_type: TgaImageType,
    /// Index of the first color map entry.
    pub color_map_first: u16,
    /// Number of color map entries.
    pub color_map_length: u16,
    /// Bits per color map entry (15, 16, 24, or 32).
    pub color_map_entry_size: u8,
    /// X origin of the image (pixels from left).
    pub x_origin: u16,
    /// Y origin of the image (pixels from top).
    pub y_origin: u16,
    /// Image width in pixels.
    pub width: u16,
    /// Image height in pixels.
    pub height: u16,
    /// Bits per pixel (8, 15, 16, 24, or 32).
    pub pixel_depth: u8,
    /// Image descriptor byte (alpha depth in bits 0–3, orientation in bits 4–5).
    pub image_descriptor: u8,
}

/// Optional TGA 2.0 footer.
///
/// Present only when the last 26 bytes of the file contain the
/// `TRUEVISION-XFILE.\0` signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgaFooter {
    /// Byte offset to the extension area (0 if absent).
    pub extension_offset: u32,
    /// Byte offset to the developer directory (0 if absent).
    pub developer_offset: u32,
}

/// A parsed TGA image file.
///
/// Provides zero-copy access to the image ID, color map, and pixel data
/// regions within the original input buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgaFile<'input> {
    /// Parsed header fields.
    pub header: TgaHeader,
    /// Image ID bytes (empty slice if `id_length == 0`).
    image_id: &'input [u8],
    /// Raw color map bytes (empty slice if no color map).
    color_map: &'input [u8],
    /// Raw image data bytes (uncompressed pixels or RLE stream).
    image_data: &'input [u8],
    /// Optional TGA 2.0 footer.
    pub footer: Option<TgaFooter>,
}

impl<'input> TgaFile<'input> {
    /// Parses a TGA file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, unrecognised image types,
    /// or image data shorter than the expected uncompressed size.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── Header (18 bytes) ────────────────────────────────────────────
        if data.len() < HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            });
        }

        let id_length = read_u8(data, 0)?;
        let color_map_type = read_u8(data, 1)?;
        let image_type_raw = read_u8(data, 2)?;
        let image_type = TgaImageType::from_u8(image_type_raw)?;

        let color_map_first = read_u16_le(data, 3)?;
        let color_map_length = read_u16_le(data, 5)?;
        let color_map_entry_size = read_u8(data, 7)?;

        let x_origin = read_u16_le(data, 8)?;
        let y_origin = read_u16_le(data, 10)?;
        let width = read_u16_le(data, 12)?;
        let height = read_u16_le(data, 14)?;
        let pixel_depth = read_u8(data, 16)?;
        let image_descriptor = read_u8(data, 17)?;

        // Validate dimensions against maximum.
        if (width as u32) > MAX_DIMENSION {
            return Err(Error::InvalidSize {
                value: width as usize,
                limit: MAX_DIMENSION as usize,
                context: "TGA image width",
            });
        }
        if (height as u32) > MAX_DIMENSION {
            return Err(Error::InvalidSize {
                value: height as usize,
                limit: MAX_DIMENSION as usize,
                context: "TGA image height",
            });
        }

        let header = TgaHeader {
            id_length,
            color_map_type,
            image_type,
            color_map_first,
            color_map_length,
            color_map_entry_size,
            x_origin,
            y_origin,
            width,
            height,
            pixel_depth,
            image_descriptor,
        };

        let mut offset = HEADER_SIZE;

        // ── Image ID (optional) ──────────────────────────────────────────
        let id_end = offset
            .checked_add(id_length as usize)
            .ok_or(Error::UnexpectedEof {
                needed: usize::MAX,
                available: data.len(),
            })?;
        if id_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: id_end,
                available: data.len(),
            });
        }
        let image_id = data.get(offset..id_end).ok_or(Error::UnexpectedEof {
            needed: id_end,
            available: data.len(),
        })?;
        offset = id_end;

        // ── Color map (optional) ─────────────────────────────────────────
        let color_map_bytes_per_entry = if color_map_type == 1 {
            // ceil(color_map_entry_size / 8)
            (color_map_entry_size as usize)
                .checked_add(7)
                .ok_or(Error::UnexpectedEof {
                    needed: usize::MAX,
                    available: data.len(),
                })?
                / 8
        } else {
            0
        };

        let color_map_total = (color_map_length as usize)
            .checked_mul(color_map_bytes_per_entry)
            .ok_or(Error::InvalidSize {
                value: color_map_length as usize,
                limit: data.len(),
                context: "TGA color map",
            })?;

        let cmap_end = offset
            .checked_add(color_map_total)
            .ok_or(Error::UnexpectedEof {
                needed: usize::MAX,
                available: data.len(),
            })?;
        if cmap_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: cmap_end,
                available: data.len(),
            });
        }
        let color_map = data.get(offset..cmap_end).ok_or(Error::UnexpectedEof {
            needed: cmap_end,
            available: data.len(),
        })?;
        offset = cmap_end;

        // ── Image data ───────────────────────────────────────────────────
        // For non-RLE types, validate that the expected pixel data fits.
        // For RLE types, just take whatever remains (RLE length is variable).
        let is_rle = matches!(
            image_type,
            TgaImageType::RleColorMapped | TgaImageType::RleTrueColor | TgaImageType::RleGrayscale
        );

        let image_data = if image_type == TgaImageType::NoImage {
            data.get(offset..offset).ok_or(Error::UnexpectedEof {
                needed: offset,
                available: data.len(),
            })?
        } else if is_rle {
            // For RLE, the compressed stream extends to the end of the
            // file (or up to the footer).  We take everything remaining
            // after the color map.
            data.get(offset..).ok_or(Error::UnexpectedEof {
                needed: offset.saturating_add(1),
                available: data.len(),
            })?
        } else {
            // Uncompressed: width × height × bytes_per_pixel
            let bytes_per_pixel =
                (pixel_depth as usize)
                    .checked_add(7)
                    .ok_or(Error::UnexpectedEof {
                        needed: usize::MAX,
                        available: data.len(),
                    })?
                    / 8;

            let pixel_count =
                (width as usize)
                    .checked_mul(height as usize)
                    .ok_or(Error::InvalidSize {
                        value: width as usize,
                        limit: MAX_DIMENSION as usize,
                        context: "TGA image dimensions",
                    })?;

            let expected_bytes =
                pixel_count
                    .checked_mul(bytes_per_pixel)
                    .ok_or(Error::InvalidSize {
                        value: pixel_count,
                        limit: data.len(),
                        context: "TGA image data size",
                    })?;

            let img_end = offset
                .checked_add(expected_bytes)
                .ok_or(Error::UnexpectedEof {
                    needed: usize::MAX,
                    available: data.len(),
                })?;

            if img_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: img_end,
                    available: data.len(),
                });
            }
            data.get(offset..img_end).ok_or(Error::UnexpectedEof {
                needed: img_end,
                available: data.len(),
            })?
        };

        // ── Footer (optional, last 26 bytes) ────────────────────────────
        let footer = if data.len() >= FOOTER_SIZE {
            let footer_start = data.len().saturating_sub(FOOTER_SIZE);
            let sig_start = footer_start.saturating_add(8);
            let sig = data.get(sig_start..).ok_or(Error::UnexpectedEof {
                needed: sig_start.saturating_add(18),
                available: data.len(),
            })?;
            if sig == FOOTER_SIGNATURE.as_slice() {
                let ext_offset = read_u32_le(data, footer_start)?;
                let dev_offset = read_u32_le(data, footer_start.saturating_add(4))?;
                Some(TgaFooter {
                    extension_offset: ext_offset,
                    developer_offset: dev_offset,
                })
            } else {
                None
            }
        } else {
            None
        };

        Ok(TgaFile {
            header,
            image_id,
            color_map,
            image_data,
            footer,
        })
    }

    /// Returns the image ID bytes, or an empty slice if none.
    #[inline]
    pub fn image_id(&self) -> &'input [u8] {
        self.image_id
    }

    /// Returns the raw color map bytes, or an empty slice if none.
    #[inline]
    pub fn color_map(&self) -> &'input [u8] {
        self.color_map
    }

    /// Returns the raw image data bytes (pixels or RLE stream).
    #[inline]
    pub fn image_data(&self) -> &'input [u8] {
        self.image_data
    }

    /// Returns `true` if the image uses RLE compression.
    #[inline]
    pub fn is_rle(&self) -> bool {
        matches!(
            self.header.image_type,
            TgaImageType::RleColorMapped | TgaImageType::RleTrueColor | TgaImageType::RleGrayscale
        )
    }

    /// Returns `true` if the image has a color map (palette).
    #[inline]
    pub fn has_color_map(&self) -> bool {
        self.header.color_map_type == 1
    }

    /// Returns `true` if a TGA 2.0 footer was detected.
    #[inline]
    pub fn has_footer(&self) -> bool {
        self.footer.is_some()
    }

    /// Returns the alpha channel depth in bits (0–15).
    #[inline]
    pub fn alpha_depth(&self) -> u8 {
        self.header.image_descriptor & 0x0F
    }

    /// Returns `true` if rows are stored top-to-bottom (bit 5 set).
    #[inline]
    pub fn is_top_to_bottom(&self) -> bool {
        self.header.image_descriptor & 0x20 != 0
    }

    /// Returns `true` if pixels are stored right-to-left (bit 4 set).
    #[inline]
    pub fn is_right_to_left(&self) -> bool {
        self.header.image_descriptor & 0x10 != 0
    }
}

#[cfg(test)]
mod tests;
