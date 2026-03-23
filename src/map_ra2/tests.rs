// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Builds a minimal RA2 `.map` file from the given sections.
///
/// Each section is a `(name, &[(key, value)])` pair.  Output uses LF
/// line endings.
fn build_ra2_map(sections: &[(&str, &[(&str, &str)])]) -> Vec<u8> {
    let mut out = String::new();
    for (section_name, entries) in sections {
        out.push('[');
        out.push_str(section_name);
        out.push_str("]\n");
        for (key, value) in *entries {
            out.push_str(key);
            out.push('=');
            out.push_str(value);
            out.push('\n');
        }
        out.push('\n');
    }
    out.into_bytes()
}

/// Builds a full RA2 map with Basic, Map, IsoMapPack5, OverlayPack,
/// OverlayDataPack, PreviewPack, and Waypoints sections.
fn build_full_ra2_map() -> Vec<u8> {
    build_ra2_map(&[
        ("Basic", &[("Name", "Test Map"), ("Author", "Test Author")]),
        (
            "Map",
            &[
                ("Size", "0,0,128,128"),
                ("LocalSize", "2,4,124,120"),
                ("Theater", "TEMPERATE"),
            ],
        ),
        ("IsoMapPack5", &[("1", "AQIDBA=="), ("2", "BQYHCA==")]),
        ("OverlayPack", &[("1", "AAAA"), ("2", "BBBB")]),
        ("OverlayDataPack", &[("1", "CCCC"), ("2", "DDDD")]),
        ("PreviewPack", &[("1", "EEEE"), ("2", "FFFF")]),
        (
            "Waypoints",
            &[("0", "14000"), ("1", "14512"), ("2", "15024")],
        ),
    ])
}

// ── Core parsing ─────────────────────────────────────────────────────────────

/// Parses a full RA2 map with Basic, Map, and pack sections.
///
/// Why: happy-path baseline — all standard sections must parse correctly.
#[test]
fn parse_valid_map() {
    let data = build_full_ra2_map();
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.name(), Some("Test Map"));
    assert_eq!(map.author(), Some("Test Author"));
    assert_eq!(map.theater(), Some("TEMPERATE"));
    assert!(map.has_section("Basic"));
    assert!(map.has_section("Map"));
    assert!(map.has_section("IsoMapPack5"));
}

// ── Field accessors ──────────────────────────────────────────────────────────

/// Verifies name, author, and theater accessors.
///
/// Why: these are the most commonly queried fields; verifying them
/// individually catches accessor-specific regressions.
#[test]
fn name_author_theater() {
    let data = build_ra2_map(&[
        ("Basic", &[("Name", "Fortress"), ("Author", "Westwood")]),
        ("Map", &[("Theater", "SNOW")]),
    ]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.name(), Some("Fortress"));
    assert_eq!(map.author(), Some("Westwood"));
    assert_eq!(map.theater(), Some("SNOW"));
}

// ── Size parsing ─────────────────────────────────────────────────────────────

/// Verifies `Size=0,0,128,128` parses into the correct `MapSize`.
///
/// Why: the comma-separated format is RA2-specific and easy to get wrong
/// with off-by-one in `split` indexing.
#[test]
fn size_parsing() {
    let data = build_ra2_map(&[("Map", &[("Size", "0,0,128,128")])]);
    let map = MapRa2File::parse(&data).unwrap();
    let size = map.size().unwrap();
    assert_eq!(
        size,
        MapSize {
            x: 0,
            y: 0,
            width: 128,
            height: 128,
        }
    );
}

/// Verifies `LocalSize` parsing with non-zero origin.
///
/// Why: LocalSize typically has non-zero X/Y values; ensures all four
/// fields are parsed from the correct position.
#[test]
fn local_size_parsing() {
    let data = build_ra2_map(&[("Map", &[("LocalSize", "2,4,124,120")])]);
    let map = MapRa2File::parse(&data).unwrap();
    let local = map.local_size().unwrap();
    assert_eq!(
        local,
        MapSize {
            x: 2,
            y: 4,
            width: 124,
            height: 120,
        }
    );
}

/// Size with whitespace around commas still parses.
///
/// Why: real map editors may insert spaces around commas; the parser
/// must trim them.
#[test]
fn size_with_whitespace() {
    let data = build_ra2_map(&[("Map", &[("Size", "0 , 0 , 64 , 64")])]);
    let map = MapRa2File::parse(&data).unwrap();
    let size = map.size().unwrap();
    assert_eq!(
        size,
        MapSize {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        }
    );
}

/// Malformed size with wrong number of parts returns `None`.
///
/// Why: graceful handling of corrupt data — must not panic.
#[test]
fn size_malformed_returns_none() {
    let data = build_ra2_map(&[("Map", &[("Size", "0,0,128")])]);
    let map = MapRa2File::parse(&data).unwrap();
    assert!(map.size().is_none());
}

/// Non-numeric size parts return `None`.
///
/// Why: letters in a size value must not cause a panic.
#[test]
fn size_non_numeric_returns_none() {
    let data = build_ra2_map(&[("Map", &[("Size", "0,abc,128,128")])]);
    let map = MapRa2File::parse(&data).unwrap();
    assert!(map.size().is_none());
}

// ── Pack section concatenation ───────────────────────────────────────────────

/// IsoMapPack5 lines are concatenated in numeric key order.
///
/// Why: the base64 data must be reassembled in order; wrong ordering
/// would produce corrupt terrain data.
#[test]
fn iso_map_pack_concat() {
    let data = build_ra2_map(&[(
        "IsoMapPack5",
        &[("2", "SECOND"), ("1", "FIRST"), ("3", "THIRD")],
    )]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.iso_map_pack_raw(), Some("FIRSTSECONDTHIRD".to_string()));
}

/// OverlayPack and OverlayDataPack are concatenated correctly.
///
/// Why: overlay data is split across two sections that must both
/// concatenate independently.
#[test]
fn overlay_packs() {
    let data = build_ra2_map(&[
        ("OverlayPack", &[("1", "AA"), ("2", "BB")]),
        ("OverlayDataPack", &[("1", "CC"), ("2", "DD")]),
    ]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.overlay_pack_raw(), Some("AABB".to_string()));
    assert_eq!(map.overlay_data_pack_raw(), Some("CCDD".to_string()));
}

/// PreviewPack lines are concatenated in numeric key order.
///
/// Why: the preview image must be reassembled correctly.
#[test]
fn preview_pack() {
    let data = build_ra2_map(&[("PreviewPack", &[("1", "PREVIEW1"), ("2", "PREVIEW2")])]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.preview_pack_raw(), Some("PREVIEW1PREVIEW2".to_string()));
}

/// Pack section with non-sequential keys still sorts correctly.
///
/// Why: keys may not be contiguous (e.g., 1, 3, 5); sort must be
/// numeric, not lexicographic (otherwise "10" < "2").
#[test]
fn pack_numeric_sort_not_lexicographic() {
    let data = build_ra2_map(&[("IsoMapPack5", &[("10", "TEN"), ("2", "TWO"), ("1", "ONE")])]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.iso_map_pack_raw(), Some("ONETWOTEN".to_string()));
}

// ── Waypoints ────────────────────────────────────────────────────────────────

/// Waypoint count matches the number of entries in [Waypoints].
///
/// Why: waypoint count is used to determine spawn positions and
/// multiplayer slot count.
#[test]
fn waypoint_count() {
    let data = build_ra2_map(&[(
        "Waypoints",
        &[("0", "14000"), ("1", "14512"), ("2", "15024")],
    )]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.waypoint_count(), 3);
}

/// Waypoint count is 0 when [Waypoints] section is missing.
///
/// Why: single-player maps may omit waypoints; must not panic.
#[test]
fn waypoint_count_missing_section() {
    let data = build_ra2_map(&[("Basic", &[("Name", "NoWaypoints")])]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.waypoint_count(), 0);
}

// ── Missing sections ─────────────────────────────────────────────────────────

/// Accessors return `None` when their sections are missing.
///
/// Why: a minimal map may contain only `[Map]`; all optional accessors
/// must degrade gracefully.
#[test]
fn missing_sections() {
    let data = build_ra2_map(&[("Map", &[("Theater", "URBAN")])]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.name(), None);
    assert_eq!(map.author(), None);
    assert_eq!(map.size(), None);
    assert_eq!(map.local_size(), None);
    assert_eq!(map.iso_map_pack_raw(), None);
    assert_eq!(map.overlay_pack_raw(), None);
    assert_eq!(map.overlay_data_pack_raw(), None);
    assert_eq!(map.preview_pack_raw(), None);
}

// ── has_section ──────────────────────────────────────────────────────────────

/// `has_section` correctly reports presence and absence.
///
/// Why: callers use this to determine which object types exist in a map
/// before iterating entries.
#[test]
fn has_section() {
    let data = build_ra2_map(&[
        ("Basic", &[("Name", "Test")]),
        ("Infantry", &[("0", "data")]),
    ]);
    let map = MapRa2File::parse(&data).unwrap();
    assert!(map.has_section("Basic"));
    assert!(map.has_section("Infantry"));
    assert!(!map.has_section("Aircraft"));
    assert!(!map.has_section("Triggers"));
}

// ── Size limit ───────────────────────────────────────────────────────────────

/// Input exceeding MAX_INPUT_SIZE is rejected with `InvalidSize`.
///
/// Why: prevents unbounded memory allocation from maliciously large files.
#[test]
fn reject_too_large() {
    let data = vec![b' '; MAX_INPUT_SIZE + 1];
    let err = MapRa2File::parse(&data).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, MAX_INPUT_SIZE + 1);
            assert_eq!(limit, MAX_INPUT_SIZE);
            assert_eq!(context, "RA2 map file");
        }
        other => panic!("Expected InvalidSize, got {other:?}"),
    }
}

// ── ini() accessor ───────────────────────────────────────────────────────────

/// The `ini()` accessor exposes the underlying `IniFile` for direct
/// querying of non-standard sections.
///
/// Why: callers may need to access trigger, team, or AI sections that
/// the typed API does not cover.
#[test]
fn ini_accessor() {
    let data = build_ra2_map(&[
        ("Basic", &[("Name", "Direct Access")]),
        ("Triggers", &[("T0", "trigger_data")]),
    ]);
    let map = MapRa2File::parse(&data).unwrap();
    assert_eq!(map.ini().get("Triggers", "T0"), Some("trigger_data"));
}

/// All-0xFF input does not panic; fails to parse as valid INI text.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFF_u8; 64];
    assert!(MapRa2File::parse(&data).is_err());
}

/// All-zero input does not panic; parses as empty map.
#[test]
fn adversarial_all_zero() {
    let data = vec![0u8; 64];
    // NUL bytes are treated as empty text; parse succeeds with no useful data.
    let map = MapRa2File::parse(&data).unwrap();
    assert!(map.name().is_none());
}
