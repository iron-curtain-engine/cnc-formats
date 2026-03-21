// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026-present Iron Curtain contributors

use super::*;

/// Helper: Build a minimal valid CSF file with the given labels.
#[allow(clippy::type_complexity)]
pub(crate) fn build_csf(labels: &[(&str, &[(&str, Option<&str>)])]) -> Vec<u8> {
    let mut data = Vec::new();
    // Header
    data.extend_from_slice(b" FSC"); // magic
    data.extend_from_slice(&3u32.to_le_bytes()); // version
    data.extend_from_slice(&(labels.len() as u32).to_le_bytes()); // num_labels
    let total_strings: usize = labels.iter().map(|(_, strs)| strs.len()).sum();
    data.extend_from_slice(&(total_strings as u32).to_le_bytes()); // num_strings
    data.extend_from_slice(&0u32.to_le_bytes()); // unused
    data.extend_from_slice(&0u32.to_le_bytes()); // language ID

    for (lbl_name, strings) in labels {
        data.extend_from_slice(b" LBL"); // label magic
        data.extend_from_slice(&(strings.len() as u32).to_le_bytes()); // num strings in this label
        data.extend_from_slice(&(lbl_name.len() as u32).to_le_bytes()); // name length
        data.extend_from_slice(lbl_name.as_bytes()); // name

        for (val, extra) in *strings {
            if extra.is_some() {
                data.extend_from_slice(b"STRW"); // string format (with extra)
            } else {
                data.extend_from_slice(b" STR"); // string format
            }

            let val_utf16: Vec<u16> = val.encode_utf16().collect();
            data.extend_from_slice(&(val_utf16.len() as u32).to_le_bytes()); // num chars

            // CSF strings are bitwise-inverted UTF-16LE
            for ch in val_utf16 {
                let bytes = ch.to_le_bytes();
                data.push(!bytes[0]);
                data.push(!bytes[1]);
            }

            if let Some(ext) = extra {
                data.extend_from_slice(&(ext.len() as u32).to_le_bytes());
                data.extend_from_slice(ext.as_bytes());
            }
        }
    }

    data
}

#[test]
fn parse_valid_csf() {
    let data = build_csf(&[
        ("GUI:Ok", &[("OK", None)]),
        ("GUI:Cancel", &[("Cancel", Some("cancel_btn"))]),
    ]);

    let csf = CsfFile::parse(&data).expect("failed to parse valid CSF");
    assert_eq!(csf.version, 3);
    assert_eq!(csf.language, 0);
    assert_eq!(csf.labels.len(), 2);

    let ok_strings = csf.labels.get("GUI:Ok").unwrap();
    assert_eq!(ok_strings.len(), 1);
    assert_eq!(ok_strings[0].value, "OK");
    assert_eq!(ok_strings[0].extra, None);

    let cancel_strings = csf.labels.get("GUI:Cancel").unwrap();
    assert_eq!(cancel_strings.len(), 1);
    assert_eq!(cancel_strings[0].value, "Cancel");
    assert_eq!(cancel_strings[0].extra.as_deref(), Some("cancel_btn"));
}

#[test]
fn parse_invalid_magic_rejected() {
    let mut data = build_csf(&[("GUI:Ok", &[("OK", None)])]);
    data[0] = b'X'; // Break magic from " FSC" to "XFSC"

    let err = CsfFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidMagic {
            context: "CSF file header (expected ' FSC')"
        }
    );
}

#[test]
fn parse_excessive_labels_rejected() {
    let mut data = build_csf(&[]);
    // Overwrite NumLabels to exceed limit
    data[8..12].copy_from_slice(&(100_001u32).to_le_bytes());

    let err = CsfFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidSize {
            value: 100_001,
            limit: 100_000,
            context: "CSF labels count",
        }
    );
}

#[test]
fn parse_truncated_csf_rejected() {
    let data = build_csf(&[("GUI:Ok", &[("OK", None)])]);
    // Truncate halfway
    let truncated = &data[..data.len() / 2];

    let err = CsfFile::parse(truncated).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}
