// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Generals / Zero Hour APT GUI animation parser (`.apt`).
//!
//! APT files describe Flash-like interactive GUI elements used by the
//! SAGE engine.  Each file contains a header pointing to movie entries
//! that define characters, frames, and action bytecode.
//!
//! This module parses the file header and movie entry table, providing
//! raw access to the detailed structure data.  Full frame/character
//! interpretation is the engine's responsibility.
//!
//! ## File Layout
//!
//! ```text
//! [Header]         8 bytes: magic + apt_data_offset
//! [Padding/Data]   bytes between header and apt_data_offset
//! [APT Data]       movie_count + entry table + movie data
//! ```
//!
//! ## Companion Files
//!
//! - `.const` — string constants pool (NUL-separated)
//! - `.dat` — bitmap/shape binary data

use crate::error::Error;
use crate::read::read_u32_le;

/// APT magic bytes: `b"Apt\0"`.
const MAGIC: &[u8; 4] = b"Apt\0";

/// Size of the APT file header in bytes (magic + apt_data_offset).
const HEADER_SIZE: usize = 8;

/// Size of one movie entry in the entry table (5 x u32).
const ENTRY_SIZE: usize = 20;

/// Maximum number of movie entries allowed (sanity bound).
const MAX_MOVIES: usize = 4096;

/// A single entry in the APT movie/clip table.
///
/// Each entry provides an offset to detailed movie data plus four
/// additional u32 fields whose semantics vary by context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AptEntry {
    /// Offset to this entry's detailed data within the file.
    pub entry_offset: u32,
    /// Four u32 fields (semantics vary by entry type).
    pub fields: [u32; 4],
}

/// A parsed APT GUI animation file.
///
/// Provides access to the file header, the movie entry table, and raw
/// file data for following entry-offset pointers into the detailed
/// structure.
#[derive(Debug)]
pub struct AptFile<'input> {
    /// Offset from file start to the APT data section.
    apt_data_offset: u32,
    /// Parsed movie/clip entries.
    entries: Vec<AptEntry>,
    /// Full file data for raw access.
    data: &'input [u8],
}

impl<'input> AptFile<'input> {
    /// Parses an APT file from a byte slice.
    ///
    /// Validates the `Apt\0` magic, reads the data-section offset, then
    /// parses the movie entry table at that offset.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // 1. Check minimum size for header.
        if data.len() < HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            });
        }

        // 2. Validate magic.
        if data.get(..4) != Some(MAGIC.as_slice()) {
            return Err(Error::InvalidMagic {
                context: "APT header",
            });
        }

        // 3. Read apt_data_offset.
        let apt_data_offset = read_u32_le(data, 4)?;
        let apt_data_off = apt_data_offset as usize;

        // 4. Validate apt_data_offset is within bounds (need at least 4 bytes
        //    for movie_count at that offset).
        if apt_data_off.checked_add(4).is_none() || apt_data_off + 4 > data.len() {
            return Err(Error::InvalidOffset {
                offset: apt_data_off,
                bound: data.len(),
            });
        }

        // 5. Read movie_count.
        let movie_count = read_u32_le(data, apt_data_off)? as usize;

        // 6. Validate movie_count against sanity limit.
        if movie_count > MAX_MOVIES {
            return Err(Error::InvalidSize {
                value: movie_count,
                limit: MAX_MOVIES,
                context: "APT movie count",
            });
        }

        // 7. Validate that the full entry table fits in the remaining data.
        let table_start = apt_data_off + 4;
        let table_size = movie_count
            .checked_mul(ENTRY_SIZE)
            .ok_or(Error::InvalidSize {
                value: movie_count,
                limit: MAX_MOVIES,
                context: "APT movie count",
            })?;
        let table_end = table_start
            .checked_add(table_size)
            .ok_or(Error::InvalidOffset {
                offset: table_start,
                bound: data.len(),
            })?;

        if table_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: table_end,
                available: data.len(),
            });
        }

        // 8. Read entries.
        let mut entries = Vec::with_capacity(movie_count);
        let mut cursor = table_start;

        for _ in 0..movie_count {
            let entry_offset = read_u32_le(data, cursor)?;
            let f0 = read_u32_le(data, cursor + 4)?;
            let f1 = read_u32_le(data, cursor + 8)?;
            let f2 = read_u32_le(data, cursor + 12)?;
            let f3 = read_u32_le(data, cursor + 16)?;

            entries.push(AptEntry {
                entry_offset,
                fields: [f0, f1, f2, f3],
            });

            cursor += ENTRY_SIZE;
        }

        Ok(Self {
            apt_data_offset,
            entries,
            data,
        })
    }

    /// Returns the offset from file start to the APT data section.
    #[inline]
    pub fn apt_data_offset(&self) -> u32 {
        self.apt_data_offset
    }

    /// Returns the parsed movie/clip entries.
    #[inline]
    pub fn entries(&self) -> &[AptEntry] {
        &self.entries
    }

    /// Returns the number of entries in the movie table.
    #[inline]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns raw data starting at the given byte offset, or `None` if
    /// the offset is out of bounds.  Useful for following `entry_offset`
    /// pointers into the detailed structure data.
    #[inline]
    pub fn data_at(&self, offset: usize) -> Option<&'input [u8]> {
        self.data.get(offset..)
    }

    /// Returns the full raw file data.
    #[inline]
    pub fn raw_data(&self) -> &'input [u8] {
        self.data
    }
}

/// Parser for APT `.const` companion files (NUL-separated string pool).
///
/// Companion `.const` files contain text strings used by APT GUI elements,
/// separated by NUL (`0x00`) bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AptConst {
    /// Decoded strings from the pool.
    strings: Vec<String>,
}

/// Maximum number of strings allowed in a `.const` file (sanity bound).
const MAX_CONST_STRINGS: usize = 100_000;

impl AptConst {
    /// Parses a `.const` companion file from a byte slice.
    ///
    /// Splits on NUL bytes and collects non-empty segments as UTF-8 strings.
    /// Returns [`Error::InvalidMagic`] if any segment contains invalid UTF-8.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut strings = Vec::new();

        for chunk in data.split(|&b| b == 0) {
            if !chunk.is_empty() {
                let s = std::str::from_utf8(chunk).map_err(|_| Error::InvalidMagic {
                    context: "APT const encoding",
                })?;
                strings.push(s.to_string());
            }
        }

        if strings.len() > MAX_CONST_STRINGS {
            return Err(Error::InvalidSize {
                value: strings.len(),
                limit: MAX_CONST_STRINGS,
                context: "APT const entries",
            });
        }

        Ok(Self { strings })
    }

    /// Returns all parsed strings.
    #[inline]
    pub fn strings(&self) -> &[String] {
        &self.strings
    }

    /// Returns the string at the given index, or `None` if out of bounds.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&str> {
        self.strings.get(index).map(|s| s.as_str())
    }

    /// Returns the number of strings in the pool.
    #[inline]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Returns `true` if the string pool is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests;
