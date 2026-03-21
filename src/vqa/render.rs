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
        for bx in 0..geo.blocks_x {
            let idx = by.saturating_mul(geo.blocks_x).saturating_add(bx);
            let lo_val = vpt.get(idx).copied().unwrap_or(0);
            let hi_val = vpt
                .get(total_blocks.saturating_add(idx))
                .copied()
                .unwrap_or(0);

            let block_x = bx.saturating_mul(geo.block_w);
            let block_y = by.saturating_mul(geo.block_h);

            if hi_val == geo.fill_marker {
                for row in 0..geo.block_h {
                    let y = block_y.saturating_add(row);
                    if y >= geo.height {
                        break;
                    }
                    let row_start = y.saturating_mul(geo.width);
                    for col in 0..geo.block_w {
                        let x = block_x.saturating_add(col);
                        if x >= geo.width {
                            break;
                        }
                        if let Some(px) = pixels.get_mut(row_start.saturating_add(x)) {
                            *px = lo_val;
                        }
                    }
                }
                continue;
            }

            let block_index = (hi_val as usize)
                .saturating_mul(256)
                .saturating_add(lo_val as usize);
            let cb_offset = block_index.saturating_mul(geo.block_size);

            for row in 0..geo.block_h {
                let y = block_y.saturating_add(row);
                if y >= geo.height {
                    break;
                }
                let row_start = y.saturating_mul(geo.width);
                for col in 0..geo.block_w {
                    let x = block_x.saturating_add(col);
                    if x >= geo.width {
                        break;
                    }
                    let src_off = cb_offset
                        .saturating_add(row.saturating_mul(geo.block_w).saturating_add(col));
                    let pixel = codebook.get(src_off).copied().unwrap_or(0);
                    if let Some(px) = pixels.get_mut(row_start.saturating_add(x)) {
                        *px = pixel;
                    }
                }
            }
        }
    }

    Ok(())
}
