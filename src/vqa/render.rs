// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use crate::error::Error;

// ─── Compact codebook ────────────────────────────────────────────────────────

/// Builds a per-frame frequency-ordered compact codebook and rewritten VPT.
///
/// ## Why this helps
///
/// VQA codebooks can be several tens of KB, and the original encoder's entry
/// ordering is essentially arbitrary.  A single frame only references a subset
/// of those entries — often just a few hundred — but the render loop accesses
/// them at random offsets, thrashing L1/L2 cache.
///
/// This function counts how often each codebook entry is referenced in the
/// current frame's VPT, then packs only the referenced entries into a new
/// compact buffer ordered hottest-first.  The companion `vpt` is rewritten
/// so every block index points at the correct compact position.
///
/// The hot entries reside at the start of the compact buffer; for a typical
/// 320×200 RA1 VQA only ~1–2 KB of the compact codebook is touched repeatedly,
/// which fits comfortably inside L1 cache.
///
/// ## Returns
///
/// `Some((compact_codebook, remapped_vpt))` on success.
/// `None` when compaction cannot proceed safely:
/// - no non-fill blocks in the frame (all solid colours)
/// - VPT or codebook are too small / malformed
/// - the number of unique referenced entries would alias the `fill_marker` byte
///   (never happens for real RA1 data but checked defensively)
pub(crate) fn build_compact_codebook(
    geo: &VqaRenderGeometry,
    codebook: &[u8],
    vpt: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let total_blocks = geo.blocks_x.saturating_mul(geo.blocks_y);
    if total_blocks == 0
        || vpt.len() < total_blocks.saturating_mul(2)
        || geo.block_size == 0
        || codebook.is_empty()
    {
        return None;
    }

    let entry_count = codebook.len() / geo.block_size;
    if entry_count == 0 {
        return None;
    }

    // ── Pass 1: count reference frequency per codebook block index ────────
    let mut freq: Vec<u32> = vec![0u32; entry_count];
    for idx in 0..total_blocks {
        let lo = vpt.get(idx).copied().unwrap_or(0) as usize;
        let hi = vpt
            .get(total_blocks.saturating_add(idx))
            .copied()
            .unwrap_or(0);
        if hi == geo.fill_marker {
            continue; // solid-colour fill — no codebook lookup
        }
        let block_index = (hi as usize).saturating_mul(256).saturating_add(lo);
        if let Some(c) = freq.get_mut(block_index) {
            *c = c.saturating_add(1);
        }
    }

    // ── Collect only referenced entries, ranked hottest-first ─────────────
    let mut ranked: Vec<(u32, usize)> = freq
        .iter()
        .enumerate()
        .filter(|(_, &f)| f > 0)
        .map(|(i, &f)| (f, i))
        .collect();

    if ranked.is_empty() {
        return None; // every block is a solid fill — nothing to compact
    }

    // Sort descending by frequency.
    ranked.sort_unstable_by(|a, b| b.0.cmp(&a.0));

    // ── Safety: compact indices must not alias the fill_marker hi-byte ─────
    // After remapping, block index = compact rank; the hi byte of that rank
    // is (rank >> 8).  If rank >> 8 == fill_marker the render loop would
    // mistake it for a solid-fill.  Guard: require ranked.len() <=
    // fill_marker * 256 (never exceeded for any known RA1 file).
    let safe_limit = (geo.fill_marker as usize).saturating_mul(256);
    if ranked.len() > safe_limit {
        return None;
    }

    // ── Build compact codebook ─────────────────────────────────────────────
    let mut compact_cb: Vec<u8> =
        Vec::with_capacity(ranked.len().saturating_mul(geo.block_size));
    // Remap table: old block_index → new compact index.
    let mut remap: Vec<u16> = vec![0u16; entry_count];

    for (new_idx, &(_, old_idx)) in ranked.iter().enumerate() {
        let src_start = old_idx.saturating_mul(geo.block_size);
        if let Some(src) = codebook.get(src_start..src_start.saturating_add(geo.block_size)) {
            compact_cb.extend_from_slice(src);
            if let Some(slot) = remap.get_mut(old_idx) {
                *slot = new_idx as u16;
            }
        }
    }

    // ── Rewrite VPT ────────────────────────────────────────────────────────
    // Clone the original VPT; fill blocks are left byte-for-byte identical
    // (their hi == fill_marker sentinel is preserved).  Only non-fill blocks
    // get their (lo, hi) pair rewritten to the compact index.
    let mut compact_vpt = vpt.to_vec();
    for idx in 0..total_blocks {
        let lo = vpt.get(idx).copied().unwrap_or(0) as usize;
        let hi_orig = vpt
            .get(total_blocks.saturating_add(idx))
            .copied()
            .unwrap_or(0);
        if hi_orig == geo.fill_marker {
            continue; // fill sentinel — leave unchanged
        }
        let old_block_index = (hi_orig as usize).saturating_mul(256).saturating_add(lo);
        let new_idx = remap.get(old_block_index).copied().unwrap_or(0) as usize;
        if let Some(slot) = compact_vpt.get_mut(idx) {
            *slot = (new_idx & 0xFF) as u8;
        }
        if let Some(slot) = compact_vpt.get_mut(total_blocks.saturating_add(idx)) {
            *slot = (new_idx >> 8) as u8;
        }
    }

    Some((compact_cb, compact_vpt))
}

pub(crate) struct VqaRenderGeometry {
    pub width: usize,
    pub height: usize,
    pub block_w: usize,
    pub block_h: usize,
    pub blocks_x: usize,
    pub blocks_y: usize,
    pub block_size: usize,
    pub fill_marker: u8,
}

pub(crate) fn render_frame_pixels(
    geo: &VqaRenderGeometry,
    codebook: &[u8],
    vpt: &[u8],
    pixels: &mut [u8],
) -> Result<(), Error> {
    let pixel_count = geo.width.saturating_mul(geo.height);
    if pixels.len() != pixel_count {
        return Err(Error::InvalidSize {
            value: pixels.len(),
            limit: pixel_count,
            context: "VQA frame pixel buffer",
        });
    }

    let total_blocks = geo.blocks_x.saturating_mul(geo.blocks_y);
    let needed = total_blocks.saturating_mul(2);
    if vpt.len() < needed {
        return Err(Error::UnexpectedEof {
            needed,
            available: vpt.len(),
        });
    }

    pixels.fill(0);

    for by in 0..geo.blocks_y {
        let block_y = by.saturating_mul(geo.block_h);
        // Number of pixel rows this block row actually covers (clamp at
        // the bottom edge of the frame).
        let rows_avail = geo.block_h.min(geo.height.saturating_sub(block_y));
        if rows_avail == 0 {
            break;
        }

        for bx in 0..geo.blocks_x {
            let idx = by.saturating_mul(geo.blocks_x).saturating_add(bx);
            let lo_val = vpt.get(idx).copied().unwrap_or(0);
            let hi_val = vpt
                .get(total_blocks.saturating_add(idx))
                .copied()
                .unwrap_or(0);

            let block_x = bx.saturating_mul(geo.block_w);
            // Number of pixel columns this block actually covers (clamp at
            // the right edge of the frame).
            let cols_avail = geo.block_w.min(geo.width.saturating_sub(block_x));
            if cols_avail == 0 {
                break;
            }

            if hi_val == geo.fill_marker {
                // ── Solid-color fill ─────────────────────────────────
                for row in 0..rows_avail {
                    let dst_start = (block_y + row) * geo.width + block_x;
                    if let Some(dst) = pixels.get_mut(dst_start..dst_start + cols_avail) {
                        dst.fill(lo_val);
                    }
                }
                continue;
            }

            // ── Codebook block copy ──────────────────────────────────
            // Each codebook entry is `block_w × block_h` pixels laid out
            // row-major.  Copy one row at a time via `copy_from_slice`.
            let block_index = (hi_val as usize)
                .saturating_mul(256)
                .saturating_add(lo_val as usize);
            let cb_offset = block_index.saturating_mul(geo.block_size);

            // Pre-check: verify the entire block fits in the codebook.
            let cb_end = cb_offset
                .saturating_add(rows_avail.saturating_sub(1).saturating_mul(geo.block_w))
                .saturating_add(cols_avail);
            if cb_end > codebook.len() {
                continue; // malformed codebook reference — skip block, pixels stay 0
            }
            for row in 0..rows_avail {
                let src_start = cb_offset + row * geo.block_w;
                let dst_start = (block_y + row) * geo.width + block_x;
                if let (Some(src), Some(dst)) = (
                    codebook.get(src_start..src_start + cols_avail),
                    pixels.get_mut(dst_start..dst_start + cols_avail),
                ) {
                    dst.copy_from_slice(src);
                }
            }
        }
    }

    Ok(())
}
