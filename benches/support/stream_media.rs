use cnc_formats::aud;
use cnc_formats::vqa::{self, VqaAudioInput, VqaEncodeParams};

use std::sync::OnceLock;

pub(crate) struct VqaFixture {
    pub bytes: Vec<u8>,
}

pub(crate) fn aud_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_aud_bytes).as_slice()
}

pub(crate) fn vqa_fixture() -> &'static VqaFixture {
    static FIXTURE: OnceLock<VqaFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_vqa_fixture)
}

pub(crate) fn vqa_seek_target_frame() -> u16 {
    6
}

fn build_aud_bytes() -> Vec<u8> {
    let samples = build_audio_samples(22_050usize.saturating_mul(2));
    aud::build_aud(&samples, 22_050, false)
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
    let audio_samples = build_audio_samples(audio_sample_frames);
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

fn build_audio_samples(sample_frames: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_frames);
    for index in 0..sample_frames {
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
