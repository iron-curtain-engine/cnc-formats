// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

fn build_lut() -> Vec<u8> {
    let mut out = Vec::with_capacity(LUT_FILE_SIZE);
    for i in 0..LUT_ENTRY_COUNT {
        out.push((i % 64) as u8);
        out.push(((i / 64) % 64) as u8);
        out.push(((i / 256) % 16) as u8);
    }
    out
}

#[test]
fn parse_valid_lut() {
    let data = build_lut();
    let lut = LutFile::parse(&data).unwrap();

    assert_eq!(lut.entry_count(), LUT_ENTRY_COUNT);
    assert_eq!(
        lut.entries.first(),
        Some(&LutEntry {
            x: 0,
            y: 0,
            value: 0
        })
    );
    assert_eq!(
        lut.entries.get(65),
        Some(&LutEntry {
            x: 1,
            y: 1,
            value: 0
        })
    );
}

#[test]
fn parse_rejects_wrong_size() {
    let err = LutFile::parse(&vec![0u8; LUT_FILE_SIZE - 1]).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "LUT file size",
            ..
        }
    ));
}

#[test]
fn parse_rejects_out_of_range_value() {
    let mut data = build_lut();
    data[2] = 16;
    let err = LutFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "LUT value component",
            ..
        }
    ));
}
