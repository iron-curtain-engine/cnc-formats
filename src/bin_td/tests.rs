// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

/// Builds a BIN grid filled with a single repeating cell value.
fn build_bin(width: usize, height: usize, fill: (u8, u8)) -> Vec<u8> {
    let cell_count = width * height;
    let mut out = Vec::with_capacity(cell_count * 2);
    for _ in 0..cell_count {
        out.push(fill.0);
        out.push(fill.1);
    }
    out
}

/// Builds a BIN grid where each cell encodes its sequential index
/// across both bytes (template_type = low byte, template_icon = high byte).
fn build_bin_sequential(width: usize, height: usize) -> Vec<u8> {
    let cell_count = width * height;
    let mut out = Vec::with_capacity(cell_count * 2);
    for i in 0..cell_count {
        out.push((i & 0xFF) as u8); // template_type
        out.push(((i >> 8) & 0xFF) as u8); // template_icon
    }
    out
}

#[test]
fn parse_valid_td() {
    let data = build_bin(64, 64, (0x0A, 0x03));
    let map = BinMap::parse(&data, 64, 64).unwrap();

    assert_eq!(map.width(), 64);
    assert_eq!(map.height(), 64);
    assert_eq!(map.cells().len(), 64 * 64);

    // Every cell should have the fill value.
    assert_eq!(
        map.cells().first(),
        Some(&BinCell {
            template_type: 0x0A,
            template_icon: 0x03,
        })
    );
    assert_eq!(
        map.cells().last(),
        Some(&BinCell {
            template_type: 0x0A,
            template_icon: 0x03,
        })
    );
}

#[test]
fn parse_valid_ra1() {
    let data = build_bin(128, 128, (0xFF, 0x00));
    let map = BinMap::parse(&data, 128, 128).unwrap();

    assert_eq!(map.width(), 128);
    assert_eq!(map.height(), 128);
    assert_eq!(map.cells().len(), 128 * 128);
}

#[test]
fn cell_access() {
    let data = build_bin_sequential(4, 3);
    let map = BinMap::parse(&data, 4, 3).unwrap();

    // Cell (0,0) → index 0 → type=0x00, icon=0x00
    assert_eq!(
        map.cell(0, 0),
        Some(&BinCell {
            template_type: 0x00,
            template_icon: 0x00,
        })
    );

    // Cell (3,0) → index 3 → type=0x03, icon=0x00
    assert_eq!(
        map.cell(3, 0),
        Some(&BinCell {
            template_type: 0x03,
            template_icon: 0x00,
        })
    );

    // Cell (0,1) → index 4 → type=0x04, icon=0x00
    assert_eq!(
        map.cell(0, 1),
        Some(&BinCell {
            template_type: 0x04,
            template_icon: 0x00,
        })
    );

    // Cell (2,2) → index 10 → type=0x0A, icon=0x00
    assert_eq!(
        map.cell(2, 2),
        Some(&BinCell {
            template_type: 0x0A,
            template_icon: 0x00,
        })
    );
}

#[test]
fn cell_out_of_bounds() {
    let data = build_bin(4, 3, (0x01, 0x02));
    let map = BinMap::parse(&data, 4, 3).unwrap();

    // x out of bounds
    assert_eq!(map.cell(4, 0), None);
    // y out of bounds
    assert_eq!(map.cell(0, 3), None);
    // both out of bounds
    assert_eq!(map.cell(4, 3), None);
    // large coordinates
    assert_eq!(map.cell(usize::MAX, 0), None);
    assert_eq!(map.cell(0, usize::MAX), None);
}

#[test]
fn reject_wrong_size() {
    // One byte too short for a 64×64 grid.
    let data = vec![0u8; 64 * 64 * 2 - 1];
    let err = BinMap::parse(&data, 64, 64).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "BIN terrain grid",
            ..
        }
    ));

    // One byte too long.
    let data = vec![0u8; 64 * 64 * 2 + 1];
    let err = BinMap::parse(&data, 64, 64).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "BIN terrain grid",
            ..
        }
    ));
}

#[test]
fn reject_zero_dimension() {
    let err = BinMap::parse(&[], 0, 64).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 0,
            context: "BIN terrain grid width",
            ..
        }
    ));

    let err = BinMap::parse(&[], 64, 0).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 0,
            context: "BIN terrain grid height",
            ..
        }
    ));
}

#[test]
fn reject_dimension_overflow() {
    // Dimensions that would overflow usize when multiplied together
    // (but individually within MAX_DIMENSION is not possible since
    // MAX_DIMENSION is 256 — so we test dimensions that exceed it
    // which would also cause large product issues).
    let err = BinMap::parse(&[], 257, 1).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 257,
            context: "BIN terrain grid width",
            ..
        }
    ));
}

#[test]
fn reject_too_large_dimension() {
    let err = BinMap::parse(&[], 257, 64).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 257,
            context: "BIN terrain grid width",
            ..
        }
    ));

    let err = BinMap::parse(&[], 64, 300).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 300,
            context: "BIN terrain grid height",
            ..
        }
    ));
}

/// All-0xFF input does not panic; rejected for size mismatch.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFF_u8; 32];
    // 32 bytes is not enough for 64×64 (8192 bytes needed).
    assert!(BinMap::parse(&data, 64, 64).is_err());
}

/// All-zero input with correct size parses as an empty map.
#[test]
fn adversarial_all_zero() {
    let data = vec![0u8; 8192];
    let map = BinMap::parse(&data, 64, 64).unwrap();
    assert_eq!(map.width(), 64);
    assert_eq!(map.height(), 64);
    assert!(map
        .cells()
        .iter()
        .all(|c| c.template_type == 0 && c.template_icon == 0));
}
