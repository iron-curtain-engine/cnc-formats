// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! EA BIG archives (`.big`).
//!
//! BIG archives are the primary container format for SAGE-era games such as
//! Command & Conquer: Generals and Zero Hour. Unlike MIX archives, BIG files
//! store full filenames directly in their index records.
//!
//! ## File Layout
//!
//! ```text
//! [magic: "BIGF" | "BIG4"]  4 bytes
//! [archive_size]            u32, little-endian
//! [entry_count]             u32, big-endian
//! [first_data_offset]       u32, big-endian
//! [entry × count]           variable
//! [file data]               at absolute offsets from file start
//! ```
//!
//! Each index entry stores:
//!
//! ```text
//! offset: u32, big-endian
//! size:   u32, big-endian
//! name:   NUL-terminated ASCII path
//! ```
//!
//! Stored names typically use Windows separators (`\`).

use crate::error::Error;
use crate::read::{read_u32_be, read_u32_le};

/// Conservative upper bound for untrusted BIG entry counts.
const MAX_BIG_ENTRIES: usize = 262_144;

/// Conservative upper bound for one stored BIG filename.
const MAX_FILENAME_LEN: usize = 4_096;

/// BIG archive variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigVersion {
    /// Standard BIG archive (`BIGF`).
    BigF,
    /// Alternate BIG archive variant (`BIG4`).
    Big4,
}

/// One entry stored in a BIG archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigEntry {
    /// Stored archive path.
    pub name: String,
    /// Absolute byte offset of the file payload.
    pub offset: u64,
    /// Byte length of the file payload.
    pub size: u64,
}

/// Parsed BIG archive.
#[derive(Debug)]
pub struct BigArchive<'a> {
    version: BigVersion,
    entries: Vec<BigEntry>,
    data: &'a [u8],
}

impl<'a> BigArchive<'a> {
    /// Parses a BIG archive from raw bytes.
    ///
    /// Supports the `BIGF` and `BIG4` variants used by SAGE-era games.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 16 {
            return Err(Error::UnexpectedEof {
                needed: 16,
                available: data.len(),
            });
        }

        let version = match data.get(..4) {
            Some(b"BIGF") => BigVersion::BigF,
            Some(b"BIG4") => BigVersion::Big4,
            _ => {
                return Err(Error::InvalidMagic {
                    context: "BIG header",
                });
            }
        };

        // Real Generals BIG archives use a hybrid header layout: archive_size
        // is stored little-endian, while entry_count, first_data_offset, and
        // entry records are big-endian.
        let archive_size = read_u32_le(data, 4)? as usize;
        if archive_size < 16 || archive_size > data.len() {
            return Err(Error::InvalidSize {
                value: archive_size,
                limit: data.len(),
                context: "BIG archive size",
            });
        }

        let entry_count = read_u32_be(data, 8)? as usize;
        if entry_count > MAX_BIG_ENTRIES {
            return Err(Error::InvalidSize {
                value: entry_count,
                limit: MAX_BIG_ENTRIES,
                context: "BIG entry count",
            });
        }

        let first_data_offset = read_u32_be(data, 12)? as usize;
        if first_data_offset < 16 || first_data_offset > archive_size {
            return Err(Error::InvalidOffset {
                offset: first_data_offset,
                bound: archive_size,
            });
        }

        let mut entries = Vec::with_capacity(entry_count);
        let mut pos = 16usize;

        for _ in 0..entry_count {
            let header_end = pos.checked_add(8).ok_or(Error::InvalidOffset {
                offset: usize::MAX,
                bound: first_data_offset,
            })?;
            if header_end > first_data_offset {
                return Err(Error::InvalidOffset {
                    offset: header_end,
                    bound: first_data_offset,
                });
            }

            let offset = read_u32_be(data, pos)? as usize;
            let size = read_u32_be(data, pos + 4)? as usize;
            pos = header_end;

            let name_slice = data
                .get(pos..first_data_offset)
                .ok_or(Error::UnexpectedEof {
                    needed: first_data_offset,
                    available: data.len(),
                })?;
            let name_len = name_slice
                .iter()
                .position(|&b| b == 0)
                .ok_or(Error::InvalidMagic {
                    context: "BIG filename terminator",
                })?;
            if name_len == 0 {
                return Err(Error::InvalidMagic {
                    context: "BIG filename",
                });
            }
            if name_len > MAX_FILENAME_LEN {
                return Err(Error::InvalidSize {
                    value: name_len,
                    limit: MAX_FILENAME_LEN,
                    context: "BIG filename length",
                });
            }

            let name_bytes = name_slice.get(..name_len).ok_or(Error::UnexpectedEof {
                needed: pos.saturating_add(name_len),
                available: data.len(),
            })?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            pos = pos.saturating_add(name_len + 1);

            let end = offset.checked_add(size).ok_or(Error::InvalidOffset {
                offset: usize::MAX,
                bound: archive_size,
            })?;
            if end > archive_size {
                return Err(Error::InvalidOffset {
                    offset: end,
                    bound: archive_size,
                });
            }

            entries.push(BigEntry {
                name,
                offset: offset as u64,
                size: size as u64,
            });
        }

        if pos > first_data_offset {
            return Err(Error::InvalidOffset {
                offset: pos,
                bound: first_data_offset,
            });
        }

        Ok(Self {
            version,
            entries,
            data,
        })
    }

    /// Returns the archive variant.
    #[inline]
    pub fn version(&self) -> BigVersion {
        self.version
    }

    /// Returns all parsed entries.
    #[inline]
    pub fn entries(&self) -> &[BigEntry] {
        &self.entries
    }

    /// Returns the file payload for the first case-insensitive name match.
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

    /// Returns the file payload for the entry at `index`.
    ///
    /// This is the preferred accessor when iterating over `entries()` because
    /// duplicate filenames can exist in real BIG archives.
    pub fn get_by_index(&self, index: usize) -> Option<&'a [u8]> {
        let entry = self.entries.get(index)?;
        let start = entry.offset as usize;
        let end = start.saturating_add(entry.size as usize);
        self.data.get(start..end)
    }
}

#[cfg(test)]
mod tests;
