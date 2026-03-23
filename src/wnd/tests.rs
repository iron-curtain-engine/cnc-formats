// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;
use crate::error::Error;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Builds a WND file byte vector with the given version and body content.
fn build_wnd(version: u32, windows: &str) -> Vec<u8> {
    format!("FILE_VERSION = {version};\n{windows}").into_bytes()
}

// ── Basic parsing ────────────────────────────────────────────────────────────

/// Parses a simple WND file with one window and a few properties.
///
/// Why: happy-path baseline for the parser.
#[test]
fn parse_valid_wnd() {
    let data = build_wnd(
        1,
        "\
WINDOW
  WINDOWTYPE = USER;
  NAME = \"MainWindow\";
  STATUS = ENABLED+IMAGE;
END
",
    );
    let wnd = WndFile::parse(&data).unwrap();
    assert_eq!(wnd.version, Some(1));
    assert_eq!(wnd.windows.len(), 1);

    let win = &wnd.windows[0];
    assert_eq!(win.window_type(), Some("USER"));
    assert_eq!(win.name(), Some("\"MainWindow\""));
    assert_eq!(win.get("STATUS"), Some("ENABLED+IMAGE"));
    assert_eq!(win.children.len(), 0);
}

/// Parses a window with nested children via CHILD/WINDOW/END/ENDALLCHILDREN.
///
/// Why: verifies the recursive nesting machinery with the real Generals format.
#[test]
fn parse_nested_children() {
    let data = build_wnd(
        1,
        "\
WINDOW
  WINDOWTYPE = USER;
  NAME = \"Parent\";
  CHILD
  WINDOW
    WINDOWTYPE = PUSHBUTTON;
    NAME = \"Child1\";
  END
  CHILD
  WINDOW
    WINDOWTYPE = STATICTEXT;
    NAME = \"Child2\";
  END
  ENDALLCHILDREN
END
",
    );
    let wnd = WndFile::parse(&data).unwrap();
    assert_eq!(wnd.windows.len(), 1);

    let parent = &wnd.windows[0];
    assert_eq!(parent.name(), Some("\"Parent\""));
    assert_eq!(parent.children.len(), 2);
    assert_eq!(parent.children[0].window_type(), Some("PUSHBUTTON"));
    assert_eq!(parent.children[0].name(), Some("\"Child1\""));
    assert_eq!(parent.children[1].window_type(), Some("STATICTEXT"));
    assert_eq!(parent.children[1].name(), Some("\"Child2\""));
}

/// Parses two top-level WINDOW blocks.
///
/// Why: while rare, multiple top-level windows are structurally valid.
#[test]
fn parse_multiple_windows() {
    let data = build_wnd(
        1,
        "\
WINDOW
  NAME = \"First\";
END
WINDOW
  NAME = \"Second\";
END
",
    );
    let wnd = WndFile::parse(&data).unwrap();
    assert_eq!(wnd.windows.len(), 2);
    assert_eq!(wnd.windows[0].name(), Some("\"First\""));
    assert_eq!(wnd.windows[1].name(), Some("\"Second\""));
}

// ── Property access ──────────────────────────────────────────────────────────

/// Verifies get(), window_type(), and name() accessors.
///
/// Why: tests the case-insensitive property lookup and convenience methods.
#[test]
fn property_access() {
    let data = build_wnd(
        1,
        "\
WINDOW
  WINDOWTYPE = COMBOBOX;
  NAME = \"MyCombo\";
  STYLE = USER;
  TOOLTIPDELAY = -1;
END
",
    );
    let wnd = WndFile::parse(&data).unwrap();
    let win = &wnd.windows[0];

    // Convenience accessors.
    assert_eq!(win.window_type(), Some("COMBOBOX"));
    assert_eq!(win.name(), Some("\"MyCombo\""));

    // Case-insensitive lookup.
    assert_eq!(win.get("style"), Some("USER"));
    assert_eq!(win.get("STYLE"), Some("USER"));
    assert_eq!(win.get("Style"), Some("USER"));

    // Numeric-ish value stays as string.
    assert_eq!(win.get("TOOLTIPDELAY"), Some("-1"));

    // Missing key.
    assert_eq!(win.get("NONEXISTENT"), None);
}

// ── window_count ─────────────────────────────────────────────────────────────

/// Verifies window_count() sums all windows recursively.
///
/// Why: window_count is the primary metric for layout complexity.
#[test]
fn window_count() {
    let data = build_wnd(
        1,
        "\
WINDOW
  NAME = \"Root\";
  CHILD
  WINDOW
    NAME = \"A\";
    CHILD
    WINDOW
      NAME = \"A1\";
    END
    ENDALLCHILDREN
  END
  CHILD
  WINDOW
    NAME = \"B\";
  END
  ENDALLCHILDREN
END
",
    );
    let wnd = WndFile::parse(&data).unwrap();
    // Root + A + A1 + B = 4
    assert_eq!(wnd.window_count(), 4);
}

// ── Version handling ─────────────────────────────────────────────────────────

/// File without FILE_VERSION line still parses; version is None.
///
/// Why: some WND files may omit the version header.
#[test]
fn parse_no_version() {
    let data = b"WINDOW\n  NAME = \"NoVersion\";\nEND\n";
    let wnd = WndFile::parse(data).unwrap();
    assert_eq!(wnd.version, None);
    assert_eq!(wnd.windows.len(), 1);
    assert_eq!(wnd.windows[0].name(), Some("\"NoVersion\""));
}

// ── Comment handling ─────────────────────────────────────────────────────────

/// Lines starting with `;` are treated as comments and skipped.
///
/// Why: WND files use `;` for comments; these must not interfere with
/// property parsing or block structure.
#[test]
fn parse_with_comments() {
    let data = build_wnd(
        1,
        "\
; This is a comment at the top level
WINDOW
  ; Comment inside a window
  WINDOWTYPE = USER;
  NAME = \"Commented\";
  ; Another comment
END
",
    );
    let wnd = WndFile::parse(&data).unwrap();
    assert_eq!(wnd.windows.len(), 1);
    assert_eq!(wnd.windows[0].window_type(), Some("USER"));
    assert_eq!(wnd.windows[0].name(), Some("\"Commented\""));
    // Comments must not appear as properties.
    assert_eq!(wnd.windows[0].properties.len(), 2);
}

// ── Error cases ──────────────────────────────────────────────────────────────

/// WINDOW without matching END produces an error.
///
/// Why: structural integrity check for unclosed blocks.
#[test]
fn reject_unclosed_window() {
    let data = build_wnd(
        1,
        "\
WINDOW
  NAME = \"Unclosed\";
",
    );
    let err = WndFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(
                context.contains("missing END"),
                "expected 'missing END' in context: {context}"
            );
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// ENDCHILD without a preceding CHILD produces an error.
///
/// Why: structural integrity check for mismatched nesting.
#[test]
fn reject_unexpected_endchild() {
    let data = build_wnd(
        1,
        "\
WINDOW
  NAME = \"Bad\";
  ENDCHILD
END
",
    );
    let err = WndFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(
                context.contains("unexpected ENDCHILD"),
                "expected 'unexpected ENDCHILD' in context: {context}"
            );
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// Input exceeding MAX_INPUT_SIZE is rejected.
///
/// Why: prevents unbounded memory allocation from oversized input.
#[test]
fn reject_too_large() {
    let data = vec![b' '; MAX_INPUT_SIZE + 1];
    let err = WndFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, MAX_INPUT_SIZE + 1);
            assert_eq!(limit, MAX_INPUT_SIZE);
            assert!(context.contains("file size"), "context: {context}");
        }
        other => panic!("Expected InvalidSize, got {other:?}"),
    }
}

/// Empty input is valid (no windows, no version).
///
/// Why: degenerate input must not panic.
#[test]
fn adversarial_empty() {
    let wnd = WndFile::parse(b"").unwrap();
    assert_eq!(wnd.version, None);
    assert!(wnd.windows.is_empty());
    assert_eq!(wnd.window_count(), 0);
}

// ── Additional structural tests ──────────────────────────────────────────────

/// Nested CHILD without ENDCHILD is rejected.
///
/// Why: double CHILD blocks without closing are invalid.
#[test]
fn reject_nested_child_without_endchild() {
    let data = build_wnd(
        1,
        "\
WINDOW
  CHILD
    CHILD
    ENDCHILD
  ENDCHILD
END
",
    );
    let err = WndFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(
                context.contains("nested CHILD"),
                "expected 'nested CHILD' in context: {context}"
            );
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// WINDOW outside of CHILD block within another WINDOW is rejected.
///
/// Why: structural integrity -- nested WINDOW must be inside CHILD.
#[test]
fn reject_window_outside_child() {
    let data = build_wnd(
        1,
        "\
WINDOW
  NAME = \"Parent\";
  WINDOW
    NAME = \"Orphan\";
  END
END
",
    );
    let err = WndFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(
                context.contains("without CHILD"),
                "expected 'without CHILD' in context: {context}"
            );
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// Complex multi-field property values are preserved as-is.
///
/// Why: values like SCREENRECT with sub-fields should be stored verbatim
/// for the engine layer to parse.
#[test]
fn complex_property_value_preserved() {
    let data = build_wnd(
        1,
        "\
WINDOW
  SCREENRECT = UPPERLEFT: 0 0, BOTTOMRIGHT: 800 600, CREATIONRESOLUTION: 800 600;
  FONT = NAME: \"Arial\" SIZE: 12 BOLD: 0;
END
",
    );
    let wnd = WndFile::parse(&data).unwrap();
    let win = &wnd.windows[0];
    assert_eq!(
        win.get("SCREENRECT"),
        Some("UPPERLEFT: 0 0, BOTTOMRIGHT: 800 600, CREATIONRESOLUTION: 800 600")
    );
    assert_eq!(win.get("FONT"), Some("NAME: \"Arial\" SIZE: 12 BOLD: 0"));
}

/// FILE_VERSION with different values is parsed correctly.
///
/// Why: exercises version extraction with non-1 values.
#[test]
fn file_version_different_values() {
    let data = b"FILE_VERSION = 2;\nWINDOW\n  NAME = \"V2\";\nEND\n";
    let wnd = WndFile::parse(data).unwrap();
    assert_eq!(wnd.version, Some(2));
}

/// Invalid UTF-8 is rejected with InvalidMagic.
///
/// Why: exercises the UTF-8 validation guard.
#[test]
fn reject_invalid_utf8() {
    let data = [0xFF, 0xFE, 0xFD];
    let err = WndFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidMagic { context } => {
            assert!(context.contains("UTF-8"), "context: {context}");
        }
        other => panic!("Expected InvalidMagic, got {other:?}"),
    }
}

/// Windows-style CRLF line endings are handled correctly.
///
/// Why: WND files are often created/edited on Windows.
#[test]
fn crlf_line_endings() {
    let data = b"FILE_VERSION = 1;\r\nWINDOW\r\n  NAME = \"CRLF\";\r\nEND\r\n";
    let wnd = WndFile::parse(data).unwrap();
    assert_eq!(wnd.version, Some(1));
    assert_eq!(wnd.windows[0].name(), Some("\"CRLF\""));
}

/// Parsing is deterministic: same input produces same output.
///
/// Why: parsers must be pure functions of their input.
#[test]
fn determinism() {
    let data = build_wnd(
        1,
        "\
WINDOW
  NAME = \"Det\";
  CHILD
  WINDOW
    NAME = \"C\";
  END
  ENDALLCHILDREN
END
",
    );
    let a = WndFile::parse(&data).unwrap();
    let b = WndFile::parse(&data).unwrap();
    assert_eq!(a.version, b.version);
    assert_eq!(a.window_count(), b.window_count());
    assert_eq!(a.windows[0].name(), b.windows[0].name());
    assert_eq!(
        a.windows[0].children[0].name(),
        b.windows[0].children[0].name()
    );
}

/// Error Display for InvalidMagic contains the context string.
///
/// Why: diagnostic output must embed context for debugging.
#[test]
fn error_display_invalid_magic() {
    let data = build_wnd(1, "WINDOW\n  NAME = \"Oops\";\n");
    let err = WndFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("WND"), "Display should mention WND: {msg}");
}

/// Error Display for InvalidSize contains the numeric limit.
///
/// Why: diagnostic output must embed the limit for debugging.
#[test]
fn error_display_invalid_size() {
    let err = Error::InvalidSize {
        value: MAX_INPUT_SIZE + 1,
        limit: MAX_INPUT_SIZE,
        context: "WND file size",
    };
    let msg = format!("{err}");
    assert!(
        msg.contains(&MAX_INPUT_SIZE.to_string()),
        "should contain limit: {msg}"
    );
}
