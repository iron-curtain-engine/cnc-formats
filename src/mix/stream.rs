// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{crc, entry_reader::MixEntryReader, lmd, MixCrc, MixEntry, MAX_MIX_ENTRIES};
use crate::error::Error;
#[cfg(feature = "encrypted-mix")]
use crate::read::{read_u16_le, read_u32_le};
use crate::stream_io::{copy_exact, read_exact_array, read_exact_vec, seek_abs, stream_len};

use std::collections::HashMap;
use std::io::{Read, Seek, Write};

/// Streaming MIX archive reader backed by a seekable byte source.
///
/// Unlike [`super::MixArchive`], this type does not require the whole archive
/// to be loaded into memory.  It keeps only the parsed entry table and reads
/// file payloads from the underlying stream on demand.
#[derive(Debug)]
pub struct MixArchiveReader<R> {
    reader: R,
    entries: Vec<MixEntry>,
    data_offset: u64,
}

impl<R: Read + Seek> MixArchiveReader<R> {
    /// Opens a MIX archive from a seekable reader.
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let archive_len = stream_len(&mut reader, "measuring MIX archive length")?;
        seek_abs(&mut reader, 0, "seeking to MIX archive start")?;

        if archive_len < 2 {
            return Err(Error::UnexpectedEof {
                needed: 2,
                available: clamp_len(archive_len),
            });
        }

        let first_word = read_exact_array::<_, 2>(&mut reader, "reading MIX marker")?;
        let first_word = u16::from_le_bytes(first_word);

        if first_word == 0 {
            if archive_len < 4 {
                return Err(Error::UnexpectedEof {
                    needed: 4,
                    available: clamp_len(archive_len),
                });
            }

            let flags = read_exact_array::<_, 2>(&mut reader, "reading MIX flags")?;
            let flags = u16::from_le_bytes(flags);

            if flags & 0x0002 != 0 {
                #[cfg(feature = "encrypted-mix")]
                {
                    return Self::open_encrypted(reader, archive_len);
                }
                #[cfg(not(feature = "encrypted-mix"))]
                {
                    return Err(Error::EncryptedArchive);
                }
            }

            return Self::open_plain(reader, archive_len, 4);
        }

        Self::open_plain(reader, archive_len, 0)
    }

    /// Returns the parsed entry table.
    #[inline]
    pub fn entries(&self) -> &[MixEntry] {
        &self.entries
    }

    /// Returns the number of files in the archive.
    #[inline]
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Reads an entry by filename into a new byte vector.
    #[inline]
    pub fn read(&mut self, filename: &str) -> Result<Option<Vec<u8>>, Error> {
        self.read_by_crc(crc(filename))
    }

    /// Reads an entry by CRC into a new byte vector.
    pub fn read_by_crc(&mut self, key: MixCrc) -> Result<Option<Vec<u8>>, Error> {
        let index = self
            .entries
            .binary_search_by_key(&key, |entry| entry.crc)
            .ok();
        match index {
            Some(index) => self.read_by_index(index),
            None => Ok(None),
        }
    }

    /// Reads the entry at `index` into a new byte vector.
    pub fn read_by_index(&mut self, index: usize) -> Result<Option<Vec<u8>>, Error> {
        let entry = match self.entries.get(index) {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let len = entry_size_to_usize(u64::from(entry.size), "MIX entry size for read")?;
        seek_abs(
            &mut self.reader,
            entry_start_offset(self.data_offset, entry),
            "seeking to MIX entry data",
        )?;
        let bytes = read_exact_vec(&mut self.reader, len, "reading MIX entry data")?;
        Ok(Some(bytes))
    }

    /// Opens the entry for `filename` as a bounded streaming reader.
    ///
    /// This is the no-allocation entry path for downstream decoders that can
    /// consume bytes incrementally. The returned reader borrows the archive
    /// cursor mutably and remains valid until dropped.
    #[inline]
    pub fn open_entry(&mut self, filename: &str) -> Result<Option<MixEntryReader<'_, R>>, Error> {
        self.open_entry_by_crc(crc(filename))
    }

    /// Opens the entry for a known CRC as a bounded streaming reader.
    pub fn open_entry_by_crc(
        &mut self,
        key: MixCrc,
    ) -> Result<Option<MixEntryReader<'_, R>>, Error> {
        let index = self
            .entries
            .binary_search_by_key(&key, |entry| entry.crc)
            .ok();
        match index {
            Some(index) => self.open_entry_by_index(index),
            None => Ok(None),
        }
    }

    /// Opens the entry at `index` as a bounded streaming reader.
    pub fn open_entry_by_index(
        &mut self,
        index: usize,
    ) -> Result<Option<MixEntryReader<'_, R>>, Error> {
        let entry = match self.entries.get(index) {
            Some(entry) => entry,
            None => return Ok(None),
        };

        Ok(Some(MixEntryReader::new(
            &mut self.reader,
            entry_start_offset(self.data_offset, entry),
            u64::from(entry.size),
        )?))
    }

    /// Streams an entry by filename into a writer.
    #[inline]
    pub fn copy<W: Write>(&mut self, filename: &str, writer: &mut W) -> Result<bool, Error> {
        self.copy_by_crc(crc(filename), writer)
    }

    /// Streams an entry by CRC into a writer.
    pub fn copy_by_crc<W: Write>(&mut self, key: MixCrc, writer: &mut W) -> Result<bool, Error> {
        let index = self
            .entries
            .binary_search_by_key(&key, |entry| entry.crc)
            .ok();
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
            entry_start_offset(self.data_offset, entry),
            "seeking to MIX entry data",
        )?;
        copy_exact(
            &mut self.reader,
            writer,
            u64::from(entry.size),
            "reading MIX entry data",
            "writing MIX entry data",
        )?;
        Ok(true)
    }

    /// Reads embedded filename mappings from the archive.
    pub fn embedded_names(&mut self) -> Result<HashMap<MixCrc, String>, Error> {
        if let Some(lmd_data) = self.read_by_crc(lmd::LMD_CRC)? {
            let names = lmd::parse_lmd(&lmd_data);
            if !names.is_empty() {
                return Ok(names);
            }
        }
        Ok(HashMap::new())
    }

    /// Returns entry indices sorted by ascending file offset.
    ///
    /// Extracting entries in this order linearises seeks, which is dramatically
    /// faster on rotational media and network-backed readers.
    pub fn indices_by_offset(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.entries.len()).collect();
        indices.sort_by_key(|&i| self.entries.get(i).map_or(u32::MAX, |entry| entry.offset));
        indices
    }

    /// Returns the underlying reader.
    #[inline]
    pub fn into_inner(self) -> R {
        self.reader
    }

    fn open_plain(mut reader: R, archive_len: u64, header_offset: u64) -> Result<Self, Error> {
        seek_abs(&mut reader, header_offset, "seeking to MIX header")?;
        let header = read_exact_array::<_, 6>(&mut reader, "reading MIX header")?;
        let [count0, count1, _, _, _, _] = header;
        let count = u16::from_le_bytes([count0, count1]) as usize;

        if count > MAX_MIX_ENTRIES {
            return Err(Error::InvalidSize {
                value: count,
                limit: MAX_MIX_ENTRIES,
                context: "MIX entry count",
            });
        }

        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let record = read_exact_array::<_, 12>(&mut reader, "reading MIX entry record")?;
            let [crc0, crc1, crc2, crc3, off0, off1, off2, off3, size0, size1, size2, size3] =
                record;
            entries.push(MixEntry {
                crc: MixCrc::from_raw(u32::from_le_bytes([crc0, crc1, crc2, crc3])),
                offset: u32::from_le_bytes([off0, off1, off2, off3]),
                size: u32::from_le_bytes([size0, size1, size2, size3]),
            });
        }

        entries.sort_by_key(|entry| entry.crc);

        let data_offset = header_offset
            .saturating_add(6)
            .saturating_add((count as u64).saturating_mul(12));
        if data_offset > archive_len {
            return Err(Error::UnexpectedEof {
                needed: clamp_len(data_offset),
                available: clamp_len(archive_len),
            });
        }

        let data_len = archive_len - data_offset;
        validate_entries(&entries, data_len)?;

        Ok(Self {
            reader,
            entries,
            data_offset,
        })
    }

    #[cfg(feature = "encrypted-mix")]
    fn open_encrypted(mut reader: R, archive_len: u64) -> Result<Self, Error> {
        use crate::mix_crypt::{self, KEY_SOURCE_LEN};

        const ENCRYPTED_HEADER_LIMIT: usize =
            (6usize + MAX_MIX_ENTRIES.saturating_mul(12)).div_ceil(8) * 8;

        seek_abs(&mut reader, 4, "seeking to MIX key source")?;
        let key_source =
            read_exact_array::<_, KEY_SOURCE_LEN>(&mut reader, "reading MIX encrypted key source")?;
        let blowfish_key = mix_crypt::derive_blowfish_key(&key_source)?;

        let encrypted_start = 4u64.saturating_add(KEY_SOURCE_LEN as u64);
        let remaining = archive_len.saturating_sub(encrypted_start);
        let header_bytes = read_exact_vec(
            &mut reader,
            remaining.min(ENCRYPTED_HEADER_LIMIT as u64) as usize,
            "reading MIX encrypted header",
        )?;
        let decrypted = mix_crypt::decrypt_mix_header(&header_bytes, &blowfish_key)?;

        let count = read_mix_entry_count(&decrypted, "encrypted MIX entry count")?;
        let mut entries = Vec::with_capacity(count);
        let mut pos = 6usize;

        for _ in 0..count {
            let record_end = pos.saturating_add(12);
            let record = decrypted.get(pos..record_end).ok_or(Error::UnexpectedEof {
                needed: record_end,
                available: decrypted.len(),
            })?;
            entries.push(MixEntry {
                crc: MixCrc::from_raw(read_u32_le(record, 0)?),
                offset: read_u32_le(record, 4)?,
                size: read_u32_le(record, 8)?,
            });
            pos = record_end;
        }

        entries.sort_by_key(|entry| entry.crc);

        let header_size = 6usize.saturating_add(count.saturating_mul(12));
        let encrypted_len = header_size.div_ceil(8) * 8;
        let data_offset = encrypted_start.saturating_add(encrypted_len as u64);
        if data_offset > archive_len {
            return Err(Error::UnexpectedEof {
                needed: clamp_len(data_offset),
                available: clamp_len(archive_len),
            });
        }

        let data_len = archive_len - data_offset;
        validate_entries(&entries, data_len)?;

        Ok(Self {
            reader,
            entries,
            data_offset,
        })
    }
}

#[cfg(feature = "encrypted-mix")]
fn read_mix_entry_count(data: &[u8], context: &'static str) -> Result<usize, Error> {
    let count = read_u16_le(data, 0)? as usize;
    if count > MAX_MIX_ENTRIES {
        return Err(Error::InvalidSize {
            value: count,
            limit: MAX_MIX_ENTRIES,
            context,
        });
    }
    Ok(count)
}

fn validate_entries(entries: &[MixEntry], data_len: u64) -> Result<(), Error> {
    for entry in entries {
        let end = u64::from(entry.offset).saturating_add(u64::from(entry.size));
        if end > data_len {
            return Err(Error::InvalidOffset {
                offset: end.min(usize::MAX as u64) as usize,
                bound: data_len.min(usize::MAX as u64) as usize,
            });
        }
    }
    Ok(())
}

#[inline]
fn entry_start_offset(data_offset: u64, entry: &MixEntry) -> u64 {
    data_offset.saturating_add(u64::from(entry.offset))
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
