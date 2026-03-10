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
//! Encrypted archives (`flags & 0x0002`) are not supported and return
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
//! Format source: `REDALERT/MIXFILE.H`, `REDALERT/CRC.H`, `REDALERT/CRC.CPP`.

use crate::error::Error;

// ─── CRC ─────────────────────────────────────────────────────────────────────

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
pub fn crc(filename: &str) -> u32 {
    let upper = filename.to_uppercase();
    let bytes = upper.as_bytes();
    let mut accum: u32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let mut buf = [0u8; 4];
        let end = (i + 4).min(bytes.len());
        buf[..end - i].copy_from_slice(&bytes[i..end]);
        let word = u32::from_le_bytes(buf);
        accum = accum.rotate_left(1).wrapping_add(word);
        i += 4;
    }
    accum
}

// ─── Structures ──────────────────────────────────────────────────────────────

/// One entry in the MIX SubBlock index table.
///
/// The SubBlock array is sorted by [`MixEntry::crc`] to allow binary-search
/// lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixEntry {
    /// CRC hash of the file's uppercase name.
    pub crc: u32,
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
    /// Supports both the **basic** and **extended** (SHA-1 / non-encrypted)
    /// formats.  Returns [`Error::EncryptedArchive`] for Blowfish-encrypted
    /// archives.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`]  — input is too short for the header/index.
    /// - [`Error::EncryptedArchive`] — archive uses Blowfish encryption.
    /// - [`Error::InvalidOffset`]  — a SubBlock offset points past the data.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let mut pos = 0usize;

        // ── Detect basic vs. extended format ──────────────────────────────
        // Extended marker: first two bytes == 0x0000.
        if data.len() < 2 {
            return Err(Error::UnexpectedEof);
        }
        let first_word = u16::from_le_bytes([data[0], data[1]]);
        if first_word == 0 {
            // Extended format: next 2 bytes are flags.
            if data.len() < 4 {
                return Err(Error::UnexpectedEof);
            }
            let flags = u16::from_le_bytes([data[2], data[3]]);
            if flags & 0x0002 != 0 {
                return Err(Error::EncryptedArchive);
            }
            // Skip marker (2) + flags (2).
            pos = 4;
        }

        // ── FileHeader: count (u16) + data_size (u32) ─────────────────────
        if pos + 6 > data.len() {
            return Err(Error::UnexpectedEof);
        }
        let count = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        // data_size is present but we derive bounds from the slice length.
        pos += 6; // skip count (2) + data_size (4)

        // ── SubBlock index ─────────────────────────────────────────────────
        let index_bytes = count * 12;
        if pos + index_bytes > data.len() {
            return Err(Error::UnexpectedEof);
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let crc = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            let offset =
                u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
            let size =
                u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
            entries.push(MixEntry { crc, offset, size });
            pos += 12;
        }

        // `pos` now points to the start of the file data section.
        let data_section = &data[pos..];

        // Validate all offsets against the data section length.
        for entry in &entries {
            let end = (entry.offset as usize).saturating_add(entry.size as usize);
            if end > data_section.len() {
                return Err(Error::InvalidOffset);
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
    pub fn get(&self, filename: &str) -> Option<&'a [u8]> {
        let key = crc(filename);
        self.get_by_crc(key)
    }

    /// Returns the file data for a known CRC, or `None` if not found.
    pub fn get_by_crc(&self, key: u32) -> Option<&'a [u8]> {
        let idx = self.entries.binary_search_by_key(&key, |e| e.crc).ok()?;
        let entry = &self.entries[idx];
        let start = entry.offset as usize;
        let end = start + entry.size as usize;
        Some(&self.data[start..end])
    }

    /// Returns a slice over all index entries.
    pub fn entries(&self) -> &[MixEntry] {
        &self.entries
    }

    /// Returns the number of files in this archive.
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }
}

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Builds a minimal well-formed basic MIX archive in memory.
///
/// `files` is a list of `(filename, data)` pairs.  The entries are sorted by
/// CRC before writing so the binary search works correctly.
#[cfg(test)]
pub(crate) fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    // Compute CRCs and sort.
    let mut entries: Vec<(u32, &[u8])> = files
        .iter()
        .map(|(name, data)| (crc(name), *data))
        .collect();
    entries.sort_by_key(|(c, _)| *c);

    // Compute offsets.
    let count = entries.len() as u16;
    let mut offsets: Vec<u32> = Vec::with_capacity(entries.len());
    let mut cur = 0u32;
    for (_, data) in &entries {
        offsets.push(cur);
        cur += data.len() as u32;
    }
    let data_size: u32 = cur;

    let mut out = Vec::new();
    // FileHeader
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    // SubBlock array
    for (i, (c, data)) in entries.iter().enumerate() {
        out.extend_from_slice(&c.to_le_bytes());
        out.extend_from_slice(&offsets[i].to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    }
    // File data
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }
    out
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── CRC tests ────────────────────────────────────────────────────────────

    /// CRC is deterministic for the same input.
    #[test]
    fn test_crc_deterministic() {
        assert_eq!(crc("CONQUER.MIX"), crc("CONQUER.MIX"));
    }

    /// CRC is case-insensitive (filename is uppercased before hashing).
    #[test]
    fn test_crc_case_insensitive() {
        assert_eq!(crc("conquer.mix"), crc("CONQUER.MIX"));
        assert_eq!(crc("Conquer.Mix"), crc("CONQUER.MIX"));
    }

    /// Different filenames produce different CRCs.
    #[test]
    fn test_crc_different_names() {
        assert_ne!(crc("GENERAL.MIX"), crc("CONQUER.MIX"));
        assert_ne!(crc("AUDIO.MIX"), crc("SCORES.MIX"));
    }

    /// CRC of a single-character name (tests edge-case padding).
    #[test]
    fn test_crc_short_name() {
        // "A" + zero-padding → single 4-byte chunk [0x41, 0x00, 0x00, 0x00]
        // CRC = 0u32.rotate_left(1) + 0x00000041 = 0x41
        assert_eq!(crc("A"), 0x41);
    }

    /// CRC of an 8-character name (exactly two 4-byte groups, no padding).
    #[test]
    fn test_crc_eight_chars() {
        let expected = {
            // "ABCDEFGH" → group1=[0x41,0x42,0x43,0x44], group2=[0x45,0x46,0x47,0x48]
            let g1 = u32::from_le_bytes([0x41, 0x42, 0x43, 0x44]);
            let g2 = u32::from_le_bytes([0x45, 0x46, 0x47, 0x48]);
            let c1: u32 = 0u32.rotate_left(1).wrapping_add(g1);
            c1.rotate_left(1).wrapping_add(g2)
        };
        assert_eq!(crc("ABCDEFGH"), expected);
    }

    // ── Archive parsing tests ─────────────────────────────────────────────────

    /// Parse an archive with zero files.
    ///
    /// A 0-file archive cannot use the basic format because `count == 0`
    /// produces a leading `0x0000` word, which is indistinguishable from the
    /// extended-format marker.  The parser correctly follows the spec and
    /// treats it as an extended-format archive.  We therefore build the
    /// empty archive in extended format (marker=0x0000, flags=0x0000) followed
    /// by a basic FileHeader with count=0.
    #[test]
    fn test_parse_empty_archive() {
        // Extended format: [0x0000 marker][0x0000 flags][count=0 u16][data_size=0 u32]
        let bytes: Vec<u8> = vec![
            0x00, 0x00, // extended marker
            0x00, 0x00, // flags = 0 (no SHA1, no encryption)
            0x00, 0x00, // count = 0
            0x00, 0x00, 0x00, 0x00, // data_size = 0
        ];
        let archive = MixArchive::parse(&bytes).unwrap();
        assert_eq!(archive.file_count(), 0);
    }

    /// Parse a single-file archive and retrieve the file by name.
    #[test]
    fn test_parse_single_file() {
        let content = b"hello, world";
        let bytes = build_mix(&[("TEST.TXT", content)]);
        let archive = MixArchive::parse(&bytes).unwrap();

        assert_eq!(archive.file_count(), 1);
        let got = archive.get("TEST.TXT").expect("file not found");
        assert_eq!(got, content);
    }

    /// Lookup is case-insensitive (same CRC regardless of case).
    #[test]
    fn test_get_case_insensitive() {
        let content = b"data";
        let bytes = build_mix(&[("FILE.BIN", content)]);
        let archive = MixArchive::parse(&bytes).unwrap();

        assert_eq!(archive.get("FILE.BIN"), Some(content.as_ref()));
        assert_eq!(archive.get("file.bin"), Some(content.as_ref()));
        assert_eq!(archive.get("File.Bin"), Some(content.as_ref()));
    }

    /// Looking up a nonexistent file returns None.
    #[test]
    fn test_get_nonexistent() {
        let bytes = build_mix(&[("PRESENT.BIN", b"x")]);
        let archive = MixArchive::parse(&bytes).unwrap();
        assert_eq!(archive.get("ABSENT.BIN"), None);
    }

    /// Multi-file archive: all files can be retrieved correctly.
    #[test]
    fn test_parse_multiple_files() {
        let files: &[(&str, &[u8])] = &[
            ("ALPHA.DAT", b"first"),
            ("BETA.DAT", b"second_file"),
            ("GAMMA.DAT", b"third"),
        ];
        let bytes = build_mix(files);
        let archive = MixArchive::parse(&bytes).unwrap();

        assert_eq!(archive.file_count(), 3);
        for (name, expected) in files {
            let got = archive.get(name).expect(name);
            assert_eq!(got, *expected, "content mismatch for {name}");
        }
    }

    /// Extended format (flags=0, non-encrypted) is parsed correctly.
    #[test]
    fn test_parse_extended_format() {
        // Build a basic archive then prepend the extended header (0x0000, 0x0000).
        let basic = build_mix(&[("EXT.BIN", b"extended")]);
        let mut extended = Vec::new();
        extended.extend_from_slice(&[0x00u8, 0x00, 0x00, 0x00]); // marker + flags=0
        extended.extend_from_slice(&basic);

        let archive = MixArchive::parse(&extended).unwrap();
        assert_eq!(archive.get("EXT.BIN"), Some(b"extended".as_ref()));
    }

    /// Encrypted archive returns EncryptedArchive error.
    #[test]
    fn test_parse_encrypted_returns_error() {
        // Extended marker + flags with bit 1 set (encrypted)
        let data = [0x00u8, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let result = MixArchive::parse(&data);
        assert_eq!(result.unwrap_err(), Error::EncryptedArchive);
    }

    /// Too-short header returns UnexpectedEof.
    #[test]
    fn test_parse_too_short() {
        assert_eq!(MixArchive::parse(&[]).unwrap_err(), Error::UnexpectedEof);
        assert_eq!(
            MixArchive::parse(&[0u8; 4]).unwrap_err(),
            Error::UnexpectedEof
        );
    }

    /// entries() returns sorted entries.
    #[test]
    fn test_entries_sorted_by_crc() {
        let bytes = build_mix(&[("B.DAT", b"b"), ("A.DAT", b"a"), ("C.DAT", b"c")]);
        let archive = MixArchive::parse(&bytes).unwrap();
        let entries = archive.entries();
        for i in 1..entries.len() {
            assert!(
                entries[i - 1].crc <= entries[i].crc,
                "entries not sorted by CRC"
            );
        }
    }
}
