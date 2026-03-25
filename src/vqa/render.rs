// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use crate::error::Error;

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
