// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! MEG archive parser (`.meg`, `.pgm`).
//!
//! A MEG file is an archive format used by Petroglyph titles including
//! the C&C Remastered Collection (2020), Empire at War (2006), and
//! Grey Goo (2015).  Unlike MIX archives, MEG files store actual filenames
//! alongside their file records.
//!
//! `.pgm` (map package) files reuse the same binary format and are parsed
//! identically.
//!
//! ## File Layout
//!
//! ```text
//! [Header]              8 bytes  (num_filenames: u32, num_files: u32)
//! [Filename Table]      variable (length-prefixed ASCII strings)
//! [FileRecord × count]  18 bytes each
//! [file data]           at absolute offsets from file start
//! ```
//!
//! ## Filename Table
//!
//! Each filename is prefixed with a `u16` length and stored as raw bytes
//! (ASCII, not null-terminated).  Filenames are used for direct lookup.
//!
//! ## File Records
//!
//! Each file record is 18 bytes:
//!
//! ```text
//! crc32:       u32  — CRC32 of uppercased filename (not verified during parse)
//! index:       u32  — index into filename table
//! size:        u32  — file size in bytes
//! start:       u32  — absolute byte offset in the archive
//! name_length: u16  — filename length (redundant with filename table)
//! ```
//!
//! ## References
//!
//! Implemented from OS Big Editor, OpenSage, and Petroglyph modding
//! community documentation (ModEnc, PPM).  This is clean-room knowledge —
//! no EA-derived or Petroglyph proprietary code.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le};

/// V38 safety cap: maximum number of entries in a MEG archive.
///
/// Real Remastered archives contain ~3,000 entries; 65,536 is generous
/// enough for any real archive while preventing a crafted header from
/// allocating excessive memory (65,536 × ~100 bytes ≈ 6.5 MB, acceptable).
pub(crate) const MAX_MEG_ENTRIES: usize = 65_536;

/// V38 safety cap: maximum filename length in a MEG entry.
///
/// Real filenames are typically under 200 characters; 4,096 prevents
/// a crafted length field from consuming excessive memory.
pub(crate) const MAX_FILENAME_LEN: usize = 4_096;

/// Size of one file record in the MEG file table (bytes).
const FILE_RECORD_SIZE: usize = 18;

// ─── Structures ──────────────────────────────────────────────────────────────

/// One entry in the MEG archive.
///
/// Filenames are stored directly (unlike MIX which uses CRC hashes).
/// The `offset` and `size` fields use `u64` to accommodate archives
/// exceeding 4 GB, though on-disk values are `u32` and widened during
/// parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MegEntry {
    /// The filename stored in the archive.
    pub name: String,
    /// Absolute byte offset of the file data within the archive.
    pub offset: u64,
    /// File size in bytes.
    pub size: u64,
}

/// A parsed MEG archive.
///
/// File data is accessed by calling [`MegArchive::get`] with a filename;
/// the method performs a case-insensitive search over the entry table.
#[derive(Debug)]
pub struct MegArchive<'a> {
    entries: Vec<MegEntry>,
    data: &'a [u8],
}

impl<'a> MegArchive<'a> {
    /// Parses a MEG archive from a byte slice.
    ///
    /// Supports `.meg` and `.pgm` files from Petroglyph titles.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`]  — input is too short for header/tables.
    /// - [`Error::InvalidSize`]   — entry count or filename length exceeds
    ///   the V38 safety cap.
    /// - [`Error::InvalidOffset`] — a file record offset points past the data.
    /// - [`Error::InvalidMagic`]  — `num_filenames != num_files` (format violation).
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // ── Header: num_filenames (u32) + num_files (u32) ────────────
        if data.len() < 8 {
            return Err(Error::UnexpectedEof {
                needed: 8,
                available: data.len(),
            });
        }
        let num_filenames = read_u32_le(data, 0)? as usize;
        let num_files = read_u32_le(data, 4)? as usize;

        // num_filenames must equal num_files per format spec.
        if num_filenames != num_files {
            return Err(Error::InvalidMagic {
                context: "MEG header: num_filenames != num_files",
            });
        }

        // V38: Reject archives with unreasonable entry counts.
        if num_files > MAX_MEG_ENTRIES {
            return Err(Error::InvalidSize {
                value: num_files,
                limit: MAX_MEG_ENTRIES,
                context: "MEG entry count",
            });
        }

        // ── Filename Table ───────────────────────────────────────────
        let mut pos = 8usize;
        let mut filenames = Vec::with_capacity(num_filenames);

        for _ in 0..num_filenames {
            if pos.saturating_add(2) > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: pos.saturating_add(2),
                    available: data.len(),
                });
            }
            let name_len = read_u16_le(data, pos)? as usize;
            pos = pos.saturating_add(2);

            // V38: Reject unreasonably long filenames.
            if name_len > MAX_FILENAME_LEN {
                return Err(Error::InvalidSize {
                    value: name_len,
                    limit: MAX_FILENAME_LEN,
                    context: "MEG filename length",
                });
            }

            let name_end = pos.saturating_add(name_len);
            if name_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: name_end,
                    available: data.len(),
                });
            }
            let name_bytes = data.get(pos..name_end).ok_or(Error::UnexpectedEof {
                needed: name_end,
                available: data.len(),
            })?;
            // MEG filenames are ASCII.  Use lossy conversion for
            // defense-in-depth against malformed archives.
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            filenames.push(name);
            pos = name_end;
        }

        // ── File Records (18 bytes each) ─────────────────────────────
        let records_bytes = num_files.saturating_mul(FILE_RECORD_SIZE);
        if pos.saturating_add(records_bytes) > data.len() {
            return Err(Error::UnexpectedEof {
                needed: pos.saturating_add(records_bytes),
                available: data.len(),
            });
        }

        let mut entries = Vec::with_capacity(num_files);

        for _ in 0..num_files {
            // crc32 (4 bytes) — stored but not verified during parse.
            // Index (4) into filename table, size (4), start offset (4),
            // redundant name_length (2).
            let _crc32 = read_u32_le(data, pos)?;
            let index = read_u32_le(data, pos + 4)? as usize;
            let size = read_u32_le(data, pos + 8)?;
            let start = read_u32_le(data, pos + 12)?;
            let _name_length = read_u16_le(data, pos + 16)?;
            pos = pos.saturating_add(FILE_RECORD_SIZE);

            // V38: Validate filename index is within bounds.
            if index >= filenames.len() {
                return Err(Error::InvalidOffset {
                    offset: index,
                    bound: filenames.len(),
                });
            }

            // V38: Validate offset+size fits within the archive.
            let end = (start as u64).saturating_add(size as u64);
            if end > data.len() as u64 {
                return Err(Error::InvalidOffset {
                    offset: end as usize,
                    bound: data.len(),
                });
            }

            entries.push(MegEntry {
                name: filenames[index].clone(),
                offset: u64::from(start),
                size: u64::from(size),
            });
        }

        Ok(MegArchive { entries, data })
    }

    /// Returns the file data for a given filename (case-insensitive),
    /// or `None` if not found.
    ///
    /// Uses `.get()` for defense-in-depth: entries are validated during
    /// `parse()`, but safe slicing prevents a panic if invariants are
    /// ever broken by a future code change.
    pub fn get(&self, filename: &str) -> Option<&'a [u8]> {
        for entry in &self.entries {
            if entry.name.eq_ignore_ascii_case(filename) {
                let start = entry.offset as usize;
                let end = start.saturating_add(entry.size as usize);
                return self.data.get(start..end);
            }
        }
        None
    }

    /// Returns the file data for the entry at `index`, or `None` if the
    /// index is out of bounds or the slice falls outside the archive.
    ///
    /// This is the preferred accessor when iterating [`entries()`] because
    /// it uses the entry's own offset/size directly, avoiding the
    /// first-match ambiguity of [`get()`] when an archive contains
    /// duplicate or case-colliding filenames.
    pub fn get_by_index(&self, index: usize) -> Option<&'a [u8]> {
        let entry = self.entries.get(index)?;
        let start = entry.offset as usize;
        let end = start.saturating_add(entry.size as usize);
        self.data.get(start..end)
    }

    /// Returns a slice over all entries.
    #[inline]
    pub fn entries(&self) -> &[MegEntry] {
        &self.entries
    }

    /// Returns the number of files in this archive.
    #[inline]
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_validation;
