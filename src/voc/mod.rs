// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Creative Voice File (`.voc`) parser.
//!
//! VOC is a container format for digital audio, used by Creative Labs
//! Sound Blaster cards and Dune II.  Files consist of a 26-byte header
//! followed by a sequence of typed data blocks (sound data, silence,
//! markers, repeat loops, etc.).
//!
//! ## File Layout
//!
//! ```text
//! [Header]        26 bytes  (magic, data_offset, version)
//! [Block 0]       1 + 3 + N bytes (type, size, payload)
//! [Block 1]       ...
//! [Terminator]    1 byte (type 0)
//! ```
//!
//! ## References
//!
//! Format source: Creative Voice File specification (Creative Labs), XCC Utilities documentation.

use crate::error::Error;
use crate::read::{read_u16_le, read_u8};

// ─── Constants ────────────────────────────────────────────────────────────────

/// The 20-byte magic signature at the start of every VOC file.
const MAGIC: &[u8; 20] = b"Creative Voice File\x1a";

/// Fixed size of the VOC file header in bytes.
const HEADER_SIZE: usize = 26;

/// Safety cap: maximum number of data blocks to prevent runaway parsing
/// on malformed input.
const MAX_BLOCKS: usize = 65_536;

/// Block type: terminator (no further blocks).
pub const BLOCK_TERMINATOR: u8 = 0;
/// Block type: sound data (freq_divisor + codec + samples).
pub const BLOCK_SOUND_DATA: u8 = 1;
/// Block type: continuation of previous sound data.
pub const BLOCK_SOUND_CONTINUE: u8 = 2;
/// Block type: silence interval.
pub const BLOCK_SILENCE: u8 = 3;
/// Block type: marker (arbitrary u16 identifier).
pub const BLOCK_MARKER: u8 = 4;
/// Block type: null-terminated ASCII text.
pub const BLOCK_TEXT: u8 = 5;
/// Block type: repeat loop start.
pub const BLOCK_REPEAT_START: u8 = 6;
/// Block type: repeat loop end.
pub const BLOCK_REPEAT_END: u8 = 7;
/// Block type: extended format descriptor.
pub const BLOCK_EXTENDED: u8 = 8;
/// Block type: new-format sound data (sample rate + bits + channels).
pub const BLOCK_NEW_SOUND_DATA: u8 = 9;

// ─── Header ───────────────────────────────────────────────────────────────────

/// The 26-byte header at the start of a VOC file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocHeader {
    /// Byte offset from the start of the file to the first data block
    /// (typically 26).
    pub data_offset: u16,
    /// Raw version word (e.g. `0x010A` for version 1.10).
    pub version: u16,
    /// Version check word (should equal `!version + 0x1234`).
    pub version_check: u16,
}

impl VocHeader {
    /// Returns the version as `(major, minor)`.
    ///
    /// For version word `0x010A`, this returns `(1, 10)`.
    #[inline]
    pub fn version_tuple(&self) -> (u8, u8) {
        let major = (self.version >> 8) as u8;
        let minor = (self.version & 0xFF) as u8;
        (major, minor)
    }
}

// ─── Block ────────────────────────────────────────────────────────────────────

/// A single data block within a VOC file.
///
/// The block stores its type, plus the byte offset and size of its payload
/// within the original input buffer.  Use [`VocFile::block_data`] to obtain
/// the payload slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocBlock {
    /// Block type identifier (1 = sound data, 2 = sound continue, etc.).
    pub block_type: u8,
    /// Byte offset of the payload within the original input buffer.
    pub offset: usize,
    /// Size of the payload in bytes.
    pub size: usize,
}

// ─── VocFile ──────────────────────────────────────────────────────────────────

/// A parsed Creative Voice File: header plus a list of data blocks.
#[derive(Debug)]
pub struct VocFile<'input> {
    header: VocHeader,
    blocks: Vec<VocBlock>,
    data: &'input [u8],
}

impl<'input> VocFile<'input> {
    /// Parses a VOC file from a byte slice.
    ///
    /// The parser reads the 26-byte header, validates the magic signature and
    /// version check word, then iterates through the data blocks until a
    /// terminator (type 0) is reached or input is exhausted.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] -- `data` is shorter than the header or a
    ///   block's declared size exceeds the remaining input.
    /// - [`Error::InvalidMagic`] -- the 20-byte magic signature is wrong or
    ///   the version check word does not match.
    /// - [`Error::InvalidSize`] -- more than `MAX_BLOCKS` blocks encountered.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        if data.len() < HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            });
        }

        // Validate magic signature.
        let magic = data.get(..20).ok_or(Error::UnexpectedEof {
            needed: 20,
            available: data.len(),
        })?;
        if magic != MAGIC.as_slice() {
            return Err(Error::InvalidMagic {
                context: "VOC header",
            });
        }

        let data_offset = read_u16_le(data, 20)?;
        let version = read_u16_le(data, 22)?;
        let version_check = read_u16_le(data, 24)?;

        // Validate version check: should equal !version + 0x1234.
        let expected_check = (!version).wrapping_add(0x1234);
        if version_check != expected_check {
            return Err(Error::InvalidMagic {
                context: "VOC version check",
            });
        }

        // data_offset must be at least HEADER_SIZE and within bounds.
        let block_start = data_offset as usize;
        if block_start < HEADER_SIZE {
            return Err(Error::InvalidOffset {
                offset: block_start,
                bound: HEADER_SIZE,
            });
        }
        if block_start > data.len() {
            return Err(Error::InvalidOffset {
                offset: block_start,
                bound: data.len(),
            });
        }

        let header = VocHeader {
            data_offset,
            version,
            version_check,
        };

        // Parse data blocks.
        let mut blocks = Vec::new();
        let mut pos = block_start;

        loop {
            if blocks.len() >= MAX_BLOCKS {
                return Err(Error::InvalidSize {
                    value: blocks.len(),
                    limit: MAX_BLOCKS,
                    context: "VOC block count",
                });
            }

            // Need at least 1 byte for the block type.
            if pos >= data.len() {
                break;
            }

            let block_type = read_u8(data, pos)?;
            pos = pos.saturating_add(1);

            if block_type == BLOCK_TERMINATOR {
                break;
            }

            // Non-terminator blocks have a 3-byte (u24) size field.
            let size_end = pos.checked_add(3).ok_or(Error::UnexpectedEof {
                needed: usize::MAX,
                available: data.len(),
            })?;
            if size_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: size_end,
                    available: data.len(),
                });
            }

            let b0 = read_u8(data, pos)? as usize;
            let b1 = read_u8(data, pos.saturating_add(1))? as usize;
            let b2 = read_u8(data, pos.saturating_add(2))? as usize;
            let block_size = b0 | (b1 << 8) | (b2 << 16);
            pos = size_end;

            // Validate that the payload fits within the remaining data.
            let payload_end = pos.checked_add(block_size).ok_or(Error::InvalidOffset {
                offset: usize::MAX,
                bound: data.len(),
            })?;
            if payload_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: payload_end,
                    available: data.len(),
                });
            }

            blocks.push(VocBlock {
                block_type,
                offset: pos,
                size: block_size,
            });

            pos = payload_end;
        }

        Ok(VocFile {
            header,
            blocks,
            data,
        })
    }

    /// Returns the parsed header.
    #[inline]
    pub fn header(&self) -> &VocHeader {
        &self.header
    }

    /// Returns all parsed data blocks (excluding the terminator).
    #[inline]
    pub fn blocks(&self) -> &[VocBlock] {
        &self.blocks
    }

    /// Returns the raw payload bytes for a given block.
    ///
    /// Returns `None` if the block's offset/size falls outside the
    /// original input buffer (should not happen for blocks returned by
    /// [`parse`](Self::parse)).
    #[inline]
    pub fn block_data(&self, block: &VocBlock) -> Option<&'input [u8]> {
        let end = block.offset.checked_add(block.size)?;
        self.data.get(block.offset..end)
    }

    /// Returns the file version as `(major, minor)`.
    ///
    /// Convenience shorthand for `self.header().version_tuple()`.
    #[inline]
    pub fn version(&self) -> (u8, u8) {
        self.header.version_tuple()
    }

    /// Computes the sample rate implied by a Sound Data (type 1) block.
    ///
    /// Returns `None` if the block is not type 1, the payload is too short,
    /// or the frequency divisor is 256 (which would cause division by zero).
    pub fn sound_data_sample_rate(&self, block: &VocBlock) -> Option<u32> {
        if block.block_type != BLOCK_SOUND_DATA || block.size < 2 {
            return None;
        }
        let payload = self.block_data(block)?;
        let freq_divisor = *payload.first()? as u32;
        let divisor = 256u32.checked_sub(freq_divisor)?;
        if divisor == 0 {
            return None;
        }
        Some(1_000_000 / divisor)
    }
}

#[cfg(test)]
mod tests;
