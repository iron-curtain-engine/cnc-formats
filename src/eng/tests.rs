// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

fn build_eng(strings: &[&[u8]]) -> Vec<u8> {
    let table_len = strings.len().saturating_mul(2);
    let mut out = vec![0u8; table_len];
    let mut offset = table_len;
    for (i, bytes) in strings.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&(offset as u16).to_le_bytes());
        out.extend_from_slice(bytes);
        out.push(0);
        offset = offset.saturating_add(bytes.len()).saturating_add(1);
    }
    out
}

#[test]
fn parse_valid_eng() {
    let data = build_eng(&[b"", b"Sell", b"Mission Failed"]);
    let file = EngFile::parse(&data).unwrap();

    assert_eq!(file.data_start, 6);
    assert_eq!(file.string_count(), 3);
    assert_eq!(file.strings[0].bytes, b"");
    assert_eq!(file.strings[1].bytes, b"Sell");
    assert_eq!(file.strings[2].as_lossy_str(), "Mission Failed");
}

#[test]
fn parse_rejects_odd_table_length() {
    let err = EngFile::parse(&[3, 0, 3, 0, 0]).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ENG offset table length",
            ..
        }
    ));
}

#[test]
fn parse_rejects_decreasing_offsets() {
    let mut data = build_eng(&[b"ABC", b"DEF", b"GHI"]);
    data[4..6].copy_from_slice(&8u16.to_le_bytes());
    let err = EngFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "ENG string offsets",
            ..
        }
    ));
}

#[test]
fn parse_rejects_missing_nul_terminator() {
    let mut data = build_eng(&[b"ABC"]);
    data.pop();
    let err = EngFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}
