// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{BigEntry, BigVersion, MAX_BIG_ENTRIES, MAX_FILENAME_LEN};
use crate::error::Error;
use crate::stream_io::{copy_exact, read_exact_array, read_exact_vec, seek_abs, stream_len};

use std::io::{Read, Seek, Write};

/// Streaming BIG archive reader backed by a seekable byte source.
#[derive(Debug)]
pub struct BigArchiveReader<R> {
    reader: R,
    version: BigVersion,
    entries: Vec<BigEntry>,
}

impl<R: Read + Seek> BigArchiveReader<R> {
    /// Opens a BIG archive from a seekable reader.
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let file_len = stream_len(&mut reader, "measuring BIG archive length")?;
        seek_abs(&mut reader, 0, "seeking to BIG archive start")?;

        if file_len < 16 {
            return Err(Error::UnexpectedEof {
                needed: 16,
                available: clamp_len(file_len),
            });
        }

        let header = read_exact_array::<_, 16>(&mut reader, "reading BIG header")?;
        let [m0, m1, m2, m3, size0, size1, size2, size3, count0, count1, count2, count3, data0, data1, data2, data3] =
            header;

        let version = match [m0, m1, m2, m3] {
            [b'B', b'I', b'G', b'F'] => BigVersion::BigF,
            [b'B', b'I', b'G', b'4'] => BigVersion::Big4,
            _ => {
                return Err(Error::InvalidMagic {
                    context: "BIG header",
                });
            }
        };

        let archive_size = u32::from_le_bytes([size0, size1, size2, size3]) as u64;
        if archive_size < 16 || archive_size > file_len {
            return Err(Error::InvalidSize {
                value: clamp_len(archive_size),
                limit: clamp_len(file_len),
                context: "BIG archive size",
            });
        }

        let entry_count = u32::from_be_bytes([count0, count1, count2, count3]) as usize;
        if entry_count > MAX_BIG_ENTRIES {
            return Err(Error::InvalidSize {
                value: entry_count,
                limit: MAX_BIG_ENTRIES,
                context: "BIG entry count",
            });
        }

        let first_data_offset = u32::from_be_bytes([data0, data1, data2, data3]) as u64;
        if first_data_offset < 16 || first_data_offset > archive_size {
            return Err(Error::InvalidOffset {
                offset: clamp_len(first_data_offset),
                bound: clamp_len(archive_size),
            });
        }

        let mut entries = Vec::with_capacity(entry_count);
        let mut pos = 16u64;

        for _ in 0..entry_count {
            let header_end = pos.saturating_add(8);
            if header_end > first_data_offset {
                return Err(Error::InvalidOffset {
                    offset: clamp_len(header_end),
                    bound: clamp_len(first_data_offset),
                });
            }

            let record = read_exact_array::<_, 8>(&mut reader, "reading BIG entry header")?;
            let [off0, off1, off2, off3, size0, size1, size2, size3] = record;
            let offset = u32::from_be_bytes([off0, off1, off2, off3]) as u64;
            let size = u32::from_be_bytes([size0, size1, size2, size3]) as u64;
            pos = header_end;

            let name = read_entry_name(&mut reader, &mut pos, first_data_offset)?;

            let end = offset.saturating_add(size);
            if end > archive_size {
                return Err(Error::InvalidOffset {
                    offset: clamp_len(end),
                    bound: clamp_len(archive_size),
                });
            }

            entries.push(BigEntry { name, offset, size });
        }

        if pos > first_data_offset {
            return Err(Error::InvalidOffset {
                offset: clamp_len(pos),
                bound: clamp_len(first_data_offset),
            });
        }

        Ok(Self {
            reader,
            version,
            entries,
        })
    }

    /// Returns the archive variant.
    #[inline]
    pub fn version(&self) -> BigVersion {
        self.version
    }

    /// Returns all parsed entries.
    #[inline]
    pub fn entries(&self) -> &[BigEntry] {
        &self.entries
    }

    /// Returns the number of files in the archive.
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

        let len = entry_size_to_usize(entry.size, "BIG entry size for read")?;
        seek_abs(&mut self.reader, entry.offset, "seeking to BIG entry data")?;
        let bytes = read_exact_vec(&mut self.reader, len, "reading BIG entry data")?;
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

        seek_abs(&mut self.reader, entry.offset, "seeking to BIG entry data")?;
        copy_exact(
            &mut self.reader,
            writer,
            entry.size,
            "reading BIG entry data",
            "writing BIG entry data",
        )?;
        Ok(true)
    }

    /// Returns entry indices sorted by ascending file offset.
    ///
    /// Extracting entries in this order linearises seeks, which is dramatically
    /// faster on rotational media and network-backed readers.
    pub fn indices_by_offset(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.entries.len()).collect();
        indices.sort_by_key(|&i| self.entries.get(i).map_or(u64::MAX, |e| e.offset));
        indices
    }

    /// Returns the underlying reader.
    #[inline]
    pub fn into_inner(self) -> R {
        self.reader
    }
}

fn read_entry_name<R: Read>(
    reader: &mut R,
    pos: &mut u64,
    first_data_offset: u64,
) -> Result<String, Error> {
    let mut bytes = Vec::new();

    while *pos < first_data_offset {
        let [byte] = read_exact_array::<_, 1>(reader, "reading BIG filename byte")?;
        *pos = pos.saturating_add(1);

        if byte == 0 {
            if bytes.is_empty() {
                return Err(Error::InvalidMagic {
                    context: "BIG filename",
                });
            }
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }

        if bytes.len() >= MAX_FILENAME_LEN {
            return Err(Error::InvalidSize {
                value: bytes.len().saturating_add(1),
                limit: MAX_FILENAME_LEN,
                context: "BIG filename length",
            });
        }

        bytes.push(byte);
    }

    Err(Error::InvalidMagic {
        context: "BIG filename terminator",
    })
}

fn entry_size_to_usize(size: u64, context: &'static str) -> Result<usize, Error> {
    if size > usize::MAX as u64 {
        return Err(Error::InvalidSize {
            value: usize::MAX,
            limit: usize::MAX,
            context,
        });
    }
    Ok(size as usize)
}

#[inline]
fn clamp_len(value: u64) -> usize {
    value.min(usize::MAX as u64) as usize
}
