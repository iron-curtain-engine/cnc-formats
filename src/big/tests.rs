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
