// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use crate::error::Error;

use super::snd::{decode_snd1_chunk_stateful, decode_snd2_chunk_stateful, VqaAudioChunkDecoder};

const FOURCC_SND0: [u8; 4] = *b"SND0";

fn append_decoded_chunk(
    out: &mut Vec<i16>,
    mut decoder: VqaAudioChunkDecoder<'_>,
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
    let read = decoder.read_samples(dst)?;
    out.truncate(start.saturating_add(read));
    Ok(read)
}

pub(super) fn append_snd0(out: &mut Vec<i16>, data: &[u8], bits: u8) -> Result<usize, Error> {
    let decoder = VqaAudioChunkDecoder::open_borrowed(&FOURCC_SND0, data, bits, false)?.ok_or(
        Error::InvalidMagic {
            context: "VQA SND0 audio chunk",
        },
    )?;
    append_decoded_chunk(out, decoder)
}

/// Decodes a SND1 chunk, carrying `cur_sample` across chunk boundaries.
///
/// Pass the same `cur_sample` for every consecutive chunk of the same stream.
/// Initialise it to `0x80` before the first chunk.
pub(super) fn append_snd1_stateful(
    out: &mut Vec<i16>,
    data: &[u8],
    cur_sample: &mut i16,
) -> Result<usize, Error> {
    let start = out.len();
    decode_snd1_chunk_stateful(out, data, cur_sample)?;
    Ok(out.len() - start)
}

/// Carries IMA ADPCM state across chunk boundaries.
///
/// Per the VQA spec, state should persist across SND2 chunks.  Pass the same
/// mutable state references for every chunk in the stream.  Initialise them
/// to `0` before the first chunk.
pub(super) fn append_snd2_stateful(
    out: &mut Vec<i16>,
    data: &[u8],
    stereo: bool,
    l_sample: &mut i32,
    l_index: &mut usize,
    r_sample: &mut i32,
    r_index: &mut usize,
) -> Result<usize, Error> {
    let start = out.len();
    decode_snd2_chunk_stateful(out, data, stereo, l_sample, l_index, r_sample, r_index);
    Ok(out.len() - start)
}
