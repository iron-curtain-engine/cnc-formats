// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ─── Helper: Build a MEG archive from in-memory files ────────────────────

/// Construct a valid MEG binary from a list of `(filename, data)` pairs.
///
/// The builder produces the three-section layout:
///   1. Header (8 bytes)
///   2. Filename table (variable)
///   3. File record table (18 bytes per entry)
///   4. Data section (concatenated file contents)
///
/// File records reference filenames by index and use absolute offsets
/// into the archive for the data section.
pub(crate) fn build_meg(files: &[(&str, &[u8])]) -> Vec<u8> {
    let count = files.len() as u32;
    let mut buf = Vec::new();

    // ── Header ───────────────────────────────────────────────────────
    buf.extend_from_slice(&count.to_le_bytes()); // num_filenames
    buf.extend_from_slice(&count.to_le_bytes()); // num_files

    // ── Filename Table ───────────────────────────────────────────────
    for (name, _) in files {
        let name_len = name.len() as u16;
        buf.extend_from_slice(&name_len.to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
    }

    // ── Calculate data offsets ────────────────────────────────────────
    // Data section starts after header + filename table + record table.
    let records_total = files.len() * FILE_RECORD_SIZE;
    let data_start = buf.len() + records_total;

    let mut offset = data_start;
    let mut offsets = Vec::with_capacity(files.len());
    for (_, data) in files {
        offsets.push(offset as u32);
        offset += data.len();
    }

    // ── File Records ─────────────────────────────────────────────────
    for (i, (name, data)) in files.iter().enumerate() {
        let crc32 = 0u32; // CRC32 not verified during parse
        buf.extend_from_slice(&crc32.to_le_bytes()); // crc32
        buf.extend_from_slice(&(i as u32).to_le_bytes()); // index
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes()); // size
        buf.extend_from_slice(&offsets[i].to_le_bytes()); // start
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes()); // name_length
    }

    // ── Data Section ─────────────────────────────────────────────────
    for (_, data) in files {
        buf.extend_from_slice(data);
    }

    buf
}

// ── Archive parsing tests ────────────────────────────────────────────────

/// Parse an archive with zero files.
///
/// Why: an empty MEG archive is valid (unlike MIX where count=0 triggers
/// extended-format detection).  The parser must handle it without error.
#[test]
fn parse_empty_archive() {
    let bytes = build_meg(&[]);
    let archive = MegArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 0);
    assert!(archive.entries().is_empty());
}

/// Parse a single-file archive and retrieve the file by name.
///
/// Why: core happy-path test for the entire parse → lookup pipeline.
#[test]
fn parse_single_file() {
    let content = b"hello, meg world";
    let bytes = build_meg(&[("TEST.TXT", content)]);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 1);
    let got = archive.get("TEST.TXT").expect("file not found");
    assert_eq!(got, content);
}

/// File lookup is case-insensitive.
///
/// Why: MEG filenames are stored in a specific case but lookups should
/// be case-insensitive to match game engine behaviour.
#[test]
fn get_case_insensitive() {
    let content = b"data";
    let bytes = build_meg(&[("FILE.BIN", content)]);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.get("FILE.BIN"), Some(content.as_ref()));
    assert_eq!(archive.get("file.bin"), Some(content.as_ref()));
    assert_eq!(archive.get("File.Bin"), Some(content.as_ref()));
}

/// Looking up a nonexistent filename returns `None`, not a panic.
///
/// Why: callers must be able to probe for optional files safely.
#[test]
fn get_nonexistent() {
    let bytes = build_meg(&[("PRESENT.BIN", b"x")]);
    let archive = MegArchive::parse(&bytes).unwrap();
    assert_eq!(archive.get("ABSENT.BIN"), None);
}

/// Multi-file archive: all files can be retrieved with correct content.
///
/// Why: verifies that filename indexing, offset calculations, and data
/// retrieval all work correctly when multiple entries coexist.
#[test]
fn parse_multiple_files() {
    let files: &[(&str, &[u8])] = &[
        ("ALPHA.DAT", b"first"),
        ("BETA.DAT", b"second_file"),
        ("GAMMA.DAT", b"third"),
    ];
    let bytes = build_meg(files);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 3);
    for (name, expected) in files {
        let got = archive.get(name).expect(name);
        assert_eq!(got, *expected, "content mismatch for {name}");
    }
}

/// Entry metadata (name, offset, size) is correctly populated.
///
/// Why: CLI subcommands (list, inspect) depend on entry metadata
/// being accurate.
#[test]
fn entry_metadata_populated() {
    let files: &[(&str, &[u8])] = &[("FOO.TXT", b"bar"), ("BAZ.BIN", b"quux!")];
    let bytes = build_meg(files);
    let archive = MegArchive::parse(&bytes).unwrap();

    let entries = archive.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "FOO.TXT");
    assert_eq!(entries[0].size, 3);
    assert_eq!(entries[1].name, "BAZ.BIN");
    assert_eq!(entries[1].size, 5);
}

/// Files with path separators in filenames are preserved.
///
/// Why: MEG archives from the Remastered Collection contain path-like
/// filenames (e.g. "DATA/ART/UNITS.SHP").  The parser must preserve
/// these as-is without stripping or modifying path components.
#[test]
fn filename_with_path_separators() {
    let content = b"nested";
    let bytes = build_meg(&[("DATA/ART/UNITS.SHP", content)]);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.entries()[0].name, "DATA/ART/UNITS.SHP");
    assert_eq!(archive.get("DATA/ART/UNITS.SHP"), Some(content.as_ref()));
    // Case-insensitive lookup with paths.
    assert_eq!(archive.get("data/art/units.shp"), Some(content.as_ref()));
}

/// Empty file entries (size = 0) are handled correctly.
///
/// Why: some MEG archives contain placeholder entries with zero-length
/// data.  The parser must return an empty slice, not an error.
#[test]
fn empty_file_entry() {
    let bytes = build_meg(&[("EMPTY.DAT", b"")]);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.entries()[0].size, 0);
    let got = archive.get("EMPTY.DAT").expect("file not found");
    assert!(got.is_empty());
}

/// Archive with mixed empty and non-empty files.
///
/// Why: verifies that zero-length entries don't corrupt offset
/// calculations for subsequent entries.
#[test]
fn mixed_empty_and_nonempty() {
    let files: &[(&str, &[u8])] = &[
        ("A.DAT", b""),
        ("B.DAT", b"content"),
        ("C.DAT", b""),
        ("D.DAT", b"more"),
    ];
    let bytes = build_meg(files);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 4);
    for (name, expected) in files {
        let got = archive.get(name).expect(name);
        assert_eq!(got, *expected, "content mismatch for {name}");
    }
}

/// Input shorter than the minimum header (8 bytes) returns `UnexpectedEof`.
///
/// Why: the parser needs at least 8 bytes for the header.
#[test]
fn parse_too_short() {
    assert!(matches!(
        MegArchive::parse(&[]),
        Err(Error::UnexpectedEof { .. })
    ));
    assert!(matches!(
        MegArchive::parse(&[0u8; 4]),
        Err(Error::UnexpectedEof { .. })
    ));
    assert!(matches!(
        MegArchive::parse(&[0u8; 7]),
        Err(Error::UnexpectedEof { .. })
    ));
}

/// `num_filenames != num_files` → `InvalidMagic`.
///
/// Why: the format requires both counts to match.  A mismatch indicates
/// a corrupt or non-MEG file.
#[test]
fn parse_mismatched_counts() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u32.to_le_bytes()); // num_filenames = 2
    bytes.extend_from_slice(&3u32.to_le_bytes()); // num_files = 3 (mismatch)
    bytes.extend_from_slice(&[0u8; 256]); // padding

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidMagic { .. })));
}

/// V38: entry count exceeding `MAX_MEG_ENTRIES` → `InvalidSize`.
///
/// Why: a crafted archive claiming millions of entries could allocate
/// excessive memory.  The parser rejects counts above the cap.
#[test]
fn parse_entry_count_exceeds_cap() {
    let big_count = (MAX_MEG_ENTRIES + 1) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&big_count.to_le_bytes()); // num_filenames
    bytes.extend_from_slice(&big_count.to_le_bytes()); // num_files
    bytes.extend_from_slice(&[0u8; 256]); // padding

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidSize { .. })));
}

/// File record with offset+size exceeding archive length → `InvalidOffset`.
///
/// Why: every entry is validated during parsing; without this check a
/// malformed archive could cause an out-of-bounds slice later.
#[test]
fn parse_invalid_offset() {
    // Build a 1-file archive manually with a record whose offset+size
    // exceeds the archive length.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_filenames = 1
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_files = 1
                                                  // Filename table: "A" (1 byte)
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.push(b'A');
    // File record: crc=0, index=0, size=9999, start=0, name_len=1
    bytes.extend_from_slice(&0u32.to_le_bytes()); // crc32
    bytes.extend_from_slice(&0u32.to_le_bytes()); // index
    bytes.extend_from_slice(&9999u32.to_le_bytes()); // size (too large)
    bytes.extend_from_slice(&0u32.to_le_bytes()); // start
    bytes.extend_from_slice(&1u16.to_le_bytes()); // name_length
                                                  // Only 5 bytes of data
    bytes.extend_from_slice(&[0xAA; 5]);

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

/// File record with filename index out of bounds → `InvalidOffset`.
///
/// Why: a crafted record could reference a filename table entry that
/// does not exist, causing a panic without bounds checking.
#[test]
fn parse_invalid_filename_index() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_filenames = 1
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_files = 1
                                                  // Filename table: "X" (1 byte)
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.push(b'X');
    // File record with index=5 (out of bounds, only 1 filename)
    bytes.extend_from_slice(&0u32.to_le_bytes()); // crc32
    bytes.extend_from_slice(&5u32.to_le_bytes()); // index = 5 (invalid)
    bytes.extend_from_slice(&0u32.to_le_bytes()); // size
    bytes.extend_from_slice(&0u32.to_le_bytes()); // start
    bytes.extend_from_slice(&1u16.to_le_bytes()); // name_length
    bytes.extend_from_slice(&[0u8; 64]); // padding

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

/// V38: filename length exceeding `MAX_FILENAME_LEN` → `InvalidSize`.
///
/// Why: a crafted name_length field could cause excessive allocation.
#[test]
fn parse_filename_length_exceeds_cap() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_filenames = 1
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_files = 1
                                                  // Filename with length > MAX_FILENAME_LEN
    let bad_len = (MAX_FILENAME_LEN + 1) as u16;
    bytes.extend_from_slice(&bad_len.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 256]); // padding

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidSize { .. })));
}
