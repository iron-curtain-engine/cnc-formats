// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use crate::stream_io::seek_abs;

use std::io::{self, Read, Seek, SeekFrom};

/// Bounded reader over one MIX entry inside a larger seekable source.
///
/// This adapter lets downstream code stream a single entry directly from a
/// [`Read`] + [`Seek`] source without materializing the whole entry in RAM
/// first. The reader is limited to the byte range of one MIX entry and never
/// reads past that boundary.
///
/// The timing/allocation model is byte-stream based: reads are pulled on
/// demand, and the reader performs no heap allocation on the hot path. It
/// borrows the underlying stream mutably for the lifetime of the entry reader,
/// so only one entry can be read from the same archive cursor at a time.
#[derive(Debug)]
pub struct MixEntryReader<'reader, R> {
    reader: &'reader mut R,
    base_offset: u64,
    len: u64,
    pos: u64,
}

impl<'reader, R: Read + Seek> MixEntryReader<'reader, R> {
    pub(crate) fn new(
        reader: &'reader mut R,
        base_offset: u64,
        len: u64,
    ) -> Result<Self, crate::Error> {
        seek_abs(reader, base_offset, "seeking to MIX entry data")?;
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

    fn seek_within_entry(&mut self, next_pos: u64) -> io::Result<u64> {
        seek_abs(
            self.reader,
            self.base_offset.saturating_add(next_pos),
            "seeking within MIX entry",
        )
        .map_err(to_io_error)?;
        self.pos = next_pos;
        Ok(self.pos)
    }
}

impl<R: Read + Seek> Read for MixEntryReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining_len();
        if remaining == 0 || buf.is_empty() {
            return Ok(0);
        }

        let limit = remaining.min(buf.len() as u64) as usize;
        let slice = buf.get_mut(..limit).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid MIX entry read range")
        })?;
        let read = self.reader.read(slice)?;
        self.pos = self.pos.saturating_add(read as u64);
        Ok(read)
    }
}

impl<R: Read + Seek> Seek for MixEntryReader<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(delta) => seek_base(self.pos, delta)?,
            SeekFrom::End(delta) => seek_base(self.len, delta)?,
        };

        if next > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot seek past MIX entry bounds",
            ));
        }

        self.seek_within_entry(next)
    }
}

fn seek_base(base: u64, delta: i64) -> io::Result<u64> {
    let next = (base as i128).saturating_add(delta as i128);
    if next < 0 || next > u64::MAX as i128 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid MIX entry seek position",
        ));
    }
    Ok(next as u64)
}

fn to_io_error(error: crate::Error) -> io::Error {
    io::Error::other(error)
}
