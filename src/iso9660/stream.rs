// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Streaming ISO 9660 reader backed by a seekable byte source.
//!
//! This reader does not require the entire ISO image in memory.  During
//! [`Iso9660ArchiveReader::open`] it reads the Primary Volume Descriptor
//! and recursively traverses directory extents (loading each into a
//! temporary buffer) to build the full file listing.  Individual files
//! are then served on demand via seeking.

use super::{
    entry_reader::Iso9660EntryReader, parse_dir_extent, validate_pvd, Iso9660Entry,
    MAX_DIRECTORY_DEPTH, MAX_ISO_ENTRIES, PVD_OFFSET, SECTOR_SIZE,
};
use crate::error::Error;
use crate::stream_io::{copy_exact, read_exact_array, read_exact_vec, seek_abs, stream_len};

use std::io::{Read, Seek, Write};

/// Streaming ISO 9660 image reader backed by a seekable byte source.
///
/// Unlike [`Iso9660Archive`], this does not require the entire image to be
/// loaded into memory.  It reads the Primary Volume Descriptor and
/// directory tree during [`open`], then serves individual file reads on
/// demand via seeking.
///
/// [`Iso9660Archive`]: super::Iso9660Archive
/// [`open`]: Iso9660ArchiveReader::open
#[derive(Debug)]
pub struct Iso9660ArchiveReader<R> {
    reader: R,
    entries: Vec<Iso9660Entry>,
}

impl<R: Read + Seek> Iso9660ArchiveReader<R> {
    /// Opens an ISO 9660 image from a seekable reader.
    ///
    /// Reads the Primary Volume Descriptor and recursively traverses the
    /// directory tree to build the file listing.  Individual files can
    /// then be read via [`read`], [`read_by_index`], [`copy`], or
    /// [`copy_by_index`].
    ///
    /// [`read`]: Iso9660ArchiveReader::read
    /// [`read_by_index`]: Iso9660ArchiveReader::read_by_index
    /// [`copy`]: Iso9660ArchiveReader::copy
    /// [`copy_by_index`]: Iso9660ArchiveReader::copy_by_index
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let file_len = stream_len(&mut reader, "measuring ISO 9660 image length")?;
        let min_size = PVD_OFFSET.saturating_add(SECTOR_SIZE) as u64;

        if file_len < min_size {
            return Err(Error::UnexpectedEof {
                needed: clamp_len(min_size),
                available: clamp_len(file_len),
            });
        }

        // ── Read and validate PVD ───────────────────────────────────────

        seek_abs(&mut reader, PVD_OFFSET as u64, "seeking to ISO 9660 PVD")?;
        let pvd = read_exact_array::<_, 2048>(&mut reader, "reading ISO 9660 PVD")?;

        validate_pvd(&pvd)?;

        // ── Extract root directory record ───────────────────────────────
        // Root directory record is at PVD offset 156.  Extent LBA at +2,
        // data length at +10, both as little-endian u32.

        let root_lba = u32::from_le_bytes(read_4(&pvd, 156 + 2)?) as u64;
        let root_size = u32::from_le_bytes(read_4(&pvd, 156 + 10)?) as u64;

        // ── Walk directory tree ─────────────────────────────────────────

        let mut entries = Vec::new();
        collect_entries_stream(
            &mut reader,
            root_lba,
            root_size,
            String::new(),
            &mut entries,
            0,
        )?;

        Ok(Self { reader, entries })
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

    /// Reads the first case-insensitive filename match into memory.
    pub fn read(&mut self, filename: &str) -> Result<Option<Vec<u8>>, Error> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(filename));
        match index {
            Some(index) => self.read_by_index(index),
            None => Ok(None),
        }
    }

    /// Reads the entry at `index` into memory.
    pub fn read_by_index(&mut self, index: usize) -> Result<Option<Vec<u8>>, Error> {
        let entry = match self.entries.get(index) {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let len = entry_size_to_usize(entry.size)?;
        seek_abs(
            &mut self.reader,
            entry.offset,
            "seeking to ISO 9660 file data",
        )?;
        let bytes = read_exact_vec(&mut self.reader, len, "reading ISO 9660 file data")?;
        Ok(Some(bytes))
    }

    /// Streams the first case-insensitive filename match into a writer.
    pub fn copy<W: Write>(&mut self, filename: &str, writer: &mut W) -> Result<bool, Error> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(filename));
        match index {
            Some(index) => self.copy_by_index(index, writer),
            None => Ok(false),
        }
    }

    /// Streams the entry at `index` into a writer.
    pub fn copy_by_index<W: Write>(&mut self, index: usize, writer: &mut W) -> Result<bool, Error> {
        let entry = match self.entries.get(index) {
            Some(entry) => entry,
            None => return Ok(false),
        };

        seek_abs(
            &mut self.reader,
            entry.offset,
            "seeking to ISO 9660 file data",
        )?;
        copy_exact(
            &mut self.reader,
            writer,
            entry.size,
            "reading ISO 9660 file data",
            "writing ISO 9660 file data",
        )?;
        Ok(true)
    }

    /// Opens a bounded reader for the first case-insensitive filename match.
    ///
    /// Returns an [`Iso9660EntryReader`] that implements `Read + Seek`
    /// constrained to the byte range of the matched entry.  This enables
    /// zero-extraction chaining: the returned reader can be passed directly
    /// to another archive parser (e.g. `MixArchiveReader::open`) to read
    /// nested archive contents without extracting to disk.
    ///
    /// Returns `Ok(None)` if no entry matches the filename.
    pub fn open_entry(
        &mut self,
        filename: &str,
    ) -> Result<Option<Iso9660EntryReader<'_, R>>, Error> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(filename));
        match index {
            Some(index) => self.open_entry_by_index(index),
            None => Ok(None),
        }
    }

    /// Opens a bounded reader for the entry at `index`.
    ///
    /// Returns `Ok(None)` if `index` is out of range.
    pub fn open_entry_by_index(
        &mut self,
        index: usize,
    ) -> Result<Option<Iso9660EntryReader<'_, R>>, Error> {
        let entry = match self.entries.get(index) {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let offset = entry.offset;
        let size = entry.size;
        let reader = Iso9660EntryReader::new(&mut self.reader, offset, size)?;
        Ok(Some(reader))
    }

    /// Returns entry indices sorted by ascending file offset.
    ///
    /// Extracting entries in this order linearises seeks, which is
    /// dramatically faster on rotational media and network-backed readers.
    pub fn indices_by_offset(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.entries.len()).collect();
        indices.sort_by_key(|&i| self.entries.get(i).map_or(u64::MAX, |e| e.offset));
        indices
    }

    /// Returns the underlying reader, consuming this archive reader.
    #[inline]
    pub fn into_inner(self) -> R {
        self.reader
    }
}

// ── Streaming directory tree walker ──────────────────────────────────────────

/// Recursively collects file entries by reading directory extents from a
/// seekable reader.
///
/// Each directory extent is read into a temporary `Vec<u8>` buffer, then
/// parsed via the shared [`parse_dir_extent`] function.  Subdirectories
/// trigger further seeks and reads.
fn collect_entries_stream<R: Read + Seek>(
    reader: &mut R,
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

    // Seek to directory extent and read it into a temporary buffer.
    let byte_offset = dir_lba
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or(Error::InvalidOffset {
            offset: usize::MAX,
            bound: 0,
        })?;
    let extent_len = entry_size_to_usize(extent_size)?;

    seek_abs(reader, byte_offset, "seeking to ISO 9660 directory extent")?;
    let dir_data = read_exact_vec(reader, extent_len, "reading ISO 9660 directory extent")?;

    let records = parse_dir_extent(&dir_data, &prefix)?;

    for rec in records {
        if rec.is_directory {
            collect_entries_stream(
                reader,
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

            let file_offset =
                rec.lba
                    .checked_mul(SECTOR_SIZE as u64)
                    .ok_or(Error::InvalidOffset {
                        offset: usize::MAX,
                        bound: 0,
                    })?;

            entries.push(Iso9660Entry {
                name: rec.full_path,
                offset: file_offset,
                size: rec.size,
            });
        }
    }

    Ok(())
}

// ── Helper functions ─────────────────────────────────────────────────────────

/// Reads 4 bytes from a fixed-size array at the given offset.
///
/// Used to extract little-endian u32 fields from the PVD buffer without
/// direct indexing.
fn read_4(buf: &[u8], offset: usize) -> Result<[u8; 4], Error> {
    let end = offset.checked_add(4).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available: buf.len(),
    })?;
    let slice = buf.get(offset..end).ok_or(Error::UnexpectedEof {
        needed: end,
        available: buf.len(),
    })?;
    let mut arr = [0u8; 4];
    arr.copy_from_slice(slice);
    Ok(arr)
}

/// Converts a u64 entry size to usize, returning an error if the value
/// exceeds the platform's address space.
fn entry_size_to_usize(size: u64) -> Result<usize, Error> {
    if size > usize::MAX as u64 {
        return Err(Error::InvalidSize {
            value: usize::MAX,
            limit: usize::MAX,
            context: "ISO 9660 entry size exceeds platform address space",
        });
    }
    Ok(size as usize)
}

/// Clamps a u64 to usize for use in error fields that require usize.
#[inline]
fn clamp_len(value: u64) -> usize {
    value.min(usize::MAX as u64) as usize
}
