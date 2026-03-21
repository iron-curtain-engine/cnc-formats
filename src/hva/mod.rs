// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! HVA (Hierarchical Voxel Animation) parser (`.hva`).
//!
//! HVA files store per-frame 3×4 transformation matrices for each section
//! (bone) of a voxel model.  They are used alongside VXL files in
//! Tiberian Sun and Red Alert 2 to animate 3D voxel units.
//!
//! ## Layout
//!
//! ```text
//! [Header]              24 bytes  (filename + frame/section counts)
//! [Section Names]       num_sections × 16 bytes
//! [Transform Matrices]  num_frames × num_sections × 48 bytes
//! ```
//!
//! Each transform is a 3×4 row-major matrix stored as 12 little-endian
//! `f32` values: 3 rows of 4 columns (rotation + translation).
//!
//! ## References
//!
//! Format source: ModEnc wiki, XCC Utilities, VXL editors.

use crate::error::Error;
use crate::read::read_u32_le;

// ── Constants ─────────────────────────────────────────────────────────────────

/// HVA header size in bytes: 16 (filename) + 4 (num_frames) + 4 (num_sections).
const HEADER_SIZE: usize = 24;

/// Size of one section name entry.
const SECTION_NAME_SIZE: usize = 16;

/// Size of one 3×4 transform matrix in bytes (12 × f32).
const MATRIX_SIZE: usize = 48;

/// V38: maximum number of sections (bones) per HVA file.
const MAX_SECTIONS: usize = 512;

/// V38: maximum number of animation frames.
const MAX_FRAMES: usize = 65_536;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Parsed HVA file header.
#[derive(Debug, Clone, PartialEq)]
pub struct HvaHeader {
    /// Original filename, null-padded to 16 bytes.
    pub filename: [u8; 16],
    /// Total number of animation frames.
    pub num_frames: u32,
    /// Number of sections (bones) in the skeleton.
    pub num_sections: u32,
}

/// Parsed HVA animation file.
///
/// Transforms are stored flat in frame-major order:
/// `transforms[frame * num_sections + section]` gives the 3×4 matrix
/// for that frame and section.
#[derive(Debug, Clone, PartialEq)]
pub struct HvaFile {
    /// File header.
    pub header: HvaHeader,
    /// Section (bone) names, one per section.
    pub section_names: Vec<[u8; 16]>,
    /// All transform matrices, flattened in frame-major order.
    pub transforms: Vec<[f32; 12]>,
}

impl HvaFile {
    /// Parses an HVA file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, section/frame counts exceeding
    /// V38 caps, or insufficient data for the declared matrices.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // ── Header (24 bytes) ────────────────────────────────────────────
        if data.len() < HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            });
        }

        let mut filename = [0u8; 16];
        let name_slice = data.get(..16).ok_or(Error::UnexpectedEof {
            needed: 16,
            available: data.len(),
        })?;
        filename.copy_from_slice(name_slice);

        let num_frames = read_u32_le(data, 16)?;
        let num_sections = read_u32_le(data, 20)?;

        // V38: cap counts.
        if (num_sections as usize) > MAX_SECTIONS {
            return Err(Error::InvalidSize {
                value: num_sections as usize,
                limit: MAX_SECTIONS,
                context: "HVA section count",
            });
        }
        if (num_frames as usize) > MAX_FRAMES {
            return Err(Error::InvalidSize {
                value: num_frames as usize,
                limit: MAX_FRAMES,
                context: "HVA frame count",
            });
        }

        let header = HvaHeader {
            filename,
            num_frames,
            num_sections,
        };

        // ── Section Names ────────────────────────────────────────────────
        let names_start = HEADER_SIZE;
        let names_total = (num_sections as usize).saturating_mul(SECTION_NAME_SIZE);
        let names_end = names_start.saturating_add(names_total);
        if names_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: names_end,
                available: data.len(),
            });
        }

        let mut section_names = Vec::with_capacity(num_sections as usize);
        for i in 0..num_sections as usize {
            let off = names_start.saturating_add(i.saturating_mul(SECTION_NAME_SIZE));
            let slice = data.get(off..off.saturating_add(SECTION_NAME_SIZE)).ok_or(
                Error::UnexpectedEof {
                    needed: off.saturating_add(SECTION_NAME_SIZE),
                    available: data.len(),
                },
            )?;
            let mut name = [0u8; 16];
            name.copy_from_slice(slice);
            section_names.push(name);
        }

        // ── Transform Matrices ───────────────────────────────────────────
        let matrices_start = names_end;
        let matrix_count = (num_frames as usize).saturating_mul(num_sections as usize);
        let matrices_total = matrix_count.saturating_mul(MATRIX_SIZE);
        let matrices_end = matrices_start.saturating_add(matrices_total);
        if matrices_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: matrices_end,
                available: data.len(),
            });
        }

        let mut transforms = Vec::with_capacity(matrix_count);
        for i in 0..matrix_count {
            let off = matrices_start.saturating_add(i.saturating_mul(MATRIX_SIZE));
            let mut mat = [0.0f32; 12];
            for (j, val) in mat.iter_mut().enumerate() {
                let f_off = off.saturating_add(j.saturating_mul(4));
                *val = f32::from_bits(read_u32_le(data, f_off)?);
            }
            transforms.push(mat);
        }

        Ok(HvaFile {
            header,
            section_names,
            transforms,
        })
    }

    /// Returns the 3×4 transform matrix for a given frame and section.
    ///
    /// Returns `None` if the frame or section index is out of range.
    #[inline]
    pub fn transform(&self, frame: u32, section: u32) -> Option<&[f32; 12]> {
        if frame >= self.header.num_frames || section >= self.header.num_sections {
            return None;
        }
        let index = (frame as usize)
            .checked_mul(self.header.num_sections as usize)?
            .checked_add(section as usize)?;
        self.transforms.get(index)
    }

    /// Returns the section name as a UTF-8 string (trimmed at the first NUL).
    #[inline]
    pub fn section_name(&self, index: usize) -> Option<&str> {
        let name = self.section_names.get(index)?;
        let nul = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        std::str::from_utf8(name.get(..nul).unwrap_or(&[])).ok()
    }
}

#[cfg(test)]
mod tests;
