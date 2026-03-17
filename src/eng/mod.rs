// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Westwood string-table files (`.eng` and localized siblings such as
//! `.ger` / `.fre`).
//!
//! The on-disk layout is a 16-bit offset table followed by NUL-terminated
//! strings. The first `u16` is both the first string offset and the total byte
//! length of the offset table, so the string count is `first_offset / 2`.

use crate::error::Error;
use crate::read::read_u16_le;
use std::borrow::Cow;

/// Maximum number of strings addressable by the 16-bit offset table.
const MAX_ENG_STRINGS: usize = (u16::MAX as usize) / 2;

/// One entry in a Westwood string table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngString<'a> {
    /// Zero-based string index.
    pub index: usize,
    /// Raw on-disk offset of the NUL-terminated string.
    pub offset: u16,
    /// String bytes without the trailing NUL terminator.
    pub bytes: &'a [u8],
}

impl<'a> EngString<'a> {
    /// Returns the string decoded lossily as UTF-8 for debugging/UI output.
    pub fn as_lossy_str(&self) -> Cow<'a, str> {
        String::from_utf8_lossy(self.bytes)
    }

    /// Returns `true` if this entry is the empty string.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Parsed Westwood language string table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngFile<'a> {
    /// Byte offset where the string blob begins.
    pub data_start: u16,
    /// Parsed table entries.
    pub strings: Vec<EngString<'a>>,
}

impl<'a> EngFile<'a> {
    /// Parses a Westwood string table.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 2 {
            return Err(Error::UnexpectedEof {
                needed: 2,
                available: data.len(),
            });
        }

        let data_start = read_u16_le(data, 0)?;
        let table_len = data_start as usize;
        if table_len < 2 || table_len > data.len() || table_len % 2 != 0 {
            return Err(Error::InvalidSize {
                value: table_len,
                limit: data.len(),
                context: "ENG offset table length",
            });
        }

        let string_count = table_len / 2;
        if string_count > MAX_ENG_STRINGS {
            return Err(Error::InvalidSize {
                value: string_count,
                limit: MAX_ENG_STRINGS,
                context: "ENG string count",
            });
        }

        let mut offsets = Vec::with_capacity(string_count);
        let mut previous = table_len;
        for i in 0..string_count {
            let offset = read_u16_le(data, i.saturating_mul(2))? as usize;
            if offset < table_len || offset >= data.len() {
                return Err(Error::InvalidOffset {
                    offset,
                    bound: data.len(),
                });
            }
            if offset < previous {
                return Err(Error::InvalidSize {
                    value: offset,
                    limit: previous,
                    context: "ENG string offsets",
                });
            }
            offsets.push(offset as u16);
            previous = offset;
        }

        let mut strings = Vec::with_capacity(string_count);
        for (index, offset) in offsets.iter().enumerate() {
            let start = *offset as usize;
            let tail = data.get(start..).ok_or(Error::InvalidOffset {
                offset: start,
                bound: data.len(),
            })?;
            let nul = tail
                .iter()
                .position(|&b| b == 0)
                .ok_or(Error::UnexpectedEof {
                    needed: data.len().saturating_add(1),
                    available: data.len(),
                })?;
            let bytes = tail.get(..nul).unwrap_or(&[]);
            strings.push(EngString {
                index,
                offset: *offset,
                bytes,
            });
        }

        Ok(Self {
            data_start,
            strings,
        })
    }

    /// Returns the number of strings in the table.
    #[inline]
    pub fn string_count(&self) -> usize {
        self.strings.len()
    }
}

#[cfg(test)]
mod tests;
