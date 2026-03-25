// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::tests::write_u16_le;
use super::*;

#[test]
fn palette_6bit_to_8bit_scaling() {
    let mut vqhd = [0u8; 42];
    write_u16_le(&mut vqhd, 0, 2);
    write_u16_le(&mut vqhd, 4, 1);
    write_u16_le(&mut vqhd, 6, 4);
    write_u16_le(&mut vqhd, 8, 2);
    vqhd[10] = 4;
    vqhd[11] = 2;
    vqhd[12] = 15;
    vqhd[13] = 1;
    write_u16_le(&mut vqhd, 16, 1);

    let mut cpl_data = vec![0u8; 768];
    cpl_data[3] = 63;
    cpl_data[4] = 63;
    cpl_data[5] = 63;

    let cbf_data = vec![0u8; 8];
    let vpt_data = vec![0u8; 2];

    let mut vqfr_payload = Vec::new();
    vqfr_payload.extend_from_slice(b"CPL0");
    vqfr_payload.extend_from_slice(&(cpl_data.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&cpl_data);
    vqfr_payload.extend_from_slice(b"CBF0");
    vqfr_payload.extend_from_slice(&(cbf_data.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&cbf_data);
    vqfr_payload.extend_from_slice(b"VPT0");
    vqfr_payload.extend_from_slice(&(vpt_data.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&vpt_data);

    let vqhd_chunk_size = vqhd.len();
    let vqfr_chunk_size = vqfr_payload.len();
    let form_data_size = 4 + 8 + vqhd_chunk_size + 8 + vqfr_chunk_size;

    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_data_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    data.extend_from_slice(b"VQHD");
    data.extend_from_slice(&(vqhd_chunk_size as u32).to_be_bytes());
    data.extend_from_slice(&vqhd);
    data.extend_from_slice(b"VQFR");
    data.extend_from_slice(&(vqfr_chunk_size as u32).to_be_bytes());
    data.extend_from_slice(&vqfr_payload);

    let vqa = VqaFile::parse(&data).unwrap();
    let frames = vqa.decode_frames().unwrap();
    assert_eq!(frames.len(), 1);

    let pal = &frames[0].palette;
    assert_eq!(pal[0], 0);
    assert_eq!(pal[1], 0);
    assert_eq!(pal[2], 0);
    assert_eq!(pal[3], 255);
    assert_eq!(pal[4], 255);
    assert_eq!(pal[5], 255);
}

#[test]
fn cbp_codebook_deferred_to_next_group() {
    let mut vqhd = [0u8; 42];
    write_u16_le(&mut vqhd, 0, 2);
    write_u16_le(&mut vqhd, 4, 2);
    write_u16_le(&mut vqhd, 6, 4);
    write_u16_le(&mut vqhd, 8, 2);
    vqhd[10] = 4;
    vqhd[11] = 2;
    vqhd[12] = 15;
    vqhd[13] = 1;
    write_u16_le(&mut vqhd, 16, 1);

    let cb_a = vec![0x01u8; 8];
    let cb_b = vec![0x02u8; 8];
    let vpt = vec![0u8; 2];

    let mut vqfr0 = Vec::new();
    vqfr0.extend_from_slice(b"CBF0");
    vqfr0.extend_from_slice(&(cb_a.len() as u32).to_be_bytes());
    vqfr0.extend_from_slice(&cb_a);
    vqfr0.extend_from_slice(b"VPT0");
    vqfr0.extend_from_slice(&(vpt.len() as u32).to_be_bytes());
    vqfr0.extend_from_slice(&vpt);

    let mut vqfr1 = Vec::new();
    vqfr1.extend_from_slice(b"CBP0");
    vqfr1.extend_from_slice(&(cb_b.len() as u32).to_be_bytes());
    vqfr1.extend_from_slice(&cb_b);
    vqfr1.extend_from_slice(b"VPT0");
    vqfr1.extend_from_slice(&(vpt.len() as u32).to_be_bytes());
    vqfr1.extend_from_slice(&vpt);

    let cpl = vec![0u8; 768];
    let mut cpl_chunk = Vec::new();
    cpl_chunk.extend_from_slice(b"CPL0");
    cpl_chunk.extend_from_slice(&(cpl.len() as u32).to_be_bytes());
    cpl_chunk.extend_from_slice(&cpl);

    let mut vqfr0_with_cpl = cpl_chunk;
    vqfr0_with_cpl.extend_from_slice(&vqfr0);

    let chunks_size = (8 + vqfr0_with_cpl.len()) + (8 + vqfr1.len());
    let form_data_size = 4 + 8 + 42 + chunks_size;
    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_data_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    data.extend_from_slice(b"VQHD");
    data.extend_from_slice(&42u32.to_be_bytes());
    data.extend_from_slice(&vqhd);
    data.extend_from_slice(b"VQFR");
    data.extend_from_slice(&(vqfr0_with_cpl.len() as u32).to_be_bytes());
    data.extend_from_slice(&vqfr0_with_cpl);
    data.extend_from_slice(b"VQFR");
    data.extend_from_slice(&(vqfr1.len() as u32).to_be_bytes());
    data.extend_from_slice(&vqfr1);

    let vqa = VqaFile::parse(&data).unwrap();
    let frames = vqa.decode_frames().unwrap();
    assert_eq!(frames.len(), 2);
    assert!(frames[0].pixels.iter().all(|&p| p == 0x01));
    assert!(frames[1].pixels.iter().all(|&p| p == 0x01));
}

// ─── Compact-codebook tests ───────────────────────────────────────────────────
//
// These tests verify `build_compact_codebook` independently from the full
// decode pipeline so correctness can be proven at the unit level.

/// Helper: 4×2-pixel geometry for a single block (1×1 block grid).
fn single_block_geo() -> super::render::VqaRenderGeometry {
    super::render::VqaRenderGeometry {
        width: 4,
        height: 2,
        block_w: 4,
        block_h: 2,
        blocks_x: 1,
        blocks_y: 1,
        block_size: 8,
        fill_marker: 0x0F,
    }
}

/// Helper: 8×2-pixel geometry for two side-by-side blocks (2×1 grid).
fn two_block_geo() -> super::render::VqaRenderGeometry {
    super::render::VqaRenderGeometry {
        width: 8,
        height: 2,
        block_w: 4,
        block_h: 2,
        blocks_x: 2,
        blocks_y: 1,
        block_size: 8,
        fill_marker: 0x0F,
    }
}

/// Build a 1-block VPT (lo=entry_lo, hi=entry_hi, no fill).
fn vpt_one_block(entry_lo: u8, entry_hi: u8) -> Vec<u8> {
    vec![entry_lo, entry_hi]
}

/// Compact render produces bit-identical pixels to original render.
#[test]
fn compact_codebook_pixel_parity_with_original() {
    use super::render::{build_compact_codebook, render_frame_pixels};

    let geo = single_block_geo();
    // Codebook: 4 entries, 8 bytes each (distinct non-zero patterns).
    let codebook: Vec<u8> = (0u8..4)
        .flat_map(|e| std::iter::repeat(e + 1).take(8))
        .collect(); // [1,1,1,1,1,1,1,1, 2,2,...,  3,3,...,  4,4,...]

    // VPT referencing entry 2 (lo=2, hi=0).
    let vpt = vpt_one_block(2, 0);

    // Original render.
    let mut orig_pixels = vec![0u8; geo.width * geo.height];
    render_frame_pixels(&geo, &codebook, &vpt, &mut orig_pixels)
        .expect("original render should succeed");

    // Compact render.
    let (compact_cb, compact_vpt) =
        build_compact_codebook(&geo, &codebook, &vpt).expect("compaction should succeed");
    let mut compact_pixels = vec![0u8; geo.width * geo.height];
    render_frame_pixels(&geo, &compact_cb, &compact_vpt, &mut compact_pixels)
        .expect("compact render should succeed");

    assert_eq!(
        orig_pixels, compact_pixels,
        "compact render must produce bit-identical pixels to original render"
    );
    // Sanity: pixels should equal entry 3 (index 2 → value 3).
    assert!(compact_pixels.iter().all(|&p| p == 3));
}

/// The most-referenced codebook entry ends up at compact index 0 (front).
#[test]
fn compact_codebook_most_used_entry_at_front() {
    use super::render::{build_compact_codebook, VqaRenderGeometry};

    // 4-block grid (4×1), block_size=8.
    let geo = VqaRenderGeometry {
        width: 16,
        height: 2,
        block_w: 4,
        block_h: 2,
        blocks_x: 4,
        blocks_y: 1,
        block_size: 8,
        fill_marker: 0x0F,
    };

    // Codebook: 3 entries with distinct constant values (10, 20, 30).
    let codebook: Vec<u8> = vec![
        10, 10, 10, 10, 10, 10, 10, 10, // entry 0
        20, 20, 20, 20, 20, 20, 20, 20, // entry 1
        30, 30, 30, 30, 30, 30, 30, 30, // entry 2 — used 3×, should be hottest
    ];

    // VPT: [0, 2, 2, 2] — entry 2 used 3×, entries 0 and 1 used 0/1×.
    // lo bytes: [0, 2, 2, 2] (4 blocks)
    // hi bytes: [0, 0, 0, 0]
    let vpt: Vec<u8> = vec![0, 2, 2, 2, 0, 0, 0, 0];

    let (compact_cb, _compact_vpt) =
        build_compact_codebook(&geo, &codebook, &vpt).expect("compaction should succeed");

    // The hottest entry (entry 2, value 30) must be at compact index 0.
    let first_entry: &[u8] = compact_cb.get(0..8).expect("compact codebook must have at least one entry");
    assert!(
        first_entry.iter().all(|&b| b == 30),
        "most-referenced entry (value 30) must occupy compact index 0 (cache-hottest slot)"
    );
}

/// Compact codebook contains only entries actually referenced in the VPT.
#[test]
fn compact_codebook_excludes_unreferenced_entries() {
    use super::render::{build_compact_codebook, VqaRenderGeometry};

    // 3-block grid.
    let geo = VqaRenderGeometry {
        width: 12,
        height: 2,
        block_w: 4,
        block_h: 2,
        blocks_x: 3,
        blocks_y: 1,
        block_size: 8,
        fill_marker: 0x0F,
    };

    // 10-entry codebook; only entries 0, 4, 9 are referenced.
    let codebook: Vec<u8> = (0u8..10)
        .flat_map(|e| std::iter::repeat(e).take(8))
        .collect();

    // VPT: lo=[0, 4, 9], hi=[0, 0, 0]
    let vpt: Vec<u8> = vec![0, 4, 9, 0, 0, 0];

    let (compact_cb, _compact_vpt) =
        build_compact_codebook(&geo, &codebook, &vpt).expect("compaction should succeed");

    assert_eq!(
        compact_cb.len(),
        3 * 8,
        "compact codebook must contain exactly 3 referenced entries (3 × block_size)"
    );
}

/// Fill blocks are preserved unchanged through compaction.
#[test]
fn compact_codebook_fill_blocks_survive_remap() {
    use super::render::{build_compact_codebook, render_frame_pixels, VqaRenderGeometry};

    // 2-block grid: block 0 = fill (palette index 42), block 1 = codebook entry 0.
    let geo = VqaRenderGeometry {
        width: 8,
        height: 2,
        block_w: 4,
        block_h: 2,
        blocks_x: 2,
        blocks_y: 1,
        block_size: 8,
        fill_marker: 0x0F,
    };

    let codebook: Vec<u8> = vec![99u8; 8]; // 1 codebook entry, all 99s.

    // VPT: block 0 = fill (lo=42, hi=0x0F), block 1 = entry 0 (lo=0, hi=0).
    let vpt: Vec<u8> = vec![42, 0, 0x0F, 0];

    // Original render.
    let mut orig = vec![0u8; 16];
    render_frame_pixels(&geo, &codebook, &vpt, &mut orig)
        .expect("original render should succeed");

    // Compact render.
    let (compact_cb, compact_vpt) =
        build_compact_codebook(&geo, &codebook, &vpt).expect("compaction should succeed");
    let mut compact_out = vec![0u8; 16];
    render_frame_pixels(&geo, &compact_cb, &compact_vpt, &mut compact_out)
        .expect("compact render should succeed");

    assert_eq!(orig, compact_out, "fill blocks must render identically after compaction");
    // Frame layout (8 wide × 2 high, row-major):
    //   Row 0: [block0_col0..3, block1_col4..7]  = indices 0..7
    //   Row 1: [block0_col0..3, block1_col4..7]  = indices 8..15
    // Block 0 (fill 42) covers columns 0-3: indices 0-3 and 8-11.
    // Block 1 (codebook 99) covers columns 4-7: indices 4-7 and 12-15.
    let block0_pixels: Vec<u8> = (0..2).flat_map(|row| orig[row * 8..row * 8 + 4].iter().copied()).collect();
    let block1_pixels: Vec<u8> = (0..2).flat_map(|row| orig[row * 8 + 4..row * 8 + 8].iter().copied()).collect();
    assert!(block0_pixels.iter().all(|&p| p == 42), "left block (fill 42) pixels must all be 42");
    assert!(block1_pixels.iter().all(|&p| p == 99), "right block (codebook entry 0 = 99) pixels must all be 99");
}

/// An all-fill frame returns None from build_compact_codebook.
#[test]
fn compact_codebook_all_fill_returns_none() {
    use super::render::build_compact_codebook;

    let geo = single_block_geo();
    let codebook = vec![0u8; 8];
    // VPT: fill block (lo=5, hi=fill_marker).
    let vpt = vpt_one_block(5, 0x0F);

    let result = build_compact_codebook(&geo, &codebook, &vpt);
    assert!(
        result.is_none(),
        "all-fill frame should return None from build_compact_codebook"
    );
}
