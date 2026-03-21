use cnc_formats::aud;
use cnc_formats::shp;
use cnc_formats::tmp;
use cnc_formats::vqa::{self, VqaAudioInput, VqaEncodeParams};
use cnc_formats::wsa;

use std::sync::OnceLock;

pub(crate) struct VqaFixture {
    pub bytes: Vec<u8>,
}

pub(crate) fn aud_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_aud_bytes).as_slice()
}

pub(crate) fn shp_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_shp_bytes).as_slice()
}

pub(crate) fn wsa_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_wsa_bytes).as_slice()
}

pub(crate) fn td_tmp_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_td_tmp_bytes).as_slice()
}

pub(crate) fn ra_tmp_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_ra_tmp_bytes).as_slice()
}

pub(crate) fn vqa_fixture() -> &'static VqaFixture {
    static FIXTURE: OnceLock<VqaFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_vqa_fixture)
}

fn build_aud_bytes() -> Vec<u8> {
    let samples = build_audio_samples(22_050usize.saturating_mul(2), 1);
    aud::build_aud(&samples, 22_050, false)
}

fn build_shp_bytes() -> Vec<u8> {
    let width = 48u16;
    let height = 32u16;
    let frames = build_indexed_frames(width, height, 12);
    let frame_refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
    shp::encode_frames(&frame_refs, width, height).unwrap_or_default()
}

fn build_wsa_bytes() -> Vec<u8> {
    let width = 48u16;
    let height = 32u16;
    let frames = build_indexed_frames(width, height, 12);
    let frame_refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
    wsa::encode_frames(&frame_refs, width, height).unwrap_or_default()
}

fn build_td_tmp_bytes() -> Vec<u8> {
    let tiles = build_tmp_tiles(12, 24, 24);
    let tile_refs: Vec<&[u8]> = tiles.iter().map(Vec::as_slice).collect();
    tmp::encode_td_tmp(&tile_refs, 24, 24).unwrap_or_default()
}

fn build_ra_tmp_bytes() -> Vec<u8> {
    build_ra_tmp(96, 48, 24, 24, &[1, 5])
}

fn build_vqa_fixture() -> VqaFixture {
    let width = 64u16;
    let height = 48u16;
    let frame_count = 12usize;
    let fps = 15u8;
    let sample_rate = 22_050u16;
    let audio_sample_frames = usize::from(sample_rate)
        .saturating_mul(frame_count)
        .saturating_div(usize::from(fps.max(1)));

    let indexed_frames = build_indexed_frames(width, height, frame_count);
    let palette = build_palette_rgb8();
    let audio_samples = build_audio_samples(audio_sample_frames, 1);
    let audio = VqaAudioInput {
        samples: &audio_samples,
        sample_rate,
        channels: 1,
    };
    let params = VqaEncodeParams {
        fps,
        ..VqaEncodeParams::default()
    };

    let bytes = vqa::encode_vqa(
        &indexed_frames,
        &palette,
        width,
        height,
        Some(&audio),
        &params,
    )
    .unwrap_or_default();

    VqaFixture { bytes }
}

fn build_indexed_frames(width: u16, height: u16, frame_count: usize) -> Vec<Vec<u8>> {
    let pixel_count = usize::from(width).saturating_mul(usize::from(height));
    let mut frames = Vec::with_capacity(frame_count);

    for frame_index in 0..frame_count {
        let mut pixels = Vec::with_capacity(pixel_count);
        for y in 0..height {
            for x in 0..width {
                let value = (u32::from(x)
                    + u32::from(y).saturating_mul(3)
                    + (frame_index as u32).saturating_mul(11))
                    & 0xFF;
                pixels.push(value as u8);
            }
        }
        frames.push(pixels);
    }

    frames
}

fn build_tmp_tiles(count: usize, width: usize, height: usize) -> Vec<Vec<u8>> {
    let tile_len = width.saturating_mul(height);
    let mut tiles = Vec::with_capacity(count);
    for tile_index in 0..count {
        let mut tile = Vec::with_capacity(tile_len);
        for pixel_index in 0..tile_len {
            tile.push(((tile_index * 17 + pixel_index) & 0xFF) as u8);
        }
        tiles.push(tile);
    }
    tiles
}

fn build_audio_samples(sample_frames: usize, channels: usize) -> Vec<i16> {
    let sample_count = sample_frames.saturating_mul(channels);
    let mut samples = Vec::with_capacity(sample_count);
    for index in 0..sample_count {
        let centered = ((index * 29) % 257) as i32 - 128;
        samples.push((centered * 192) as i16);
    }
    samples
}

fn build_palette_rgb8() -> [u8; 768] {
    let mut palette = [0u8; 768];
    for index in 0..256usize {
        let base = index * 3;
        palette[base] = index as u8;
        palette[base + 1] = 255u8.saturating_sub(index as u8);
        palette[base + 2] = ((index * 3) & 0xFF) as u8;
    }
    palette
}

fn build_ra_tmp(image_w: u32, image_h: u32, tile_w: u32, tile_h: u32, empty: &[usize]) -> Vec<u8> {
    let cols = image_w / tile_w;
    let rows = image_h / tile_h;
    let grid_count = (cols * rows) as usize;
    let tile_area = (tile_w * tile_h) as usize;
    let non_empty = grid_count.saturating_sub(empty.len());
    let offsets_size = grid_count.saturating_mul(4);
    let header_and_offsets = 16usize.saturating_add(offsets_size);
    let total = header_and_offsets.saturating_add(non_empty.saturating_mul(tile_area));
    let mut buf = vec![0u8; total];

    buf[0..4].copy_from_slice(&image_w.to_le_bytes());
    buf[4..8].copy_from_slice(&image_h.to_le_bytes());
    buf[8..12].copy_from_slice(&tile_w.to_le_bytes());
    buf[12..16].copy_from_slice(&tile_h.to_le_bytes());

    let mut data_pos = header_and_offsets;
    for index in 0..grid_count {
        let offset_pos = 16usize.saturating_add(index.saturating_mul(4));
        if empty.contains(&index) {
            buf[offset_pos..offset_pos + 4].copy_from_slice(&0u32.to_le_bytes());
            continue;
        }

        buf[offset_pos..offset_pos + 4].copy_from_slice(&(data_pos as u32).to_le_bytes());
        for pixel in 0..tile_area {
            buf[data_pos + pixel] = ((index * 13 + pixel) & 0xFF) as u8;
        }
        data_pos = data_pos.saturating_add(tile_area);
    }

    buf
}
