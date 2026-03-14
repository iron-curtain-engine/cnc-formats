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
