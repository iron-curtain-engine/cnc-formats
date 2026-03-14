// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Validation, error, security, and conversion tests for the MiniYAML module.
//! Split from `tests.rs` to stay within the ~600-line file-size cap.

use super::*;

// ── to_yaml conversion ──────────────────────────────────────────────────────

/// Conversion to YAML preserves key-value pairs.
///
/// Why: the converter must faithfully reproduce all parsed content.
#[test]
fn to_yaml_simple() {
    let input = "Key: Value\nOther: 42\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let yaml = to_yaml(&doc);
    assert!(yaml.contains("Key: Value"));
    assert!(yaml.contains("Other: 42"));
}

/// Conversion to YAML uses two-space indentation for children.
///
/// Why: standard YAML uses spaces, not tabs; the converter must translate
/// MiniYAML's tab nesting to space indentation.
#[test]
fn to_yaml_nesting() {
    let input = "Parent:\n\tChild: Value\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let yaml = to_yaml(&doc);
    assert!(yaml.contains("Parent:\n"));
    assert!(yaml.contains("  Child: Value\n"));
}

/// Values starting with `@` are quoted in YAML output.
///
/// Why: `@` is a YAML reserved character; unquoted values would produce
/// invalid YAML.
#[test]
fn to_yaml_quotes_at_values() {
    let input = "Inherits: @infantry\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let yaml = to_yaml(&doc);
    assert!(yaml.contains("'@infantry'"));
}

/// Boolean-like values are quoted in YAML output.
///
/// Why: YAML parsers interpret unquoted `true`/`false`/`yes`/`no` as
/// booleans; quoting preserves them as strings.
#[test]
fn to_yaml_quotes_booleans() {
    let input = "Enabled: true\nDisabled: false\nToggle: yes\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let yaml = to_yaml(&doc);
    assert!(yaml.contains("'true'"));
    assert!(yaml.contains("'false'"));
    assert!(yaml.contains("'yes'"));
}

// ── Error field & Display verification ───────────────────────────────────────

/// Invalid UTF-8 produces an `InvalidMagic` error with context.
///
/// Why: error variant must carry the correct context tag for diagnostics.
#[test]
fn parse_invalid_utf8() {
    let data: &[u8] = &[0x80, 0x81, 0x82];
    let err = MiniYamlDoc::parse(data).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(
                context.contains("UTF-8"),
                "context should mention UTF-8: {context}"
            );
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// Oversized input (> 16 MB) produces an `InvalidSize` error with correct fields.
///
/// Why: error variant must carry the exact value and limit for diagnostics.
#[test]
fn parse_oversized_input() {
    let data = vec![b'A'; MAX_INPUT_SIZE + 1];
    let err = MiniYamlDoc::parse(&data).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, MAX_INPUT_SIZE + 1);
            assert_eq!(limit, MAX_INPUT_SIZE);
            assert!(context.contains("file size"));
        }
        other => panic!("Expected InvalidSize, got {other:?}"),
    }
}

/// Display output for `InvalidMagic` contains the context string.
///
/// Why: AGENTS.md requires Display tests for every error variant.
#[test]
fn error_display_invalid_magic() {
    let err = Error::InvalidMagic {
        context: "MiniYAML file (invalid UTF-8)",
    };
    let msg = format!("{err}");
    assert!(msg.contains("UTF-8"), "should contain context: {msg}");
}

/// Display output for `InvalidSize` contains the numeric values.
///
/// Why: AGENTS.md requires Display tests embedding key numeric values.
#[test]
fn error_display_invalid_size() {
    let err = Error::InvalidSize {
        value: 200_000,
        limit: MAX_NODES,
        context: "MiniYAML node count",
    };
    let msg = format!("{err}");
    assert!(msg.contains("100000"), "should contain limit: {msg}");
    assert!(msg.contains("200000"), "should contain value: {msg}");
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice produces identical results.
#[test]
fn deterministic_parse() {
    let input = "A:\n\tB: 1\n\tC: 2\nD: 3\n";
    let doc1 = MiniYamlDoc::parse_str(input).unwrap();
    let doc2 = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc1, doc2);
}

/// Converting the same document to YAML twice produces identical output.
#[test]
fn deterministic_to_yaml() {
    let input = "A:\n\tB: 1\nC: 2\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let yaml1 = to_yaml(&doc);
    let yaml2 = to_yaml(&doc);
    assert_eq!(yaml1, yaml2);
}

// ── Security / adversarial (V38) ─────────────────────────────────────────────

/// `MiniYamlDoc::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer is invalid UTF-8, which exercises the
/// UTF-8 validation error path.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = MiniYamlDoc::parse(&data);
}

/// `MiniYamlDoc::parse` on 256 bytes of `0x00` must not panic.
///
/// Why (V38): an all-zero buffer is valid UTF-8 (null chars), which
/// exercises the empty-content path.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = MiniYamlDoc::parse(&data);
}

/// Deeply nested input at the maximum depth limit is rejected.
///
/// Why (V38): prevents stack overflow from unbounded recursion in
/// tree building.
#[test]
fn adversarial_excessive_nesting() {
    let mut input = String::new();
    for i in 0..MAX_DEPTH + 1 {
        let tabs = "\t".repeat(i);
        input.push_str(&format!("{tabs}Level{i}:\n"));
    }
    let err = MiniYamlDoc::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize { limit, context, .. } => {
            assert_eq!(limit, MAX_DEPTH);
            assert!(context.contains("nesting depth"));
        }
        other => panic!("Expected InvalidSize for depth, got {other:?}"),
    }
}

/// Input with many nodes at the limit is rejected.
///
/// Why (V38): prevents unbounded memory allocation from a file with
/// millions of trivial nodes.
#[test]
fn adversarial_excessive_nodes() {
    // Build input just past the limit.  Each line is a separate node.
    let line = "K: V\n";
    let input = line.repeat(MAX_NODES + 1);
    let err = MiniYamlDoc::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize { limit, context, .. } => {
            assert_eq!(limit, MAX_NODES);
            assert!(context.contains("node count"));
        }
        other => panic!("Expected InvalidSize for nodes, got {other:?}"),
    }
}

// ── Boundary tests ───────────────────────────────────────────────────────────

/// Input at exactly MAX_DEPTH - 1 nesting (last valid depth) is accepted.
///
/// Why (V38): boundary test — the cap must be inclusive at the last valid
/// indent level while rejecting the level above.
#[test]
fn nesting_at_max_depth_minus_one_accepted() {
    let mut input = String::new();
    for i in 0..MAX_DEPTH {
        let tabs = "\t".repeat(i);
        input.push_str(&format!("{tabs}Level{i}:\n"));
    }
    let doc = MiniYamlDoc::parse_str(&input).unwrap();
    assert_eq!(doc.nodes().len(), 1);
}

/// Input with exactly MAX_NODES nodes is accepted.
///
/// Why (V38): boundary test — exactly at the cap must succeed.
#[test]
fn node_count_at_max_accepted() {
    let line = "K: V\n";
    let input = line.repeat(MAX_NODES);
    let doc = MiniYamlDoc::parse_str(&input).unwrap();
    assert_eq!(doc.nodes().len(), MAX_NODES);
}

/// Input at exactly MAX_INPUT_SIZE bytes is accepted.
///
/// Why (V38): boundary test — exactly at the cap must succeed.
#[test]
fn input_at_max_size_accepted() {
    // Build a valid MiniYAML at max size by padding with comment lines.
    let header = "Key: Value\n";
    let padding = "# padding\n";
    let mut input = String::from(header);
    while input.len() + padding.len() <= MAX_INPUT_SIZE {
        input.push_str(padding);
    }
    assert!(input.len() <= MAX_INPUT_SIZE);
    let doc = MiniYamlDoc::parse_str(&input).unwrap();
    assert_eq!(doc.nodes().len(), 1);
}

/// Thousands of `#` comment lines must not panic.
#[test]
fn adversarial_many_comments() {
    let input = "# comment\n".repeat(10_000);
    let doc = MiniYamlDoc::parse_str(&input).unwrap();
    assert!(doc.nodes().is_empty());
}

/// A line consisting of many tabs with no content must not panic.
#[test]
fn adversarial_many_tabs() {
    let input = "\t".repeat(63) + "\n";
    let doc = MiniYamlDoc::parse_str(&input).unwrap();
    assert!(doc.nodes().is_empty());
}

/// Rapidly alternating indent levels (zigzag) must not panic or corrupt
/// the tree structure.
///
/// Why (V38): malformed input may alternate between deep and shallow
/// nesting across consecutive lines.  The tree-builder's orphan-skipping
/// and scope-closing logic must handle this without panicking or producing
/// an incorrect tree.
#[test]
fn adversarial_zigzag_nesting() {
    // 0, 5, 0, 5, 0 — the depth-5 lines are orphans with no parent at depth 1–4.
    let input = "A:\n\t\t\t\t\tDeep1: x\nB:\n\t\t\t\t\tDeep2: y\nC:\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    // The depth-5 orphans should be silently skipped; three root nodes remain.
    assert_eq!(doc.nodes().len(), 3);
    assert_eq!(doc.nodes()[0].key(), "A");
    assert_eq!(doc.nodes()[1].key(), "B");
    assert_eq!(doc.nodes()[2].key(), "C");
}

/// A single line consuming nearly the full 16 MB input cap must not panic.
///
/// Why (V38): a file with no newlines is a single huge line, producing one
/// key or value that may be megabytes long.  The parser must not panic or
/// exhibit pathological behaviour on extremely long lines.
#[test]
fn adversarial_very_long_line() {
    // Build a single key with a ~1 MB value (well within the 16 MB cap).
    let value = "x".repeat(1_000_000);
    let input = format!("K: {value}\n");
    let doc = MiniYamlDoc::parse_str(&input).unwrap();
    assert_eq!(doc.nodes().len(), 1);
    assert_eq!(doc.nodes()[0].value().unwrap().len(), 1_000_000);
}

/// `to_yaml` must quote values containing `#` to prevent YAML comment
/// injection.
///
/// Why: after `\#` → `#` unescaping, values can contain literal `#`.
/// Without quoting, a YAML parser interprets `#` as a comment start,
/// silently truncating the value — a data corruption vulnerability.
#[test]
fn to_yaml_quotes_hash_in_values() {
    let input = "Color: \\#FF0000\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("#FF0000"));

    let yaml = to_yaml(&doc);
    // The `#` must be inside quotes to prevent YAML comment interpretation.
    assert!(
        yaml.contains("'#FF0000'"),
        "value containing # must be quoted: {yaml}"
    );
}

/// `to_yaml` must quote values containing `: ` (colon-space).
///
/// Why: a `: ` in a YAML plain scalar can cause certain YAML parsers
/// to misinterpret the value as a mapping key.
#[test]
fn to_yaml_quotes_colon_space() {
    let input = "Key: host: port\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    // Value is everything after the first colon: "host: port"
    assert_eq!(doc.nodes()[0].value(), Some("host: port"));

    let yaml = to_yaml(&doc);
    assert!(
        yaml.contains("'host: port'"),
        "value containing `: ` must be quoted: {yaml}"
    );
}

/// `to_yaml` must quote values starting with `-`.
///
/// Why: `-` is the YAML sequence indicator; an unquoted value starting
/// with `-` could be misinterpreted as a list item.
#[test]
fn to_yaml_quotes_dash_prefix() {
    let input = "Key: -value\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let yaml = to_yaml(&doc);
    assert!(
        yaml.contains("'-value'"),
        "value starting with `-` must be quoted: {yaml}"
    );
}

// ── Space indentation rejection ──────────────────────────────────────────────

/// Space-indented children are rejected with an error, not silently misparsed.
///
/// Why: our parser is tab-only.  Silently treating space-indented lines as
/// depth 0 would produce a wrong tree without any indication of failure.
/// Erroring immediately gives the user a clear diagnostic.
#[test]
fn space_indentation_rejected() {
    let input = "Parent:\n    Child: val\n";
    let err = MiniYamlDoc::parse_str(input).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(
                context.contains("space indentation"),
                "should mention space indentation: {context}"
            );
        }
        other => panic!("Expected InvalidMagic (space indentation), got {other:?}"),
    }
}

/// A single leading space (not a full indent) is still detected.
///
/// Why: even one leading space indicates the file uses space indentation,
/// which our tab-only parser cannot interpret correctly.
#[test]
fn single_leading_space_rejected() {
    let input = "Key:\n Value: x\n";
    let err = MiniYamlDoc::parse_str(input).unwrap_err();
    match &err {
        Error::InvalidMagic { context } => {
            assert!(context.contains("space indentation"));
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// Tab-indented content after a root node continues to work normally.
///
/// Why: regression guard — the space-detection check must not interfere
/// with valid tab-indented input.
#[test]
fn tab_indentation_still_works() {
    let input = "Parent:\n\tChild: val\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].children()[0].value(), Some("val"));
}

/// A value containing spaces (not indentation) is fine.
///
/// Why: only *leading* spaces are rejected — spaces in key names, values,
/// or after tabs are perfectly valid MiniYAML.
#[test]
fn spaces_in_values_are_fine() {
    let input = "Key: value with spaces\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("value with spaces"));
}

/// Comment-only lines starting with spaces are tolerated because after
/// comment stripping the remaining content is empty (pure whitespace),
/// which is skipped as a blank line before the space-indentation check.
#[test]
fn space_indented_comment_tolerated() {
    // The `#` is stripped first, leaving `"    "` → empty after trim → skipped.
    let input = "Key: val\n    # indented comment\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 1);
    assert_eq!(doc.nodes()[0].value(), Some("val"));
}

// ── Realistic scenario ───────────────────────────────────────────────────────

/// Parse a realistic OpenRA-style unit definition.
#[test]
fn parse_realistic_unit() {
    let input = "\
Inherits: @infantry
Name: Rifle Infantry
Health:
\tHP: 100
\tRegenRate: 0
Mobile:
\tSpeed: 56
\tTurnSpeed: 20
Armament:
\tWeapon: M1Carbine
\tLocalOffset: 0,0,300
";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 5);

    // Check inheritance.
    assert_eq!(doc.node("Inherits").unwrap().value(), Some("@infantry"));

    // Check nested children.
    let health = doc.node("Health").unwrap();
    assert_eq!(health.child("HP").unwrap().value(), Some("100"));
    assert_eq!(health.child("RegenRate").unwrap().value(), Some("0"));

    let armament = doc.node("Armament").unwrap();
    assert_eq!(armament.child("Weapon").unwrap().value(), Some("M1Carbine"));
}

// ── Integer overflow safety ──────────────────────────────────────────

/// `saturating_add(1)` in the node-count error path reports the correct
/// value when the count is exactly at `MAX_NODES`.
///
/// Why: the error variant uses `entries.len().saturating_add(1)` to report
/// the attempted count.  At the cap boundary this must produce `MAX_NODES + 1`
/// (not wrap), documenting the invariant and catching regressions.
///
/// How: builds input with exactly `MAX_NODES + 1` root-level nodes.
/// The parser should reject the last node with `InvalidSize`.
#[test]
fn overflow_node_count_saturating_add_reports_correct_value() {
    // MAX_NODES = 100_000.  Build MAX_NODES + 1 root nodes.
    let mut input = String::with_capacity(12 * (super::MAX_NODES + 1));
    for i in 0..=super::MAX_NODES {
        input.push_str(&format!("K{i}: V\n"));
    }
    let err = MiniYamlDoc::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, super::MAX_NODES + 1);
            assert_eq!(limit, super::MAX_NODES);
            assert!(context.contains("node"), "context: {context}");
        }
        other => panic!("Expected InvalidSize, got: {other}"),
    }
}

/// Nesting depth at exactly `MAX_DEPTH` is rejected (the cap is exclusive).
///
/// Why: documents the exact boundary of the depth cap, which prevents
/// stack overflow from deeply recursive tree construction.
#[test]
fn overflow_depth_at_max_is_rejected() {
    // Build a line with exactly MAX_DEPTH leading tabs.
    let tabs = "\t".repeat(super::MAX_DEPTH);
    let input = format!("Root:\n{tabs}Deep: val\n");
    let err = MiniYamlDoc::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize { value, limit, .. } => {
            assert_eq!(value, super::MAX_DEPTH);
            assert_eq!(limit, super::MAX_DEPTH);
        }
        other => panic!("Expected InvalidSize for depth, got: {other}"),
    }
}
