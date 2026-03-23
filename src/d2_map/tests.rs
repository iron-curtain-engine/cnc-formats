// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

/// Builds a scenario byte buffer from header fields and optional extra data.
#[allow(clippy::too_many_arguments)]
fn build_scenario(
    lose: u16,
    win: u16,
    seed: u16,
    time: u16,
    cx: u16,
    cy: u16,
    scale: u16,
    house: u16,
    extra: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&lose.to_le_bytes());
    out.extend_from_slice(&win.to_le_bytes());
    out.extend_from_slice(&seed.to_le_bytes());
    out.extend_from_slice(&time.to_le_bytes());
    out.extend_from_slice(&cx.to_le_bytes());
    out.extend_from_slice(&cy.to_le_bytes());
    out.extend_from_slice(&scale.to_le_bytes());
    out.extend_from_slice(&house.to_le_bytes());
    out.extend_from_slice(extra);
    out
}

/// Full header with placement data parses correctly.
#[test]
fn parse_valid_scenario() {
    let extra = [0xAA, 0xBB, 0xCC, 0xDD];
    let data = build_scenario(0x0001, 0x0002, 0xBEEF, 500, 32, 64, 1, 1, &extra);
    let scn = D2Scenario::parse(&data).unwrap();

    assert_eq!(scn.header.lose_flags, 0x0001);
    assert_eq!(scn.header.win_flags, 0x0002);
    assert_eq!(scn.header.map_seed, 0xBEEF);
    assert_eq!(scn.header.time_limit, 500);
    assert_eq!(scn.header.cursor_x, 32);
    assert_eq!(scn.header.cursor_y, 64);
    assert_eq!(scn.header.map_scale, 1);
    assert_eq!(scn.header.active_house, 1);
    assert_eq!(scn.placement_data(), &extra);
}

/// Exactly 16 bytes (no placement data) parses with empty placements.
#[test]
fn parse_minimal() {
    let data = build_scenario(0, 0, 42, 0, 0, 0, 0, 0, &[]);
    let scn = D2Scenario::parse(&data).unwrap();

    assert_eq!(scn.header.map_seed, 42);
    assert!(scn.placement_data().is_empty());
}

/// All house values 0..=5 map to the expected enum variants.
#[test]
fn house_mapping() {
    let expected = [
        (0, D2House::Harkonnen),
        (1, D2House::Atreides),
        (2, D2House::Ordos),
        (3, D2House::Fremen),
        (4, D2House::Sardaukar),
        (5, D2House::Mercenary),
    ];
    for (index, house) in expected {
        let data = build_scenario(0, 0, 0, 0, 0, 0, 0, index, &[]);
        let scn = D2Scenario::parse(&data).unwrap();
        assert_eq!(scn.house(), Some(house), "house index {index}");
    }
}

/// House value beyond known range returns None.
#[test]
fn house_unknown() {
    let data = build_scenario(0, 0, 0, 0, 0, 0, 0, 6, &[]);
    let scn = D2Scenario::parse(&data).unwrap();
    assert_eq!(scn.house(), None);

    let data = build_scenario(0, 0, 0, 0, 0, 0, 0, 0xFFFF, &[]);
    let scn = D2Scenario::parse(&data).unwrap();
    assert_eq!(scn.house(), None);
}

/// Placement data is accessible and matches trailing bytes.
#[test]
fn placement_data_access() {
    let extra: Vec<u8> = (0..20).collect();
    let data = build_scenario(0, 0, 0, 0, 0, 0, 2, 0, &extra);
    let scn = D2Scenario::parse(&data).unwrap();
    assert_eq!(scn.placement_data().len(), 20);
    assert_eq!(scn.placement_data(), extra.as_slice());
}

/// Input shorter than the 16-byte header is rejected with UnexpectedEof.
#[test]
fn reject_truncated() {
    let data = build_scenario(0, 0, 0, 0, 0, 0, 0, 0, &[]);
    // Try every length from 0 to 15.
    for len in 0..HEADER_SIZE {
        let err = D2Scenario::parse(&data[..len]).unwrap_err();
        assert!(
            matches!(
                err,
                Error::UnexpectedEof {
                    needed: HEADER_SIZE,
                    ..
                }
            ),
            "expected UnexpectedEof for {len} bytes, got {err:?}",
        );
    }
}

/// Map scale > 2 is rejected with InvalidSize.
#[test]
fn reject_invalid_map_scale() {
    let data = build_scenario(0, 0, 0, 0, 0, 0, 3, 0, &[]);
    let err = D2Scenario::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 3,
            limit: 2,
            context: "D2 scenario map scale",
        }
    ));
}

/// All-0xFF input does not panic; scale 0xFFFF triggers InvalidSize.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFF_u8; 32];
    let err = D2Scenario::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "D2 scenario map scale",
            ..
        }
    ));
}

/// All-zero input does not panic; produces a valid minimal scenario.
#[test]
fn adversarial_all_zero() {
    let data = vec![0u8; 16];
    let scn = D2Scenario::parse(&data).unwrap();
    assert_eq!(scn.header.lose_flags, 0);
    assert_eq!(scn.header.win_flags, 0);
    assert_eq!(scn.header.map_seed, 0);
    assert_eq!(scn.header.time_limit, 0);
    assert_eq!(scn.header.cursor_x, 0);
    assert_eq!(scn.header.cursor_y, 0);
    assert_eq!(scn.header.map_scale, 0);
    assert_eq!(scn.header.active_house, 0);
    assert_eq!(scn.house(), Some(D2House::Harkonnen));
    assert!(scn.placement_data().is_empty());
}
