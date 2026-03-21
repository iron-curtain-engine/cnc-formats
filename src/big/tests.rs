// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

fn build_big(magic: &[u8; 4], files: &[(&str, &[u8])]) -> Vec<u8> {
    let table_size: usize = files.iter().map(|(name, _)| 8 + name.len() + 1).sum();
    let data_start = 16 + table_size;
    let archive_size = data_start + files.iter().map(|(_, data)| data.len()).sum::<usize>();

    let mut out = Vec::with_capacity(archive_size);
    out.extend_from_slice(magic);
    out.extend_from_slice(&(archive_size as u32).to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_be_bytes());
    out.extend_from_slice(&(data_start as u32).to_be_bytes());

    let mut offset = data_start as u32;
    for (name, data) in files {
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        offset = offset.saturating_add(data.len() as u32);
    }

    for (_, data) in files {
        out.extend_from_slice(data);
    }

    out
}

#[test]
fn parse_bigf_archive() {
    let data = build_big(b"BIGF", &[("Data\\INI\\GameData.ini", b"abc")]);
    let archive = BigArchive::parse(&data).unwrap();

    assert_eq!(archive.version(), BigVersion::BigF);
    assert_eq!(archive.entries().len(), 1);
    assert_eq!(archive.entries()[0].name, "Data\\INI\\GameData.ini");
    assert_eq!(archive.get("data\\ini\\gamedata.ini").unwrap(), b"abc");
}

#[test]
fn stream_reader_reads_big_entry() {
    let data = build_big(b"BIGF", &[("Data\\INI\\GameData.ini", b"abc")]);
    let cursor = std::io::Cursor::new(data);
    let mut archive = BigArchiveReader::open(cursor).unwrap();

    assert_eq!(archive.version(), BigVersion::BigF);
    assert_eq!(
        archive.read("data\\ini\\gamedata.ini").unwrap().unwrap(),
        b"abc"
    );
}

#[test]
fn parse_big4_archive() {
    let data = build_big(b"BIG4", &[("Texture.tga", &[0xAA; 4])]);
    let archive = BigArchive::parse(&data).unwrap();

    assert_eq!(archive.version(), BigVersion::Big4);
    assert_eq!(archive.entries()[0].size, 4);
}

#[test]
fn get_by_index_preserves_duplicate_names() {
    let data = build_big(b"BIGF", &[("dup.txt", b"first"), ("dup.txt", b"second")]);
    let archive = BigArchive::parse(&data).unwrap();

    assert_eq!(archive.entries().len(), 2);
    assert_eq!(archive.get("dup.txt").unwrap(), b"first");
    assert_eq!(archive.get_by_index(0).unwrap(), b"first");
    assert_eq!(archive.get_by_index(1).unwrap(), b"second");
}

#[test]
fn reject_invalid_magic() {
    let mut data = build_big(b"BIGF", &[("ok.txt", b"x")]);
    data[..4].copy_from_slice(b"NOPE");

    let err = BigArchive::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "BIG header"
        }
    ));
}

#[test]
fn reject_missing_name_terminator() {
    let mut data = build_big(b"BIGF", &[("broken.txt", b"abc")]);
    let terminator_offset = 16 + 8 + "broken.txt".len();
    data[terminator_offset] = b'X';

    let err = BigArchive::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "BIG filename terminator"
        }
    ));
}

#[test]
fn reject_entry_past_archive_end() {
    let mut data = build_big(b"BIGF", &[("bad.bin", b"abc")]);
    data[20..24].copy_from_slice(&0xFFFF_FFF0u32.to_be_bytes());

    let err = BigArchive::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

#[test]
fn padding_before_data_start_is_accepted() {
    let data = build_big(b"BIGF", &[("pad.bin", b"abc")]);
    let index_end = 16 + 8 + "pad.bin".len() + 1;
    let first_data = u32::from_be_bytes(data[12..16].try_into().unwrap()) as usize;
    let padded_first_data = first_data + 8;

    let mut padded = Vec::new();
    padded.extend_from_slice(&data[..12]);
    padded.extend_from_slice(&(padded_first_data as u32).to_be_bytes());
    padded.extend_from_slice(&data[16..index_end]);
    padded.resize(padded_first_data, 0);
    padded.extend_from_slice(&data[first_data..]);
    let padded_len = padded.len() as u32;
    padded[4..8].copy_from_slice(&padded_len.to_le_bytes());
    padded[16..20].copy_from_slice(&(padded_first_data as u32).to_be_bytes());

    let archive = BigArchive::parse(&padded).unwrap();
    assert_eq!(archive.get("pad.bin").unwrap(), b"abc");
}

// ─── Streaming-vs-batch correctness proofs ──────────────────────────────────

/// Proves streaming entry reads are byte-identical to batch for BIGF archives
/// across all access methods (by name, by index).
#[test]
fn streaming_reads_match_batch_for_all_entries() {
    let files: &[(&str, &[u8])] = &[
        ("Data\\INI\\GameData.ini", b"ObjectCreationList"),
        ("Art\\Textures\\Terrain.tga", &[0xBB; 32]),
        ("Audio\\Speech\\GLA01.mp3", b"mp3-payload-data"),
    ];
    let data = build_big(b"BIGF", files);

    let batch = BigArchive::parse(&data).unwrap();
    let mut stream = BigArchiveReader::open(std::io::Cursor::new(&data)).unwrap();

    assert_eq!(batch.entries().len(), stream.file_count());

    // Compare by index.
    for i in 0..batch.entries().len() {
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

/// Proves streaming works identically for BIG4 variant.
#[test]
fn streaming_reads_match_batch_big4() {
    let files: &[(&str, &[u8])] = &[
        ("meshes\\tank.w3d", &[0xDD; 16]),
        ("textures\\camo.dds", b"dds-header-here"),
    ];
    let data = build_big(b"BIG4", files);

    let batch = BigArchive::parse(&data).unwrap();
    let mut stream = BigArchiveReader::open(std::io::Cursor::new(&data)).unwrap();

    assert_eq!(batch.version(), BigVersion::Big4);
    assert_eq!(stream.version(), BigVersion::Big4);

    for i in 0..batch.entries().len() {
        let batch_data = batch.get_by_index(i).unwrap();
        let stream_data = stream.read_by_index(i).unwrap().unwrap();
        assert_eq!(
            batch_data,
            stream_data.as_slice(),
            "BIG4 index {i} mismatch"
        );
    }
}

/// Proves `copy_by_index` writes the same bytes as batch `get_by_index`.
#[test]
fn streaming_copy_matches_batch_get() {
    let files: &[(&str, &[u8])] = &[("map.str", b"string-table"), ("sounds.big", &[0xEE; 48])];
    let data = build_big(b"BIGF", files);

    let batch = BigArchive::parse(&data).unwrap();
    let mut stream = BigArchiveReader::open(std::io::Cursor::new(&data)).unwrap();

    for i in 0..batch.entries().len() {
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
        ("A.bin", b"alpha"),
        ("B.bin", b"bravo-longer"),
        ("C.bin", b"c"),
    ];
    let data = build_big(b"BIGF", files);

    let batch = BigArchive::parse(&data).unwrap();
    let stream = BigArchiveReader::open(std::io::Cursor::new(&data)).unwrap();

    for (b, s) in batch.entries().iter().zip(stream.entries().iter()) {
        assert_eq!(b.name, s.name, "name mismatch");
        assert_eq!(b.offset, s.offset, "offset mismatch");
        assert_eq!(b.size, s.size, "size mismatch");
    }
}
