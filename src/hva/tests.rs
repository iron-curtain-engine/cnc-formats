// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Test Helpers ──────────────────────────────────────────────────────────────

/// Builds a minimal valid HVA with `num_sections` sections and `num_frames` frames.
fn build_hva(num_sections: u32, num_frames: u32) -> Vec<u8> {
    let names_size = num_sections as usize * SECTION_NAME_SIZE;
    let matrices = num_frames as usize * num_sections as usize * MATRIX_SIZE;
    let total = HEADER_SIZE + names_size + matrices;
    let mut buf = Vec::with_capacity(total);

    // Header: filename
    buf.extend_from_slice(b"test.hva\0\0\0\0\0\0\0\0");
    buf.extend_from_slice(&num_frames.to_le_bytes());
    buf.extend_from_slice(&num_sections.to_le_bytes());

    // Section names
    for i in 0..num_sections {
        let mut name = [0u8; 16];
        let s = format!("bone_{i}");
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(15);
        name[..copy_len].copy_from_slice(&bytes[..copy_len]);
        buf.extend_from_slice(&name);
    }

    // Transform matrices: identity-ish (1 on diagonal, 0 elsewhere).
    for _frame in 0..num_frames {
        for _section in 0..num_sections {
            // 3×4 identity: [[1,0,0,0], [0,1,0,0], [0,0,1,0]]
            let identity: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
            for val in &identity {
                buf.extend_from_slice(&val.to_le_bytes());
            }
        }
    }

    buf
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a minimal valid HVA file and verify header fields.
#[test]
fn parse_valid() {
    let data = build_hva(2, 3);
    let hva = HvaFile::parse(&data).unwrap();
    assert_eq!(hva.header.num_frames, 3);
    assert_eq!(hva.header.num_sections, 2);
    assert_eq!(hva.section_names.len(), 2);
    assert_eq!(hva.transforms.len(), 6); // 3 frames × 2 sections
}

/// Section names are correctly parsed and retrievable.
#[test]
fn section_name_lookup() {
    let data = build_hva(2, 1);
    let hva = HvaFile::parse(&data).unwrap();
    assert_eq!(hva.section_name(0), Some("bone_0"));
    assert_eq!(hva.section_name(1), Some("bone_1"));
    assert_eq!(hva.section_name(2), None);
}

/// Transform lookup returns the correct matrix for frame/section.
#[test]
fn transform_lookup() {
    let data = build_hva(2, 3);
    let hva = HvaFile::parse(&data).unwrap();
    // Frame 0, section 0 should be identity.
    let t = hva.transform(0, 0).unwrap();
    assert!((t[0] - 1.0).abs() < f32::EPSILON);
    assert!((t[5] - 1.0).abs() < f32::EPSILON);
    assert!((t[10] - 1.0).abs() < f32::EPSILON);
    // Out of range returns None.
    assert!(hva.transform(3, 0).is_none());
    assert!(hva.transform(0, 2).is_none());
}

/// Zero sections and zero frames produce an empty but valid HVA.
#[test]
fn parse_zero_sections_zero_frames() {
    let data = build_hva(0, 0);
    let hva = HvaFile::parse(&data).unwrap();
    assert_eq!(hva.section_names.len(), 0);
    assert_eq!(hva.transforms.len(), 0);
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Input shorter than the 24-byte header is rejected.
#[test]
fn truncated_header() {
    let err = HvaFile::parse(&[0u8; 23]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { needed: 24, .. }));
}

/// Section count exceeding V38 cap is rejected.
#[test]
fn too_many_sections() {
    let mut data = build_hva(0, 0);
    // Overwrite num_sections to exceed cap.
    let sections = (MAX_SECTIONS as u32) + 1;
    data[20..24].copy_from_slice(&sections.to_le_bytes());
    let err = HvaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "HVA section count",
            ..
        }
    ));
}

/// Frame count exceeding V38 cap is rejected.
#[test]
fn too_many_frames() {
    let mut data = build_hva(0, 0);
    let frames = (MAX_FRAMES as u32) + 1;
    data[16..20].copy_from_slice(&frames.to_le_bytes());
    let err = HvaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "HVA frame count",
            ..
        }
    ));
}

/// Declared sections but not enough data for section names.
#[test]
fn truncated_section_names() {
    let mut data = vec![0u8; HEADER_SIZE + 8]; // only 8 bytes after header, not 16
    data[20..24].copy_from_slice(&1u32.to_le_bytes()); // 1 section
    let err = HvaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Declared matrices but not enough data.
#[test]
fn truncated_matrices() {
    // 1 section, 1 frame = needs 16 (name) + 48 (matrix) after header = 64 bytes
    let mut data = vec![0u8; HEADER_SIZE + 16 + 10]; // only 10 bytes for matrix
    data[16..20].copy_from_slice(&1u32.to_le_bytes()); // 1 frame
    data[20..24].copy_from_slice(&1u32.to_le_bytes()); // 1 section
    let err = HvaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn determinism() {
    let data = build_hva(2, 3);
    let a = HvaFile::parse(&data).unwrap();
    let b = HvaFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ── Security Edge Cases (V38) ────────────────────────────────────────────────

/// `HvaFile::parse` on 256 bytes of `0xFF` must not panic.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = HvaFile::parse(&data);
}

/// `HvaFile::parse` on 256 bytes of `0x00` must not panic.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = HvaFile::parse(&data);
}
