// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Test Helpers ──────────────────────────────────────────────────────────────

/// Builds a minimal valid VXL file with the given number of limbs and body bytes.
fn build_vxl(limb_count: u32, body_bytes: &[u8]) -> Vec<u8> {
    let tailer_count = limb_count;
    let body_size = body_bytes.len() as u32;
    let total = HEADER_SIZE
        + (limb_count as usize) * LIMB_HEADER_SIZE
        + body_bytes.len()
        + (tailer_count as usize) * LIMB_TAILER_SIZE;

    let mut buf = Vec::with_capacity(total);

    // ── Header (802 bytes) ───────────────────────────────────────────────
    // Magic
    buf.extend_from_slice(b"Voxel Animation\0");
    // palette_count
    buf.extend_from_slice(&1u32.to_le_bytes());
    // limb_count
    buf.extend_from_slice(&limb_count.to_le_bytes());
    // tailer_count
    buf.extend_from_slice(&tailer_count.to_le_bytes());
    // body_size
    buf.extend_from_slice(&body_size.to_le_bytes());
    // start_palette_remap, end_palette_remap
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    // Palette: 256 × RGB (768 bytes)
    for i in 0..256u16 {
        buf.push((i & 0xFF) as u8);
        buf.push(0);
        buf.push(0);
    }
    // Pad header to exactly 804 bytes.
    while buf.len() < HEADER_SIZE {
        buf.push(0);
    }
    assert_eq!(buf.len(), HEADER_SIZE);

    // ── Limb Headers ─────────────────────────────────────────────────────
    for i in 0..limb_count {
        let mut name = [0u8; 16];
        let s = format!("limb_{i}");
        let bytes = s.as_bytes();
        let n = bytes.len().min(15);
        name[..n].copy_from_slice(&bytes[..n]);
        buf.extend_from_slice(&name);
        buf.extend_from_slice(&i.to_le_bytes()); // limb_number
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown1
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown2
    }

    // ── Body Data ────────────────────────────────────────────────────────
    buf.extend_from_slice(body_bytes);

    // ── Limb Tailers ─────────────────────────────────────────────────────
    for _i in 0..tailer_count {
        buf.extend_from_slice(&0u32.to_le_bytes()); // span_start_offset
        buf.extend_from_slice(&0u32.to_le_bytes()); // span_end_offset
        buf.extend_from_slice(&0u32.to_le_bytes()); // span_data_offset
        buf.extend_from_slice(&1.0f32.to_le_bytes()); // det
                                                      // Transform: 3×4 identity
        let identity: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        for val in &identity {
            buf.extend_from_slice(&val.to_le_bytes());
        }
        // min_bounds
        for _ in 0..3 {
            buf.extend_from_slice(&(-1.0f32).to_le_bytes());
        }
        // max_bounds
        for _ in 0..3 {
            buf.extend_from_slice(&1.0f32.to_le_bytes());
        }
        buf.push(2); // size_x
        buf.push(2); // size_y
        buf.push(2); // size_z
        buf.push(2); // normals_mode
    }

    buf
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a minimal valid VXL with 1 limb and verify structure.
#[test]
fn parse_valid() {
    let body = vec![0xABu8; 16];
    let data = build_vxl(1, &body);
    let vxl = VxlFile::parse(&data).unwrap();
    assert_eq!(vxl.header.limb_count, 1);
    assert_eq!(vxl.header.tailer_count, 1);
    assert_eq!(vxl.header.body_size, 16);
    assert_eq!(vxl.limb_headers.len(), 1);
    assert_eq!(vxl.limb_tailers.len(), 1);
    assert_eq!(vxl.body_data.len(), 16);
}

/// Limb header names are correctly parsed.
#[test]
fn limb_header_name() {
    let data = build_vxl(2, &[]);
    let vxl = VxlFile::parse(&data).unwrap();
    assert_eq!(vxl.limb_headers[0].name_str(), "limb_0");
    assert_eq!(vxl.limb_headers[1].name_str(), "limb_1");
}

/// Tailer fields (transform, bounds, dimensions) are correctly read.
#[test]
fn tailer_fields() {
    let data = build_vxl(1, &[0; 4]);
    let vxl = VxlFile::parse(&data).unwrap();
    let t = &vxl.limb_tailers[0];
    assert!((t.det - 1.0).abs() < f32::EPSILON);
    assert_eq!(t.size_x, 2);
    assert_eq!(t.size_y, 2);
    assert_eq!(t.size_z, 2);
    assert_eq!(t.normals_mode, 2);
    assert!((t.min_bounds[0] - (-1.0)).abs() < f32::EPSILON);
    assert!((t.max_bounds[0] - 1.0).abs() < f32::EPSILON);
}

/// Palette has 256 entries.
#[test]
fn palette_entries() {
    let data = build_vxl(0, &[]);
    let vxl = VxlFile::parse(&data).unwrap();
    assert_eq!(vxl.header.palette.len(), 256);
}

/// Zero limbs and zero body produces a valid but empty VXL.
#[test]
fn parse_zero_limbs() {
    let data = build_vxl(0, &[]);
    let vxl = VxlFile::parse(&data).unwrap();
    assert_eq!(vxl.limb_headers.len(), 0);
    assert_eq!(vxl.limb_tailers.len(), 0);
    assert!(vxl.body_data.is_empty());
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Input shorter than the 804-byte header is rejected.
#[test]
fn truncated_header() {
    let err = VxlFile::parse(&[0u8; 803]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { needed: 804, .. }));
}

/// Limb count exceeding V38 cap is rejected.
#[test]
fn too_many_limbs() {
    let mut data = build_vxl(0, &[]);
    let limbs = (MAX_LIMBS as u32) + 1;
    data[20..24].copy_from_slice(&limbs.to_le_bytes());
    let err = VxlFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "VXL limb count",
            ..
        }
    ));
}

/// Body size exceeding V38 cap is rejected.
#[test]
fn body_size_exceeds_cap() {
    let mut data = build_vxl(0, &[]);
    let big = (MAX_BODY_SIZE as u32) + 1;
    data[28..32].copy_from_slice(&big.to_le_bytes());
    let err = VxlFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "VXL body size",
            ..
        }
    ));
}

/// Not enough data for declared limb headers.
#[test]
fn truncated_limb_headers() {
    // Build VXL with 0 limbs, then lie about having 1.
    let mut data = build_vxl(0, &[]);
    data[20..24].copy_from_slice(&1u32.to_le_bytes()); // claim 1 limb
    let err = VxlFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn determinism() {
    let data = build_vxl(1, &[0; 8]);
    let a = VxlFile::parse(&data).unwrap();
    let b = VxlFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ── Security Edge Cases (V38) ────────────────────────────────────────────────

/// `VxlFile::parse` on 1024 bytes of `0xFF` must not panic.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 1024];
    let _ = VxlFile::parse(&data);
}

/// `VxlFile::parse` on 1024 bytes of `0x00` must not panic.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 1024];
    let _ = VxlFile::parse(&data);
}
