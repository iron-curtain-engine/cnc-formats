// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

/// Helper: Build a valid APT file with the given entries.
///
/// The APT data section is placed immediately after the 8-byte header
/// (apt_data_offset = 8).
fn build_apt(entries: &[(u32, [u32; 4])]) -> Vec<u8> {
    let movie_count = entries.len();
    let apt_data_offset = 8u32; // Right after header

    let mut out = Vec::new();

    // Header
    out.extend_from_slice(b"Apt\0");
    out.extend_from_slice(&apt_data_offset.to_le_bytes());

    // APT data section
    out.extend_from_slice(&(movie_count as u32).to_le_bytes());
    for (offset, fields) in entries {
        out.extend_from_slice(&offset.to_le_bytes());
        for f in fields {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }

    out
}

/// Helper: Build a `.const` companion file from NUL-separated strings.
fn build_apt_const(strings: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, s) in strings.iter().enumerate() {
        out.extend_from_slice(s.as_bytes());
        if i < strings.len() - 1 {
            out.push(0);
        }
    }
    out
}

// ── AptFile parsing ─────────────────────────────────────────────────────

#[test]
fn parse_valid_apt() {
    let data = build_apt(&[(100, [1, 2, 3, 4]), (200, [10, 20, 30, 40])]);

    let apt = AptFile::parse(&data).expect("failed to parse valid APT");
    assert_eq!(apt.apt_data_offset(), 8);
    assert_eq!(apt.entry_count(), 2);

    let entries = apt.entries();
    assert_eq!(entries[0].entry_offset, 100);
    assert_eq!(entries[0].fields, [1, 2, 3, 4]);
    assert_eq!(entries[1].entry_offset, 200);
    assert_eq!(entries[1].fields, [10, 20, 30, 40]);
}

#[test]
fn parse_single_entry() {
    let data = build_apt(&[(42, [0, 0, 0, 0])]);

    let apt = AptFile::parse(&data).expect("failed to parse single-entry APT");
    assert_eq!(apt.entry_count(), 1);
    assert_eq!(apt.entries()[0].entry_offset, 42);
    assert_eq!(apt.entries()[0].fields, [0, 0, 0, 0]);
}

#[test]
fn parse_no_entries() {
    let data = build_apt(&[]);

    let apt = AptFile::parse(&data).expect("failed to parse zero-entry APT");
    assert_eq!(apt.entry_count(), 0);
    assert!(apt.entries().is_empty());
}

#[test]
fn entry_access() {
    let data = build_apt(&[
        (10, [1, 2, 3, 4]),
        (20, [5, 6, 7, 8]),
        (30, [9, 10, 11, 12]),
    ]);

    let apt = AptFile::parse(&data).unwrap();
    assert_eq!(apt.entry_count(), 3);
    assert_eq!(apt.entries().len(), 3);

    // Verify each entry
    assert_eq!(apt.entries()[0].entry_offset, 10);
    assert_eq!(apt.entries()[1].entry_offset, 20);
    assert_eq!(apt.entries()[2].entry_offset, 30);
}

#[test]
fn data_at_access() {
    let data = build_apt(&[(0, [0xAA, 0xBB, 0xCC, 0xDD])]);

    let apt = AptFile::parse(&data).unwrap();

    // data_at(0) should return the full file
    let slice = apt.data_at(0).unwrap();
    assert_eq!(slice.len(), data.len());

    // data_at(4) should skip the magic
    let slice = apt.data_at(4).unwrap();
    assert_eq!(slice.len(), data.len() - 4);

    // data_at past end should return None
    assert!(apt.data_at(data.len() + 1).is_none());

    // data_at exactly at end should return empty slice
    let slice = apt.data_at(data.len()).unwrap();
    assert!(slice.is_empty());
}

// ── Error cases ─────────────────────────────────────────────────────────

#[test]
fn reject_invalid_magic() {
    let mut data = build_apt(&[]);
    data[0] = b'X'; // Break magic

    let err = AptFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidMagic {
            context: "APT header"
        }
    );
}

#[test]
fn reject_truncated_header() {
    // Less than 8 bytes
    let data = b"Apt\0";

    let err = AptFile::parse(data).unwrap_err();
    assert_eq!(
        err,
        Error::UnexpectedEof {
            needed: HEADER_SIZE,
            available: 4,
        }
    );
}

#[test]
fn reject_data_offset_out_of_bounds() {
    let mut data = build_apt(&[]);
    // Set apt_data_offset to point way past EOF
    data[4..8].copy_from_slice(&9999u32.to_le_bytes());

    let err = AptFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidOffset {
            offset: 9999,
            bound: data.len(),
        }
    );
}

#[test]
fn reject_too_many_movies() {
    let mut data = build_apt(&[]);
    // Overwrite movie_count to exceed MAX_MOVIES
    let offset = 8; // apt_data_offset points here
    data[offset..offset + 4].copy_from_slice(&(MAX_MOVIES as u32 + 1).to_le_bytes());

    let err = AptFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidSize {
            value: MAX_MOVIES + 1,
            limit: MAX_MOVIES,
            context: "APT movie count",
        }
    );
}

#[test]
fn reject_truncated_entry_table() {
    // Build a valid 2-entry file, then truncate so only 1 entry fits
    let data = build_apt(&[(10, [1, 2, 3, 4]), (20, [5, 6, 7, 8])]);
    // Full size: 8 (header) + 4 (count) + 40 (2*20 entries) = 52
    // Truncate to remove part of the second entry
    let truncated = &data[..40];

    let err = AptFile::parse(truncated).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

#[test]
fn adversarial_all_ff() {
    let data = vec![0xFF; 64];
    // Should not panic — magic will be wrong
    let err = AptFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidMagic {
            context: "APT header"
        }
    );
}

#[test]
fn adversarial_all_zero() {
    let data = vec![0x00; 64];
    // Should not panic — magic will be wrong
    let err = AptFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidMagic {
            context: "APT header"
        }
    );
}

// ── AptConst parsing ────────────────────────────────────────────────────

#[test]
fn parse_valid_const() {
    let data = build_apt_const(&["Hello", "World", "Test"]);

    let c = AptConst::parse(&data).expect("failed to parse valid const");
    assert_eq!(c.len(), 3);
    assert_eq!(c.get(0), Some("Hello"));
    assert_eq!(c.get(1), Some("World"));
    assert_eq!(c.get(2), Some("Test"));
    assert!(!c.is_empty());
}

#[test]
fn const_empty() {
    let data: &[u8] = &[];

    let c = AptConst::parse(data).expect("failed to parse empty const");
    assert_eq!(c.len(), 0);
    assert!(c.is_empty());
    assert!(c.strings().is_empty());
}

#[test]
fn const_single_string() {
    let data = b"OnlyOne";

    let c = AptConst::parse(data).expect("failed to parse single-string const");
    assert_eq!(c.len(), 1);
    assert_eq!(c.get(0), Some("OnlyOne"));
    assert_eq!(c.get(1), None);
}
