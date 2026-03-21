// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Shared reader/writer helpers for streaming format APIs.

use crate::error::Error;

use std::io::{Read, Seek, SeekFrom, Write};

/// Maps an underlying I/O error into the crate's shared [`Error`] type.
#[inline]
pub(crate) fn io_error(context: &'static str, err: std::io::Error) -> Error {
    Error::Io {
        context,
        kind: err.kind(),
    }
}

/// Reads exactly `buf.len()` bytes or returns a structured EOF error.
pub(crate) fn read_exact_fill<R: Read>(
    reader: &mut R,
    buf: &mut [u8],
    context: &'static str,
) -> Result<(), Error> {
    let mut filled = 0usize;
    let total = buf.len();
    while filled < total {
        let tail = buf.get_mut(filled..).ok_or(Error::UnexpectedEof {
            needed: total,
            available: filled,
        })?;
        let read = reader.read(tail).map_err(|err| io_error(context, err))?;
        if read == 0 {
            return Err(Error::UnexpectedEof {
                needed: total,
                available: filled,
            });
        }
        filled = filled.saturating_add(read);
    }
    Ok(())
}

/// Reads a fixed-size array from a reader.
pub(crate) fn read_exact_array<R: Read, const N: usize>(
    reader: &mut R,
    context: &'static str,
) -> Result<[u8; N], Error> {
    let mut buf = [0u8; N];
    read_exact_fill(reader, &mut buf, context)?;
    Ok(buf)
}

/// Reads an exact number of bytes into a newly allocated vector.
pub(crate) fn read_exact_vec<R: Read>(
    reader: &mut R,
    len: usize,
    context: &'static str,
) -> Result<Vec<u8>, Error> {
    let mut buf = vec![0u8; len];
    read_exact_fill(reader, &mut buf, context)?;
    Ok(buf)
}

/// Reads an exact number of bytes into a reusable vector buffer.
pub(crate) fn read_exact_reuse_vec<R: Read>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    len: usize,
    context: &'static str,
) -> Result<(), Error> {
    buf.clear();
    buf.resize(len, 0);
    read_exact_fill(reader, buf, context)
}

/// Returns the total byte length of a seekable stream without changing the current cursor.
pub(crate) fn stream_len<R: Seek>(reader: &mut R, context: &'static str) -> Result<u64, Error> {
    let pos = reader
        .stream_position()
        .map_err(|err| io_error(context, err))?;
    let end = reader
        .seek(SeekFrom::End(0))
        .map_err(|err| io_error(context, err))?;
    reader
        .seek(SeekFrom::Start(pos))
        .map_err(|err| io_error(context, err))?;
    Ok(end)
}

/// Seeks to an absolute byte offset.
#[inline]
pub(crate) fn seek_abs<R: Seek>(
    reader: &mut R,
    offset: u64,
    context: &'static str,
) -> Result<(), Error> {
    reader
        .seek(SeekFrom::Start(offset))
        .map(|_| ())
        .map_err(|err| io_error(context, err))
}

/// Copies exactly `len` bytes from `reader` to `writer`.
pub(crate) fn copy_exact<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    len: u64,
    read_context: &'static str,
    write_context: &'static str,
) -> Result<(), Error> {
    let mut remaining = len;
    let mut buf = [0u8; 8192];

    while remaining > 0 {
        let want = remaining.min(buf.len() as u64) as usize;
        let chunk = buf.get_mut(..want).ok_or(Error::UnexpectedEof {
            needed: want,
            available: 0,
        })?;
        read_exact_fill(reader, chunk, read_context)?;
        writer
            .write_all(chunk)
            .map_err(|err| io_error(write_context, err))?;
        remaining -= want as u64;
    }

    Ok(())
}
