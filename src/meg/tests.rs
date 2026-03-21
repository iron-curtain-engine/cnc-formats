// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ─── Helper: Build a MEG archive from in-memory files ────────────────────

/// Construct a valid legacy-format MEG binary from `(filename, data)` pairs.
///
/// Layout:
///   1. Header (8 bytes)
///   2. Filename table (variable)
///   3. File record table (20 bytes per entry)
///   4. Data section (concatenated payloads)
///
/// This matches the original Petroglyph community-documented format.
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

    // ── Calculate data offsets ───────────────────────────────────────
    // Data section starts after header + filename table + record table.
    let records_total = files.len() * FILE_RECORD_SIZE;
    let data_start = buf.len() + records_total;

    let mut offset = data_start;
    let mut offsets = Vec::with_capacity(files.len());
    for (_, data) in files {
        offsets.push(offset as u32);
        offset += data.len();
    }

    // ── File Records ────────────────────────────────────────────────
    for (i, (_, data)) in files.iter().enumerate() {
        let crc32 = 0u32; // CRC32 not verified during parse
        buf.extend_from_slice(&crc32.to_le_bytes()); // crc32
        buf.extend_from_slice(&(i as u32).to_le_bytes()); // record index
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes()); // size
        buf.extend_from_slice(&offsets[i].to_le_bytes()); // start
        buf.extend_from_slice(&(i as u32).to_le_bytes()); // filename index
    }

    // ── Data Section ─────────────────────────────────────────────────
    for (_, data) in files {
        buf.extend_from_slice(data);
    }

    buf
}

/// Construct a Remastered-style format 3 MEG archive.
pub(crate) fn build_remastered_meg(files: &[(&str, &[u8])]) -> Vec<u8> {
    let count = files.len() as u32;
    let mut names = Vec::new();
    for (name, _) in files {
        names.extend_from_slice(&(name.len() as u16).to_le_bytes());
        names.extend_from_slice(name.as_bytes());
    }

    let data_start = REMASTERED_HEADER_SIZE + names.len() + files.len() * FILE_RECORD_SIZE;
    let mut buf = Vec::with_capacity(data_start);
    buf.extend_from_slice(&PETRO_FLAG_UNENCRYPTED.to_le_bytes());
    buf.extend_from_slice(&PETRO_FORMAT_ID.to_le_bytes());
    buf.extend_from_slice(&(data_start as u32).to_le_bytes());
    buf.extend_from_slice(&count.to_le_bytes()); // num_files
    buf.extend_from_slice(&count.to_le_bytes()); // num_filenames
    buf.extend_from_slice(&(names.len() as u32).to_le_bytes());
    buf.extend_from_slice(&names);

    let mut offset = data_start as u32;
    for (i, (_, data)) in files.iter().enumerate() {
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&0u32.to_le_bytes()); // crc32 (ignored)
        buf.extend_from_slice(&(i as u32).to_le_bytes()); // record index
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes()); // size
        buf.extend_from_slice(&offset.to_le_bytes()); // absolute start
        buf.extend_from_slice(&(i as u16).to_le_bytes()); // filename index
        offset = offset.saturating_add(data.len() as u32);
    }

    for (_, data) in files {
        buf.extend_from_slice(data);
    }

    buf
}

// ── Archive parsing tests ────────────────────────────────────────────────

/// Parse an empty legacy archive.
#[test]
fn parse_empty_archive() {
    let bytes = build_meg(&[]);
    let archive = MegArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 0);
    assert!(archive.entries().is_empty());
}

/// Parse a single-file legacy archive and retrieve the file by name.
#[test]
fn parse_single_file() {
    let content = b"hello, meg world";
    let bytes = build_meg(&[("TEST.TXT", content)]);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 1);
    let got = archive.get("TEST.TXT").expect("file not found");
    assert_eq!(got, content);
}

/// Streaming reader loads the MEG index and reads payloads on demand.
#[test]
fn stream_reader_reads_single_file() {
    let content = b"hello, meg world";
    let bytes = build_meg(&[("TEST.TXT", content)]);
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = MegArchiveReader::open(cursor).unwrap();

    assert_eq!(archive.file_count(), 1);
    let got = archive.read("TEST.TXT").unwrap().expect("file not found");
    assert_eq!(got, content);
}

/// File lookup is case-insensitive.
#[test]
fn get_case_insensitive() {
    let content = b"data";
    let bytes = build_meg(&[("FILE.BIN", content)]);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.get("FILE.BIN"), Some(content.as_ref()));
    assert_eq!(archive.get("file.bin"), Some(content.as_ref()));
    assert_eq!(archive.get("File.Bin"), Some(content.as_ref()));
}

/// Looking up a nonexistent filename returns `None`.
#[test]
fn get_nonexistent() {
    let bytes = build_meg(&[("PRESENT.BIN", b"x")]);
    let archive = MegArchive::parse(&bytes).unwrap();
    assert_eq!(archive.get("ABSENT.BIN"), None);
}

/// Multi-file legacy archive: all files can be retrieved with correct content.
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

/// Input shorter than the minimum header returns `UnexpectedEof`.
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

/// Legacy archives may carry different filename and file counts.
///
/// The parser accepts that as long as every file record points at a valid
/// filename index.
#[test]
fn parse_mismatched_counts() {
    let names = [
        ("ONE.DAT", b"one".as_slice()),
        ("TWO.DAT", b"two".as_slice()),
    ];

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u32.to_le_bytes()); // num_filenames
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_files
    for (name, _) in names {
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }

    let record_start = bytes.len();
    let data_start = record_start + FILE_RECORD_SIZE;
    bytes.extend_from_slice(&0u32.to_le_bytes()); // crc32
    bytes.extend_from_slice(&0u32.to_le_bytes()); // record index
    bytes.extend_from_slice(&3u32.to_le_bytes()); // size
    bytes.extend_from_slice(&(data_start as u32).to_le_bytes()); // start
    bytes.extend_from_slice(&1u32.to_le_bytes()); // filename index ("TWO.DAT")
    bytes.extend_from_slice(b"two");

    let archive = MegArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.entries()[0].name, "TWO.DAT");
    assert_eq!(archive.get("TWO.DAT"), Some(b"two".as_slice()));
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

/// File record with offset+size exceeding archive length returns `InvalidOffset`.
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
    // File record: crc=0, index=0, size=9999, start=data_start, name_index=0
    let data_start = LEGACY_HEADER_SIZE + 3 + FILE_RECORD_SIZE;
    bytes.extend_from_slice(&0u32.to_le_bytes()); // crc32
    bytes.extend_from_slice(&0u32.to_le_bytes()); // index
    bytes.extend_from_slice(&9999u32.to_le_bytes()); // size (too large)
    bytes.extend_from_slice(&(data_start as u32).to_le_bytes()); // start
    bytes.extend_from_slice(&0u32.to_le_bytes()); // name_index
                                                  // Only 5 bytes of data
    bytes.extend_from_slice(&[0xAA; 5]);

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

/// File record with filename index out of bounds returns `InvalidOffset`.
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
    let data_start = LEGACY_HEADER_SIZE + 3 + FILE_RECORD_SIZE;
    bytes.extend_from_slice(&0u32.to_le_bytes()); // size
    bytes.extend_from_slice(&(data_start as u32).to_le_bytes()); // start
    bytes.extend_from_slice(&5u32.to_le_bytes()); // filename index = 5 (invalid)
    bytes.extend_from_slice(&[0u8; 64]); // padding

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

/// V38: filename length exceeding `MAX_FILENAME_LEN` returns `InvalidSize`.
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

/// Remastered format 3 archives parse successfully.
#[test]
fn parse_remastered_archive() {
    let content = b"<?xml version=\"1.0\"?><XML/>";
    let bytes = build_remastered_meg(&[("DATA\\XML\\ATTRIBUTES.XML", content)]);
    let archive = MegArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.entries()[0].name, "DATA\\XML\\ATTRIBUTES.XML");
    assert_eq!(
        archive.get("data\\xml\\attributes.xml"),
        Some(content.as_slice())
    );
}

// ─── Streaming-vs-batch correctness proofs ──────────────────────────────────

/// Proves streaming entry reads are byte-identical to batch for legacy MEG
/// archives across all access methods (by name, by index).
#[test]
fn streaming_reads_match_batch_for_all_entries() {
    let files: &[(&str, &[u8])] = &[
        ("DATA\\XML\\ATTRIBUTES.XML", b"<XML>attributes</XML>"),
        ("ART\\SHADERS\\DEFAULT.FX", &[0xCC; 48]),
        ("AUDIO\\MUSIC\\THEME.AUD", b"audio-payload"),
    ];
    let bytes = build_meg(files);

    let batch = MegArchive::parse(&bytes).unwrap();
    let mut stream = MegArchiveReader::open(std::io::Cursor::new(&bytes)).unwrap();

    assert_eq!(batch.file_count(), stream.file_count());

    // Compare by index.
    for i in 0..batch.file_count() {
        let batch_data = batch.get_by_index(i).unwrap();
        let stream_data = stream.read_by_index(i).unwrap().unwrap();
        assert_eq!(batch_data, stream_data.as_slice(), "index {i} mismatch");
    }

    // Compare by filename (case-insensitive).
    for (name, _) in files {
        let batch_data = batch.get(name).unwrap();
        let stream_data = stream.read(name).unwrap().unwrap();
        assert_eq!(batch_data, stream_data.as_slice(), "{name} mismatch");
    }
}

/// Proves streaming works identically for Remastered format 3 MEG archives.
#[test]
fn streaming_reads_match_batch_remastered() {
    let files: &[(&str, &[u8])] = &[
        (
            "DATA\\XML\\GAMEOBJECTS.XML",
            b"<objects>remastered</objects>",
        ),
        ("ART\\SPRITES\\ICONS.DDS", &[0x44, 0x44, 0x53, 0x20]),
    ];
    let bytes = build_remastered_meg(files);

    let batch = MegArchive::parse(&bytes).unwrap();
    let mut stream = MegArchiveReader::open(std::io::Cursor::new(&bytes)).unwrap();

    assert_eq!(batch.file_count(), stream.file_count());

    for i in 0..batch.file_count() {
        let batch_data = batch.get_by_index(i).unwrap();
        let stream_data = stream.read_by_index(i).unwrap().unwrap();
        assert_eq!(
            batch_data,
            stream_data.as_slice(),
            "remastered index {i} mismatch"
        );
    }

    for (name, _) in files {
        let batch_data = batch.get(name).unwrap();
        let stream_data = stream.read(name).unwrap().unwrap();
        assert_eq!(
            batch_data,
            stream_data.as_slice(),
            "remastered {name} mismatch"
        );
    }
}

/// Proves `copy_by_index` writes the same bytes as batch `get_by_index`.
#[test]
fn streaming_copy_matches_batch_get() {
    let files: &[(&str, &[u8])] = &[
        ("CONFIG.MEG", &[0x01; 16]),
        ("MODELS\\UNIT.W3D", &[0xAB; 96]),
    ];
    let bytes = build_meg(files);

    let batch = MegArchive::parse(&bytes).unwrap();
    let mut stream = MegArchiveReader::open(std::io::Cursor::new(&bytes)).unwrap();

    for i in 0..batch.file_count() {
        let batch_data = batch.get_by_index(i).unwrap();
        let mut copied = Vec::new();
        let found = stream.copy_by_index(i, &mut copied).unwrap();
        assert!(found, "copy_by_index({i}) should find entry");
        assert_eq!(copied.as_slice(), batch_data, "copy mismatch at index {i}");
    }
}

/// Proves entry metadata is identical between batch and streaming parsers.
#[test]
fn streaming_entry_metadata_matches_batch() {
    let files: &[(&str, &[u8])] = &[
        ("ALPHA.DAT", b"first"),
        ("BETA.DAT", b"second-longer"),
        ("GAMMA.DAT", b"g"),
    ];
    let bytes = build_meg(files);

    let batch = MegArchive::parse(&bytes).unwrap();
    let stream = MegArchiveReader::open(std::io::Cursor::new(&bytes)).unwrap();

    for (b, s) in batch.entries().iter().zip(stream.entries().iter()) {
        assert_eq!(b.name, s.name, "name mismatch");
        assert_eq!(b.offset, s.offset, "offset mismatch");
        assert_eq!(b.size, s.size, "size mismatch");
    }
}
