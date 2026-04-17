// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! ISO 9660 / ECMA-119 filesystem image parser (`.iso`).
//!
//! ISO 9660 is the standard filesystem for CD-ROM media.  C&C game discs
//! (Red Alert, Tiberian Dawn, Tiberian Sun) are distributed as ISO images.
//! This module parses the filesystem structure so that individual files can
//! be extracted by name without mounting or full extraction.
//!
//! ## How It Works
//!
//! The parser reads the Primary Volume Descriptor at sector 16 to locate the
//! root directory, then recursively walks the directory tree to build a flat
//! list of file entries.  Each entry records its full path, absolute byte
//! offset (LBA × 2048), and size.  File data is then accessible by seeking
//! to the byte offset and reading the indicated number of bytes — no
//! decompression needed, since ISO 9660 stores files as contiguous byte
//! runs.
//!
//! Two APIs are provided:
//!
//! - [`Iso9660Archive`](crate::iso9660::Iso9660Archive) — in-memory parser that borrows a `&[u8]` slice.
//! - [`Iso9660ArchiveReader`](crate::iso9660::Iso9660ArchiveReader) — streaming reader backed by any `Read + Seek`.
//!
//! ## File Layout
//!
//! ```text
//! [System Area]             sectors 0–15 (32 KiB, ignored)
//! [Primary Volume Desc]     sector 16 (2048 bytes, "CD001")
//! [Volume Desc Set Term]    sector 17+ (type 255)
//! [Directory Tree]          at LBA from root directory record
//! [File Data]               at LBAs from directory entries
//! ```
//!
//! Each directory record:
//!
//! ```text
//! record_len:   u8           (offset 0)
//! extent_lba:   u32 LE+BE   (offset 2, we read LE)
//! data_length:  u32 LE+BE   (offset 10, we read LE)
//! file_flags:   u8           (offset 25; bit 1 = directory)
//! name_len:     u8           (offset 32)
//! name:         [u8]         (offset 33; ASCII, ";1" version suffix stripped)
//! ```
//!
//! ## Limitations
//!
//! - Only the Primary Volume Descriptor is used (no Joliet, no Rock Ridge,
//!   no El Torito boot records).  This is sufficient for all C&C game ISOs.
//! - Logical block size must be 2048 bytes (universal for CD-ROM media).
//! - Multi-extent files (split across non-contiguous extents) are not
//!   supported — extremely rare in practice.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le, read_u8};

mod entry_reader;
mod stream;
pub use entry_reader::Iso9660EntryReader;
pub use stream::Iso9660ArchiveReader;

// ── Constants ────────────────────────────────────────────────────────────────

/// Standard CD-ROM sector (logical block) size in bytes.
pub(super) const SECTOR_SIZE: usize = 2048;

/// Byte offset of the Primary Volume Descriptor (`16 × 2048 = 32768`).
/// The first 16 sectors are the System Area, reserved for platform-specific
/// boot code and ignored by the ISO 9660 filesystem.
pub(super) const PVD_OFFSET: usize = 16 * SECTOR_SIZE;

/// Minimum valid image size: system area (16 sectors) + one PVD sector.
const MIN_ISO_SIZE: usize = PVD_OFFSET + SECTOR_SIZE;

/// Standard identifier bytes present in every volume descriptor.
const STANDARD_ID: &[u8; 5] = b"CD001";

/// Type code for the Primary Volume Descriptor.
const PVD_TYPE: u8 = 1;

/// File flags bit: entry describes a directory, not a regular file.
pub(super) const FLAG_DIRECTORY: u8 = 0x02;

/// Conservative upper bound on total file entries to prevent allocation
/// bombs from malformed images.
pub(super) const MAX_ISO_ENTRIES: usize = 524_288;

/// Maximum assembled path length after joining directory components.
pub(super) const MAX_PATH_LEN: usize = 4_096;

/// Maximum directory nesting depth to prevent stack overflow from cyclic
/// structures in malformed images.
pub(super) const MAX_DIRECTORY_DEPTH: usize = 32;

/// Minimum valid directory record length: 33 bytes of fixed header fields
/// plus at least 1 byte for the file identifier.
pub(super) const MIN_RECORD_LEN: usize = 34;

// ── Public types ─────────────────────────────────────────────────────────────

/// One file entry in an ISO 9660 image.
///
/// Directories are not included — only regular files appear as entries.
/// Paths use forward-slash separators and are relative to the image root
/// (e.g. `"INSTALL/MAIN.MIX"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iso9660Entry {
    /// Full path within the ISO (forward-slash separated, no leading slash).
    pub name: String,
    /// Absolute byte offset of the file data (`extent_lba × 2048`).
    pub offset: u64,
    /// File size in bytes.
    pub size: u64,
}

/// Parsed ISO 9660 filesystem image.
///
/// Created by [`Iso9660Archive::parse`], which reads the Primary Volume
/// Descriptor and recursively walks the directory tree to build a flat
/// list of file entries.  Individual files can then be retrieved by name
/// or index without re-parsing.
#[derive(Debug)]
pub struct Iso9660Archive<'input> {
    entries: Vec<Iso9660Entry>,
    data: &'input [u8],
}

impl<'input> Iso9660Archive<'input> {
    /// Parses an ISO 9660 image from raw bytes.
    ///
    /// Reads the Primary Volume Descriptor at sector 16, validates its
    /// header fields, and recursively traverses the directory tree to
    /// build a flat file listing.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        if data.len() < MIN_ISO_SIZE {
            return Err(Error::UnexpectedEof {
                needed: MIN_ISO_SIZE,
                available: data.len(),
            });
        }

        // ── Read and validate PVD ───────────────────────────────────────

        let pvd = data
            .get(PVD_OFFSET..PVD_OFFSET.saturating_add(SECTOR_SIZE))
            .ok_or(Error::UnexpectedEof {
                needed: PVD_OFFSET.saturating_add(SECTOR_SIZE),
                available: data.len(),
            })?;

        validate_pvd(pvd)?;

        // ── Extract root directory record ───────────────────────────────
        // The root directory record is a 34-byte structure embedded at
        // offset 156 within the PVD.  Its extent LBA tells us where the
        // root directory data starts on disc.

        let root_lba = read_u32_le(pvd, 156 + 2)? as u64;
        let root_size = read_u32_le(pvd, 156 + 10)? as u64;

        // ── Walk directory tree ─────────────────────────────────────────

        let mut entries = Vec::new();
        collect_entries_mem(data, root_lba, root_size, String::new(), &mut entries, 0)?;

        Ok(Self { entries, data })
    }

    /// Returns all parsed file entries.
    #[inline]
    pub fn entries(&self) -> &[Iso9660Entry] {
        &self.entries
    }

    /// Returns the number of files in the image.
    #[inline]
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the file payload for the first case-insensitive name match.
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

    /// Returns the file payload for the entry at `index`.
    #[inline]
    pub fn get_by_index(&self, index: usize) -> Option<&'input [u8]> {
        let entry = self.entries.get(index)?;
        let start = entry.offset as usize;
        let end = start.saturating_add(entry.size as usize);
        self.data.get(start..end)
    }
}

// ── PVD validation ───────────────────────────────────────────────────────────

/// Validates the Primary Volume Descriptor header fields.
///
/// Checks: type code (must be 1), standard identifier ("CD001"), version
/// (must be 1), and logical block size (must be 2048).  Shared between the
/// in-memory and streaming code paths.
pub(super) fn validate_pvd(pvd: &[u8]) -> Result<(), Error> {
    // Type code must be 1 (Primary Volume Descriptor).
    let type_code = read_u8(pvd, 0)?;
    if type_code != PVD_TYPE {
        return Err(Error::InvalidMagic {
            context: "ISO 9660 PVD type code (expected 1)",
        });
    }

    // Standard identifier must be "CD001" at bytes 1–5.
    let std_id = pvd.get(1..6).ok_or(Error::UnexpectedEof {
        needed: 6,
        available: pvd.len(),
    })?;
    if std_id != STANDARD_ID {
        return Err(Error::InvalidMagic {
            context: "ISO 9660 standard identifier (expected CD001)",
        });
    }

    // Version must be 1.
    let version = read_u8(pvd, 6)?;
    if version != 1 {
        return Err(Error::InvalidMagic {
            context: "ISO 9660 PVD version (expected 1)",
        });
    }

    // Logical block size must be 2048 for CD-ROM media.
    let block_size = read_u16_le(pvd, 128)? as usize;
    if block_size != SECTOR_SIZE {
        return Err(Error::InvalidSize {
            value: block_size,
            limit: SECTOR_SIZE,
            context: "ISO 9660 logical block size (expected 2048)",
        });
    }

    Ok(())
}

// ── Shared directory record parsing ──────────────────────────────────────────

/// Parsed fields from one non-dot/non-dotdot ISO 9660 directory record.
///
/// Used internally by both the in-memory and streaming walkers.  The
/// `full_path` field already incorporates the parent directory prefix.
pub(super) struct DirRecord {
    /// Assembled full path (e.g. `"INSTALL/MAIN.MIX"`).
    pub(super) full_path: String,
    /// Logical Block Address of the file or subdirectory extent.
    pub(super) lba: u64,
    /// Data length in bytes.
    pub(super) size: u64,
    /// Whether this record describes a subdirectory.
    pub(super) is_directory: bool,
}

/// Parses all meaningful directory records from a directory extent buffer.
///
/// Skips the "." (self) and ".." (parent) pseudo-entries.  Returns a list
/// of [`DirRecord`] with fully assembled paths.  The caller is responsible
/// for recursing into subdirectories and enforcing the global entry limit.
///
/// ## Sector-Boundary Padding
///
/// Directory records never cross sector boundaries.  A record-length byte
/// of zero signals that the rest of the current sector is unused padding;
/// parsing resumes at the next sector boundary.
pub(super) fn parse_dir_extent(dir_data: &[u8], prefix: &str) -> Result<Vec<DirRecord>, Error> {
    let mut records = Vec::new();
    let mut pos = 0usize;

    while pos < dir_data.len() {
        // ── Read record length ──────────────────────────────────────────
        // A zero byte means the rest of this sector is padding.
        let record_len = read_u8(dir_data, pos)? as usize;

        if record_len == 0 {
            let sector_offset = pos % SECTOR_SIZE;
            if sector_offset == 0 {
                // Already at a sector boundary with zero record length —
                // this is the end of the directory data.
                break;
            }
            // Skip remaining padding in this sector.
            pos = pos.saturating_add(SECTOR_SIZE - sector_offset);
            continue;
        }

        // ── Validate record structure ───────────────────────────────────

        if record_len < MIN_RECORD_LEN {
            return Err(Error::InvalidSize {
                value: record_len,
                limit: MIN_RECORD_LEN,
                context: "ISO 9660 directory record too short",
            });
        }

        let record_end = pos.checked_add(record_len).ok_or(Error::InvalidOffset {
            offset: usize::MAX,
            bound: dir_data.len(),
        })?;
        let record = dir_data.get(pos..record_end).ok_or(Error::InvalidOffset {
            offset: record_end,
            bound: dir_data.len(),
        })?;

        // ── Extract fixed fields ────────────────────────────────────────

        let entry_lba = read_u32_le(record, 2)? as u64;
        let entry_size = read_u32_le(record, 10)? as u64;
        let flags = read_u8(record, 25)?;
        let name_len = read_u8(record, 32)? as usize;

        // ── Skip "." (0x00) and ".." (0x01) pseudo-entries ──────────────
        // ISO 9660 encodes "." as a single 0x00 byte and ".." as a single
        // 0x01 byte in the file identifier field.  These are always the
        // first two records in a directory extent.

        let is_dot_entry = name_len == 1
            && record
                .get(33)
                .copied()
                .is_some_and(|b| b == 0x00 || b == 0x01);

        if !is_dot_entry && name_len > 0 {
            let name_end = 33_usize.checked_add(name_len).ok_or(Error::InvalidOffset {
                offset: usize::MAX,
                bound: record.len(),
            })?;
            let name_bytes = record.get(33..name_end).ok_or(Error::UnexpectedEof {
                needed: name_end,
                available: record.len(),
            })?;

            let raw_name = String::from_utf8_lossy(name_bytes);
            let clean_name = strip_version_suffix(&raw_name);

            let full_path = if prefix.is_empty() {
                clean_name.to_string()
            } else {
                format!("{prefix}/{clean_name}")
            };

            if full_path.len() > MAX_PATH_LEN {
                return Err(Error::InvalidSize {
                    value: full_path.len(),
                    limit: MAX_PATH_LEN,
                    context: "ISO 9660 file path length",
                });
            }

            records.push(DirRecord {
                full_path,
                lba: entry_lba,
                size: entry_size,
                is_directory: flags & FLAG_DIRECTORY != 0,
            });
        }

        pos = record_end;
    }

    Ok(records)
}

// ── In-memory directory tree walker ──────────────────────────────────────────

/// Recursively collects file entries from an in-memory ISO image.
///
/// Reads directory extent data directly from the `data` slice, parses
/// records via [`parse_dir_extent`], and recurses into subdirectories.
fn collect_entries_mem(
    data: &[u8],
    dir_lba: u64,
    extent_size: u64,
    prefix: String,
    entries: &mut Vec<Iso9660Entry>,
    depth: usize,
) -> Result<(), Error> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(Error::InvalidSize {
            value: depth,
            limit: MAX_DIRECTORY_DEPTH,
            context: "ISO 9660 directory nesting depth",
        });
    }

    // Compute byte range for this directory extent.
    let start = dir_lba
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or(Error::InvalidOffset {
            offset: usize::MAX,
            bound: data.len(),
        })? as usize;
    let end = start
        .checked_add(extent_size as usize)
        .ok_or(Error::InvalidOffset {
            offset: usize::MAX,
            bound: data.len(),
        })?;

    let dir_data = data.get(start..end).ok_or(Error::InvalidOffset {
        offset: end,
        bound: data.len(),
    })?;

    let records = parse_dir_extent(dir_data, &prefix)?;

    for rec in records {
        if rec.is_directory {
            collect_entries_mem(
                data,
                rec.lba,
                rec.size,
                rec.full_path,
                entries,
                depth.saturating_add(1),
            )?;
        } else {
            if entries.len() >= MAX_ISO_ENTRIES {
                return Err(Error::InvalidSize {
                    value: entries.len().saturating_add(1),
                    limit: MAX_ISO_ENTRIES,
                    context: "ISO 9660 total file count",
                });
            }

            let byte_offset =
                rec.lba
                    .checked_mul(SECTOR_SIZE as u64)
                    .ok_or(Error::InvalidOffset {
                        offset: usize::MAX,
                        bound: data.len(),
                    })?;

            entries.push(Iso9660Entry {
                name: rec.full_path,
                offset: byte_offset,
                size: rec.size,
            });
        }
    }

    Ok(())
}

// ── Filename cleanup ─────────────────────────────────────────────────────────

/// Strips the ISO 9660 version suffix (`;1`, `;2`, etc.) and any trailing
/// period from a filename.
///
/// ISO 9660 Level 1 filenames are stored as `"FILE.EXT;1"` where `;1` is
/// the file version number.  Some images also include a trailing period
/// when there is no extension (e.g. `"README.;1"` → `"README"`).
pub(super) fn strip_version_suffix(name: &str) -> &str {
    // Strip ";N" version suffix.  rfind is used because the semicolon is
    // always the last delimiter before the version number.
    let base = match name.rfind(';') {
        Some(pos) => name.get(..pos).unwrap_or(name),
        None => name,
    };
    // Strip trailing period that remains after removing an empty extension.
    base.strip_suffix('.').unwrap_or(base)
}

#[cfg(test)]
mod tests;
