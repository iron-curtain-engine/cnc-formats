// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! VQA audio chunk decoders (SND0, SND1, SND2).
//!
//! SND0 = raw PCM, SND1 = Westwood ADPCM (8-bit), SND2 = IMA ADPCM (16-bit).
//! Each decoder converts its input chunk into signed 16-bit PCM samples.
//!
//! ## References
//!
//! Gordan Ugarkovic, "VQA_INFO.TXT" (2004); Valery V. Anisimovsky;
//! community C&C Modding Wiki; MultimediaWiki VQA page.  Clean-room
//! implementation from publicly documented format specifications.

use crate::error::Error;
use crate::read::read_u16_le;

/// V38: maximum decompressed audio chunk size (1 MB).  Half a second of
/// 44.1kHz stereo 16-bit = ~176 KB; 1 MB is very generous.
const MAX_AUDIO_CHUNK_SIZE: usize = 1024 * 1024;

// ─── SND0: Raw PCM ──────────────────────────────────────────────────────────

/// Decodes raw PCM from an SND0 chunk.
///
/// 8-bit data is unsigned (0–255, centered at 128); 16-bit data is signed
/// little-endian.  Both are converted to signed 16-bit samples.
pub(super) fn decode_snd0(data: &[u8], bits: u8) -> Result<Vec<i16>, Error> {
    if bits == 16 {
        // 16-bit signed LE samples.
        let sample_count = data.len() / 2;
        let mut samples = Vec::with_capacity(sample_count);
        let mut pos: usize = 0;
        while pos.saturating_add(1) < data.len() {
            let lo = data.get(pos).copied().unwrap_or(0) as u16;
            let hi = data.get(pos.saturating_add(1)).copied().unwrap_or(0) as u16;
            samples.push((lo | (hi << 8)) as i16);
            pos = pos.saturating_add(2);
        }
        Ok(samples)
    } else {
        // 8-bit unsigned samples, convert to signed 16-bit.
        let mut samples = Vec::with_capacity(data.len());
        for &byte in data {
            // Unsigned 8-bit (0–255) → signed 16-bit: (byte - 128) * 256
            let signed = (byte as i16 - 128) * 256;
            samples.push(signed);
        }
        Ok(samples)
    }
}

// ─── SND1: Westwood ADPCM ───────────────────────────────────────────────────

/// Westwood ADPCM (SND1) delta tables.
const WS_TABLE_2BIT: [i8; 4] = [-2, -1, 0, 1];
const WS_TABLE_4BIT: [i8; 16] = [-9, -8, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 8];

/// Decodes Westwood ADPCM from an SND1 chunk (8-bit unsigned output).
///
/// The chunk has a 4-byte header: out_size (u16 LE) + size (u16 LE).
/// If size == out_size, data is uncompressed.  Otherwise, WS ADPCM.
/// Result is converted to signed 16-bit for consistency.
pub(super) fn decode_snd1(data: &[u8]) -> Result<Vec<i16>, Error> {
    if data.len() < 4 {
        return Err(Error::UnexpectedEof {
            needed: 4,
            available: data.len(),
        });
    }
    let out_size = read_u16_le(data, 0)? as usize;
    let size = read_u16_le(data, 2)? as usize;

    // V38: cap output size.
    if out_size > MAX_AUDIO_CHUNK_SIZE {
        return Err(Error::InvalidSize {
            value: out_size,
            limit: MAX_AUDIO_CHUNK_SIZE,
            context: "SND1 output size",
        });
    }

    // The `size` field is the declared payload length after the 4-byte SND1
    // header.  Use it as a structural bound instead of decoding whatever
    // trailing bytes happen to be present in the chunk buffer.
    let payload_end = 4usize.saturating_add(size);
    let payload = data.get(4..payload_end).ok_or(Error::UnexpectedEof {
        needed: payload_end,
        available: data.len(),
    })?;

    if size == out_size {
        // Uncompressed: raw 8-bit unsigned → signed 16-bit.
        let raw = payload.get(..out_size).ok_or(Error::UnexpectedEof {
            needed: 4usize.saturating_add(out_size),
            available: data.len(),
        })?;
        let mut samples = Vec::with_capacity(out_size);
        for &byte in raw {
            samples.push((byte as i16 - 128) * 256);
        }
        return Ok(samples);
    }

    // Westwood ADPCM decompression.
    let mut cur_sample: i16 = 0x80; // unsigned 8-bit start
    let mut samples = Vec::with_capacity(out_size);
    let mut remaining = out_size;
    let mut i: usize = 0;

    while remaining > 0 && i < payload.len() {
        let input_byte = payload.get(i).copied().ok_or(Error::UnexpectedEof {
            needed: 4usize.saturating_add(i).saturating_add(1),
            available: data.len(),
        })?;
        i = i.saturating_add(1);
        let input = (input_byte as u16) << 2;
        let code = (input >> 8) as u8;
        let count_raw = ((input & 0xFF) >> 2) as i8;

        match code {
            2 => {
                // No compression / small delta.
                if count_raw & 0x20 != 0 {
                    // Signed delta.
                    let delta = ((count_raw << 3) >> 3) as i16;
                    cur_sample = cur_sample.saturating_add(delta).clamp(0, 255);
                    samples.push((cur_sample - 128) * 256);
                    remaining = remaining.saturating_sub(1);
                } else {
                    // Copy (count+1) bytes from input.
                    let count = (count_raw as u8).saturating_add(1) as usize;
                    for _ in 0..count {
                        if remaining == 0 {
                            break;
                        }
                        let byte = payload.get(i).copied().ok_or(Error::UnexpectedEof {
                            needed: 4usize.saturating_add(i).saturating_add(1),
                            available: data.len(),
                        })?;
                        i = i.saturating_add(1);
                        samples.push((byte as i16 - 128) * 256);
                        cur_sample = byte as i16;
                        remaining = remaining.saturating_sub(1);
                    }
                }
            }
            1 => {
                // ADPCM 8-bit → 4-bit.
                let count = (count_raw as u8).saturating_add(1) as usize;
                for _ in 0..count {
                    if remaining == 0 {
                        break;
                    }
                    let byte = payload.get(i).copied().ok_or(Error::UnexpectedEof {
                        needed: 4usize.saturating_add(i).saturating_add(1),
                        available: data.len(),
                    })?;
                    i = i.saturating_add(1);

                    // Lower nibble.
                    let delta_lo = WS_TABLE_4BIT
                        .get((byte & 0x0F) as usize)
                        .copied()
                        .unwrap_or(0) as i16;
                    cur_sample = cur_sample.saturating_add(delta_lo).clamp(0, 255);
                    samples.push((cur_sample - 128) * 256);
                    remaining = remaining.saturating_sub(1);

                    if remaining == 0 {
                        break;
                    }

                    // Higher nibble.
                    let delta_hi = WS_TABLE_4BIT
                        .get((byte >> 4) as usize)
                        .copied()
                        .unwrap_or(0) as i16;
                    cur_sample = cur_sample.saturating_add(delta_hi).clamp(0, 255);
                    samples.push((cur_sample - 128) * 256);
                    remaining = remaining.saturating_sub(1);
                }
            }
            0 => {
                // ADPCM 8-bit → 2-bit.
                let count = (count_raw as u8).saturating_add(1) as usize;
                for _ in 0..count {
                    if remaining == 0 {
                        break;
                    }
                    let byte = payload.get(i).copied().ok_or(Error::UnexpectedEof {
                        needed: 4usize.saturating_add(i).saturating_add(1),
                        available: data.len(),
                    })?;
                    i = i.saturating_add(1);

                    for shift in [0u8, 2, 4, 6] {
                        if remaining == 0 {
                            break;
                        }
                        let nibble = ((byte >> shift) & 0x03) as usize;
                        let delta = WS_TABLE_2BIT.get(nibble).copied().unwrap_or(0) as i16;
                        cur_sample = cur_sample.saturating_add(delta).clamp(0, 255);
                        samples.push((cur_sample - 128) * 256);
                        remaining = remaining.saturating_sub(1);
                    }
                }
            }
            _ => {
                // Default: repeat cur_sample (count+1) times.
                let count = (count_raw as u8).saturating_add(1) as usize;
                for _ in 0..count {
                    if remaining == 0 {
                        break;
                    }
                    samples.push((cur_sample - 128) * 256);
                    remaining = remaining.saturating_sub(1);
                }
            }
        }
    }

    if remaining > 0 {
        return Err(Error::UnexpectedEof {
            needed: 4usize.saturating_add(i).saturating_add(1),
            available: data.len(),
        });
    }

    Ok(samples)
}

// ─── SND2: IMA ADPCM ────────────────────────────────────────────────────────

/// Standard IMA ADPCM step table (89 entries, indices 0–88).
/// Same table used by aud::decode_adpcm — duplicated here to keep VQA
/// audio decoding self-contained without depending on aud module internals.
const IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// IMA ADPCM step-index adjustment table (16 entries).
const IMA_INDEX_ADJ: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Decodes a single IMA ADPCM nibble, updating state in place.
#[inline]
fn ima_decode_nibble(nibble: u8, sample: &mut i32, index: &mut usize) -> i16 {
    let step = IMA_STEP_TABLE.get(*index).copied().unwrap_or(7);
    let code = (nibble & 0x07) as i32;

    // Reconstruct delta from step and code bits.
    let mut delta = step >> 3;
    if code & 4 != 0 {
        delta = delta.saturating_add(step);
    }
    if code & 2 != 0 {
        delta = delta.saturating_add(step >> 1);
    }
    if code & 1 != 0 {
        delta = delta.saturating_add(step >> 2);
    }
    if nibble & 0x08 != 0 {
        delta = -delta;
    }

    *sample = (*sample).saturating_add(delta).clamp(-32768, 32767);
    let adj = IMA_INDEX_ADJ
        .get((nibble & 0x0F) as usize)
        .copied()
        .unwrap_or(-1);
    *index = ((*index as i32).saturating_add(adj)).clamp(0, 88) as usize;

    *sample as i16
}

/// Decodes IMA ADPCM from an SND2 chunk.
///
/// State (sample + step_index) is maintained across chunks per VQA spec.
/// For stereo, C&C/RA use interleaved layout: `LL RR LL RR …` (each byte
/// has two nibbles for the same channel).
#[allow(clippy::too_many_arguments)]
pub(super) fn decode_snd2(
    data: &[u8],
    stereo: bool,
    left_sample: &mut i32,
    left_index: &mut usize,
    right_sample: &mut i32,
    right_index: &mut usize,
) -> Result<Vec<i16>, Error> {
    let mut samples = Vec::with_capacity(data.len().saturating_mul(4));

    if stereo {
        // C&C/RA stereo: bytes alternate L/R channels.
        // Each byte has 2 nibbles for the same channel.
        for (i, &byte) in data.iter().enumerate() {
            let (sample, index) = if i % 2 == 0 {
                (left_sample as &mut i32, left_index as &mut usize)
            } else {
                (right_sample as &mut i32, right_index as &mut usize)
            };
            let lo = ima_decode_nibble(byte & 0x0F, sample, index);
            let hi = ima_decode_nibble(byte >> 4, sample, index);
            samples.push(lo);
            samples.push(hi);
        }
    } else {
        for &byte in data {
            let lo = ima_decode_nibble(byte & 0x0F, left_sample, left_index);
            let hi = ima_decode_nibble(byte >> 4, left_sample, left_index);
            samples.push(lo);
            samples.push(hi);
        }
    }

    Ok(samples)
}
