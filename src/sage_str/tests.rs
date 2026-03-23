// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

/// Builds a minimal STR file from a slice of `(id, value)` pairs.
fn build_str(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut out = String::new();
    for (id, value) in entries {
        out.push_str(id);
        out.push('\n');
        out.push('"');
        out.push_str(value);
        out.push('"');
        out.push('\n');
        out.push_str("END\n");
    }
    out.into_bytes()
}

#[test]
fn parse_valid_str() {
    let data = build_str(&[("HELLO", "Hello World"), ("BYE", "Goodbye")]);
    let file = StrFile::parse(&data).unwrap();

    assert_eq!(file.len(), 2);
    assert_eq!(file.entries()[0].id, "HELLO");
    assert_eq!(file.entries()[0].value, "Hello World");
    assert_eq!(file.entries()[1].id, "BYE");
    assert_eq!(file.entries()[1].value, "Goodbye");
}

#[test]
fn parse_single_entry() {
    let data = build_str(&[("ONLY_ONE", "Solo")]);
    let file = StrFile::parse(&data).unwrap();

    assert_eq!(file.len(), 1);
    assert_eq!(file.entries()[0].id, "ONLY_ONE");
    assert_eq!(file.entries()[0].value, "Solo");
}

#[test]
fn parse_with_comments() {
    let input = b"; This is a comment\n\
                   \n\
                   GREETING\n\
                   \"Hello\"\n\
                   END\n\
                   ; Another comment\n\
                   \n\
                   FAREWELL\n\
                   \"Bye\"\n\
                   END\n";
    let file = StrFile::parse(input).unwrap();

    assert_eq!(file.len(), 2);
    assert_eq!(file.entries()[0].id, "GREETING");
    assert_eq!(file.entries()[0].value, "Hello");
    assert_eq!(file.entries()[1].id, "FAREWELL");
    assert_eq!(file.entries()[1].value, "Bye");
}

#[test]
fn get_case_insensitive() {
    let data = build_str(&[("GUI:Button", "Click Me")]);
    let file = StrFile::parse(&data).unwrap();

    assert_eq!(file.get("gui:button"), Some("Click Me"));
    assert_eq!(file.get("GUI:BUTTON"), Some("Click Me"));
    assert_eq!(file.get("GUI:Button"), Some("Click Me"));
    assert_eq!(file.get("nonexistent"), None);
}

#[test]
fn parse_empty() {
    let file = StrFile::parse(b"").unwrap();
    assert!(file.is_empty());
    assert_eq!(file.len(), 0);
}

#[test]
fn parse_only_comments_and_blanks() {
    let input = b"; just a comment\n\n; another\n\n";
    let file = StrFile::parse(input).unwrap();
    assert!(file.is_empty());
}

#[test]
fn parse_colon_identifier() {
    let data = build_str(&[("GUI:SomeButton", "Press Here")]);
    let file = StrFile::parse(&data).unwrap();

    assert_eq!(file.entries()[0].id, "GUI:SomeButton");
    assert_eq!(file.entries()[0].value, "Press Here");
}

#[test]
fn reject_missing_end() {
    let input = b"MY_STRING\n\"Some value\"\n";
    let err = StrFile::parse(input).unwrap_err();
    assert!(
        matches!(err, Error::UnexpectedEof { .. }),
        "expected UnexpectedEof, got {err:?}"
    );
}

#[test]
fn reject_missing_quotes() {
    let input = b"MY_STRING\nSome value without quotes\nEND\n";
    let err = StrFile::parse(input).unwrap_err();
    assert!(
        matches!(
            err,
            Error::InvalidMagic {
                context: "STR string entry (value must be double-quoted)"
            }
        ),
        "expected InvalidMagic for missing quotes, got {err:?}"
    );
}

#[test]
fn reject_non_utf8() {
    let input: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x01];
    let err = StrFile::parse(&input).unwrap_err();
    assert!(
        matches!(
            err,
            Error::InvalidMagic {
                context: "STR file encoding (expected UTF-8)"
            }
        ),
        "expected InvalidMagic for non-UTF-8, got {err:?}"
    );
}

#[test]
fn reject_too_large() {
    let input = vec![b' '; 16 * 1024 * 1024 + 1];
    let err = StrFile::parse(&input).unwrap_err();
    assert!(
        matches!(
            err,
            Error::InvalidSize {
                context: "STR input size",
                ..
            }
        ),
        "expected InvalidSize for oversized input, got {err:?}"
    );
}

/// All-0xFF input does not panic; rejected as non-UTF-8.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFF_u8; 64];
    assert!(StrFile::parse(&data).is_err());
}

/// All-zero input does not panic; NUL bytes are not valid STR text.
#[test]
fn adversarial_all_zero() {
    let data = vec![0u8; 64];
    assert!(StrFile::parse(&data).is_err());
}
