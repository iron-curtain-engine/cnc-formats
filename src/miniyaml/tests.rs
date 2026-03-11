// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Basic functionality ──────────────────────────────────────────────────────

/// Parsing an empty string produces an empty document.
///
/// Why: degenerate input must not cause errors or produce phantom nodes.
#[test]
fn parse_empty_string() {
    let doc = MiniYamlDoc::parse_str("").unwrap();
    assert!(doc.nodes().is_empty());
}

/// Parsing a single `Key: Value` pair produces one root node.
///
/// Why: happy-path baseline — if this fails, nothing works.
#[test]
fn parse_single_key_value() {
    let doc = MiniYamlDoc::parse_str("Name: TestValue\n").unwrap();
    assert_eq!(doc.nodes().len(), 1);
    let node = &doc.nodes()[0];
    assert_eq!(node.key(), "Name");
    assert_eq!(node.value(), Some("TestValue"));
    assert!(node.children().is_empty());
}

/// Parsing a bare `Key:` (no value) produces a node with `None` value.
///
/// Why: bare keys are common in MiniYAML for parent nodes with only children.
#[test]
fn parse_bare_key() {
    let doc = MiniYamlDoc::parse_str("Section:\n").unwrap();
    assert_eq!(doc.nodes()[0].key(), "Section");
    assert_eq!(doc.nodes()[0].value(), None);
}

/// Parsing a key without a colon produces a bare key with no value.
///
/// This supports removal markers like `^NodeName`.
#[test]
fn parse_bare_key_no_colon() {
    let doc = MiniYamlDoc::parse_str("^RemoveMe\n").unwrap();
    assert_eq!(doc.nodes()[0].key(), "^RemoveMe");
    assert_eq!(doc.nodes()[0].value(), None);
}

/// Multiple root-level nodes are parsed in order.
///
/// Why: insertion order must be preserved for deterministic output.
#[test]
fn parse_multiple_roots() {
    let input = "A: 1\nB: 2\nC: 3\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 3);
    assert_eq!(doc.nodes()[0].key(), "A");
    assert_eq!(doc.nodes()[1].key(), "B");
    assert_eq!(doc.nodes()[2].key(), "C");
}

// ── Indentation / tree structure ─────────────────────────────────────────────

/// Tab-indented children are attached to their parent node.
///
/// Why: indentation-based nesting is the core MiniYAML feature.
#[test]
fn parse_children() {
    let input = "Parent:\n\tChild1: val1\n\tChild2: val2\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 1);
    let parent = &doc.nodes()[0];
    assert_eq!(parent.children().len(), 2);
    assert_eq!(parent.children()[0].key(), "Child1");
    assert_eq!(parent.children()[0].value(), Some("val1"));
    assert_eq!(parent.children()[1].key(), "Child2");
}

/// Deep nesting with multiple tab levels works correctly.
///
/// Why: multi-level nesting is common in OpenRA trait definitions.
#[test]
fn parse_deep_nesting() {
    let input = "L0:\n\tL1:\n\t\tL2:\n\t\t\tL3: deep\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let l3 = &doc.nodes()[0].children()[0].children()[0].children()[0];
    assert_eq!(l3.key(), "L3");
    assert_eq!(l3.value(), Some("deep"));
}

/// Siblings at the same depth after a nested section are handled correctly.
///
/// Why: verifies the tree-builder correctly closes nested sections and
/// returns to the parent depth.
#[test]
fn parse_siblings_after_nesting() {
    let input = "A:\n\tChild: 1\nB: 2\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 2);
    assert_eq!(doc.nodes()[0].key(), "A");
    assert_eq!(doc.nodes()[0].children().len(), 1);
    assert_eq!(doc.nodes()[1].key(), "B");
    assert_eq!(doc.nodes()[1].value(), Some("2"));
}

/// Blank lines between nested children don't break the tree structure.
///
/// Why: OpenRA files commonly have blank lines between sections for
/// readability.  These must not disrupt parent-child relationships.
#[test]
fn parse_blank_lines_in_nesting() {
    let input = "Root:\n\tChild1: a\n\n\tChild2: b\n\n\t\tGrand: c\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 1);
    let root = &doc.nodes()[0];
    assert_eq!(root.children().len(), 2);
    assert_eq!(root.children()[0].key(), "Child1");
    assert_eq!(root.children()[1].key(), "Child2");
    assert_eq!(root.children()[1].children().len(), 1);
    assert_eq!(root.children()[1].children()[0].key(), "Grand");
}

// ── Comments ─────────────────────────────────────────────────────────────────

/// Full-line comments (starting with `#`) are ignored.
///
/// Why: comment handling is a core format feature; failure to strip comments
/// would create phantom nodes.
#[test]
fn parse_full_line_comments() {
    let input = "# This is a comment\nKey: Value\n# Another comment\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 1);
    assert_eq!(doc.nodes()[0].key(), "Key");
}

/// Inline comments (after `#`) strip the comment portion.
///
/// Why: inline comments must not leak into node values.
#[test]
fn parse_inline_comments() {
    let input = "Key: Value # this is a comment\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("Value"));
}

/// Inline comment without a preceding space still strips correctly.
///
/// Why: the `#` acts as comment separator regardless of surrounding spaces.
/// OpenRA treats `key:value#comment` as value="value".
#[test]
fn parse_inline_comment_no_space() {
    let input = "Key: value#comment\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("value"));
}

/// Escaped hash `\#` in a value is preserved as a literal `#`.
///
/// Why: OpenRA MiniYAML uses `\#` to embed literal `#` characters in values.
/// Without this, `key: color #FF0000` would be truncated at the `#`.
///
/// How: the parser finds the first *unescaped* `#` for comment stripping,
/// then unescapes `\#` → `#` in the resulting value.
#[test]
fn parse_escaped_hash_in_value() {
    let input = "Key: before \\# after\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("before # after"));
}

/// Escaped hash followed by an actual comment strips correctly.
///
/// Why: the first *unescaped* `#` starts the comment; escaped ones are
/// preserved in the value.
#[test]
fn parse_escaped_hash_with_trailing_comment() {
    let input = "Key: has \\# hash # real comment\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("has # hash"));
}

/// Multiple escaped hashes in a single value are all unescaped.
///
/// Why: values may contain multiple `#` characters (e.g., hex colors).
#[test]
fn parse_multiple_escaped_hashes() {
    let input = "Colors: \\#FF0000 and \\#00FF00\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("#FF0000 and #00FF00"));
}

/// A value with only `\#` and no unescaped hash preserves the literal `#`.
///
/// Why: when no real comment exists, the entire value (with escaped hashes)
/// must be preserved.
#[test]
fn parse_escaped_hash_no_comment() {
    let input = "Hex: \\#AABBCC\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("#AABBCC"));
}

/// `key:#` (hash immediately after colon) produces a bare key with no value.
///
/// Why: the `#` starts a comment even without a space separator, leaving
/// no value text after the colon.
#[test]
fn parse_hash_immediately_after_colon() {
    let input = "Key:#\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].key(), "Key");
    assert_eq!(doc.nodes()[0].value(), None);
}

// ── OpenRA-specific features ─────────────────────────────────────────────────

/// `Inherits: @parent` is parsed as a normal key-value pair.
///
/// The inheritance resolution is the engine's responsibility, not the parser's.
#[test]
fn parse_inherits() {
    let input = "Inherits: @infantry\nName: Rifle\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.node("Inherits").unwrap().value(), Some("@infantry"));
}

/// `^RemovalMarker` nodes are parsed as bare keys.
///
/// The `^` prefix signals node removal during inheritance merge —
/// this is resolved by the engine, not the parser.
#[test]
fn parse_removal_marker() {
    let input = "^Buildable\n^Tooltip\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 2);
    assert_eq!(doc.nodes()[0].key(), "^Buildable");
    assert_eq!(doc.nodes()[1].key(), "^Tooltip");
}

/// `-Key:` removal prefix is parsed as a key starting with `-`.
///
/// Why: OpenRA uses `-Key:` to remove an inherited child during merge.
/// The parser preserves the `-` prefix as part of the key; the engine
/// performs the actual removal at merge time.
#[test]
fn parse_removal_prefix_dash() {
    let input = "Parent:\n\t-Child:\n\tOther: val\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let parent = &doc.nodes()[0];
    assert_eq!(parent.children().len(), 2);
    assert_eq!(parent.children()[0].key(), "-Child");
    assert_eq!(parent.children()[0].value(), None);
    assert_eq!(parent.children()[1].key(), "Other");
}

/// `-Key` without a colon is also parsed (bare removal marker).
///
/// Why: some OpenRA files use `-Key` without a trailing colon for removal.
#[test]
fn parse_removal_prefix_dash_no_colon() {
    let input = "Parent:\n\t-MockA2\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].children()[0].key(), "-MockA2");
    assert_eq!(doc.nodes()[0].children()[0].value(), None);
}

/// `@` suffixed keys (template names) are parsed as normal keys.
#[test]
fn parse_at_suffix_keys() {
    let input = "@infantry:\n\tHealth: 100\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].key(), "@infantry");
    assert_eq!(doc.nodes()[0].children()[0].value(), Some("100"));
}

/// `Inherits@suffix:` keyed inheritance is parsed with the `@suffix` as
/// part of the key.
///
/// Why: OpenRA uses `Inherits@a: ^Base` to reference multiple parents.
/// The `@a` suffix disambiguates them.  The parser treats the full
/// `Inherits@a` as the key name.
#[test]
fn parse_keyed_inheritance_suffix() {
    let input = "Actor:\n\tInherits@a: ^BaseA\n\tInherits@b: ^BaseB\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let actor = &doc.nodes()[0];
    assert_eq!(actor.children().len(), 2);
    assert_eq!(actor.children()[0].key(), "Inherits@a");
    assert_eq!(actor.children()[0].value(), Some("^BaseA"));
    assert_eq!(actor.children()[1].key(), "Inherits@b");
    assert_eq!(actor.children()[1].value(), Some("^BaseB"));
}

// ── Lookup ───────────────────────────────────────────────────────────────────

/// `node()` looks up a top-level node by exact key.
///
/// Why: lookup API must return correct results and `None` for missing keys.
#[test]
fn lookup_by_key() {
    let input = "Weapons:\n\tRifle: 10\nArmor: 5\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert!(doc.node("Weapons").is_some());
    assert!(doc.node("Armor").is_some());
    assert!(doc.node("Missing").is_none());
}

/// `child()` looks up a direct child node by exact key.
///
/// Why: child lookup is the primary tree-navigation API.
#[test]
fn lookup_child_by_key() {
    let input = "Parent:\n\tChild1: A\n\tChild2: B\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    let parent = doc.node("Parent").unwrap();
    assert_eq!(parent.child("Child1").unwrap().value(), Some("A"));
    assert_eq!(parent.child("Child2").unwrap().value(), Some("B"));
    assert!(parent.child("Child3").is_none());
}

/// Lookup is case-sensitive (matching OpenRA behavior).
#[test]
fn lookup_is_case_sensitive() {
    let input = "Key: Value\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert!(doc.node("Key").is_some());
    assert!(doc.node("key").is_none());
    assert!(doc.node("KEY").is_none());
}

// ── Whitespace handling ──────────────────────────────────────────────────────

/// Blank lines are ignored.
///
/// Why: blank lines are common separators in MiniYAML files; they must
/// not create empty nodes.
#[test]
fn parse_blank_lines() {
    let input = "A: 1\n\n\nB: 2\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 2);
}

/// Trailing whitespace on values is trimmed.
///
/// Why: trailing spaces are invisible and would cause subtle matching bugs.
#[test]
fn parse_trims_whitespace() {
    let input = "Key:   spaced value   \n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("spaced value"));
}

/// Windows-style CRLF line endings are handled.
///
/// Why: files edited on Windows may have CRLF; the parser must not corrupt
/// values with trailing `\r`.
#[test]
fn parse_crlf() {
    let input = "A: 1\r\nB: 2\r\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes().len(), 2);
    assert_eq!(doc.nodes()[0].value(), Some("1"));
}

// ── Edge cases ───────────────────────────────────────────────────────────────

/// Values containing colons (e.g., paths, URLs) are preserved.
///
/// Why: only the first colon splits key from value; colons in values must
/// not cause additional splitting.
#[test]
fn parse_value_with_colons() {
    let input = "Path: C:\\game\\assets\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert_eq!(doc.nodes()[0].value(), Some("C:\\game\\assets"));
}

/// Comment-only input produces an empty document.
///
/// Why: a file with only comments is valid and must produce an empty result.
#[test]
fn parse_only_comments() {
    let input = "# comment 1\n# comment 2\n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert!(doc.nodes().is_empty());
}

/// Whitespace-only input produces an empty document.
///
/// Why: whitespace-only input is a degenerate case that must parse cleanly.
#[test]
fn parse_whitespace_only() {
    let input = "   \n\t\n  \t  \n";
    let doc = MiniYamlDoc::parse_str(input).unwrap();
    assert!(doc.nodes().is_empty());
}

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
