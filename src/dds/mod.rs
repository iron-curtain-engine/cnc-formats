// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! DDS (DirectDraw Surface) header parser (`.dds`).
//!
//! DDS files store GPU-ready texture data used by Generals/SAGE and the
//! Command & Conquer Remastered Collection.  This module parses the
//! header to extract dimensions, pixel format, and compression type.
//! It does not decode pixel data — that is the engine's job.
//!
//! ## Layout
//!
//! ```text
//! [Magic]          4 bytes   b"DDS " (0x20534444 LE)
//! [DDS_HEADER]     124 bytes (size field must be 124)
//!   [DDS_PIXELFORMAT] 32 bytes embedded at offset 76 within header
//! [DX10 Header]    20 bytes  (optional, only if four_cc == b"DX10")
//! [Pixel Data]     remainder of the file
//! ```
//!
//! ## References
//!
//! Format source: Microsoft DDS documentation (MSDN), DXGI format spec.

use crate::error::Error;
use crate::read::{read_u32_le, read_u8};

// ── Constants ─────────────────────────────────────────────────────────────────

/// DDS file magic: `b"DDS "` (note trailing space).
const MAGIC: &[u8; 4] = b"DDS ";

/// Required value of the DDS_HEADER `size` field.
const HEADER_SIZE: u32 = 124;

/// Required value of the DDS_PIXELFORMAT `size` field.
const PF_SIZE: u32 = 32;

/// Minimum file size: 4 (magic) + 124 (header).
const MIN_FILE_SIZE: usize = 128;

/// Size of the optional DX10 extended header.
const DX10_HEADER_SIZE: usize = 20;

/// FourCC value that signals a DX10 extended header follows.
const FOURCC_DX10: &[u8; 4] = b"DX10";

// ── DDSD flags ───────────────────────────────────────────────────────────────

/// Header contains a valid `caps` field.
pub const DDSD_CAPS: u32 = 0x1;
/// Header contains a valid `height` field.
pub const DDSD_HEIGHT: u32 = 0x2;
/// Header contains a valid `width` field.
pub const DDSD_WIDTH: u32 = 0x4;
/// Header contains a valid `pitch` field.
pub const DDSD_PITCH: u32 = 0x8;
/// Header contains a valid `pixel_format` field.
pub const DDSD_PIXELFORMAT: u32 = 0x1000;
/// Header contains a valid `mip_map_count` field.
pub const DDSD_MIPMAPCOUNT: u32 = 0x2_0000;
/// Header contains a valid `linear_size` field.
pub const DDSD_LINEARSIZE: u32 = 0x8_0000;
/// Header contains a valid `depth` field.
pub const DDSD_DEPTH: u32 = 0x80_0000;

// ── DDPF flags ───────────────────────────────────────────────────────────────

/// Pixel format contains valid alpha channel data.
pub const DDPF_ALPHAPIXELS: u32 = 0x1;
/// Pixel format contains a valid `four_cc` field.
pub const DDPF_FOURCC: u32 = 0x4;
/// Pixel format contains valid uncompressed RGB data.
pub const DDPF_RGB: u32 = 0x40;

// ── Types ─────────────────────────────────────────────────────────────────────

/// DDS pixel format descriptor (32 bytes in the file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdsPixelFormat {
    /// DDPF flags describing the pixel format contents.
    pub flags: u32,
    /// FourCC compression code (e.g. `b"DXT1"`, `b"DXT5"`, `b"DX10"`).
    pub four_cc: [u8; 4],
    /// Number of bits per pixel for uncompressed formats.
    pub rgb_bit_count: u32,
    /// Red channel bitmask.
    pub r_bitmask: u32,
    /// Green channel bitmask.
    pub g_bitmask: u32,
    /// Blue channel bitmask.
    pub b_bitmask: u32,
    /// Alpha channel bitmask.
    pub a_bitmask: u32,
}

/// DX10 extended header (20 bytes, present only when `four_cc == b"DX10"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdsDx10Header {
    /// DXGI format enum value.
    pub dxgi_format: u32,
    /// Resource dimension (1D/2D/3D).
    pub resource_dimension: u32,
    /// Miscellaneous flags (e.g. cube map indicator).
    pub misc_flag: u32,
    /// Array size (for texture arrays).
    pub array_size: u32,
    /// Additional miscellaneous flags.
    pub misc_flags2: u32,
}

/// Parsed DDS texture file.
///
/// Provides access to the header fields and a reference to the raw pixel
/// data that follows the header(s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdsFile<'a> {
    /// DDSD flags describing which header fields are valid.
    pub flags: u32,
    /// Texture height in pixels.
    pub height: u32,
    /// Texture width in pixels.
    pub width: u32,
    /// Pitch (bytes per scan line) or total byte count for compressed formats.
    pub pitch_or_linear_size: u32,
    /// Depth of a volume texture (0 for 2D textures).
    pub depth: u32,
    /// Number of mipmap levels (0 or 1 means a single level).
    pub mip_map_count: u32,
    /// Pixel format descriptor.
    pub pixel_format: DdsPixelFormat,
    /// Surface capabilities (e.g. texture, mipmap, complex).
    pub caps: u32,
    /// Additional capabilities (e.g. cubemap faces, volume).
    pub caps2: u32,
    /// DX10 extended header, if present.
    pub dx10: Option<DdsDx10Header>,
    /// Raw pixel/block data following the header(s).
    pixel_data: &'a [u8],
}

impl<'a> DdsFile<'a> {
    /// Parses a DDS file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, invalid magic, wrong header
    /// size, wrong pixel format size, or truncated DX10 header.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // ── Minimum size check ───────────────────────────────────────────
        if data.len() < MIN_FILE_SIZE {
            return Err(Error::UnexpectedEof {
                needed: MIN_FILE_SIZE,
                available: data.len(),
            });
        }

        // ── Magic ────────────────────────────────────────────────────────
        let m0 = read_u8(data, 0)?;
        let m1 = read_u8(data, 1)?;
        let m2 = read_u8(data, 2)?;
        let m3 = read_u8(data, 3)?;
        if [m0, m1, m2, m3] != *MAGIC {
            return Err(Error::InvalidMagic {
                context: "DDS magic",
            });
        }

        // ── DDS_HEADER (124 bytes at offset 4) ──────────────────────────
        let header_size = read_u32_le(data, 4)?;
        if header_size != HEADER_SIZE {
            return Err(Error::InvalidSize {
                value: header_size as usize,
                limit: HEADER_SIZE as usize,
                context: "DDS header size",
            });
        }

        let flags = read_u32_le(data, 8)?;
        let height = read_u32_le(data, 12)?;
        let width = read_u32_le(data, 16)?;
        let pitch_or_linear_size = read_u32_le(data, 20)?;
        let depth = read_u32_le(data, 24)?;
        let mip_map_count = read_u32_le(data, 28)?;

        // reserved1: [u32; 11] at offsets 32..76 — skipped

        // ── DDS_PIXELFORMAT (32 bytes at header offset 72, i.e. file
        //    offset 76) ──────────────────────────────────────────────────
        let pf_offset: usize = 4 + 72; // 76
        let pf_size = read_u32_le(data, pf_offset)?;
        if pf_size != PF_SIZE {
            return Err(Error::InvalidSize {
                value: pf_size as usize,
                limit: PF_SIZE as usize,
                context: "DDS pixel format size",
            });
        }

        let pf_flags = read_u32_le(data, pf_offset.saturating_add(4))?;

        let four_cc = [
            read_u8(data, pf_offset.saturating_add(8))?,
            read_u8(data, pf_offset.saturating_add(9))?,
            read_u8(data, pf_offset.saturating_add(10))?,
            read_u8(data, pf_offset.saturating_add(11))?,
        ];

        let rgb_bit_count = read_u32_le(data, pf_offset.saturating_add(12))?;
        let r_bitmask = read_u32_le(data, pf_offset.saturating_add(16))?;
        let g_bitmask = read_u32_le(data, pf_offset.saturating_add(20))?;
        let b_bitmask = read_u32_le(data, pf_offset.saturating_add(24))?;
        let a_bitmask = read_u32_le(data, pf_offset.saturating_add(28))?;

        let pixel_format = DdsPixelFormat {
            flags: pf_flags,
            four_cc,
            rgb_bit_count,
            r_bitmask,
            g_bitmask,
            b_bitmask,
            a_bitmask,
        };

        // ── Caps (after pixel format, file offset 108) ───────────────────
        let caps = read_u32_le(data, 108)?;
        let caps2 = read_u32_le(data, 112)?;
        // caps3, caps4, reserved2 at 116..128 — skipped

        // ── Optional DX10 extended header ────────────────────────────────
        let has_dx10 = four_cc == *FOURCC_DX10;
        let dx10 = if has_dx10 {
            let dx10_end =
                MIN_FILE_SIZE
                    .checked_add(DX10_HEADER_SIZE)
                    .ok_or(Error::UnexpectedEof {
                        needed: usize::MAX,
                        available: data.len(),
                    })?;

            if data.len() < dx10_end {
                return Err(Error::UnexpectedEof {
                    needed: dx10_end,
                    available: data.len(),
                });
            }

            let dx10_base = MIN_FILE_SIZE; // 128
            Some(DdsDx10Header {
                dxgi_format: read_u32_le(data, dx10_base)?,
                resource_dimension: read_u32_le(data, dx10_base.saturating_add(4))?,
                misc_flag: read_u32_le(data, dx10_base.saturating_add(8))?,
                array_size: read_u32_le(data, dx10_base.saturating_add(12))?,
                misc_flags2: read_u32_le(data, dx10_base.saturating_add(16))?,
            })
        } else {
            None
        };

        // ── Pixel data ──────────────────────────────────────────────────
        let data_start = if has_dx10 {
            MIN_FILE_SIZE.saturating_add(DX10_HEADER_SIZE)
        } else {
            MIN_FILE_SIZE
        };

        let pixel_data = data.get(data_start..).unwrap_or(&[]);

        Ok(DdsFile {
            flags,
            height,
            width,
            pitch_or_linear_size,
            depth,
            mip_map_count,
            pixel_format,
            caps,
            caps2,
            dx10,
            pixel_data,
        })
    }

    /// Returns the raw pixel/block data following the header(s).
    #[inline]
    pub fn pixel_data(&self) -> &'a [u8] {
        self.pixel_data
    }

    /// Returns `true` if this file includes a DX10 extended header.
    #[inline]
    pub fn has_dx10(&self) -> bool {
        self.dx10.is_some()
    }

    /// Returns `true` if the pixel format uses FourCC block compression.
    #[inline]
    pub fn is_compressed(&self) -> bool {
        self.pixel_format.flags & DDPF_FOURCC != 0
    }

    /// Returns the FourCC code as a UTF-8 string, or `None` if the bytes
    /// are not valid ASCII.
    ///
    /// Common values: `"DXT1"`, `"DXT3"`, `"DXT5"`, `"DX10"`.
    pub fn four_cc_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.pixel_format.four_cc).ok()
    }
}

#[cfg(test)]
mod tests;
