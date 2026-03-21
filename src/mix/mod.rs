// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! MIX archive parser (`.mix`).
//!
//! A MIX file is a **flat archive** where files are identified by a CRC hash of
//! their uppercase filename — there is no filename table stored on disk.
//! The CRC is a lookup key derived from the filename text, not a checksum of
//! the file contents.
//!
//! ## File Layout (Basic Format)
//!
//! ```text
//! [FileHeader]          6 bytes  (count: u16, data_size: u32)
//! [SubBlock × count]   12 bytes each, sorted by CRC for binary search
//! [file data]           concatenated file bodies
//! ```
//!
//! ## Extended Format Detection
//!
//! If the first two bytes are `0x00 0x00`, the archive uses the extended format:
//!
//! ```text
//! [0x0000]      2 bytes  (extended marker)
//! [flags]       2 bytes  (bit 0 = SHA-1 digest present, bit 1 = Blowfish encrypted)
//! [FileHeader]  6 bytes
//! [SubBlock × count]
//! [file data]
//! [optional SHA-1 digest]   20 bytes, present when flags bit 0 is set
//! ```
//!
//! ## Blowfish-Encrypted Archives
//!
//! When the `encrypted-mix` feature is enabled (default), archives with the
//! encryption flag (`flags & 0x0002`) are decrypted transparently using the
//! publicly known Westwood RSA + Blowfish key derivation algorithm.  See
//! [`crate::mix_crypt`] for details.
//!
//! When the feature is disabled, encrypted archives return
//! [`Error::EncryptedArchive`].
//!
//! ## CRC Filename Hashing
//!
//! Filenames are uppercased, then accumulated into a 32-bit CRC by processing
//! 4 bytes at a time:
//!
//! ```text
//! CRC = rotate_left(CRC, 1) + u32::from_le_bytes([b0, b1, b2, b3])
//! ```
//!
//! Partial trailing bytes (< 4) are zero-padded before the final accumulation.
//! This CRC is used to find an entry in the archive index; it does not verify
//! the contents stored at that entry.
//!
//! ## References
//!
//! Implemented from XCC Utilities documentation (Olaf van der Spek),
//! OpenRA's MIX loader, and binary analysis of game files.  Cross-reference:
//! the original game defines the format in `MIXFILE.H`, `CRC.H`, `CRC.CPP`.

mod builtin_names;
mod entry_reader;
pub mod known_names;
pub mod lmd;
pub mod metadata;
/// Overlay-resolution helpers for mounted MIX archive sets.
pub mod overlay;
mod stream;
pub use builtin_names::{builtin_name_map, builtin_name_stats, BuiltinNameStats};
pub use entry_reader::MixEntryReader;
pub use overlay::{MixOverlayIndex, MixOverlayRecord};
pub use stream::MixArchiveReader;

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le};
use std::collections::HashMap;

/// V38 safety cap: maximum number of entries in a MIX archive.
///
/// RA1's MAIN.MIX contains ~64,000 entries (it nests sub-archives).
/// 131,072 is generous enough for any real archive while preventing a
/// crafted header from allocating gigabytes of SubBlock entries
/// (131,072 × 12 bytes ≈ 1.5 MB, acceptable).
pub(crate) const MAX_MIX_ENTRIES: usize = 131_072;

// ─── CRC ─────────────────────────────────────────────────────────────────────

/// Newtype wrapper for a Westwood MIX CRC hash.
///
/// Prevents accidental mixing of raw `u32` values with CRC identifiers.
/// Use [`crc()`] to compute from a filename, or [`MixCrc::from_raw()`] when
/// reading a pre-computed value from binary data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MixCrc(u32);

impl MixCrc {
    /// Wraps a raw `u32` as a [`MixCrc`].
    #[inline]
    pub const fn from_raw(value: u32) -> Self {
        Self(value)
    }

    /// Returns the inner `u32` value.
    #[inline]
    pub const fn to_raw(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for MixCrc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "MixCrc(0x{:08X})", self.0)
    }
}

/// Computes the Westwood MIX CRC for a filename.
///
/// The filename is converted to uppercase before hashing.  The algorithm
/// processes the bytes in 4-byte groups (little-endian), applying a
/// rotate-left-1 + add accumulation:
///
/// ```text
/// CRC = CRC.rotate_left(1).wrapping_add(u32::from_le_bytes(group))
/// ```
///
/// Partial groups are zero-padded on the right.
///
/// This is a hash of the filename text used for archive lookup, not a checksum
/// of file contents.
///
/// ## Allocation
///
/// This function performs **zero heap allocation**.  MIX filenames are always
/// ASCII, so uppercasing is done byte-by-byte via [`u8::to_ascii_uppercase`]
/// instead of [`str::to_uppercase`] (which allocates a `String`).  This is
/// critical because `crc()` is called on every [`MixArchive::get`] lookup.
#[inline]
pub fn crc(filename: &str) -> MixCrc {
    let bytes = filename.as_bytes();
    let mut accum: u32 = 0;
    // Process 4-byte groups with inline ASCII uppercasing.
    // Partial trailing groups are zero-padded on the right.
    for chunk in bytes.chunks(4) {
        let mut buf = [0u8; 4];
        for (j, &b) in chunk.iter().enumerate() {
            if let Some(slot) = buf.get_mut(j) {
                *slot = b.to_ascii_uppercase();
            }
        }
        let word = u32::from_le_bytes(buf);
        // rotate_left(1) + wrapping_add: the publicly documented MIX CRC
        // algorithm (XCC Utilities, OpenRA Classic hash).
        accum = accum.rotate_left(1).wrapping_add(word);
    }
    MixCrc(accum)
}

// ─── Structures ──────────────────────────────────────────────────────────────

/// One entry in the MIX SubBlock index table.
///
/// The SubBlock array is sorted by CRC to allow binary-search lookup.
/// On disk, entries are sorted by **signed** `i32` comparison (Westwood
/// convention).  The parser re-sorts by unsigned `u32` after reading so
/// that [`MixArchive::get_by_crc`] can use the derived [`Ord`] on
/// [`MixCrc`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixEntry {
    /// CRC hash of the file's uppercase name.
    pub crc: MixCrc,
    /// Byte offset from the start of the data section.
    pub offset: u32,
    /// File size in bytes.
    pub size: u32,
}

/// A parsed MIX archive.
///
/// File data is accessed by calling [`MixArchive::get`] with a filename; the
/// method hashes the name and performs a binary search over the entry table.
#[derive(Debug)]
pub struct MixArchive<'input> {
    entries: Vec<MixEntry>,
    data: &'input [u8],
}

impl<'input> MixArchive<'input> {
    /// Parses a MIX archive from a byte slice.
    ///
    /// Supports **basic**, **extended** (SHA-1), and **Blowfish-encrypted**
    /// formats.  Encrypted archives are decrypted transparently when the
    /// `encrypted-mix` feature is enabled (default).
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`]  — input is too short for the header/index.
    /// - [`Error::EncryptedArchive`] — archive uses Blowfish encryption and
    ///   the `encrypted-mix` feature is not enabled.
    /// - [`Error::InvalidOffset`]  — a SubBlock offset points past the data.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        let mut pos = 0usize;

        // ── Detect basic vs. extended format ──────────────────────────────
        //
        // The original engine exploits the fact that a valid basic-format
        // archive can never start with 0x0000 (that would mean zero files,
        // which never ships).  An extended-format archive starts with the
        // marker 0x0000 followed by a 16-bit flags word.
        if data.len() < 2 {
            return Err(Error::UnexpectedEof {
                needed: 2,
                available: data.len(),
            });
        }
        // Safe reads via helpers (defense-in-depth over the upfront check).
        let first_word = read_u16_le(data, 0)?;
        if first_word == 0 {
            // Extended format: next 2 bytes are flags.
            if data.len() < 4 {
                return Err(Error::UnexpectedEof {
                    needed: 4,
                    available: data.len(),
                });
            }
            let flags = read_u16_le(data, 2)?;

            // Bit 1 = Blowfish encryption.
            if flags & 0x0002 != 0 {
                #[cfg(feature = "encrypted-mix")]
                {
                    return Self::parse_encrypted(data, flags);
                }
                #[cfg(not(feature = "encrypted-mix"))]
                {
                    return Err(Error::EncryptedArchive);
                }
            }
            // Bit 0 = SHA-1 digest appended after the data section.
            // We don't use this flag during parsing — digest verification
            // is a cache-time concern (not implemented here).
            // Skip marker (2) + flags (2) = 4 bytes.
            pos = 4;
        }

        // ── FileHeader: count (u16) + data_size (u32) ─────────────────────
        if pos + 6 > data.len() {
            return Err(Error::UnexpectedEof {
                needed: pos + 6,
                available: data.len(),
            });
        }
        let count = read_u16_le(data, pos)? as usize;
        // data_size (u32) is present in the header but unused — we derive
        // actual bounds from the buffer length, which is always authoritative.
        pos += 6; // skip count (2) + data_size (4)

        // V38: Reject archives with unreasonable entry counts.
        if count > MAX_MIX_ENTRIES {
            return Err(Error::InvalidSize {
                value: count,
                limit: MAX_MIX_ENTRIES,
                context: "MIX entry count",
            });
        }

        // ── SubBlock index ─────────────────────────────────────────────────
        let index_bytes = count * 12;
        if pos + index_bytes > data.len() {
            return Err(Error::UnexpectedEof {
                needed: pos + index_bytes,
                available: data.len(),
            });
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let crc = MixCrc::from_raw(read_u32_le(data, pos)?);
            let offset = read_u32_le(data, pos + 4)?;
            let size = read_u32_le(data, pos + 8)?;
            entries.push(MixEntry { crc, offset, size });
            pos += 12;
        }

        // Re-sort entries by unsigned CRC.  On disk, Westwood tools sort
        // SubBlocks by signed i32 comparison.  Our binary_search uses the
        // unsigned Ord derived on MixCrc(u32), so we re-sort to match.
        entries.sort_by_key(|e| e.crc);

        // `pos` now points to the start of the file data section.
        // The optional SHA-1 digest (flags bit 0) is stored at the END of
        // the file, after DataSize bytes of data — not between the index
        // and data.  We don't skip it here; it's simply trailing bytes
        // that no SubBlock entry references.
        // All SubBlock offsets are relative to this point.
        let data_section = data.get(pos..).ok_or(Error::UnexpectedEof {
            needed: pos,
            available: data.len(),
        })?;

        // V38: Validate that every entry’s offset+size fits within the
        // data section.  Uses saturating_add to prevent wrap-around on
        // 32-bit platforms when offset and size are both near u32::MAX.
        for entry in &entries {
            let end = (entry.offset as usize).saturating_add(entry.size as usize);
            if end > data_section.len() {
                return Err(Error::InvalidOffset {
                    offset: end,
                    bound: data_section.len(),
                });
            }
        }

        Ok(MixArchive {
            entries,
            data: data_section,
        })
    }

    /// Parses a Blowfish-encrypted MIX archive.
    ///
    /// Called when `flags & 0x0002` indicates encryption and the
    /// `encrypted-mix` feature is enabled.  The 80-byte `key_source` follows
    /// the flags word at offset 4.  After key derivation and header
    /// decryption, the file data section starts after the encrypted blocks.
    #[cfg(feature = "encrypted-mix")]
    fn parse_encrypted(data: &'input [u8], _flags: u16) -> Result<Self, Error> {
        use crate::mix_crypt;

        // flags bit 0 (SHA-1) is not used during parsing — the digest sits
        // at the end of the file, after the data section.

        // ── Read key_source (80 bytes starting at offset 4) ──────────────
        let ks_start = 4usize;
        let ks_end = ks_start.saturating_add(mix_crypt::KEY_SOURCE_LEN);
        if ks_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: ks_end,
                available: data.len(),
            });
        }
        let key_source = data.get(ks_start..ks_end).ok_or(Error::UnexpectedEof {
            needed: ks_end,
            available: data.len(),
        })?;

        // ── Derive Blowfish key from key_source ─────────────────────────
        let bf_key = mix_crypt::derive_blowfish_key(key_source)?;

        // ── Decrypt header ───────────────────────────────────────────────
        let encrypted_start = ks_end; // byte offset where encrypted blocks begin
        if encrypted_start >= data.len() {
            return Err(Error::UnexpectedEof {
                needed: encrypted_start + 8,
                available: data.len(),
            });
        }
        let encrypted_region = data.get(encrypted_start..).ok_or(Error::UnexpectedEof {
            needed: encrypted_start + 8,
            available: data.len(),
        })?;
        let header_bytes = mix_crypt::decrypt_mix_header(encrypted_region, &bf_key)?;

        // ── Parse decrypted FileHeader: count (u16) + data_size (u32) ────
        if header_bytes.len() < 6 {
            return Err(Error::UnexpectedEof {
                needed: 6,
                available: header_bytes.len(),
            });
        }
        // Safe read via helper (defense-in-depth over the upfront check).
        let count = read_u16_le(&header_bytes, 0)? as usize;

        // V38: Reject archives with unreasonable entry counts.
        if count > MAX_MIX_ENTRIES {
            return Err(Error::InvalidSize {
                value: count,
                limit: MAX_MIX_ENTRIES,
                context: "encrypted MIX entry count",
            });
        }

        // ── Parse decrypted SubBlock index ───────────────────────────────
        let index_start = 6usize;
        let index_bytes = count.saturating_mul(12);
        let index_end = index_start.saturating_add(index_bytes);
        if index_end > header_bytes.len() {
            return Err(Error::UnexpectedEof {
                needed: index_end,
                available: header_bytes.len(),
            });
        }

        let mut entries = Vec::with_capacity(count);
        let mut pos = index_start;
        for _ in 0..count {
            let crc = MixCrc::from_raw(read_u32_le(&header_bytes, pos)?);
            let offset = read_u32_le(&header_bytes, pos + 4)?;
            let size = read_u32_le(&header_bytes, pos + 8)?;
            entries.push(MixEntry { crc, offset, size });
            pos += 12;
        }

        // Re-sort entries by unsigned CRC (same reason as non-encrypted path).
        entries.sort_by_key(|e| e.crc);

        // ── Locate data section ──────────────────────────────────────────
        // The encrypted blocks cover ceil((6 + count*12) / 8) * 8 bytes.
        let header_size = 6usize.saturating_add(count.saturating_mul(12));
        let num_blocks = header_size.div_ceil(8);
        let encrypted_len = num_blocks * 8;
        let data_offset = encrypted_start.saturating_add(encrypted_len);

        // The optional SHA-1 digest (flags bit 0) is at the END of the
        // file, after the data section — not between the encrypted header
        // and the data.  No skip needed here.

        if data_offset > data.len() {
            return Err(Error::UnexpectedEof {
                needed: data_offset,
                available: data.len(),
            });
        }
        let data_section = data.get(data_offset..).ok_or(Error::UnexpectedEof {
            needed: data_offset,
            available: data.len(),
        })?;

        // V38: Validate that every entry's offset+size fits within the
        // data section.
        for entry in &entries {
            let end = (entry.offset as usize).saturating_add(entry.size as usize);
            if end > data_section.len() {
                return Err(Error::InvalidOffset {
                    offset: end,
                    bound: data_section.len(),
                });
            }
        }

        Ok(MixArchive {
            entries,
            data: data_section,
        })
    }

    /// Returns the file data for a given filename, or `None` if not found.
    ///
    /// The filename is uppercased and hashed with [`crc`] before the binary
    /// search, matching the original engine's lookup behaviour.
    #[inline]
    pub fn get(&self, filename: &str) -> Option<&'input [u8]> {
        let key = crc(filename);
        self.get_by_crc(key)
    }

    /// Returns the file data for a known CRC, or `None` if not found.
    ///
    /// Uses binary search on the CRC-sorted entry table, then slices the
    /// data section.  The `saturating_add` on `offset + size` prevents
    /// integer wrap on 32-bit targets (V38).
    ///
    /// Uses `.get()` for defense-in-depth: entries are validated during
    /// `parse()`, but safe slicing prevents a panic if invariants are
    /// ever broken by a future code change.
    #[inline]
    pub fn get_by_crc(&self, key: MixCrc) -> Option<&'input [u8]> {
        let idx = self.entries.binary_search_by_key(&key, |e| e.crc).ok()?;
        let entry = self.entries.get(idx)?;
        let start = entry.offset as usize;
        let end = start.saturating_add(entry.size as usize);
        self.data.get(start..end)
    }

    /// Returns the file data for the entry at `index`, or `None` if out of range.
    ///
    /// Unlike [`MixArchive::get_by_crc`], this preserves duplicate CRC entries:
    /// callers can retrieve each physical SubBlock payload exactly as stored in
    /// the archive.
    #[inline]
    pub fn get_by_index(&self, index: usize) -> Option<&'input [u8]> {
        let entry = self.entries.get(index)?;
        let start = entry.offset as usize;
        let end = start.saturating_add(entry.size as usize);
        self.data.get(start..end)
    }

    /// Returns a slice over all index entries.
    #[inline]
    pub fn entries(&self) -> &[MixEntry] {
        &self.entries
    }

    /// Returns the number of files in this archive.
    #[inline]
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Extracts embedded filename mappings from the archive.
    ///
    /// Checks two sources in priority order:
    ///
    /// 1. **XCC local mix database** — an entry with CRC `0x54C2_D545`
    ///    (the hash of `"local mix database.dat"`), containing filename
    ///    strings in XCC's NUL-terminated format.
    /// 2. **(future) CNFM trailing metadata** — not yet auto-detected
    ///    during parse, but can be read from trailing bytes.
    ///
    /// Returns an empty map if no embedded names are found.
    pub fn embedded_names(&self) -> HashMap<MixCrc, String> {
        // Try XCC local mix database first.
        if let Some(lmd_data) = self.get_by_crc(lmd::LMD_CRC) {
            let names = lmd::parse_lmd(lmd_data);
            if !names.is_empty() {
                return names;
            }
        }
        HashMap::new()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_encrypted;
#[cfg(test)]
mod tests_streaming;
#[cfg(test)]
mod tests_validation;
