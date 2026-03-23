// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

/// Builds a raw IDX buffer from a list of entry tuples.
fn build_idx(entries: &[(&str, u32, u32, u32, u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, offset, size, sample_rate, flags, chunk_size) in entries {
        let mut name_buf = [0u8; 16];
        let bytes = name.as_bytes();
        name_buf[..bytes.len().min(16)].copy_from_slice(&bytes[..bytes.len().min(16)]);
        out.extend_from_slice(&name_buf);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&chunk_size.to_le_bytes());
    }
    out
}

/// Builds a raw BAG buffer by concatenating data segments.
fn build_bag(segments: &[&[u8]]) -> Vec<u8> {
    segments.iter().flat_map(|s| s.iter().copied()).collect()
}

/// Parses a valid IDX with two entries and verifies all fields.
#[test]
fn parse_valid_idx() {
    let data = build_idx(&[
        ("explode.aud", 0, 1024, 22050, 1, 512),
        ("shot.aud", 1024, 2048, 44100, 2, 1024),
    ]);
    let idx = IdxFile::parse(&data).unwrap();

    assert_eq!(idx.entries().len(), 2);

    let e0 = &idx.entries()[0];
    assert_eq!(e0.name, "explode.aud");
    assert_eq!(e0.offset, 0);
    assert_eq!(e0.size, 1024);
    assert_eq!(e0.sample_rate, 22050);
    assert_eq!(e0.flags, 1);
    assert_eq!(e0.chunk_size, 512);

    let e1 = &idx.entries()[1];
    assert_eq!(e1.name, "shot.aud");
    assert_eq!(e1.offset, 1024);
    assert_eq!(e1.size, 2048);
    assert_eq!(e1.sample_rate, 44100);
    assert_eq!(e1.flags, 2);
    assert_eq!(e1.chunk_size, 1024);
}

/// Parses a valid IDX with a single entry.
#[test]
fn parse_single_entry() {
    let data = build_idx(&[("click.aud", 0, 256, 11025, 0, 128)]);
    let idx = IdxFile::parse(&data).unwrap();

    assert_eq!(idx.entries().len(), 1);
    assert_eq!(idx.entries()[0].name, "click.aud");
    assert_eq!(idx.entries()[0].size, 256);
}

/// Case-insensitive name lookup via `get()`.
#[test]
fn get_case_insensitive() {
    let data = build_idx(&[
        ("Boom.Aud", 0, 100, 22050, 0, 64),
        ("click.aud", 100, 50, 22050, 0, 64),
    ]);
    let idx = IdxFile::parse(&data).unwrap();

    let entry = idx.get("boom.aud").unwrap();
    assert_eq!(entry.name, "Boom.Aud");
    assert_eq!(entry.size, 100);

    let entry = idx.get("BOOM.AUD").unwrap();
    assert_eq!(entry.name, "Boom.Aud");

    assert!(idx.get("nonexistent").is_none());
}

/// Positional access via `get_by_index()`.
#[test]
fn get_by_index() {
    let data = build_idx(&[
        ("first.aud", 0, 10, 22050, 0, 8),
        ("second.aud", 10, 20, 22050, 0, 8),
    ]);
    let idx = IdxFile::parse(&data).unwrap();

    let e0 = idx.get_by_index(0).unwrap();
    assert_eq!(e0.name, "first.aud");

    let e1 = idx.get_by_index(1).unwrap();
    assert_eq!(e1.name, "second.aud");

    assert!(idx.get_by_index(2).is_none());
}

/// Extracting audio data from a BAG buffer returns the correct slice.
#[test]
fn extract_from_bag() {
    let seg_a = b"AUDIO_A_DATA";
    let seg_b = b"AUDIO_B_DATA_LONGER";
    let bag = build_bag(&[seg_a.as_slice(), seg_b.as_slice()]);

    let offset_a = 0u32;
    let size_a = seg_a.len() as u32;
    let offset_b = size_a;
    let size_b = seg_b.len() as u32;

    let data = build_idx(&[
        ("a.aud", offset_a, size_a, 22050, 0, 512),
        ("b.aud", offset_b, size_b, 22050, 0, 512),
    ]);
    let idx = IdxFile::parse(&data).unwrap();

    let extracted_a = idx.extract(&idx.entries()[0], &bag).unwrap();
    assert_eq!(extracted_a, seg_a);

    let extracted_b = idx.extract(&idx.entries()[1], &bag).unwrap();
    assert_eq!(extracted_b, seg_b);
}

/// Entry pointing past the BAG buffer returns `None`.
#[test]
fn extract_out_of_bounds() {
    let bag = vec![0u8; 100];
    let data = build_idx(&[("oob.aud", 50, 200, 22050, 0, 64)]);
    let idx = IdxFile::parse(&data).unwrap();

    assert!(idx.extract(&idx.entries()[0], &bag).is_none());
}

/// IDX data whose length is not a multiple of 36 is rejected.
#[test]
fn reject_invalid_size() {
    let mut data = build_idx(&[("ok.aud", 0, 100, 22050, 0, 64)]);
    data.push(0xFF); // Make it 37 bytes.

    let err = IdxFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "IDX file size",
            ..
        }
    ));
}

/// Empty IDX (0 bytes) is valid and produces zero entries.
#[test]
fn parse_empty() {
    let idx = IdxFile::parse(&[]).unwrap();
    assert!(idx.entries().is_empty());
}

/// All-0xFF entry data does not panic.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFFu8; ENTRY_SIZE];
    // Should parse without panicking.
    let result = IdxFile::parse(&data);
    assert!(result.is_ok());

    let idx = result.unwrap();
    assert_eq!(idx.entries().len(), 1);
    // All-FF name has no NUL, so all 16 bytes are used (lossy UTF-8).
    assert_eq!(idx.entries()[0].offset, u32::MAX);
    assert_eq!(idx.entries()[0].size, u32::MAX);
}

/// All-zero entry data does not panic.
#[test]
fn adversarial_all_zero() {
    let data = vec![0u8; ENTRY_SIZE];
    let result = IdxFile::parse(&data);
    assert!(result.is_ok());

    let idx = result.unwrap();
    assert_eq!(idx.entries().len(), 1);
    assert_eq!(idx.entries()[0].name, "");
    assert_eq!(idx.entries()[0].offset, 0);
    assert_eq!(idx.entries()[0].size, 0);
    assert_eq!(idx.entries()[0].sample_rate, 0);
    assert_eq!(idx.entries()[0].flags, 0);
    assert_eq!(idx.entries()[0].chunk_size, 0);
}
