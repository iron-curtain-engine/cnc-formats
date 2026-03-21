// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! MEG archive parser (`.meg`, `.pgm`).
//!
//! MEG is the archive format used by Petroglyph titles including
//! Empire at War, Grey Goo, and the C&C Remastered Collection.
//! Unlike MIX archives, MEG files store real filenames on disk.
//!
//! `.pgm` (map package) files reuse the same container format.
//!
//! ## Supported layout variants
//!
//! Petroglyph shipped multiple closely related MEG headers:
//!
//! - **Format 1:** legacy header with just `num_filenames` + `num_files`
//! - **Format 2:** 20-byte header with a Petroglyph marker and absolute
//!   `data_start`
//! - **Format 3:** Remastered-era 24-byte header adding `filenames_size`
//!
//! All three variants store:
//!
//! - a length-prefixed filename table (`u16` length + raw ASCII bytes)
//! - a fixed-width file record table
//! - file payloads addressed by absolute offsets from file start
//!
//! Remastered `.meg` files use format 3.  Older Petroglyph titles commonly
//! use format 2.  The original 8-byte header format is retained for
//! compatibility with legacy community tooling.
//!
//! ## References
//!
//! Implemented from Petroglyph community documentation and binary analysis of
//! real archives.  This is clean-room knowledge with no EA-derived code.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le};

mod stream;
pub use stream::MegArchiveReader;

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

/// Legacy MEG format header size (bytes).
const LEGACY_HEADER_SIZE: usize = 8;

/// Petroglyph MEG format 2 header size (bytes).
const PETRO_HEADER_SIZE: usize = 20;

/// Petroglyph MEG format 3 header size (bytes).
const REMASTERED_HEADER_SIZE: usize = 24;

/// Size of one MEG file record in all supported unencrypted variants (bytes).
const FILE_RECORD_SIZE: usize = 20;

/// Petroglyph MEG marker at offset 0 in formats 2 and 3.
const PETRO_FLAG_UNENCRYPTED: u32 = 0xFFFF_FFFF;

/// Petroglyph MEG encrypted marker (unsupported by this crate).
const PETRO_FLAG_ENCRYPTED: u32 = 0x8FFF_FFFF;

/// Petroglyph MEG format identifier at offset 4 in formats 2 and 3.
const PETRO_FORMAT_ID: u32 = 0x3F7D_70A4;

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
pub struct MegArchive<'input> {
    entries: Vec<MegEntry>,
    data: &'input [u8],
}

impl<'input> MegArchive<'input> {
    /// Parses a MEG archive from a byte slice.
    ///
    /// Supports `.meg` and `.pgm` files from Petroglyph titles.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`]  — input is too short for the declared tables.
    /// - [`Error::InvalidSize`]    — an entry count or filename length exceeds
    ///   the V38 safety cap.
    /// - [`Error::InvalidOffset`]  — a file record points outside the archive.
    /// - [`Error::InvalidMagic`]   — the archive uses an unsupported MEG
    ///   variant or encrypted layout.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        if data.len() < LEGACY_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: LEGACY_HEADER_SIZE,
                available: data.len(),
            });
        }
        let marker = read_u32_le(data, 0)?;
        if marker == PETRO_FLAG_ENCRYPTED {
            return Err(Error::InvalidMagic {
                context: "MEG encrypted archive",
            });
        }
        if marker == PETRO_FLAG_UNENCRYPTED {
            return Self::parse_petroglyph(data);
        }
        Self::parse_legacy(data)
    }

    /// Returns the file data for a given filename (case-insensitive),
    /// or `None` if not found.
    ///
    /// Uses `.get()` for defense-in-depth: entries are validated during
    /// `parse()`, but safe slicing prevents a panic if invariants are
    /// ever broken by a future code change.
    #[inline]
    pub fn get(&self, filename: &str) -> Option<&'input [u8]> {
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
    /// This is the preferred accessor when iterating `entries()` because
    /// it uses the entry's own offset/size directly, avoiding the
    /// first-match ambiguity of `get()` when an archive contains
    /// duplicate or case-colliding filenames.
    #[inline]
    pub fn get_by_index(&self, index: usize) -> Option<&'input [u8]> {
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

impl<'input> MegArchive<'input> {
    fn parse_legacy(data: &'input [u8]) -> Result<Self, Error> {
        let num_filenames = read_u32_le(data, 0)? as usize;
        let num_files = read_u32_le(data, 4)? as usize;
        validate_entry_count(num_filenames, "MEG filename count")?;
        validate_entry_count(num_files, "MEG entry count")?;

        let (filenames, records_start) =
            parse_filename_table(data, LEGACY_HEADER_SIZE, num_filenames, None)?;
        let records_bytes = checked_mul(num_files, FILE_RECORD_SIZE, data.len())?;
        let data_start = checked_add(records_start, records_bytes, data.len())?;
        if data_start > data.len() {
            return Err(Error::UnexpectedEof {
                needed: data_start,
                available: data.len(),
            });
        }

        let entries = parse_legacy_records(data, &filenames, records_start, num_files, data_start)?;
        Ok(Self { entries, data })
    }

    fn parse_petroglyph(data: &'input [u8]) -> Result<Self, Error> {
        if data.len() < PETRO_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: PETRO_HEADER_SIZE,
                available: data.len(),
            });
        }

        let format_id = read_u32_le(data, 4)?;
        if format_id != PETRO_FORMAT_ID {
            return Err(Error::InvalidMagic {
                context: "MEG header",
            });
        }

        let data_start = read_u32_le(data, 8)? as usize;
        let num_files = read_u32_le(data, 12)? as usize;
        let num_filenames = read_u32_le(data, 16)? as usize;

        validate_entry_count(num_filenames, "MEG filename count")?;
        validate_entry_count(num_files, "MEG entry count")?;
        validate_data_start(data_start, PETRO_HEADER_SIZE, data.len())?;

        if looks_like_remastered_layout(data, data_start, num_files)? {
            return Self::parse_remastered(data, data_start, num_filenames, num_files);
        }

        let records_bytes = checked_mul(num_files, FILE_RECORD_SIZE, data_start)?;
        if records_bytes > data_start {
            return Err(Error::InvalidOffset {
                offset: records_bytes,
                bound: data_start,
            });
        }
        let records_start = data_start - records_bytes;
        if records_start < PETRO_HEADER_SIZE {
            return Err(Error::InvalidOffset {
                offset: records_start,
                bound: data_start,
            });
        }

        let (filenames, names_end) =
            parse_filename_table(data, PETRO_HEADER_SIZE, num_filenames, Some(records_start))?;
        if names_end != records_start {
            return Err(Error::InvalidMagic {
                context: "MEG filename table size",
            });
        }

        let entries = parse_legacy_records(data, &filenames, records_start, num_files, data_start)?;
        Ok(Self { entries, data })
    }

    fn parse_remastered(
        data: &'input [u8],
        data_start: usize,
        num_filenames: usize,
        num_files: usize,
    ) -> Result<Self, Error> {
        if data.len() < REMASTERED_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: REMASTERED_HEADER_SIZE,
                available: data.len(),
            });
        }

        let filenames_size = read_u32_le(data, 20)? as usize;
        let names_end = checked_add(REMASTERED_HEADER_SIZE, filenames_size, data_start)?;
        if names_end > data_start {
            return Err(Error::InvalidOffset {
                offset: names_end,
                bound: data_start,
            });
        }

        let (filenames, parsed_end) =
            parse_filename_table(data, REMASTERED_HEADER_SIZE, num_filenames, Some(names_end))?;
        if parsed_end != names_end {
            return Err(Error::InvalidMagic {
                context: "MEG filename table size",
            });
        }

        let records_bytes = data_start - names_end;
        let expected_bytes = checked_mul(num_files, FILE_RECORD_SIZE, data_start)?;
        if records_bytes != expected_bytes {
            return Err(Error::InvalidMagic {
                context: "MEG record table size",
            });
        }

        let entries = parse_remastered_records(data, &filenames, names_end, num_files, data_start)?;
        Ok(Self { entries, data })
    }
}

fn looks_like_remastered_layout(
    data: &[u8],
    data_start: usize,
    num_files: usize,
) -> Result<bool, Error> {
    if data.len() < REMASTERED_HEADER_SIZE || data_start < REMASTERED_HEADER_SIZE {
        return Ok(false);
    }

    let filenames_size = read_u32_le(data, 20)? as usize;
    let Some(names_end) = REMASTERED_HEADER_SIZE.checked_add(filenames_size) else {
        return Ok(false);
    };
    if names_end > data_start {
        return Ok(false);
    }

    let records_bytes = data_start - names_end;
    if num_files == 0 {
        return Ok(records_bytes == 0);
    }

    Ok(records_bytes == num_files.saturating_mul(FILE_RECORD_SIZE))
}

fn parse_filename_table(
    data: &[u8],
    mut pos: usize,
    count: usize,
    exact_end: Option<usize>,
) -> Result<(Vec<String>, usize), Error> {
    let mut filenames = Vec::with_capacity(count);

    for _ in 0..count {
        let len_end = checked_add(pos, 2, data.len())?;
        if let Some(bound) = exact_end {
            if len_end > bound {
                return Err(Error::UnexpectedEof {
                    needed: len_end,
                    available: bound.min(data.len()),
                });
            }
        }

        let name_len = read_u16_le(data, pos)? as usize;
        pos = len_end;

        if name_len > MAX_FILENAME_LEN {
            return Err(Error::InvalidSize {
                value: name_len,
                limit: MAX_FILENAME_LEN,
                context: "MEG filename length",
            });
        }

        let name_end = checked_add(pos, name_len, data.len())?;
        if let Some(bound) = exact_end {
            if name_end > bound {
                return Err(Error::UnexpectedEof {
                    needed: name_end,
                    available: bound.min(data.len()),
                });
            }
        }

        let name_bytes = data.get(pos..name_end).ok_or(Error::UnexpectedEof {
            needed: name_end,
            available: data.len(),
        })?;
        filenames.push(String::from_utf8_lossy(name_bytes).into_owned());
        pos = name_end;
    }

    Ok((filenames, pos))
}

fn parse_legacy_records(
    data: &[u8],
    filenames: &[String],
    mut pos: usize,
    count: usize,
    data_start: usize,
) -> Result<Vec<MegEntry>, Error> {
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        let _crc32 = read_u32_le(data, pos)?;
        let _record_index = read_u32_le(data, pos + 4)?;
        let size = read_u32_le(data, pos + 8)?;
        let start = read_u32_le(data, pos + 12)?;
        let name_index = read_u32_le(data, pos + 16)? as usize;
        pos = checked_add(pos, FILE_RECORD_SIZE, data.len())?;

        entries.push(build_entry(
            filenames,
            name_index,
            start as usize,
            size as usize,
            data_start,
            data.len(),
        )?);
    }

    Ok(entries)
}

fn parse_remastered_records(
    data: &[u8],
    filenames: &[String],
    mut pos: usize,
    count: usize,
    data_start: usize,
) -> Result<Vec<MegEntry>, Error> {
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        let flags = read_u16_le(data, pos)?;
        if flags != 0 {
            return Err(Error::InvalidMagic {
                context: "MEG encrypted file record",
            });
        }

        let _crc32 = read_u32_le(data, pos + 2)?;
        let _record_index = read_u32_le(data, pos + 6)?;
        let size = read_u32_le(data, pos + 10)?;
        let start = read_u32_le(data, pos + 14)?;
        let name_index = read_u16_le(data, pos + 18)? as usize;
        pos = checked_add(pos, FILE_RECORD_SIZE, data.len())?;

        entries.push(build_entry(
            filenames,
            name_index,
            start as usize,
            size as usize,
            data_start,
            data.len(),
        )?);
    }

    Ok(entries)
}

fn build_entry(
    filenames: &[String],
    name_index: usize,
    start: usize,
    size: usize,
    data_start: usize,
    archive_len: usize,
) -> Result<MegEntry, Error> {
    let name = filenames.get(name_index).ok_or(Error::InvalidOffset {
        offset: name_index,
        bound: filenames.len(),
    })?;

    if start < data_start {
        return Err(Error::InvalidOffset {
            offset: start,
            bound: archive_len,
        });
    }

    let end = checked_add(start, size, archive_len)?;
    if end > archive_len {
        return Err(Error::InvalidOffset {
            offset: end,
            bound: archive_len,
        });
    }

    Ok(MegEntry {
        name: name.clone(),
        offset: start as u64,
        size: size as u64,
    })
}

fn validate_entry_count(value: usize, context: &'static str) -> Result<(), Error> {
    if value > MAX_MEG_ENTRIES {
        return Err(Error::InvalidSize {
            value,
            limit: MAX_MEG_ENTRIES,
            context,
        });
    }
    Ok(())
}

fn validate_data_start(data_start: usize, minimum: usize, archive_len: usize) -> Result<(), Error> {
    if data_start < minimum || data_start > archive_len {
        return Err(Error::InvalidOffset {
            offset: data_start,
            bound: archive_len,
        });
    }
    Ok(())
}

fn checked_add(lhs: usize, rhs: usize, available: usize) -> Result<usize, Error> {
    lhs.checked_add(rhs).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available,
    })
}

fn checked_mul(lhs: usize, rhs: usize, available: usize) -> Result<usize, Error> {
    lhs.checked_mul(rhs).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available,
    })
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_validation;
