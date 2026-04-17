// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ── Test ISO builder ─────────────────────────────────────────────────────────

/// Builds a minimal valid ISO 9660 image with the given files.
///
/// Each file is specified as `(path, data)` where `path` uses forward
/// slashes.  Single-level paths (e.g. `"README.TXT"`) go into the root
/// directory; paths with slashes (e.g. `"INSTALL/MAIN.MIX"`) create the
/// necessary subdirectories.  All filenames are stored as uppercase ASCII
/// with ";1" version suffixes, matching real ISO 9660 Level 1 images.
fn build_iso(files: &[(&str, &[u8])]) -> Vec<u8> {
    // ── Plan layout ─────────────────────────────────────────────────────
    // Collect unique directory paths and build the directory tree.
    // We need to know all directories before we can compute sector offsets.

    let mut dirs: Vec<String> = vec![String::new()]; // root = ""
    for (path, _) in files {
        let parts: Vec<&str> = path.split('/').collect();
        // For "A/B/FILE.TXT", directories are "" (root), "A", "A/B"
        for i in 1..parts.len() {
            let dir = parts[..i].join("/");
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }

    // Assign files to their parent directories.
    struct FileEntry<'a> {
        name: &'a str,  // leaf filename
        parent: String, // parent directory path ("" = root)
        data: &'a [u8],
        sector: u32, // assigned later
    }
    let mut file_entries: Vec<FileEntry> = Vec::new();
    for (path, data) in files {
        let (parent, name) = match path.rfind('/') {
            Some(pos) => (path[..pos].to_string(), &path[pos + 1..]),
            None => (String::new(), *path),
        };
        file_entries.push(FileEntry {
            name,
            parent,
            data,
            sector: 0, // will be set below
        });
    }

    // ── Assign sectors ──────────────────────────────────────────────────
    // Layout: sectors 0–15 = system area, sector 16 = PVD,
    // sector 17 = terminator, sector 18+ = directory extents,
    // then file data sectors.

    let mut next_sector: u32 = 18;

    // Each directory gets one sector (enough for small test directories).
    struct DirLayout {
        path: String,
        sector: u32,
        extent_size: u32, // will be set after building records
    }
    let mut dir_layouts: Vec<DirLayout> = Vec::new();
    for dir in &dirs {
        dir_layouts.push(DirLayout {
            path: dir.clone(),
            sector: next_sector,
            extent_size: 0,
        });
        next_sector += 1;
    }

    // Assign sectors for file data.
    for entry in &mut file_entries {
        entry.sector = next_sector;
        let sectors_needed = entry.data.len().div_ceil(SECTOR_SIZE).max(1) as u32;
        next_sector += sectors_needed;
    }

    let total_sectors = next_sector;
    let image_size = total_sectors as usize * SECTOR_SIZE;
    let mut image = vec![0u8; image_size];

    // ── Build directory records for each directory ───────────────────────

    for dir_idx in 0..dir_layouts.len() {
        let dir_path = dir_layouts[dir_idx].path.clone();
        let dir_sector = dir_layouts[dir_idx].sector;
        let mut records = Vec::new();

        // "." record — self reference
        records.extend_from_slice(&build_dir_record(
            dir_sector,
            SECTOR_SIZE as u32, // will be patched
            FLAG_DIRECTORY,
            &[0x00],
        ));

        // ".." record — parent reference
        let parent_sector = if dir_path.is_empty() {
            dir_sector // root's parent is itself
        } else {
            let parent_path = match dir_path.rfind('/') {
                Some(pos) => &dir_path[..pos],
                None => "",
            };
            dir_layouts
                .iter()
                .find(|d| d.path == parent_path)
                .map_or(dir_sector, |d| d.sector)
        };
        records.extend_from_slice(&build_dir_record(
            parent_sector,
            SECTOR_SIZE as u32,
            FLAG_DIRECTORY,
            &[0x01],
        ));

        // Subdirectory entries — directories whose parent is this directory.
        for sub_dir in &dir_layouts {
            if sub_dir.path.is_empty() {
                continue; // root is not a child of anyone
            }
            let sub_parent = match sub_dir.path.rfind('/') {
                Some(pos) => &sub_dir.path[..pos],
                None => "",
            };
            if sub_parent == dir_path {
                // Leaf name of the subdirectory.
                let leaf = match sub_dir.path.rfind('/') {
                    Some(pos) => &sub_dir.path[pos + 1..],
                    None => &sub_dir.path,
                };
                let name_with_version = leaf.to_ascii_uppercase();
                records.extend_from_slice(&build_dir_record(
                    sub_dir.sector,
                    SECTOR_SIZE as u32,
                    FLAG_DIRECTORY,
                    name_with_version.as_bytes(),
                ));
            }
        }

        // File entries in this directory.
        for entry in &file_entries {
            if entry.parent == dir_path {
                let name_with_version = format!("{};1", entry.name.to_ascii_uppercase());
                records.extend_from_slice(&build_dir_record(
                    entry.sector,
                    entry.data.len() as u32,
                    0, // regular file
                    name_with_version.as_bytes(),
                ));
            }
        }

        // Write records into the directory sector.
        let extent_size = records.len();
        let dest_offset = dir_sector as usize * SECTOR_SIZE;
        image[dest_offset..dest_offset + extent_size].copy_from_slice(&records);

        // Patch the "." self-reference data length to actual extent size.
        // The data length field is at offset 10 within the first record.
        let len_bytes = (extent_size as u32).to_le_bytes();
        image[dest_offset + 10..dest_offset + 14].copy_from_slice(&len_bytes);

        dir_layouts[dir_idx].extent_size = extent_size as u32;
    }

    // ── Write file data ─────────────────────────────────────────────────

    for entry in &file_entries {
        let offset = entry.sector as usize * SECTOR_SIZE;
        image[offset..offset + entry.data.len()].copy_from_slice(entry.data);
    }

    // ── Write Primary Volume Descriptor (sector 16) ─────────────────────

    let pvd_offset = 16 * SECTOR_SIZE;
    image[pvd_offset] = 1; // PVD type
    image[pvd_offset + 1..pvd_offset + 6].copy_from_slice(b"CD001");
    image[pvd_offset + 6] = 1; // version

    // Volume space size at offset 80 (LE) + 84 (BE).
    image[pvd_offset + 80..pvd_offset + 84].copy_from_slice(&total_sectors.to_le_bytes());
    image[pvd_offset + 84..pvd_offset + 88].copy_from_slice(&total_sectors.to_be_bytes());

    // Logical block size at offset 128 (LE) + 130 (BE).
    image[pvd_offset + 128..pvd_offset + 130].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
    image[pvd_offset + 130..pvd_offset + 132].copy_from_slice(&(SECTOR_SIZE as u16).to_be_bytes());

    // Root directory record at offset 156 (34 bytes).
    let root_sector = dir_layouts[0].sector;
    let root_extent_size = dir_layouts[0].extent_size;
    let root_record = build_dir_record(root_sector, root_extent_size, FLAG_DIRECTORY, &[0x00]);
    image[pvd_offset + 156..pvd_offset + 156 + root_record.len()].copy_from_slice(&root_record);

    // ── Write Volume Descriptor Set Terminator (sector 17) ──────────────

    let term_offset = 17 * SECTOR_SIZE;
    image[term_offset] = 255; // terminator type code
    image[term_offset + 1..term_offset + 6].copy_from_slice(b"CD001");
    image[term_offset + 6] = 1; // version

    image
}

/// Builds a single ISO 9660 directory record.
///
/// Returns the raw bytes for one directory record with the given extent
/// location (LBA), data length, file flags, and file identifier bytes.
fn build_dir_record(extent_lba: u32, data_length: u32, flags: u8, identifier: &[u8]) -> Vec<u8> {
    let id_len = identifier.len();
    // Record length: 33 fixed header bytes + identifier length + padding.
    // ISO 9660 requires even-length records, so add a padding byte if the
    // identifier length is even (making total odd before padding).
    let base_len = 33 + id_len;
    let record_len = if base_len % 2 == 0 {
        base_len
    } else {
        base_len + 1
    };

    let mut rec = vec![0u8; record_len];

    // Offset 0: record length.
    rec[0] = record_len as u8;

    // Offset 1: extended attribute record length (0).
    rec[1] = 0;

    // Offset 2–5: extent location (LE u32).
    rec[2..6].copy_from_slice(&extent_lba.to_le_bytes());
    // Offset 6–9: extent location (BE u32).
    rec[6..10].copy_from_slice(&extent_lba.to_be_bytes());

    // Offset 10–13: data length (LE u32).
    rec[10..14].copy_from_slice(&data_length.to_le_bytes());
    // Offset 14–17: data length (BE u32).
    rec[14..18].copy_from_slice(&data_length.to_be_bytes());

    // Offset 18–24: date/time (7 bytes, zeroed for tests).

    // Offset 25: file flags.
    rec[25] = flags;

    // Offset 32: file identifier length.
    rec[32] = id_len as u8;

    // Offset 33+: file identifier.
    rec[33..33 + id_len].copy_from_slice(identifier);

    rec
}

// ── Happy path tests ─────────────────────────────────────────────────────────

/// Parsing a minimal ISO with one file at the root level succeeds and
/// produces the correct entry.
#[test]
fn parse_single_root_file() {
    let iso = build_iso(&[("README.TXT", b"Hello, World!")]);
    let archive = Iso9660Archive::parse(&iso).unwrap();

    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.entries()[0].name, "README.TXT");
    assert_eq!(archive.entries()[0].size, 13);
    assert_eq!(archive.get("readme.txt").unwrap(), b"Hello, World!");
}

/// Parsing an ISO with multiple root-level files produces all entries.
#[test]
fn parse_multiple_root_files() {
    let iso = build_iso(&[
        ("FILE1.DAT", b"aaa"),
        ("FILE2.DAT", b"bbbb"),
        ("FILE3.DAT", b"ccccc"),
    ]);
    let archive = Iso9660Archive::parse(&iso).unwrap();

    assert_eq!(archive.file_count(), 3);
    assert_eq!(archive.get("file1.dat").unwrap(), b"aaa");
    assert_eq!(archive.get("file2.dat").unwrap(), b"bbbb");
    assert_eq!(archive.get("file3.dat").unwrap(), b"ccccc");
}

/// Files inside subdirectories are found with their full slash-separated path.
#[test]
fn parse_nested_directory() {
    let iso = build_iso(&[
        ("INSTALL/MAIN.MIX", b"mix-data"),
        ("INSTALL/SETUP.EXE", b"setup-data"),
        ("README.TXT", b"root-file"),
    ]);
    let archive = Iso9660Archive::parse(&iso).unwrap();

    assert_eq!(archive.file_count(), 3);
    assert!(archive.get("INSTALL/MAIN.MIX").is_some());
    assert!(archive.get("install/setup.exe").is_some());
    assert_eq!(archive.get("readme.txt").unwrap(), b"root-file");
}

/// Two levels of nesting are traversed correctly.
#[test]
fn parse_deeply_nested() {
    let iso = build_iso(&[("A/B/DEEP.TXT", b"deep-content")]);
    let archive = Iso9660Archive::parse(&iso).unwrap();

    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.get("A/B/DEEP.TXT").unwrap(), b"deep-content");
}

/// `get_by_index` returns the correct file for each index.
#[test]
fn get_by_index_returns_correct_data() {
    let iso = build_iso(&[("A.TXT", b"aaa"), ("B.TXT", b"bbb")]);
    let archive = Iso9660Archive::parse(&iso).unwrap();

    assert_eq!(archive.get_by_index(0).unwrap(), b"aaa");
    assert_eq!(archive.get_by_index(1).unwrap(), b"bbb");
    assert!(archive.get_by_index(2).is_none());
}

/// Filenames are matched case-insensitively, consistent with the BIG module.
#[test]
fn case_insensitive_lookup() {
    let iso = build_iso(&[("MyFile.Dat", b"data")]);
    let archive = Iso9660Archive::parse(&iso).unwrap();

    assert!(archive.get("MYFILE.DAT").is_some());
    assert!(archive.get("myfile.dat").is_some());
    assert!(archive.get("MyFile.Dat").is_some());
}

/// An empty ISO (no files, only the root directory) produces zero entries.
#[test]
fn parse_empty_iso() {
    let iso = build_iso(&[]);
    let archive = Iso9660Archive::parse(&iso).unwrap();

    assert_eq!(archive.file_count(), 0);
    assert!(archive.get("anything").is_none());
}

// ── Streaming reader tests ───────────────────────────────────────────────────

/// The streaming reader produces the same entries as the in-memory parser.
#[test]
fn stream_reader_matches_in_memory() {
    let iso = build_iso(&[
        ("INSTALL/MAIN.MIX", b"mix-data"),
        ("README.TXT", b"root-file"),
    ]);

    let mem_archive = Iso9660Archive::parse(&iso).unwrap();
    let cursor = std::io::Cursor::new(&iso);
    let stream_archive = Iso9660ArchiveReader::open(cursor).unwrap();

    assert_eq!(stream_archive.file_count(), mem_archive.file_count());

    for (i, entry) in mem_archive.entries().iter().enumerate() {
        let stream_entry = stream_archive.entries().get(i).unwrap();
        assert_eq!(stream_entry.name, entry.name);
        assert_eq!(stream_entry.offset, entry.offset);
        assert_eq!(stream_entry.size, entry.size);
    }
}

/// The streaming reader's `read` method returns file data correctly.
#[test]
fn stream_reader_reads_file() {
    let iso = build_iso(&[("DATA.BIN", b"binary-content")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    assert_eq!(
        archive.read("data.bin").unwrap().unwrap(),
        b"binary-content"
    );
}

/// The streaming reader's `copy` method writes data to a writer.
#[test]
fn stream_reader_copies_to_writer() {
    let iso = build_iso(&[("OUT.TXT", b"output-data")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut buf = Vec::new();
    assert!(archive.copy("out.txt", &mut buf).unwrap());
    assert_eq!(buf, b"output-data");
}

/// `read` returns `None` for a filename that does not exist.
#[test]
fn stream_reader_returns_none_for_missing() {
    let iso = build_iso(&[("EXISTS.TXT", b"yes")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    assert!(archive.read("NOPE.TXT").unwrap().is_none());
}

/// `copy` returns `false` for a filename that does not exist.
#[test]
fn stream_reader_copy_returns_false_for_missing() {
    let iso = build_iso(&[("EXISTS.TXT", b"yes")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut buf = Vec::new();
    assert!(!archive.copy("NOPE.TXT", &mut buf).unwrap());
    assert!(buf.is_empty());
}

/// `read_by_index` returns `None` for an out-of-range index.
#[test]
fn stream_reader_read_by_index_out_of_range() {
    let iso = build_iso(&[("A.TXT", b"a")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    assert!(archive.read_by_index(99).unwrap().is_none());
}

/// `indices_by_offset` returns indices sorted by file offset.
#[test]
fn indices_by_offset_sorted() {
    let iso = build_iso(&[
        ("FIRST.TXT", b"aaa"),
        ("SECOND.TXT", b"bbb"),
        ("THIRD.TXT", b"ccc"),
    ]);
    let cursor = std::io::Cursor::new(iso);
    let archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let indices = archive.indices_by_offset();
    assert_eq!(indices.len(), 3);

    // Offsets should be monotonically increasing in the sorted order.
    let offsets: Vec<u64> = indices
        .iter()
        .map(|&i| archive.entries().get(i).unwrap().offset)
        .collect();
    for window in offsets.windows(2) {
        assert!(window[0] <= window[1]);
    }
}

/// `into_inner` returns the underlying reader.
#[test]
fn into_inner_returns_reader() {
    let iso = build_iso(&[]);
    let cursor = std::io::Cursor::new(iso.clone());
    let archive = Iso9660ArchiveReader::open(cursor).unwrap();
    let recovered = archive.into_inner();

    assert_eq!(recovered.into_inner(), iso);
}

// ── Error path tests ─────────────────────────────────────────────────────────

/// An input shorter than the minimum ISO size (system area + PVD) is
/// rejected with UnexpectedEof.
#[test]
fn reject_too_short() {
    let data = vec![0u8; 100];
    let err = Iso9660Archive::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// A PVD with the wrong type code is rejected.
#[test]
fn reject_wrong_pvd_type() {
    let mut iso = build_iso(&[]);
    // Corrupt PVD type code (offset 0 within PVD sector).
    iso[PVD_OFFSET] = 99;
    let err = Iso9660Archive::parse(&iso).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "ISO 9660 PVD type code (expected 1)"
        }
    ));
}

/// A PVD with the wrong standard identifier is rejected.
#[test]
fn reject_wrong_standard_id() {
    let mut iso = build_iso(&[]);
    // Corrupt "CD001" at PVD offset 1–5.
    iso[PVD_OFFSET + 1..PVD_OFFSET + 6].copy_from_slice(b"NOPE!");
    let err = Iso9660Archive::parse(&iso).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "ISO 9660 standard identifier (expected CD001)"
        }
    ));
}

/// A PVD with a wrong version byte is rejected.
#[test]
fn reject_wrong_pvd_version() {
    let mut iso = build_iso(&[]);
    // Corrupt PVD version (offset 6 within PVD).
    iso[PVD_OFFSET + 6] = 42;
    let err = Iso9660Archive::parse(&iso).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "ISO 9660 PVD version (expected 1)"
        }
    ));
}

/// A PVD with a non-2048 logical block size is rejected.
#[test]
fn reject_wrong_block_size() {
    let mut iso = build_iso(&[]);
    // Corrupt logical block size at PVD offset 128 (LE u16).
    iso[PVD_OFFSET + 128..PVD_OFFSET + 130].copy_from_slice(&512u16.to_le_bytes());
    let err = Iso9660Archive::parse(&iso).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 512,
            limit: 2048,
            ..
        }
    ));
}

/// The streaming reader rejects a too-short input.
#[test]
fn stream_reader_rejects_too_short() {
    let data = vec![0u8; 100];
    let cursor = std::io::Cursor::new(data);
    let err = Iso9660ArchiveReader::open(cursor).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── Determinism test ─────────────────────────────────────────────────────────

/// Parsing the same ISO twice produces identical entry lists.
#[test]
fn deterministic_parsing() {
    let iso = build_iso(&[
        ("A/FILE1.TXT", b"aaa"),
        ("B/FILE2.TXT", b"bbb"),
        ("ROOT.DAT", b"ccc"),
    ]);

    let first = Iso9660Archive::parse(&iso).unwrap();
    let second = Iso9660Archive::parse(&iso).unwrap();

    assert_eq!(first.entries(), second.entries());
}

// ── Version suffix stripping ─────────────────────────────────────────────────

/// The version suffix ";1" is stripped from filenames.
///
/// ISO 9660 Level 1 stores filenames as "FILE.EXT;1".  The parser must
/// strip the ";1" so callers see clean filenames.
#[test]
fn version_suffix_stripped() {
    assert_eq!(strip_version_suffix("FILE.TXT;1"), "FILE.TXT");
    assert_eq!(strip_version_suffix("FILE.TXT;2"), "FILE.TXT");
    assert_eq!(strip_version_suffix("FILE.TXT"), "FILE.TXT");
}

/// A trailing period (empty extension) is stripped after removing the
/// version suffix.
#[test]
fn trailing_period_stripped() {
    assert_eq!(strip_version_suffix("README.;1"), "README");
    assert_eq!(strip_version_suffix("README."), "README");
}

/// A filename with no version suffix and no trailing period is unchanged.
#[test]
fn clean_name_unchanged() {
    assert_eq!(strip_version_suffix("MAIN.MIX"), "MAIN.MIX");
    assert_eq!(strip_version_suffix("SETUP"), "SETUP");
}

// ── Display message tests ────────────────────────────────────────────────────

/// Error display messages contain the key context values needed for
/// diagnosis.
#[test]
fn error_display_contains_context() {
    let err = Error::InvalidMagic {
        context: "ISO 9660 standard identifier (expected CD001)",
    };
    let msg = err.to_string();
    assert!(msg.contains("CD001"));
    assert!(msg.contains("ISO 9660"));
}

/// UnexpectedEof display includes both needed and available byte counts.
#[test]
fn unexpected_eof_display() {
    let err = Error::UnexpectedEof {
        needed: 34816,
        available: 100,
    };
    let msg = err.to_string();
    assert!(msg.contains("34816"));
    assert!(msg.contains("100"));
}

// ── Entry reader tests ───────────────────────────────────────────────────────

/// `open_entry` returns a bounded reader that produces the correct file
/// data when read to completion.
#[test]
fn entry_reader_reads_full_content() {
    let iso = build_iso(&[("DATA.BIN", b"entry-reader-content")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut reader = archive.open_entry("data.bin").unwrap().unwrap();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).unwrap();
    assert_eq!(buf, b"entry-reader-content");
}

/// `open_entry` returns `None` for a filename that does not exist.
#[test]
fn entry_reader_returns_none_for_missing() {
    let iso = build_iso(&[("EXISTS.TXT", b"yes")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    assert!(archive.open_entry("NOPE.TXT").unwrap().is_none());
}

/// `open_entry_by_index` returns `None` for out-of-range indices.
#[test]
fn entry_reader_by_index_out_of_range() {
    let iso = build_iso(&[("A.TXT", b"a")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    assert!(archive.open_entry_by_index(99).unwrap().is_none());
}

/// The entry reader reports correct `len()` and `remaining_len()`.
#[test]
fn entry_reader_length_tracking() {
    let content = b"twelve bytes";
    let iso = build_iso(&[("FILE.DAT", content)]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let reader = archive.open_entry("file.dat").unwrap().unwrap();
    assert_eq!(reader.len(), 12);
    assert_eq!(reader.remaining_len(), 12);
    assert_eq!(reader.position(), 0);
    assert!(!reader.is_empty());
}

/// The entry reader supports seeking within the bounded range.
#[test]
fn entry_reader_seek_within_bounds() {
    let iso = build_iso(&[("SEEK.DAT", b"ABCDEFGHIJ")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut reader = archive.open_entry("seek.dat").unwrap().unwrap();

    // Seek to offset 5, read the rest.
    std::io::Seek::seek(&mut reader, std::io::SeekFrom::Start(5)).unwrap();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).unwrap();
    assert_eq!(buf, b"FGHIJ");
}

/// Seeking past the entry boundary returns an error.
#[test]
fn entry_reader_seek_past_boundary_fails() {
    let iso = build_iso(&[("SMALL.DAT", b"abc")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut reader = archive.open_entry("small.dat").unwrap().unwrap();
    let result = std::io::Seek::seek(&mut reader, std::io::SeekFrom::Start(100));
    assert!(result.is_err());
}

/// Seeking from the end works correctly.
#[test]
fn entry_reader_seek_from_end() {
    let iso = build_iso(&[("END.DAT", b"0123456789")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut reader = archive.open_entry("end.dat").unwrap().unwrap();

    // Seek to 3 bytes before the end.
    std::io::Seek::seek(&mut reader, std::io::SeekFrom::End(-3)).unwrap();
    let mut buf = [0u8; 3];
    std::io::Read::read_exact(&mut reader, &mut buf).unwrap();
    assert_eq!(&buf, b"789");
}

/// Reading from a nested subdirectory file via the entry reader works.
#[test]
fn entry_reader_nested_directory() {
    let iso = build_iso(&[("INSTALL/MAIN.MIX", b"nested-mix-data")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut reader = archive.open_entry("INSTALL/MAIN.MIX").unwrap().unwrap();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).unwrap();
    assert_eq!(buf, b"nested-mix-data");
}

/// An entry reader for an empty file reports `is_empty()` and reads zero bytes.
#[test]
fn entry_reader_empty_file() {
    let iso = build_iso(&[("EMPTY.DAT", b"")]);
    let cursor = std::io::Cursor::new(iso);
    let mut archive = Iso9660ArchiveReader::open(cursor).unwrap();

    let mut reader = archive.open_entry("empty.dat").unwrap().unwrap();
    assert!(reader.is_empty());
    assert_eq!(reader.len(), 0);

    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).unwrap();
    assert!(buf.is_empty());
}
