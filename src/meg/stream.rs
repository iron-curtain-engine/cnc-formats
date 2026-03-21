// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{
    validate_data_start, validate_entry_count, MegEntry, FILE_RECORD_SIZE, LEGACY_HEADER_SIZE,
    MAX_FILENAME_LEN, PETRO_FLAG_ENCRYPTED, PETRO_FLAG_UNENCRYPTED, PETRO_FORMAT_ID,
    PETRO_HEADER_SIZE, REMASTERED_HEADER_SIZE,
};
use crate::error::Error;
use crate::stream_io::{copy_exact, read_exact_array, read_exact_vec, seek_abs, stream_len};

use std::io::{Read, Seek, Write};

/// Streaming MEG archive reader backed by a seekable byte source.
#[derive(Debug)]
pub struct MegArchiveReader<R> {
    reader: R,
    entries: Vec<MegEntry>,
}

impl<R: Read + Seek> MegArchiveReader<R> {
    /// Opens a MEG archive from a seekable reader.
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let archive_len = stream_len(&mut reader, "measuring MEG archive length")?;
        seek_abs(&mut reader, 0, "seeking to MEG archive start")?;

        if archive_len < LEGACY_HEADER_SIZE as u64 {
            return Err(Error::UnexpectedEof {
                needed: LEGACY_HEADER_SIZE,
                available: clamp_len(archive_len),
            });
        }

        let marker = read_exact_array::<_, 4>(&mut reader, "reading MEG marker")?;
        let marker = u32::from_le_bytes(marker);

        if marker == PETRO_FLAG_ENCRYPTED {
            return Err(Error::InvalidMagic {
                context: "MEG encrypted archive",
            });
        }

        if marker == PETRO_FLAG_UNENCRYPTED {
            return Self::open_petroglyph(reader, archive_len);
        }

        Self::open_legacy(reader, archive_len)
    }

    /// Returns the parsed entry table.
    #[inline]
    pub fn entries(&self) -> &[MegEntry] {
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

        let len = entry_size_to_usize(entry.size, "MEG entry size for read")?;
        seek_abs(&mut self.reader, entry.offset, "seeking to MEG entry data")?;
        let bytes = read_exact_vec(&mut self.reader, len, "reading MEG entry data")?;
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

        seek_abs(&mut self.reader, entry.offset, "seeking to MEG entry data")?;
        copy_exact(
            &mut self.reader,
            writer,
            entry.size,
            "reading MEG entry data",
            "writing MEG entry data",
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

    fn open_legacy(mut reader: R, archive_len: u64) -> Result<Self, Error> {
        seek_abs(&mut reader, 0, "seeking to MEG legacy header")?;
        let header = read_exact_array::<_, LEGACY_HEADER_SIZE>(&mut reader, "reading MEG header")?;
        let [n0, n1, n2, n3, f0, f1, f2, f3] = header;
        let num_filenames = u32::from_le_bytes([n0, n1, n2, n3]) as usize;
        let num_files = u32::from_le_bytes([f0, f1, f2, f3]) as usize;

        validate_entry_count(num_filenames, "MEG filename count")?;
        validate_entry_count(num_files, "MEG entry count")?;

        let mut pos = LEGACY_HEADER_SIZE as u64;
        let filenames = read_filename_table(&mut reader, &mut pos, num_filenames, None)?;
        let records_bytes = (num_files as u64).saturating_mul(FILE_RECORD_SIZE as u64);
        let data_start = pos.saturating_add(records_bytes);
        if data_start > archive_len {
            return Err(Error::UnexpectedEof {
                needed: clamp_len(data_start),
                available: clamp_len(archive_len),
            });
        }

        let entries = read_legacy_records(
            &mut reader,
            &mut pos,
            &filenames,
            num_files,
            data_start,
            archive_len,
        )?;
        Ok(Self { reader, entries })
    }

    fn open_petroglyph(mut reader: R, archive_len: u64) -> Result<Self, Error> {
        if archive_len < PETRO_HEADER_SIZE as u64 {
            return Err(Error::UnexpectedEof {
                needed: PETRO_HEADER_SIZE,
                available: clamp_len(archive_len),
            });
        }

        seek_abs(&mut reader, 0, "seeking to MEG Petroglyph header")?;
        let header = read_exact_array::<_, PETRO_HEADER_SIZE>(&mut reader, "reading MEG header")?;
        let [_marker0, _marker1, _marker2, _marker3, id0, id1, id2, id3, data0, data1, data2, data3, file0, file1, file2, file3, name0, name1, name2, name3] =
            header;

        let format_id = u32::from_le_bytes([id0, id1, id2, id3]);
        if format_id != PETRO_FORMAT_ID {
            return Err(Error::InvalidMagic {
                context: "MEG header",
            });
        }

        let data_start = u32::from_le_bytes([data0, data1, data2, data3]) as u64;
        let num_files = u32::from_le_bytes([file0, file1, file2, file3]) as usize;
        let num_filenames = u32::from_le_bytes([name0, name1, name2, name3]) as usize;

        validate_entry_count(num_filenames, "MEG filename count")?;
        validate_entry_count(num_files, "MEG entry count")?;
        validate_data_start(
            clamp_len(data_start),
            PETRO_HEADER_SIZE,
            clamp_len(archive_len),
        )?;

        if archive_len >= REMASTERED_HEADER_SIZE as u64 {
            let extra = read_exact_array::<_, 4>(&mut reader, "reading MEG filename table size")?;
            let filenames_size = u32::from_le_bytes(extra) as u64;
            let names_end = (REMASTERED_HEADER_SIZE as u64).saturating_add(filenames_size);
            let expected_records = (num_files as u64).saturating_mul(FILE_RECORD_SIZE as u64);
            if names_end <= data_start && data_start.saturating_sub(names_end) == expected_records {
                return Self::open_remastered(
                    reader,
                    archive_len,
                    data_start,
                    num_filenames,
                    num_files,
                    filenames_size,
                );
            }
        }

        Self::open_petroglyph_v2(reader, archive_len, data_start, num_filenames, num_files)
    }

    fn open_petroglyph_v2(
        mut reader: R,
        archive_len: u64,
        data_start: u64,
        num_filenames: usize,
        num_files: usize,
    ) -> Result<Self, Error> {
        let records_bytes = (num_files as u64).saturating_mul(FILE_RECORD_SIZE as u64);
        if records_bytes > data_start {
            return Err(Error::InvalidOffset {
                offset: clamp_len(records_bytes),
                bound: clamp_len(data_start),
            });
        }
        let records_start = data_start - records_bytes;
        if records_start < PETRO_HEADER_SIZE as u64 {
            return Err(Error::InvalidOffset {
                offset: clamp_len(records_start),
                bound: clamp_len(data_start),
            });
        }

        seek_abs(
            &mut reader,
            PETRO_HEADER_SIZE as u64,
            "seeking to MEG filename table",
        )?;
        let mut pos = PETRO_HEADER_SIZE as u64;
        let filenames =
            read_filename_table(&mut reader, &mut pos, num_filenames, Some(records_start))?;
        if pos != records_start {
            return Err(Error::InvalidMagic {
                context: "MEG filename table size",
            });
        }

        let entries = read_legacy_records(
            &mut reader,
            &mut pos,
            &filenames,
            num_files,
            data_start,
            archive_len,
        )?;
        Ok(Self { reader, entries })
    }

    fn open_remastered(
        mut reader: R,
        archive_len: u64,
        data_start: u64,
        num_filenames: usize,
        num_files: usize,
        filenames_size: u64,
    ) -> Result<Self, Error> {
        let names_end = (REMASTERED_HEADER_SIZE as u64).saturating_add(filenames_size);
        if names_end > data_start {
            return Err(Error::InvalidOffset {
                offset: clamp_len(names_end),
                bound: clamp_len(data_start),
            });
        }

        seek_abs(
            &mut reader,
            REMASTERED_HEADER_SIZE as u64,
            "seeking to MEG remastered filename table",
        )?;
        let mut pos = REMASTERED_HEADER_SIZE as u64;
        let filenames = read_filename_table(&mut reader, &mut pos, num_filenames, Some(names_end))?;
        if pos != names_end {
            return Err(Error::InvalidMagic {
                context: "MEG filename table size",
            });
        }

        let records_bytes = data_start - names_end;
        let expected_bytes = (num_files as u64).saturating_mul(FILE_RECORD_SIZE as u64);
        if records_bytes != expected_bytes {
            return Err(Error::InvalidMagic {
                context: "MEG record table size",
            });
        }

        let entries = read_remastered_records(
            &mut reader,
            &mut pos,
            &filenames,
            num_files,
            data_start,
            archive_len,
        )?;
        Ok(Self { reader, entries })
    }
}

fn read_filename_table<R: Read>(
    reader: &mut R,
    pos: &mut u64,
    count: usize,
    exact_end: Option<u64>,
) -> Result<Vec<String>, Error> {
    let mut filenames = Vec::with_capacity(count);

    for _ in 0..count {
        let len_end = pos.saturating_add(2);
        if let Some(bound) = exact_end {
            if len_end > bound {
                return Err(Error::UnexpectedEof {
                    needed: clamp_len(len_end),
                    available: clamp_len(bound),
                });
            }
        }

        let raw_len = read_exact_array::<_, 2>(reader, "reading MEG filename length")?;
        let name_len = u16::from_le_bytes(raw_len) as usize;
        *pos = len_end;

        if name_len > MAX_FILENAME_LEN {
            return Err(Error::InvalidSize {
                value: name_len,
                limit: MAX_FILENAME_LEN,
                context: "MEG filename length",
            });
        }

        let name_end = pos.saturating_add(name_len as u64);
        if let Some(bound) = exact_end {
            if name_end > bound {
                return Err(Error::UnexpectedEof {
                    needed: clamp_len(name_end),
                    available: clamp_len(bound),
                });
            }
        }

        let name = read_exact_vec(reader, name_len, "reading MEG filename bytes")?;
        *pos = name_end;
        filenames.push(String::from_utf8_lossy(&name).into_owned());
    }

    Ok(filenames)
}

fn read_legacy_records<R: Read>(
    reader: &mut R,
    pos: &mut u64,
    filenames: &[String],
    count: usize,
    data_start: u64,
    archive_len: u64,
) -> Result<Vec<MegEntry>, Error> {
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        let record = read_exact_array::<_, FILE_RECORD_SIZE>(reader, "reading MEG file record")?;
        let [_crc0, _crc1, _crc2, _crc3, _index0, _index1, _index2, _index3, size0, size1, size2, size3, start0, start1, start2, start3, name0, name1, name2, name3] =
            record;
        *pos = pos.saturating_add(FILE_RECORD_SIZE as u64);

        entries.push(build_entry_stream(
            filenames,
            u32::from_le_bytes([name0, name1, name2, name3]) as usize,
            u32::from_le_bytes([start0, start1, start2, start3]) as u64,
            u32::from_le_bytes([size0, size1, size2, size3]) as u64,
            data_start,
            archive_len,
        )?);
    }

    Ok(entries)
}

fn read_remastered_records<R: Read>(
    reader: &mut R,
    pos: &mut u64,
    filenames: &[String],
    count: usize,
    data_start: u64,
    archive_len: u64,
) -> Result<Vec<MegEntry>, Error> {
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        let record =
            read_exact_array::<_, FILE_RECORD_SIZE>(reader, "reading MEG remastered record")?;
        let [flags0, flags1, _crc0, _crc1, _crc2, _crc3, _index0, _index1, _index2, _index3, size0, size1, size2, size3, start0, start1, start2, start3, name0, name1] =
            record;
        *pos = pos.saturating_add(FILE_RECORD_SIZE as u64);

        if u16::from_le_bytes([flags0, flags1]) != 0 {
            return Err(Error::InvalidMagic {
                context: "MEG encrypted file record",
            });
        }

        entries.push(build_entry_stream(
            filenames,
            u16::from_le_bytes([name0, name1]) as usize,
            u32::from_le_bytes([start0, start1, start2, start3]) as u64,
            u32::from_le_bytes([size0, size1, size2, size3]) as u64,
            data_start,
            archive_len,
        )?);
    }

    Ok(entries)
}

fn build_entry_stream(
    filenames: &[String],
    name_index: usize,
    start: u64,
    size: u64,
    data_start: u64,
    archive_len: u64,
) -> Result<MegEntry, Error> {
    let name = filenames.get(name_index).ok_or(Error::InvalidOffset {
        offset: name_index,
        bound: filenames.len(),
    })?;

    if start < data_start {
        return Err(Error::InvalidOffset {
            offset: clamp_len(start),
            bound: clamp_len(archive_len),
        });
    }

    let end = start.saturating_add(size);
    if end > archive_len {
        return Err(Error::InvalidOffset {
            offset: clamp_len(end),
            bound: clamp_len(archive_len),
        });
    }

    Ok(MegEntry {
        name: name.clone(),
        offset: start,
        size,
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
