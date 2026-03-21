// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Westwood setup/installer DIP data files (`.dip`).
//!
//! Two related DIP variants are present in the classic setup assets:
//!
//! - String-table DIPs, which reuse the same offset-table layout as `.eng`
//!   language files.
//! - Segmented installer-data DIPs, which begin with a small section table and
//!   are used by the classic setup programs for non-text UI/script data.

use crate::eng::EngFile;
use crate::error::Error;
use crate::read::read_u16_le;

/// Conservative upper bound for segmented DIP section counts.
const MAX_SEGMENT_COUNT: usize = 256;

/// One contiguous segment in a segmented DIP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DipSection<'input> {
    /// Zero-based section index.
    pub index: usize,
    /// Start byte offset of the section payload.
    pub start: usize,
    /// Exclusive end byte offset of the section payload.
    pub end: usize,
    /// Raw bytes belonging to the section.
    pub data: &'input [u8],
}

/// Parsed segmented installer-data DIP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DipSegmentedFile<'input> {
    /// Number of section end offsets stored in the header.
    pub section_count: u16,
    /// Total byte length of the header and section table.
    pub header_size: u16,
    /// End offsets for each section, in ascending order.
    pub end_offsets: Vec<usize>,
    /// Contiguous section payloads derived from the end-offset table.
    pub sections: Vec<DipSection<'input>>,
    /// Optional trailing control word left after the section data.
    pub trailer: &'input [u8],
}

impl<'input> DipSegmentedFile<'input> {
    /// Parses the segmented installer-data DIP variant.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        if data.len() < 8 {
            return Err(Error::UnexpectedEof {
                needed: 8,
                available: data.len(),
            });
        }

        let section_count = read_u16_le(data, 0)? as usize;
        if section_count == 0 || section_count > MAX_SEGMENT_COUNT {
            return Err(Error::InvalidSize {
                value: section_count,
                limit: MAX_SEGMENT_COUNT,
                context: "DIP segmented section count",
            });
        }

        let header_size = read_u16_le(data, 2)? as usize;
        let expected_header =
            4usize
                .checked_add(section_count.saturating_mul(4))
                .ok_or(Error::InvalidSize {
                    value: section_count,
                    limit: MAX_SEGMENT_COUNT,
                    context: "DIP segmented header size overflow",
                })?;
        if header_size != expected_header || header_size > data.len() {
            return Err(Error::InvalidSize {
                value: header_size,
                limit: data.len(),
                context: "DIP segmented header size",
            });
        }

        let mut end_offsets = Vec::with_capacity(section_count);
        let mut previous_end = header_size;
        for index in 0..section_count {
            let base = 4usize.saturating_add(index.saturating_mul(4));
            let hi = read_u16_le(data, base)? as usize;
            let lo = read_u16_le(data, base + 2)? as usize;
            let end = (hi << 16) | lo;
            if end <= previous_end || end > data.len() {
                return Err(Error::InvalidOffset {
                    offset: end,
                    bound: data.len(),
                });
            }
            end_offsets.push(end);
            previous_end = end;
        }

        let mut sections = Vec::with_capacity(section_count);
        let mut start = header_size;
        for (index, &end) in end_offsets.iter().enumerate() {
            let payload = data.get(start..end).ok_or(Error::InvalidOffset {
                offset: end,
                bound: data.len(),
            })?;
            sections.push(DipSection {
                index,
                start,
                end,
                data: payload,
            });
            start = end;
        }

        let trailer = data.get(start..).ok_or(Error::InvalidOffset {
            offset: start,
            bound: data.len(),
        })?;
        if trailer.len() > 2 {
            return Err(Error::InvalidSize {
                value: trailer.len(),
                limit: 2,
                context: "DIP segmented trailer size",
            });
        }
        if trailer.len() == 2 {
            let trailer_word = read_u16_le(data, start)?;
            if trailer_word < 0x8000 {
                return Err(Error::InvalidMagic {
                    context: "DIP segmented trailer",
                });
            }
        }

        if !sections
            .iter()
            .any(|section| section.data.iter().any(|&byte| byte >= 0x80))
        {
            return Err(Error::InvalidMagic {
                context: "DIP segmented control stream",
            });
        }

        Ok(Self {
            section_count: section_count as u16,
            header_size: header_size as u16,
            end_offsets,
            sections,
            trailer,
        })
    }
}

/// Parsed DIP file, covering both known on-disk variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DipFile<'input> {
    /// Offset-table string data, identical to `.eng` layout.
    StringTable(EngFile<'input>),
    /// Segmented installer-data control/layout file.
    Segmented(DipSegmentedFile<'input>),
}

impl<'input> DipFile<'input> {
    /// Parses either supported DIP variant.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        match DipSegmentedFile::parse(data) {
            Ok(segmented) => Ok(Self::Segmented(segmented)),
            Err(segmented_err) => match EngFile::parse(data) {
                Ok(strings) => Ok(Self::StringTable(strings)),
                Err(_) => Err(segmented_err),
            },
        }
    }
}

#[cfg(test)]
mod tests;
