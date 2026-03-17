// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! VQP palette interpolation tables (`.vqp`).
//!
//! VQP files store one or more packed 256x256 interpolation tables used by
//! classic Westwood VQA playback when horizontally stretching paletted video.
//! Each stored table is the lower triangle of a symmetric 256x256 matrix,
//! packed row-by-row:
//!
//! ```text
//! [u32 num_tables]
//! [table 0 packed triangle]   32,896 bytes
//! [table 1 packed triangle]   32,896 bytes
//! ...
//! ```
//!
//! A full expanded table would be 65,536 bytes, but the on-disk format stores
//! only the unique lower-triangle entries because `table[a][b] == table[b][a]`.

use crate::error::Error;
use crate::read::read_u32_le;

/// Packed byte size of one VQP interpolation table.
pub const VQP_TABLE_SIZE: usize = 32_896;

/// Maximum number of tables accepted from untrusted input.
///
/// Original game files are far smaller than this. The cap prevents hostile
/// headers from driving oversized table vectors while leaving wide headroom
/// for valid content.
const MAX_TABLE_COUNT: usize = 4096;

/// One packed VQP interpolation table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqpTable<'a> {
    /// Zero-based table index.
    pub index: usize,
    /// Packed lower-triangle bytes.
    pub packed: &'a [u8],
}

impl<'a> VqpTable<'a> {
    /// Returns the palette index for the `(left, right)` pair.
    ///
    /// The on-disk table stores only the lower triangle. This accessor mirrors
    /// `(left, right)` into that stored half and returns the corresponding byte.
    pub fn get(&self, left: u8, right: u8) -> u8 {
        let (row, col) = if left >= right {
            (left as usize, right as usize)
        } else {
            (right as usize, left as usize)
        };
        let base = row.saturating_mul(row.saturating_add(1)) / 2;
        let offset = base.saturating_add(col);
        self.packed.get(offset).copied().unwrap_or(0)
    }
}

/// Parsed VQP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqpFile<'a> {
    /// Number of interpolation tables stored in the file.
    pub num_tables: u32,
    /// Packed interpolation tables.
    pub tables: Vec<VqpTable<'a>>,
}

impl<'a> VqpFile<'a> {
    /// Parses a VQP file from raw bytes.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] if the input is shorter than the header.
    /// - [`Error::InvalidSize`] if the table count exceeds limits or if the
    ///   file size does not match the packed-table layout exactly.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 4 {
            return Err(Error::UnexpectedEof {
                needed: 4,
                available: data.len(),
            });
        }

        let num_tables = read_u32_le(data, 0)?;
        let table_count = num_tables as usize;
        if table_count > MAX_TABLE_COUNT {
            return Err(Error::InvalidSize {
                value: table_count,
                limit: MAX_TABLE_COUNT,
                context: "VQP table count",
            });
        }

        let table_bytes = table_count
            .checked_mul(VQP_TABLE_SIZE)
            .ok_or(Error::InvalidSize {
                value: table_count,
                limit: MAX_TABLE_COUNT,
                context: "VQP table byte count overflow",
            })?;
        let expected_len = 4usize.checked_add(table_bytes).ok_or(Error::InvalidSize {
            value: table_bytes,
            limit: usize::MAX.saturating_sub(4),
            context: "VQP file size overflow",
        })?;

        if data.len() != expected_len {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: expected_len,
                context: "VQP file size",
            });
        }

        let mut tables = Vec::with_capacity(table_count);
        let mut offset = 4usize;
        for index in 0..table_count {
            let end = offset
                .checked_add(VQP_TABLE_SIZE)
                .ok_or(Error::InvalidOffset {
                    offset: usize::MAX,
                    bound: data.len(),
                })?;
            let packed = data.get(offset..end).ok_or(Error::UnexpectedEof {
                needed: end,
                available: data.len(),
            })?;
            tables.push(VqpTable { index, packed });
            offset = end;
        }

        Ok(Self { num_tables, tables })
    }
}

#[cfg(test)]
mod tests;
