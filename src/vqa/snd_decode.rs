// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use crate::error::Error;

use super::snd::VqaAudioChunkDecoder;

const FOURCC_SND0: [u8; 4] = *b"SND0";
const FOURCC_SND1: [u8; 4] = *b"SND1";
const FOURCC_SND2: [u8; 4] = *b"SND2";

fn append_decoded_chunk(
    out: &mut Vec<i16>,
    mut decoder: VqaAudioChunkDecoder<'_>,
    left_sample: &mut i32,
    left_index: &mut usize,
    right_sample: &mut i32,
    right_index: &mut usize,
) -> Result<usize, Error> {
    let sample_count = decoder.remaining_sample_count();
    let start = out.len();
    let end = start.saturating_add(sample_count);
    out.reserve(sample_count);
    out.resize(end, 0);
    let out_len = out.len();
    let dst = out.get_mut(start..end).ok_or(Error::UnexpectedEof {
        needed: end,
        available: out_len,
    })?;
    let read = decoder.read_samples(dst, left_sample, left_index, right_sample, right_index)?;
    out.truncate(start.saturating_add(read));
    Ok(read)
}

pub(super) fn append_snd0(out: &mut Vec<i16>, data: &[u8], bits: u8) -> Result<usize, Error> {
    let mut left_sample = 0i32;
    let mut left_index = 0usize;
    let mut right_sample = 0i32;
    let mut right_index = 0usize;
    let decoder = VqaAudioChunkDecoder::open_borrowed(&FOURCC_SND0, data, bits, false)?.ok_or(
        Error::InvalidMagic {
            context: "VQA SND0 audio chunk",
        },
    )?;
    append_decoded_chunk(
        out,
        decoder,
        &mut left_sample,
        &mut left_index,
        &mut right_sample,
        &mut right_index,
    )
}

pub(super) fn append_snd1(out: &mut Vec<i16>, data: &[u8]) -> Result<usize, Error> {
    let mut left_sample = 0i32;
    let mut left_index = 0usize;
    let mut right_sample = 0i32;
    let mut right_index = 0usize;
    let decoder = VqaAudioChunkDecoder::open_borrowed(&FOURCC_SND1, data, 8, false)?.ok_or(
        Error::InvalidMagic {
            context: "VQA SND1 audio chunk",
        },
    )?;
    append_decoded_chunk(
        out,
        decoder,
        &mut left_sample,
        &mut left_index,
        &mut right_sample,
        &mut right_index,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_snd2(
    out: &mut Vec<i16>,
    data: &[u8],
    stereo: bool,
    left_sample: &mut i32,
    left_index: &mut usize,
    right_sample: &mut i32,
    right_index: &mut usize,
) -> Result<usize, Error> {
    let decoder = VqaAudioChunkDecoder::open_borrowed(&FOURCC_SND2, data, 16, stereo)?.ok_or(
        Error::InvalidMagic {
            context: "VQA SND2 audio chunk",
        },
    )?;
    append_decoded_chunk(
        out,
        decoder,
        left_sample,
        left_index,
        right_sample,
        right_index,
    )
}
