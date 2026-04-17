// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Bounded reader over one file entry inside an ISO 9660 image.
//!
//! [`Iso9660EntryReader`] is a zero-copy adapter that presents a single file
//! within an ISO image as an independent `Read + Seek` stream bounded to that
//! file's byte range.  No data is copied to RAM; reads are served directly
//! from the underlying seekable source.
//!
//! ## Why This Exists
//!
//! ISO 9660 stores files as contiguous byte runs at known offsets.  This
//! adapter exploits that property to enable **chained reading** without
//! extraction:
//!
//! ```text
//! Iso9660ArchiveReader::open_entry("INSTALL/REDALERT.MIX")
//!   → Iso9660EntryReader (bounded to the MIX's byte range)
//!     → MixArchiveReader::open(entry_reader)
//!       → MixEntryReader (bounded to one MIX entry)
//!         → engine reads game data
//! ```
//!
//! The entire chain is zero-extraction: data is read on demand from the
//! original ISO image via seeking.  This is the same access pattern the
//! original games used when reading from CD-ROM.

use crate::stream_io::seek_abs;

use std::io::{self, Read, Seek, SeekFrom};

/// Bounded reader over one file entry inside an ISO 9660 image.
///
/// Created by [`Iso9660ArchiveReader::open_entry`] or
/// [`Iso9660ArchiveReader::open_entry_by_index`].  Implements `Read` and
/// `Seek` within the byte range of the entry.  Reads past the entry
/// boundary return EOF; seeks past the boundary return an error.
///
/// The reader borrows the underlying seekable source mutably, so only one
/// entry reader can be active at a time from the same archive.
///
/// [`Iso9660ArchiveReader::open_entry`]: super::Iso9660ArchiveReader::open_entry
/// [`Iso9660ArchiveReader::open_entry_by_index`]: super::Iso9660ArchiveReader::open_entry_by_index
#[derive(Debug)]
pub struct Iso9660EntryReader<'reader, R> {
    reader: &'reader mut R,
    /// Absolute byte offset of the entry's first byte in the ISO image.
    base_offset: u64,
    /// Total byte length of the entry.
    len: u64,
    /// Current read position within the entry (0-based).
    pos: u64,
}

impl<'reader, R: Read + Seek> Iso9660EntryReader<'reader, R> {
    /// Creates a new entry reader positioned at the start of the entry.
    ///
    /// The underlying reader is immediately seeked to `base_offset`.
    pub(crate) fn new(
        reader: &'reader mut R,
        base_offset: u64,
        len: u64,
    ) -> Result<Self, crate::Error> {
        seek_abs(reader, base_offset, "seeking to ISO 9660 entry data")?;
        Ok(Self {
            reader,
            base_offset,
            len,
            pos: 0,
        })
    }

    /// Returns the total byte length of this entry.
    #[inline]
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Returns `true` when the entry contains no payload bytes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the current position within the entry.
    #[inline]
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Returns the number of unread bytes remaining in the entry.
    #[inline]
    pub fn remaining_len(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }

    /// Seeks the underlying reader to the absolute position corresponding
    /// to `next_pos` within the entry.
    fn seek_within_entry(&mut self, next_pos: u64) -> io::Result<u64> {
        seek_abs(
            self.reader,
            self.base_offset.saturating_add(next_pos),
            "seeking within ISO 9660 entry",
        )
        .map_err(to_io_error)?;
        self.pos = next_pos;
        Ok(self.pos)
    }
}

impl<R: Read + Seek> Read for Iso9660EntryReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining_len();
        if remaining == 0 || buf.is_empty() {
            return Ok(0);
        }

        // Clamp the read to the entry boundary.
        let limit = remaining.min(buf.len() as u64) as usize;
        let slice = buf.get_mut(..limit).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid ISO 9660 entry read range",
            )
        })?;
        let read = self.reader.read(slice)?;
        self.pos = self.pos.saturating_add(read as u64);
        Ok(read)
    }
}

impl<R: Read + Seek> Seek for Iso9660EntryReader<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(delta) => seek_base(self.pos, delta)?,
            SeekFrom::End(delta) => seek_base(self.len, delta)?,
        };

        if next > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot seek past ISO 9660 entry bounds",
            ));
        }

        self.seek_within_entry(next)
    }
}

/// Computes a seek target from a base position and a signed delta.
///
/// Returns an error if the result would be negative or overflow u64.
fn seek_base(base: u64, delta: i64) -> io::Result<u64> {
    let next = (base as i128).saturating_add(delta as i128);
    if next < 0 || next > u64::MAX as i128 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid ISO 9660 entry seek position",
        ));
    }
    Ok(next as u64)
}

/// Converts a crate error to a standard I/O error.
fn to_io_error(error: crate::Error) -> io::Error {
    io::Error::other(error)
}
