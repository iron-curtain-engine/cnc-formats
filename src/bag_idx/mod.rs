// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Red Alert 2 / Yuri's Revenge audio archive parser (`.bag` + `.idx`).
//!
//! RA2 audio is stored as a pair of files: an IDX index file containing
//! fixed-size entry records, and a BAG data file holding concatenated audio.
//!
//! ## IDX Layout
//!
//! ```text
//! [Entry 0]   36 bytes: name[16] + offset + size + sample_rate + flags + chunk_size
//! [Entry 1]   36 bytes
//! ...
//! ```
//!
//! ## References
//!
//! Format source: XCC Utilities documentation, OpenRA source analysis.

use crate::error::Error;
use crate::read::read_u32_le;

/// Size of a single IDX entry in bytes.
const ENTRY_SIZE: usize = 36;

/// Size of the filename field within an IDX entry.
const NAME_SIZE: usize = 16;

/// Conservative upper bound for untrusted IDX entry counts.
const MAX_ENTRIES: usize = 65_536;

/// One entry stored in an IDX index file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdxEntry {
    /// Trimmed ASCII filename (NUL padding removed).
    pub name: String,
    /// Byte offset into the companion BAG file.
    pub offset: u32,
    /// Byte count in the companion BAG file.
    pub size: u32,
    /// Audio sample rate (e.g. 22050).
    pub sample_rate: u32,
    /// Audio format flags.
    pub flags: u32,
    /// Streaming chunk size.
    pub chunk_size: u32,
}

/// Parsed IDX index file.
#[derive(Debug)]
pub struct IdxFile {
    entries: Vec<IdxEntry>,
}

impl IdxFile {
    /// Parses an IDX index file from raw bytes.
    ///
    /// The IDX format is a flat array of 36-byte entries with no header.
    /// The input length must be a multiple of 36.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() % ENTRY_SIZE != 0 {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: ENTRY_SIZE,
                context: "IDX file size",
            });
        }

        let entry_count = data.len() / ENTRY_SIZE;
        if entry_count > MAX_ENTRIES {
            return Err(Error::InvalidSize {
                value: entry_count,
                limit: MAX_ENTRIES,
                context: "IDX entry count",
            });
        }

        let mut entries = Vec::with_capacity(entry_count);

        for i in 0..entry_count {
            let base = i * ENTRY_SIZE;

            // Read the 16-byte name field.
            let name_slice = data
                .get(base..base + NAME_SIZE)
                .ok_or(Error::UnexpectedEof {
                    needed: base + NAME_SIZE,
                    available: data.len(),
                })?;

            // Find the first NUL byte or use the full 16 bytes.
            let name_len = name_slice.iter().position(|&b| b == 0).unwrap_or(NAME_SIZE);
            let name = String::from_utf8_lossy(name_slice.get(..name_len).unwrap_or(name_slice))
                .into_owned();

            let offset = read_u32_le(data, base + NAME_SIZE)?;
            let size = read_u32_le(data, base + NAME_SIZE + 4)?;
            let sample_rate = read_u32_le(data, base + NAME_SIZE + 8)?;
            let flags = read_u32_le(data, base + NAME_SIZE + 12)?;
            let chunk_size = read_u32_le(data, base + NAME_SIZE + 16)?;

            entries.push(IdxEntry {
                name,
                offset,
                size,
                sample_rate,
                flags,
                chunk_size,
            });
        }

        Ok(Self { entries })
    }

    /// Returns all parsed entries.
    #[inline]
    pub fn entries(&self) -> &[IdxEntry] {
        &self.entries
    }

    /// Returns the first entry whose name matches `filename` (case-insensitive).
    #[inline]
    pub fn get(&self, filename: &str) -> Option<&IdxEntry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(filename))
    }

    /// Returns the entry at `index`.
    ///
    /// This is the preferred accessor when iterating over `entries()` because
    /// it avoids ambiguity from potential duplicate filenames.
    #[inline]
    pub fn get_by_index(&self, index: usize) -> Option<&IdxEntry> {
        self.entries.get(index)
    }

    /// Extracts audio data for an entry from a BAG file buffer.
    ///
    /// Returns `None` if the entry's offset+size range exceeds the BAG data.
    #[inline]
    pub fn extract<'bag>(&self, entry: &IdxEntry, bag_data: &'bag [u8]) -> Option<&'bag [u8]> {
        let start = entry.offset as usize;
        let end = start.checked_add(entry.size as usize)?;
        bag_data.get(start..end)
    }
}

#[cfg(test)]
mod tests;
