// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Basic functionality ──────────────────────────────────────────────────────

/// Parses a minimal INI file with one section and one key.
///
/// Why: happy-path baseline — if this fails, nothing works.
#[test]
fn parse_single_section_single_key() {
    let input = b"[General]\nName=test\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.section_count(), 1);
    assert_eq!(ini.get("General", "Name"), Some("test"));
}

/// Parses multiple sections with multiple keys.
///
/// Why: exercises multi-section parsing, the primary real-world use case.
#[test]
fn parse_multiple_sections() {
    let input = b"[General]\nName=test\n\n[Combat]\nDamage=100\nRange=5\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.section_count(), 2);
    assert_eq!(ini.get("General", "Name"), Some("test"));
    assert_eq!(ini.get("Combat", "Damage"), Some("100"));
    assert_eq!(ini.get("Combat", "Range"), Some("5"));
}

/// Empty input produces zero sections.
///
/// Why: degenerate input must not cause errors or produce phantom sections.
#[test]
fn parse_empty_input() {
    let ini = IniFile::parse(b"").unwrap();
    assert_eq!(ini.section_count(), 0);
}

/// Whitespace-only input produces zero sections.
///
/// Why: whitespace-only input is a realistic edge case (empty files with
/// trailing newlines) that must parse cleanly.
#[test]
fn parse_whitespace_only() {
    let ini = IniFile::parse(b"  \n  \n  \n").unwrap();
    assert_eq!(ini.section_count(), 0);
}

/// Comments-only input produces zero sections.
///
/// Why: a file with only comments is valid and must produce an empty result.
#[test]
fn parse_comments_only() {
    let ini = IniFile::parse(b"; this is a comment\n; another one\n").unwrap();
    assert_eq!(ini.section_count(), 0);
}

/// `parse_str` works identically to `parse` with valid UTF-8.
///
/// Why: both APIs must produce identical results for the same logical input.
#[test]
fn parse_str_equivalent() {
    let text = "[Section]\nKey=Value\n";
    let ini = IniFile::parse_str(text).unwrap();
    assert_eq!(ini.get("Section", "Key"), Some("Value"));
}

// ── Case-insensitive matching ────────────────────────────────────────────────

/// Section lookup is case-insensitive (matching original Win32 behaviour).
#[test]
fn section_lookup_case_insensitive() {
    let input = b"[General]\nName=test\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("general", "Name"), Some("test"));
    assert_eq!(ini.get("GENERAL", "Name"), Some("test"));
    assert_eq!(ini.get("General", "Name"), Some("test"));
}

/// Key lookup is case-insensitive.
#[test]
fn key_lookup_case_insensitive() {
    let input = b"[Section]\nMyKey=hello\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("Section", "mykey"), Some("hello"));
    assert_eq!(ini.get("Section", "MYKEY"), Some("hello"));
    assert_eq!(ini.get("Section", "MyKey"), Some("hello"));
}

// ── Comment handling ─────────────────────────────────────────────────────────

/// Semicolon-prefixed lines are ignored.
///
/// Why: comment handling is a core format feature; malformed comment
/// stripping could leak data into keys or values.
#[test]
fn full_line_comment() {
    let input = b"[S]\n; Comment\nKey=Value\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Key"), Some("Value"));
    // The comment should not appear as a key.
    assert_eq!(ini.section("S").unwrap().len(), 1);
}

/// Inline comments after a value are stripped.
///
/// Why: the original game strips inline comments; failure to do so would
/// include `;`-suffixed garbage in values.
#[test]
fn inline_comment() {
    let input = b"[S]\nKey=Value ; this is a comment\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Key"), Some("Value"));
}

// ── Whitespace handling ──────────────────────────────────────────────────────

/// Whitespace around keys and values is trimmed.
///
/// Why: the original game ignores surrounding whitespace; callers expect
/// clean values without leading/trailing spaces.
#[test]
fn whitespace_trimming() {
    let input = b"[S]\n  Key  =  Value  \n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Key"), Some("Value"));
}

/// Whitespace inside section name brackets is trimmed.
///
/// Why: `[ Section ]` must match `[Section]`.
#[test]
fn section_name_whitespace() {
    let input = b"[  Section Name  ]\nKey=Value\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("Section Name", "Key"), Some("Value"));
}

// ── Duplicate handling ───────────────────────────────────────────────────────

/// Duplicate sections are merged (keys from later occurrences override).
///
/// Why: matches original game's sequential-read behaviour where the last
/// value wins.
#[test]
fn duplicate_sections_merged() {
    let input = b"[S]\nA=1\n[S]\nB=2\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.section_count(), 1);
    assert_eq!(ini.get("S", "A"), Some("1"));
    assert_eq!(ini.get("S", "B"), Some("2"));
}

/// Duplicate keys: last value wins.
///
/// Why: matches original game's sequential-read behaviour.
#[test]
fn duplicate_keys_last_wins() {
    let input = b"[S]\nKey=first\nKey=second\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Key"), Some("second"));
}

// ── Edge cases ───────────────────────────────────────────────────────────────

/// Empty value (`Key=`) is stored as empty string.
///
/// Why: empty values are valid in C&C INI files (e.g., clearing a
/// property); they must not be confused with missing keys.
#[test]
fn empty_value() {
    let input = b"[S]\nKey=\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Key"), Some(""));
}

/// Value containing `=` is preserved (only first `=` splits key from value).
///
/// Why: values like `Formula=a=b+c` are valid; splitting on every `=`
/// would corrupt them.
#[test]
fn value_with_equals() {
    let input = b"[S]\nFormula=a=b+c\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Formula"), Some("a=b+c"));
}

/// Key-value lines before any section header are silently ignored.
///
/// Why: matches original game's permissive parsing — orphan keys are
/// discarded, not treated as errors.
#[test]
fn keys_before_section_ignored() {
    let input = b"Orphan=Value\n[S]\nKey=Value\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.section_count(), 1);
    assert_eq!(ini.get("S", "Key"), Some("Value"));
}

/// Lines without `=` inside a section are silently ignored.
///
/// Why: the original game ignores non-key-value lines; strict rejection
/// would break modded INI files that include bare text.
#[test]
fn lines_without_equals_ignored() {
    let input = b"[S]\nNotAKeyValue\nKey=Value\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.section("S").unwrap().len(), 1);
    assert_eq!(ini.get("S", "Key"), Some("Value"));
}

/// Missing section returns `None`.
///
/// Why: callers must be able to safely query non-existent sections.
#[test]
fn nonexistent_section() {
    let input = b"[S]\nKey=Value\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("Missing", "Key"), None);
}

/// Missing key returns `None`.
///
/// Why: callers must be able to safely query non-existent keys.
#[test]
fn nonexistent_key() {
    let input = b"[S]\nKey=Value\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Missing"), None);
}

/// Section and key iteration preserves insertion order.
///
/// Why: deterministic iteration order is required for reproducible output;
/// insertion order matches the original file layout.
#[test]
fn iteration_order() {
    let input = b"[B]\nZ=1\nA=2\n[A]\nM=3\n";
    let ini = IniFile::parse(input).unwrap();

    let section_names: Vec<&str> = ini.sections().map(|s| s.name()).collect();
    assert_eq!(section_names, &["B", "A"]);

    let b_keys: Vec<(&str, &str)> = ini.section("B").unwrap().iter().collect();
    assert_eq!(b_keys, &[("Z", "1"), ("A", "2")]);
}

/// `IniSection::is_empty` works correctly.
///
/// Why: `is_empty()` is a public API; testing it prevents regressions.
#[test]
fn section_is_empty() {
    let input = b"[Empty]\n[HasKey]\nK=V\n";
    let ini = IniFile::parse(input).unwrap();
    assert!(ini.section("Empty").unwrap().is_empty());
    assert!(!ini.section("HasKey").unwrap().is_empty());
}

/// Windows-style `\r\n` line endings are handled correctly.
///
/// Why: game files may have been edited on Windows; CRLF must not
/// corrupt section names or key-value parsing.
#[test]
fn crlf_line_endings() {
    let input = b"[S]\r\nKey=Value\r\n";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.get("S", "Key"), Some("Value"));
}

// ── Error field & Display verification ───────────────────────────────────────

/// Invalid UTF-8 input produces `InvalidMagic`.
#[test]
fn invalid_utf8_error() {
    let data = [0xFF, 0xFE, 0xFD];
    let err = IniFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(context.contains("UTF-8"));
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// Error Display for InvalidMagic contains the context string.
#[test]
fn error_display_invalid_magic() {
    let data = [0xFF, 0xFE];
    let err = IniFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("UTF-8"));
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
///
/// Why: parsers must be pure functions of their input per AGENTS.md.
#[test]
fn determinism() {
    let input = b"[A]\nX=1\n[B]\nY=2\n";
    let a = IniFile::parse(input).unwrap();
    let b = IniFile::parse(input).unwrap();
    assert_eq!(a.section_count(), b.section_count());
    assert_eq!(a.get("A", "X"), b.get("A", "X"));
    assert_eq!(a.get("B", "Y"), b.get("B", "Y"));
}

// ── Boundary tests ───────────────────────────────────────────────────────────

/// Input at exactly MAX_INPUT_SIZE is accepted.
#[test]
fn input_at_max_size_accepted() {
    // Build a valid INI at max size by padding with comment lines.
    let header = "[S]\nK=V\n";
    let padding_line = "; padding\n";
    let mut input = String::from(header);
    while input.len() + padding_line.len() <= MAX_INPUT_SIZE {
        input.push_str(padding_line);
    }
    // Ensure we're within limit.
    assert!(input.len() <= MAX_INPUT_SIZE);
    let ini = IniFile::parse_str(&input).unwrap();
    assert_eq!(ini.get("S", "K"), Some("V"));
}

/// Input exceeding MAX_INPUT_SIZE is rejected.
///
/// Why (V38): prevents unbounded memory allocation from maliciously large files.
#[test]
fn input_over_max_size_rejected() {
    let data = vec![b' '; MAX_INPUT_SIZE + 1];
    let err = IniFile::parse(&data).unwrap_err();
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

/// Input with exactly MAX_SECTIONS sections is accepted.
///
/// Why (V38): boundary test — the cap must be inclusive, accepting exactly
/// the limit while rejecting limit + 1.
#[test]
fn section_count_at_max_accepted() {
    let mut input = String::new();
    for i in 0..MAX_SECTIONS {
        input.push_str(&format!("[S{i}]\n"));
    }
    let ini = IniFile::parse_str(&input).unwrap();
    assert_eq!(ini.section_count(), MAX_SECTIONS);
}

/// Input exceeding MAX_SECTIONS is rejected.
///
/// Why (V38): prevents unbounded allocation from a file with millions
/// of section headers.
#[test]
fn section_count_over_max_rejected() {
    let mut input = String::new();
    for i in 0..MAX_SECTIONS + 1 {
        input.push_str(&format!("[S{i}]\n"));
    }
    let err = IniFile::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize { limit, context, .. } => {
            assert_eq!(limit, MAX_SECTIONS);
            assert!(context.contains("section count"));
        }
        other => panic!("Expected InvalidSize, got {other:?}"),
    }
}

/// Section with exactly MAX_KEYS_PER_SECTION keys is accepted.
///
/// Why (V38): boundary test — the cap must be inclusive.
#[test]
fn keys_per_section_at_max_accepted() {
    let mut input = String::from("[S]\n");
    for i in 0..MAX_KEYS_PER_SECTION {
        input.push_str(&format!("K{i}=V\n"));
    }
    let ini = IniFile::parse_str(&input).unwrap();
    assert_eq!(ini.section("S").unwrap().len(), MAX_KEYS_PER_SECTION);
}

/// Section exceeding MAX_KEYS_PER_SECTION is rejected.
///
/// Why (V38): prevents unbounded allocation from a section with millions
/// of key-value pairs.
#[test]
fn keys_per_section_over_max_rejected() {
    let mut input = String::from("[S]\n");
    for i in 0..MAX_KEYS_PER_SECTION + 1 {
        input.push_str(&format!("K{i}=V\n"));
    }
    let err = IniFile::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize { limit, context, .. } => {
            assert_eq!(limit, MAX_KEYS_PER_SECTION);
            assert!(context.contains("keys per section"));
        }
        other => panic!("Expected InvalidSize, got {other:?}"),
    }
}

/// Display output for `InvalidSize` contains the numeric limit.
///
/// Why: AGENTS.md requires Display tests for every error variant a module
/// can produce; the message must embed key numeric values for diagnostics.
#[test]
fn error_display_invalid_size() {
    let err = Error::InvalidSize {
        value: 20_000,
        limit: MAX_SECTIONS,
        context: "INI section count",
    };
    let msg = format!("{err}");
    assert!(msg.contains("16384"), "should contain limit: {msg}");
    assert!(msg.contains("20000"), "should contain value: {msg}");
}

// ── Security edge-case tests ─────────────────────────────────────────────────

/// All-`0xFF` input: invalid UTF-8, must not panic.
///
/// Why (V38): exercises the UTF-8 validation guard.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = IniFile::parse(&data);
}

/// All-zero input: valid UTF-8 (NUL bytes), must not panic.
///
/// Why (V38): exercises the parser with degenerate all-NUL content.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = IniFile::parse(&data);
}

/// Input consisting entirely of section headers.
///
/// Why (V38): tests the section count cap path without any key-value pairs.
#[test]
fn adversarial_many_empty_sections() {
    let mut input = String::new();
    for i in 0..100 {
        input.push_str(&format!("[Section{i}]\n"));
    }
    let ini = IniFile::parse_str(&input).unwrap();
    assert_eq!(ini.section_count(), 100);
}

/// Deeply nested brackets: `[[[...]]]` — must not panic.
///
/// Why (V38): malformed section headers should be handled gracefully.
#[test]
fn adversarial_nested_brackets_no_panic() {
    let input = b"[[[Nested]]]\nKey=Value\n";
    let _ = IniFile::parse(input);
}

/// Very long line: single key-value pair at 1 MB.
///
/// Why (V38): ensures no per-line allocation bomb.
#[test]
fn adversarial_long_line_no_panic() {
    let value = "x".repeat(1_000_000);
    let input = format!("[S]\nKey={value}\n");
    let ini = IniFile::parse_str(&input).unwrap();
    assert_eq!(ini.get("S", "Key").unwrap().len(), 1_000_000);
}

// ── Integer overflow safety ──────────────────────────────────────────

/// `saturating_add(1)` in the section-count error path reports the correct
/// value when the count is exactly at `MAX_SECTIONS`.
///
/// Why: the error variant uses `sections.len().saturating_add(1)` to report
/// the attempted count.  At the cap boundary this must not wrap (it can't
/// because `MAX_SECTIONS` is far below `usize::MAX`, but the test documents
/// the invariant and catches regressions).
///
/// How: builds an input with exactly `MAX_SECTIONS + 1` unique sections.
/// The parser should reject the last section with `InvalidSize`.
#[test]
fn overflow_section_count_saturating_add_reports_correct_value() {
    let mut input = String::new();
    for i in 0..=MAX_SECTIONS {
        input.push_str(&format!("[S{i}]\n"));
    }
    let err = IniFile::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, MAX_SECTIONS + 1);
            assert_eq!(limit, MAX_SECTIONS);
            assert!(context.contains("section"), "context: {context}");
        }
        other => panic!("Expected InvalidSize, got: {other}"),
    }
}

/// `saturating_add(1)` in the key-count error path reports the correct
/// value when the count is exactly at `MAX_KEYS_PER_SECTION`.
///
/// Why: same rationale as the section-count test — documents that the
/// error payload is accurate at the rejection boundary.
///
/// How: builds one section with `MAX_KEYS_PER_SECTION + 1` unique keys.
#[test]
fn overflow_key_count_saturating_add_reports_correct_value() {
    let mut input = String::from("[S]\n");
    for i in 0..=MAX_KEYS_PER_SECTION {
        input.push_str(&format!("K{i}=V\n"));
    }
    let err = IniFile::parse_str(&input).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, MAX_KEYS_PER_SECTION + 1);
            assert_eq!(limit, MAX_KEYS_PER_SECTION);
            assert!(context.contains("key"), "context: {context}");
        }
        other => panic!("Expected InvalidSize, got: {other}"),
    }
}
