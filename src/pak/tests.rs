// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

/// Builds a valid PAK archive from a list of (filename, data) pairs.
fn build_pak(files: &[(&str, &[u8])]) -> Vec<u8> {
    // Directory size: each entry = 4 bytes (offset) + name.len() + 1 (NUL).
    let dir_size: usize = files.iter().map(|(name, _)| 4 + name.len() + 1).sum();

    let mut out = Vec::new();
    let mut data_offset = dir_size;

    // Write directory entries.
    for (name, file_data) in files {
        out.extend_from_slice(&(data_offset as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(0); // NUL terminator
        data_offset += file_data.len();
    }

    // Append file data.
    for (_, file_data) in files {
        out.extend_from_slice(file_data);
    }

    out
}

// ─── Positive tests ─────────────────────────────────────────────────────────

/// Two files: verify names, offsets, sizes, and data retrieval.
#[test]
fn parse_valid_pak() {
    let data = build_pak(&[("INTRO.PAL", b"palette-data"), ("SOUND.VOC", b"audio")]);
    let archive = PakArchive::parse(&data).unwrap();

    assert_eq!(archive.entries().len(), 2);

    assert_eq!(archive.entries()[0].name, "INTRO.PAL");
    assert_eq!(archive.entries()[0].size, b"palette-data".len());
    assert_eq!(archive.get("INTRO.PAL").unwrap(), b"palette-data");

    assert_eq!(archive.entries()[1].name, "SOUND.VOC");
    assert_eq!(archive.entries()[1].size, b"audio".len());
    assert_eq!(archive.get("SOUND.VOC").unwrap(), b"audio");
}

/// A single file is correctly parsed — the last (and only) file extends to EOF.
#[test]
fn parse_single_file() {
    let data = build_pak(&[("ONLY.DAT", b"single-file-content")]);
    let archive = PakArchive::parse(&data).unwrap();

    assert_eq!(archive.entries().len(), 1);
    assert_eq!(archive.entries()[0].name, "ONLY.DAT");
    assert_eq!(archive.get("ONLY.DAT").unwrap(), b"single-file-content");
}

/// Lookup is case-insensitive (DOS filenames).
#[test]
fn get_case_insensitive() {
    let data = build_pak(&[("Menu.CPS", b"cps-pixels")]);
    let archive = PakArchive::parse(&data).unwrap();

    assert_eq!(archive.get("menu.cps").unwrap(), b"cps-pixels");
    assert_eq!(archive.get("MENU.CPS").unwrap(), b"cps-pixels");
    assert_eq!(archive.get("Menu.CPS").unwrap(), b"cps-pixels");
}

/// Positional access works and returns the correct data.
#[test]
fn get_by_index() {
    let data = build_pak(&[("A.BIN", b"alpha"), ("B.BIN", b"bravo")]);
    let archive = PakArchive::parse(&data).unwrap();

    assert_eq!(archive.get_by_index(0).unwrap(), b"alpha");
    assert_eq!(archive.get_by_index(1).unwrap(), b"bravo");
    assert!(archive.get_by_index(2).is_none());
}

// ─── Negative tests ─────────────────────────────────────────────────────────

/// Data shorter than MIN_SIZE is rejected with UnexpectedEof.
#[test]
fn reject_truncated() {
    let err = PakArchive::parse(&[0x00; 4]).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 5,
            available: 4,
        }
    ));
}

/// An offset pointing past EOF is rejected.
#[test]
fn reject_offset_out_of_bounds() {
    let mut data = build_pak(&[("TEST.DAT", b"data")]);
    // Overwrite the first entry's offset to point way past EOF.
    let bad_offset = (data.len() as u32 + 1000).to_le_bytes();
    data[..4].copy_from_slice(&bad_offset);

    let err = PakArchive::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

/// A name that runs to the end of the directory without a NUL terminator is
/// rejected.
#[test]
fn reject_missing_nul() {
    let mut data = build_pak(&[("BROKEN.DAT", b"stuff")]);
    // Replace the NUL terminator with a non-zero byte.
    let nul_offset = 4 + "BROKEN.DAT".len();
    data[nul_offset] = b'X';

    let err = PakArchive::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "PAK entry name terminator"
        }
    ));
}

// ─── Adversarial inputs ─────────────────────────────────────────────────────

/// All-0xFF input must not panic (every u32 reads as a huge offset).
#[test]
fn adversarial_all_ff() {
    let data = [0xFF; 64];
    let result = PakArchive::parse(&data);
    assert!(result.is_err());
}

/// All-zero input must not panic (first offset = 0, which is structurally
/// invalid because the directory cannot be empty).
#[test]
fn adversarial_all_zero() {
    let data = [0x00; 64];
    let result = PakArchive::parse(&data);
    assert!(result.is_err());
}
