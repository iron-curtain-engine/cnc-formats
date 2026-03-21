// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{
    parse_finf, parse_vqhd, VqaHeader, FOURCC_FINF, FOURCC_FORM, FOURCC_VQHD, FOURCC_WVQA,
    MAX_CHUNK_SIZE,
};
use crate::error::Error;
use crate::stream_io::{read_exact_array, read_exact_reuse_vec};

use std::io::{Read, Seek, SeekFrom};

/// One VQA chunk borrowed from a streaming source.
///
/// The payload slice is backed by the stream's internal reusable scratch
/// buffer and stays valid only until the next call to [`VqaStream::next_chunk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VqaChunkRef<'data> {
    /// Four-character code identifying the chunk type.
    pub fourcc: [u8; 4],
    /// Raw chunk payload bytes.
    pub data: &'data [u8],
}

impl VqaChunkRef<'_> {
    /// Clones this borrowed chunk into an owned payload buffer.
    pub fn to_owned(self) -> VqaChunkOwned {
        VqaChunkOwned {
            fourcc: self.fourcc,
            data: self.data.to_vec(),
        }
    }
}

/// One owned VQA chunk.
///
/// This is the convenience representation for callers that need the payload to
/// outlive the stream's reusable chunk buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaChunkOwned {
    /// Four-character code identifying the chunk type.
    pub fourcc: [u8; 4],
    /// Raw chunk payload bytes.
    pub data: Vec<u8>,
}

/// Sequential VQA chunk reader backed by any byte stream.
///
/// This reader consumes the VQA IFF container one chunk at a time, retaining
/// only the current chunk payload in memory.
#[derive(Debug)]
pub struct VqaStream<R> {
    reader: R,
    body_len: u64,
    body_read: u64,
    header: Option<VqaHeader>,
    frame_index: Option<Vec<u32>>,
    chunk_data: Vec<u8>,
}

impl<R: Read> VqaStream<R> {
    /// Opens a VQA stream and validates the outer FORM/WVQA envelope.
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let envelope = read_exact_array::<_, 12>(&mut reader, "reading VQA FORM envelope")?;
        let [f0, f1, f2, f3, s0, s1, s2, s3, t0, t1, t2, t3] = envelope;

        if [f0, f1, f2, f3] != FOURCC_FORM {
            return Err(Error::InvalidMagic {
                context: "VQA FORM",
            });
        }

        if [t0, t1, t2, t3] != FOURCC_WVQA {
            return Err(Error::InvalidMagic {
                context: "VQA WVQA type",
            });
        }

        let form_size = u32::from_be_bytes([s0, s1, s2, s3]) as u64;
        let body_len = form_size.saturating_sub(4);

        Ok(Self {
            reader,
            body_len,
            body_read: 0,
            header: None,
            frame_index: None,
            chunk_data: Vec::new(),
        })
    }

    /// Returns the parsed VQHD header once its chunk has been seen.
    #[inline]
    pub fn header(&self) -> Option<&VqaHeader> {
        self.header.as_ref()
    }

    /// Returns the parsed FINF frame index once its chunk has been seen.
    #[inline]
    pub fn frame_index(&self) -> Option<&[u32]> {
        self.frame_index.as_deref()
    }

    /// Reserves reusable payload capacity for subsequent chunk reads.
    #[inline]
    pub(crate) fn reserve_chunk_capacity(&mut self, capacity: usize) {
        self.chunk_data
            .reserve(capacity.saturating_sub(self.chunk_data.capacity()));
    }

    /// Reads the next chunk from the stream into the internal reusable buffer.
    ///
    /// The returned payload borrow stays valid until the next call to this
    /// method. Call [`VqaChunkRef::to_owned`] when the chunk must outlive the
    /// stream's rolling scratch buffer.
    pub fn next_chunk(&mut self) -> Result<Option<VqaChunkRef<'_>>, Error> {
        if self.body_read >= self.body_len {
            return Ok(None);
        }

        let remaining = self.body_len - self.body_read;
        if remaining < 8 {
            return Err(Error::UnexpectedEof {
                needed: 8,
                available: remaining as usize,
            });
        }

        let chunk_header = read_exact_array::<_, 8>(&mut self.reader, "reading VQA chunk header")?;
        let [c0, c1, c2, c3, s0, s1, s2, s3] = chunk_header;
        let fourcc = [c0, c1, c2, c3];
        let chunk_size = u32::from_be_bytes([s0, s1, s2, s3]);

        if chunk_size > MAX_CHUNK_SIZE {
            return Err(Error::InvalidSize {
                value: chunk_size as usize,
                limit: MAX_CHUNK_SIZE as usize,
                context: "VQA chunk size",
            });
        }

        let padded_size = (chunk_size as u64).saturating_add((chunk_size & 1) as u64);
        let chunk_total = 8u64.saturating_add(padded_size);
        let next_body_read = self.body_read.saturating_add(chunk_total);
        if next_body_read > self.body_len {
            return Err(Error::InvalidOffset {
                offset: next_body_read.min(usize::MAX as u64) as usize,
                bound: self.body_len.min(usize::MAX as u64) as usize,
            });
        }

        read_exact_reuse_vec(
            &mut self.reader,
            &mut self.chunk_data,
            chunk_size as usize,
            "reading VQA chunk payload",
        )?;
        if chunk_size & 1 != 0 {
            let _ = read_exact_array::<_, 1>(&mut self.reader, "reading VQA chunk padding")?;
        }
        let payload = self.chunk_data.as_slice();

        if fourcc == FOURCC_VQHD && self.header.is_none() {
            self.header = Some(parse_vqhd(payload)?);
        }

        if fourcc == FOURCC_FINF && self.frame_index.is_none() {
            if let Some(ref header) = self.header {
                self.frame_index = Some(parse_finf(payload, header.num_frames)?);
            }
        }

        self.body_read = next_body_read;
        Ok(Some(VqaChunkRef {
            fourcc,
            data: self.chunk_data.as_slice(),
        }))
    }

    /// Reads the next chunk and clones the payload into an owned buffer.
    pub fn next_chunk_owned(&mut self) -> Result<Option<VqaChunkOwned>, Error> {
        Ok(self.next_chunk()?.map(VqaChunkRef::to_owned))
    }

    /// Returns the underlying reader.
    #[inline]
    pub fn into_inner(self) -> R {
        self.reader
    }
}

/// Maximum bytes to scan when attempting VQA resync.
const RESYNC_SCAN_LIMIT: usize = 256 * 1024;

impl<R: Read + Seek> VqaStream<R> {
    /// Attempts to recover from a corrupted chunk boundary.
    ///
    /// Scans forward up to 256 KB looking for the next valid IFF chunk header:
    /// a 4-byte printable-ASCII FourCC followed by a big-endian u32 size that
    /// does not exceed `MAX_CHUNK_SIZE`.
    ///
    /// If found, positions the reader at the start of that chunk header so the
    /// next [`Self::next_chunk`] call reads it normally. Returns `true` on
    /// success, `false` if the scan limit was reached without finding a valid
    /// chunk.
    pub fn try_resync(&mut self) -> Result<bool, Error> {
        let mut window = [0u8; 8];
        let mut filled = 0usize;
        let mut scanned = 0usize;

        while scanned < RESYNC_SCAN_LIMIT {
            let byte = match read_exact_array::<_, 1>(&mut self.reader, "scanning for VQA resync") {
                Ok([b]) => b,
                Err(_) => return Ok(false),
            };

            if filled < 8 {
                window[filled] = byte;
                filled = filled.saturating_add(1);
            } else {
                window.rotate_left(1);
                window[7] = byte;
            }
            scanned = scanned.saturating_add(1);

            if filled >= 8 && is_plausible_vqa_chunk_header(&window) {
                // Seek back 8 bytes to position at the chunk header start.
                self.reader
                    .seek(SeekFrom::Current(-8))
                    .map_err(|err| crate::stream_io::io_error("seeking during VQA resync", err))?;
                return Ok(true);
            }
        }

        Ok(false)
    }
}

/// Returns `true` if `header` looks like a valid IFF chunk header:
/// 4 printable ASCII bytes (FourCC) + big-endian u32 size <= MAX_CHUNK_SIZE.
fn is_plausible_vqa_chunk_header(header: &[u8; 8]) -> bool {
    let fourcc = &header[..4];
    let size_bytes = [header[4], header[5], header[6], header[7]];
    let size = u32::from_be_bytes(size_bytes);

    fourcc.iter().all(|&b| b.is_ascii_graphic()) && size > 0 && size <= MAX_CHUNK_SIZE
}
