// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! VXL voxel model parser (`.vxl`).
//!
//! VXL files store three-dimensional voxel geometry for unit models in
//! Tiberian Sun and Red Alert 2.  Each file contains one or more "limbs"
//! (articulated body parts), each defined by a 3D grid of coloured voxels
//! with surface normals.
//!
//! ## Layout
//!
//! ```text
//! [Header]          802 bytes  (magic + counts + 256-colour palette)
//! [Limb Headers]    limb_count × 28 bytes
//! [Body Data]       body_size bytes  (packed voxel span data)
//! [Limb Tailers]    tailer_count × 92 bytes  (per-limb bounds + transform)
//! ```
//!
//! The body data region contains packed column spans for all limbs.
//! Each limb's tailer records byte offsets into the body region.
//!
//! ## References
//!
//! Format source: ModEnc wiki, XCC Utilities, community VXL editors.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le, read_u8};

// ── Constants ─────────────────────────────────────────────────────────────────

/// VXL file header size in bytes.
/// 16 (magic) + 4 (palette_count) + 4 (limb_count) + 4 (tailer_count)
/// + 4 (body_size) + 2 (remap_start) + 2 (remap_end) + 768 (palette) = 804.
const HEADER_SIZE: usize = 804;

/// Size of one limb header entry.
const LIMB_HEADER_SIZE: usize = 28;

/// Size of one limb tailer entry.
const LIMB_TAILER_SIZE: usize = 92;

/// VXL magic string length.
const MAGIC_SIZE: usize = 16;

/// V38: maximum number of limbs per VXL file.
const MAX_LIMBS: usize = 512;

/// V38: maximum body data size (16 MB).
const MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

/// Offset where the palette starts in the header.
const PALETTE_OFFSET: usize = 34;

/// Number of palette entries.
const PALETTE_ENTRIES: usize = 256;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Parsed VXL file header (802 bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct VxlHeader {
    /// Magic identifier (typically "Voxel Animation\0").
    pub file_type: [u8; 16],
    /// Number of palette sets (typically 1).
    pub palette_count: u32,
    /// Number of limb sections.
    pub limb_count: u32,
    /// Number of tailer entries (usually equals limb_count).
    pub tailer_count: u32,
    /// Total size of the packed body (span) data region.
    pub body_size: u32,
    /// Start of the palette remap range.
    pub start_palette_remap: u16,
    /// End of the palette remap range.
    pub end_palette_remap: u16,
    /// 256-colour RGB palette (8-bit values).
    pub palette: Vec<(u8, u8, u8)>,
}

/// Per-limb header (28 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlLimbHeader {
    /// Null-padded ASCII limb name.
    pub name: [u8; 16],
    /// Limb index number.
    pub limb_number: u32,
    /// Reserved field 1.
    pub unknown1: u32,
    /// Reserved field 2.
    pub unknown2: u32,
}

impl VxlLimbHeader {
    /// Returns the limb name as a UTF-8 string (trimmed at the first NUL).
    #[inline]
    pub fn name_str(&self) -> &str {
        let nul = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        std::str::from_utf8(self.name.get(..nul).unwrap_or(&[])).unwrap_or("")
    }
}

/// Per-limb tailer (92 bytes) — bounding box, transform, and dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct VxlLimbTailer {
    /// Byte offset into body data where span-start entries begin.
    pub span_start_offset: u32,
    /// Byte offset into body data where span-end entries begin.
    pub span_end_offset: u32,
    /// Byte offset into body data where span voxel data begins.
    pub span_data_offset: u32,
    /// Scale / determinant value.
    pub det: f32,
    /// 3×4 transformation matrix (row-major, 12 floats).
    pub transform: [f32; 12],
    /// Axis-aligned bounding box minimum corner.
    pub min_bounds: [f32; 3],
    /// Axis-aligned bounding box maximum corner.
    pub max_bounds: [f32; 3],
    /// Voxel grid dimension along X.
    pub size_x: u8,
    /// Voxel grid dimension along Y.
    pub size_y: u8,
    /// Voxel grid dimension along Z.
    pub size_z: u8,
    /// Surface normal encoding mode (1–4).
    pub normals_mode: u8,
}

/// Parsed VXL voxel model file.
#[derive(Debug, Clone, PartialEq)]
pub struct VxlFile<'input> {
    /// File header (magic, counts, palette).
    pub header: VxlHeader,
    /// Per-limb headers.
    pub limb_headers: Vec<VxlLimbHeader>,
    /// Per-limb tailers (bounds, transforms, dimensions).
    pub limb_tailers: Vec<VxlLimbTailer>,
    /// Raw packed body (span) data for all limbs, borrowed from input.
    pub body_data: &'input [u8],
}

impl<'input> VxlFile<'input> {
    /// Parses a VXL file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, counts exceeding V38 caps,
    /// or body sizes that exceed the safety limit.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── Header (802 bytes) ───────────────────────────────────────────
        if data.len() < HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            });
        }

        let mut file_type = [0u8; MAGIC_SIZE];
        let magic_slice = data.get(..MAGIC_SIZE).ok_or(Error::UnexpectedEof {
            needed: MAGIC_SIZE,
            available: data.len(),
        })?;
        file_type.copy_from_slice(magic_slice);

        let palette_count = read_u32_le(data, 16)?;
        let limb_count = read_u32_le(data, 20)?;
        let tailer_count = read_u32_le(data, 24)?;
        let body_size = read_u32_le(data, 28)?;
        let start_palette_remap = read_u16_le(data, 32)?;
        let end_palette_remap = read_u16_le(data, PALETTE_OFFSET)?;

        // V38: cap limb counts.
        if (limb_count as usize) > MAX_LIMBS {
            return Err(Error::InvalidSize {
                value: limb_count as usize,
                limit: MAX_LIMBS,
                context: "VXL limb count",
            });
        }
        if (tailer_count as usize) > MAX_LIMBS {
            return Err(Error::InvalidSize {
                value: tailer_count as usize,
                limit: MAX_LIMBS,
                context: "VXL tailer count",
            });
        }
        if (body_size as usize) > MAX_BODY_SIZE {
            return Err(Error::InvalidSize {
                value: body_size as usize,
                limit: MAX_BODY_SIZE,
                context: "VXL body size",
            });
        }

        // ── Palette (768 bytes starting at offset 34) ────────────────────
        // Actually the palette starts at offset 34 in some docs, 36 in others.
        // The reliable layout is: remap fields end at 34, palette at 34.
        let pal_start = PALETTE_OFFSET.saturating_add(2); // after end_palette_remap
        let mut palette = Vec::with_capacity(PALETTE_ENTRIES);
        for i in 0..PALETTE_ENTRIES {
            let base = pal_start.saturating_add(i.saturating_mul(3));
            let r = read_u8(data, base)?;
            let g = read_u8(data, base.saturating_add(1))?;
            let b = read_u8(data, base.saturating_add(2))?;
            palette.push((r, g, b));
        }

        let header = VxlHeader {
            file_type,
            palette_count,
            limb_count,
            tailer_count,
            body_size,
            start_palette_remap,
            end_palette_remap,
            palette,
        };

        // ── Limb Headers ─────────────────────────────────────────────────
        let limb_headers_start = HEADER_SIZE;
        let limb_headers_total = (limb_count as usize).saturating_mul(LIMB_HEADER_SIZE);
        let limb_headers_end = limb_headers_start.saturating_add(limb_headers_total);
        if limb_headers_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: limb_headers_end,
                available: data.len(),
            });
        }

        let mut limb_headers = Vec::with_capacity(limb_count as usize);
        for i in 0..limb_count as usize {
            let off = limb_headers_start.saturating_add(i.saturating_mul(LIMB_HEADER_SIZE));

            let mut name = [0u8; 16];
            let name_slice = data
                .get(off..off.saturating_add(16))
                .ok_or(Error::UnexpectedEof {
                    needed: off.saturating_add(16),
                    available: data.len(),
                })?;
            name.copy_from_slice(name_slice);

            let limb_number = read_u32_le(data, off.saturating_add(16))?;
            let unknown1 = read_u32_le(data, off.saturating_add(20))?;
            let unknown2 = read_u32_le(data, off.saturating_add(24))?;

            limb_headers.push(VxlLimbHeader {
                name,
                limb_number,
                unknown1,
                unknown2,
            });
        }

        // ── Body Data ────────────────────────────────────────────────────
        let body_start = limb_headers_end;
        let body_end = body_start.saturating_add(body_size as usize);
        if body_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: body_end,
                available: data.len(),
            });
        }
        let body_data = data.get(body_start..body_end).ok_or(Error::UnexpectedEof {
            needed: body_end,
            available: data.len(),
        })?;

        // ── Limb Tailers ─────────────────────────────────────────────────
        let tailers_start = body_end;
        let tailers_total = (tailer_count as usize).saturating_mul(LIMB_TAILER_SIZE);
        let tailers_end = tailers_start.saturating_add(tailers_total);
        if tailers_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: tailers_end,
                available: data.len(),
            });
        }

        let mut limb_tailers = Vec::with_capacity(tailer_count as usize);
        for i in 0..tailer_count as usize {
            let off = tailers_start.saturating_add(i.saturating_mul(LIMB_TAILER_SIZE));

            let span_start_offset = read_u32_le(data, off)?;
            let span_end_offset = read_u32_le(data, off.saturating_add(4))?;
            let span_data_offset = read_u32_le(data, off.saturating_add(8))?;
            let det = f32::from_bits(read_u32_le(data, off.saturating_add(12))?);

            let mut transform = [0.0f32; 12];
            for (j, val) in transform.iter_mut().enumerate() {
                let f_off = off.saturating_add(16).saturating_add(j.saturating_mul(4));
                *val = f32::from_bits(read_u32_le(data, f_off)?);
            }

            let mut min_bounds = [0.0f32; 3];
            for (j, val) in min_bounds.iter_mut().enumerate() {
                let f_off = off.saturating_add(64).saturating_add(j.saturating_mul(4));
                *val = f32::from_bits(read_u32_le(data, f_off)?);
            }

            let mut max_bounds = [0.0f32; 3];
            for (j, val) in max_bounds.iter_mut().enumerate() {
                let f_off = off.saturating_add(76).saturating_add(j.saturating_mul(4));
                *val = f32::from_bits(read_u32_le(data, f_off)?);
            }

            let size_x = read_u8(data, off.saturating_add(88))?;
            let size_y = read_u8(data, off.saturating_add(89))?;
            let size_z = read_u8(data, off.saturating_add(90))?;
            let normals_mode = read_u8(data, off.saturating_add(91))?;

            limb_tailers.push(VxlLimbTailer {
                span_start_offset,
                span_end_offset,
                span_data_offset,
                det,
                transform,
                min_bounds,
                max_bounds,
                size_x,
                size_y,
                size_z,
                normals_mode,
            });
        }

        Ok(VxlFile {
            header,
            limb_headers,
            limb_tailers,
            body_data,
        })
    }
}

#[cfg(test)]
mod tests;
