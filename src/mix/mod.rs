// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! MIX archive parser (`.mix`).
//!
//! A MIX file is a **flat archive** where files are identified by a CRC hash of
//! their uppercase filename — there is no filename table stored on disk.
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
//! [flags]       2 bytes  (bit 1 = SHA-1 digest present, bit 2 = Blowfish encrypted)
//! [FileHeader]  6 bytes
//! [SubBlock × count]
//! [optional SHA-1 digest]
//! [file data]
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
//!
//! ## References
//!
//! Implemented from XCC Utilities documentation (Olaf van der Spek),
//! OpenRA's MIX loader, and binary analysis of game files.  Cross-reference:
//! the original game defines the format in `MIXFILE.H`, `CRC.H`, `CRC.CPP`.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le};

/// V38 safety cap: maximum number of entries in a MIX archive.
///
/// Original RA archives contain ~1,500 entries; 16,384 is generous enough
/// for any real archive while preventing a crafted header from allocating
/// gigabytes of SubBlock entries (16,384 × 12 bytes = 192 KB, acceptable).
pub(crate) const MAX_MIX_ENTRIES: usize = 16_384;

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
            buf[j] = b.to_ascii_uppercase();
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
/// The SubBlock array is sorted by [`MixEntry::crc`] to allow binary-search
/// lookup.
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
pub struct MixArchive<'a> {
    entries: Vec<MixEntry>,
    data: &'a [u8],
}

impl<'a> MixArchive<'a> {
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
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let mut pos = 0usize;
        let mut has_sha1 = false;

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
            // Bit 0 = SHA-1 digest follows the SubBlock table.
            has_sha1 = flags & 0x0001 != 0;
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

        // ── Optional SHA-1 digest (extended format, flags & 0x0001) ────────
        // The 20-byte digest is not verified — we just skip past it to reach
        // the file data section.  Verification is deferred to a future
        // integrity-check API (if needed).
        if has_sha1 {
            if pos + 20 > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: pos + 20,
                    available: data.len(),
                });
            }
            pos += 20; // skip the 20-byte SHA-1 digest
        }

        // `pos` now points to the start of the file data section.
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
    fn parse_encrypted(data: &'a [u8], flags: u16) -> Result<Self, Error> {
        use crate::mix_crypt;

        let has_sha1 = flags & 0x0001 != 0;

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

        // ── Locate data section ──────────────────────────────────────────
        // The encrypted blocks cover ceil((6 + count*12) / 8) * 8 bytes.
        let header_size = 6usize.saturating_add(count.saturating_mul(12));
        let num_blocks = header_size.div_ceil(8);
        let encrypted_len = num_blocks * 8;
        let mut data_offset = encrypted_start.saturating_add(encrypted_len);

        // Skip optional SHA-1 digest (20 bytes after the encrypted header).
        if has_sha1 {
            let sha1_end = data_offset.saturating_add(20);
            if sha1_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: sha1_end,
                    available: data.len(),
                });
            }
            data_offset = sha1_end;
        }

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
    pub fn get(&self, filename: &str) -> Option<&'a [u8]> {
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
    pub fn get_by_crc(&self, key: MixCrc) -> Option<&'a [u8]> {
        let idx = self.entries.binary_search_by_key(&key, |e| e.crc).ok()?;
        let entry = &self.entries[idx];
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
}

#[cfg(test)]
mod tests;
