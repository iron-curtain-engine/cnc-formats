// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Generals / Zero Hour binary map parser (`.map`).
//!
//! SAGE engine maps use a binary chunk format.  Each chunk has a name,
//! version, and raw data payload.  Chunk payloads are opaque at this
//! level — game-specific interpretation is the engine's responsibility.
//!
//! ## File Layout
//!
//! ```text
//! [EAR outer header]   18 bytes (present in real game files)
//!   EAR\0              4 bytes outer magic
//!   hash (u32 LE)      4 bytes
//!   hash (u32 LE)      4 bytes
//!   flags (u16 LE)     2 bytes
//!   CkMp               4 bytes inner magic
//! [Chunk 0]        name_len(4) + name(N) + version(4) + data_len(4) + data(M)
//! [Chunk 1]        ...
//! ```
//!
//! Synthetic test data may omit the outer EAR header and start directly
//! with `CkMp`.

use crate::error::Error;
use crate::read::read_u32_le;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Inner magic: `CkMp` ("Chunk Map").
const MAGIC: &[u8; 4] = b"CkMp";

/// Outer magic used by real Generals / Zero Hour map files.
const OUTER_MAGIC: &[u8; 4] = b"EAR\0";

/// Size of the outer EAR header including the trailing `CkMp` inner magic.
///
/// Layout: `EAR\0`(4) + hash(4) + hash(4) + flags(2) + `CkMp`(4) = 18 bytes.
const OUTER_HEADER_SIZE: usize = 18;

/// Byte offset of the `CkMp` inner magic within the outer EAR header.
const INNER_MAGIC_OFFSET: usize = 14;

/// Minimum valid file size: just the 4-byte `CkMp` magic (no chunks).
const MIN_FILE_SIZE: usize = 4;

/// Maximum number of chunks allowed in a single map file.
const MAX_CHUNKS: usize = 4096;

/// Maximum length of a chunk name in bytes.
const MAX_CHUNK_NAME: usize = 256;

/// Maximum size of a single chunk payload (64 MB).
const MAX_CHUNK_SIZE: usize = 64 * 1024 * 1024;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single named chunk from a SAGE binary map file.
#[derive(Debug, Clone)]
pub struct MapSageChunk<'input> {
    /// ASCII chunk name (e.g. `"HeightMapData"`, `"WorldInfo"`).
    pub name: String,
    /// Chunk format version.
    pub version: u32,
    /// Raw chunk payload bytes (opaque at this level).
    pub data: &'input [u8],
}

/// A parsed SAGE binary map file containing zero or more named chunks.
#[derive(Debug)]
pub struct MapSageFile<'input> {
    chunks: Vec<MapSageChunk<'input>>,
}

impl<'input> MapSageFile<'input> {
    /// Parses a SAGE binary map from a byte slice.
    ///
    /// Validates the `CkMp` magic, then iterates over chunks until the end
    /// of input.  Returns an error if the magic is wrong, any chunk header is
    /// truncated, or a chunk's data extends past the end of the file.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // 1. Determine whether the file starts with the EAR\0 outer header
        //    (real game files) or bare CkMp (synthetic test data).
        let chunk_start: usize = if data.get(..4) == Some(OUTER_MAGIC.as_slice()) {
            // Real Generals / Zero Hour map: outer EAR header present.
            if data.len() < OUTER_HEADER_SIZE {
                return Err(Error::UnexpectedEof {
                    needed: OUTER_HEADER_SIZE,
                    available: data.len(),
                });
            }
            // Verify CkMp inner magic at offset 14.
            if data.get(INNER_MAGIC_OFFSET..INNER_MAGIC_OFFSET + 4) != Some(MAGIC.as_slice()) {
                return Err(Error::InvalidMagic {
                    context: "SAGE map inner magic (expected 'CkMp' at offset 14)",
                });
            }
            OUTER_HEADER_SIZE
        } else if data.get(..4) == Some(MAGIC.as_slice()) {
            // Bare CkMp (no outer wrapper).
            if data.len() < MIN_FILE_SIZE {
                return Err(Error::UnexpectedEof {
                    needed: MIN_FILE_SIZE,
                    available: data.len(),
                });
            }
            4
        } else {
            return Err(Error::InvalidMagic {
                context: "SAGE map magic (expected 'CkMp' or 'EAR\\0')",
            });
        };

        let mut offset: usize = chunk_start;
        let mut chunks = Vec::new();

        // 3. Iterate chunks until EOF.
        while offset < data.len() {
            // Guard against excessive chunk counts.
            if chunks.len() >= MAX_CHUNKS {
                return Err(Error::InvalidSize {
                    value: chunks.len() + 1,
                    limit: MAX_CHUNKS,
                    context: "SAGE map chunk",
                });
            }

            // 3a. Read name_length (u32 LE).
            let name_len = read_u32_le(data, offset)? as usize;
            offset += 4;

            // Validate name length.
            if name_len > MAX_CHUNK_NAME {
                return Err(Error::InvalidSize {
                    value: name_len,
                    limit: MAX_CHUNK_NAME,
                    context: "SAGE map chunk name",
                });
            }

            // 3b. Read name bytes.
            let name_end = offset.checked_add(name_len).ok_or(Error::UnexpectedEof {
                needed: usize::MAX,
                available: data.len(),
            })?;
            let name_bytes = data.get(offset..name_end).ok_or(Error::UnexpectedEof {
                needed: name_end,
                available: data.len(),
            })?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            offset = name_end;

            // 3c. Read version (u32 LE).
            let version = read_u32_le(data, offset)?;
            offset += 4;

            // 3d. Read data_size (u32 LE).
            let data_size = read_u32_le(data, offset)? as usize;
            offset += 4;

            // Validate data size.
            if data_size > MAX_CHUNK_SIZE {
                return Err(Error::InvalidSize {
                    value: data_size,
                    limit: MAX_CHUNK_SIZE,
                    context: "SAGE map chunk",
                });
            }

            // 3e. Slice data_size bytes as chunk data.
            let data_end = offset.checked_add(data_size).ok_or(Error::UnexpectedEof {
                needed: usize::MAX,
                available: data.len(),
            })?;
            let chunk_data = data.get(offset..data_end).ok_or(Error::InvalidOffset {
                offset: data_end,
                bound: data.len(),
            })?;

            chunks.push(MapSageChunk {
                name,
                version,
                data: chunk_data,
            });

            // 3f. Advance offset past chunk data.
            offset = data_end;
        }

        Ok(Self { chunks })
    }

    /// Returns a slice of all parsed chunks.
    pub fn chunks(&self) -> &[MapSageChunk<'input>] {
        &self.chunks
    }

    /// Finds the first chunk with the given name (case-sensitive).
    pub fn chunk(&self, name: &str) -> Option<&MapSageChunk<'input>> {
        self.chunks.iter().find(|c| c.name == name)
    }

    /// Finds all chunks with the given name (case-sensitive).
    pub fn chunks_by_name(&self, name: &str) -> Vec<&MapSageChunk<'input>> {
        self.chunks.iter().filter(|c| c.name == name).collect()
    }

    /// Returns the total number of chunks in the map file.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
}

#[cfg(test)]
mod tests;
