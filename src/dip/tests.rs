// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Unit tests for DIP/segmented-DIP palette parsing.
use super::*;

fn segmented_dip_bytes() -> Vec<u8> {
    let section0 = [0x00, 0x00, 0x3C, 0x3C];
    let section1 = [0x01, 0x80, 0x00, 0x00];
    let trailer = [0x0B, 0x80];

    let header_size = 12u16;
    let end0 = header_size as usize + section0.len();
    let end1 = end0 + section1.len();

    let mut out = Vec::new();
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(end0 as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(end1 as u16).to_le_bytes());
    out.extend_from_slice(&section0);
    out.extend_from_slice(&section1);
    out.extend_from_slice(&trailer);
    out
}

fn string_table_dip_bytes() -> Vec<u8> {
    let strings: [&[u8]; 3] = [b"", b"Setup", b"Continue"];
    let table_len = strings.len() * 2;
    let mut out = vec![0u8; table_len];
    let mut offset = table_len as u16;
    for (i, bytes) in strings.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(bytes);
        out.push(0);
        offset = offset.saturating_add(bytes.len() as u16).saturating_add(1);
    }
    out
}

#[test]
fn parse_segmented_dip() {
    let data = segmented_dip_bytes();
    let dip = DipSegmentedFile::parse(&data).expect("segmented DIP should parse");
    assert_eq!(dip.section_count, 2);
    assert_eq!(dip.header_size, 12);
    assert_eq!(dip.end_offsets, vec![16, 20]);
    assert_eq!(dip.sections.len(), 2);
    assert_eq!(dip.sections[0].data, &[0x00, 0x00, 0x3C, 0x3C]);
    assert_eq!(dip.sections[1].data, &[0x01, 0x80, 0x00, 0x00]);
    assert_eq!(dip.trailer, &[0x0B, 0x80]);
}

#[test]
fn parse_string_table_dip() {
    let data = string_table_dip_bytes();
    let dip = DipFile::parse(&data).expect("string-table DIP should parse");
    match dip {
        DipFile::StringTable(strings) => {
            assert_eq!(strings.string_count(), 3);
            assert_eq!(strings.strings[1].as_lossy_str(), "Setup");
            assert_eq!(strings.strings[2].as_lossy_str(), "Continue");
        }
        DipFile::Segmented(_) => panic!("expected string-table DIP"),
    }
}

#[test]
fn reject_segmented_dip_with_bad_trailer() {
    let mut data = segmented_dip_bytes();
    let last = data.len() - 2;
    data[last..].copy_from_slice(&0x1234u16.to_le_bytes());
    assert!(DipSegmentedFile::parse(&data).is_err());
}
