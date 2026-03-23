// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Dune II PAK archive parser (`.pak`).
//!
//! PAK is a simple archive format used by Dune II.  The file begins with a
//! directory of (offset, filename) pairs followed by the concatenated file data.
//!
//! ## File Layout
//!
//! ```text
//! [Directory]     N × (u32 offset + NUL-terminated name)
//! [File 0 data]   ...
//! [File 1 data]   ...
//! ```
//!
//! ## References
//!
//! Format source: Dune Legacy project documentation, CnC-Tools wiki.

use crate::error::Error;
use crate::read::read_u32_le;

/// Minimum valid PAK size: one u32 offset + at least one NUL byte for the name.
const MIN_SIZE: usize = 5;

/// Conservative upper bound on entry count to guard against malformed inputs.
const MAX_ENTRIES: usize = 16_384;

/// Maximum length of a single stored filename (DOS 8.3 names are typically
/// much shorter, but we allow room for longer paths).
const MAX_NAME_LEN: usize = 260;

/// One entry stored in a PAK archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakEntry {
    /// Stored filename.
    pub name: String,
    /// Absolute byte offset of the file payload within the archive.
    pub offset: usize,
    /// Byte length of the file payload.
    pub size: usize,
}

/// Parsed Dune II PAK archive.
#[derive(Debug)]
pub struct PakArchive<'input> {
    entries: Vec<PakEntry>,
    data: &'input [u8],
}

impl<'input> PakArchive<'input> {
    /// Parses a PAK archive from raw bytes.
    ///
    /// The directory is read from the start of the file until the read position
    /// reaches the first file's data offset.  File sizes are derived from
    /// successive offset differences; the last file extends to EOF.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        if data.len() < MIN_SIZE {
            return Err(Error::UnexpectedEof {
                needed: MIN_SIZE,
                available: data.len(),
            });
        }

        // The first u32 is the offset to the first file's data, which also
        // marks where the directory ends.
        let first_offset = read_u32_le(data, 0)? as usize;

        if first_offset < MIN_SIZE {
            return Err(Error::InvalidOffset {
                offset: first_offset,
                bound: data.len(),
            });
        }
        if first_offset > data.len() {
            return Err(Error::InvalidOffset {
                offset: first_offset,
                bound: data.len(),
            });
        }

        let mut entries: Vec<PakEntry> = Vec::new();
        let mut pos: usize = 0;

        // Walk the directory: each entry is a u32 LE offset followed by a
        // NUL-terminated ASCII filename.  We stop when the read position
        // reaches (or exceeds) the first file's data offset.
        while pos < first_offset {
            // We need at least 4 bytes for the offset and 1 byte for the NUL.
            let offset_end = pos.checked_add(4).ok_or(Error::InvalidOffset {
                offset: usize::MAX,
                bound: first_offset,
            })?;
            if offset_end > first_offset {
                // Not enough room for another full entry — directory is done.
                break;
            }

            let entry_offset = read_u32_le(data, pos)? as usize;
            pos = offset_end;

            // Validate the offset points within the file.
            if entry_offset > data.len() {
                return Err(Error::InvalidOffset {
                    offset: entry_offset,
                    bound: data.len(),
                });
            }

            // Scan for the NUL terminator within the remaining directory space.
            let name_region = data.get(pos..first_offset).ok_or(Error::UnexpectedEof {
                needed: first_offset,
                available: data.len(),
            })?;

            let nul_pos = name_region
                .iter()
                .position(|&b| b == 0)
                .ok_or(Error::InvalidMagic {
                    context: "PAK entry name terminator",
                })?;

            if nul_pos == 0 {
                return Err(Error::InvalidMagic {
                    context: "PAK entry name",
                });
            }

            if nul_pos > MAX_NAME_LEN {
                return Err(Error::InvalidSize {
                    value: nul_pos,
                    limit: MAX_NAME_LEN,
                    context: "PAK filename length",
                });
            }

            let name_bytes = name_region.get(..nul_pos).ok_or(Error::UnexpectedEof {
                needed: pos.saturating_add(nul_pos),
                available: data.len(),
            })?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();

            // Advance past the name and its NUL terminator.
            pos = pos.saturating_add(nul_pos + 1);

            entries.push(PakEntry {
                name,
                offset: entry_offset,
                size: 0, // computed below
            });

            if entries.len() > MAX_ENTRIES {
                return Err(Error::InvalidSize {
                    value: entries.len(),
                    limit: MAX_ENTRIES,
                    context: "PAK entry count",
                });
            }
        }

        if entries.is_empty() {
            return Err(Error::InvalidMagic {
                context: "PAK offset table",
            });
        }

        // Compute file sizes from offset differences.
        // size[i] = offset[i+1] - offset[i]; last file: data.len() - offset[last].
        let entry_count = entries.len();
        for i in 0..entry_count {
            let start = entries
                .get(i)
                .map(|e| e.offset)
                .ok_or(Error::InvalidOffset {
                    offset: i,
                    bound: entry_count,
                })?;
            let end = if i + 1 < entry_count {
                entries
                    .get(i + 1)
                    .map(|e| e.offset)
                    .ok_or(Error::InvalidOffset {
                        offset: i.saturating_add(1),
                        bound: entry_count,
                    })?
            } else {
                data.len()
            };

            if end < start {
                return Err(Error::InvalidOffset {
                    offset: start,
                    bound: end,
                });
            }

            if let Some(entry) = entries.get_mut(i) {
                entry.size = end - start;
            }
        }

        Ok(Self { entries, data })
    }

    /// Returns all parsed entries.
    #[inline]
    pub fn entries(&self) -> &[PakEntry] {
        &self.entries
    }

    /// Returns the file payload for the first case-insensitive name match.
    ///
    /// DOS filenames are case-insensitive, so lookups ignore ASCII case.
    #[inline]
    pub fn get(&self, filename: &str) -> Option<&'input [u8]> {
        for entry in &self.entries {
            if entry.name.eq_ignore_ascii_case(filename) {
                let start = entry.offset;
                let end = start.saturating_add(entry.size);
                return self.data.get(start..end);
            }
        }
        None
    }

    /// Returns the file payload for the entry at `index`.
    ///
    /// This is the preferred accessor when iterating over `entries()`.
    #[inline]
    pub fn get_by_index(&self, index: usize) -> Option<&'input [u8]> {
        let entry = self.entries.get(index)?;
        let start = entry.offset;
        let end = start.saturating_add(entry.size);
        self.data.get(start..end)
    }
}

#[cfg(test)]
mod tests;
