// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

fn build_vqp(table_count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&table_count.to_le_bytes());
    for table in 0..table_count {
        for row in 0u16..256 {
            for col in 0u16..=row {
                let value = ((table as u16).wrapping_add(row).wrapping_add(col) & 0xFF) as u8;
                out.push(value);
            }
        }
    }
    out
}

#[test]
fn parse_valid_vqp() {
    let data = build_vqp(2);
    let file = VqpFile::parse(&data).unwrap();

    assert_eq!(file.num_tables, 2);
    assert_eq!(file.tables.len(), 2);
    assert_eq!(file.tables[0].packed.len(), VQP_TABLE_SIZE);
    assert_eq!(file.tables[1].packed.len(), VQP_TABLE_SIZE);
}

#[test]
fn parse_rejects_truncated_header() {
    let err = VqpFile::parse(&[0x01, 0x00, 0x00]).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 4,
            available: 3,
        }
    ));
}

#[test]
fn parse_rejects_size_mismatch() {
    let mut data = build_vqp(1);
    data.pop();
    let err = VqpFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "VQP file size",
            ..
        }
    ));
}

#[test]
fn parse_rejects_excessive_table_count() {
    let mut data = Vec::new();
    data.extend_from_slice(&((MAX_TABLE_COUNT as u32) + 1).to_le_bytes());
    let err = VqpFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "VQP table count",
            ..
        }
    ));
}

#[test]
fn table_lookup_is_symmetric() {
    let data = build_vqp(1);
    let file = VqpFile::parse(&data).unwrap();
    let table = &file.tables[0];

    assert_eq!(table.get(10, 3), table.get(3, 10));
    assert_eq!(table.get(0, 0), 0);
    assert_eq!(table.get(3, 10), 13);
}
