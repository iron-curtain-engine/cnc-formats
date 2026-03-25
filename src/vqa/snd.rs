// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! VQA audio chunk decoders (SND0, SND1, SND2).
//!
//! SND0 = raw PCM, SND1 = Westwood ADPCM (8-bit), SND2 = IMA ADPCM (16-bit).
//! Each decoder converts its input chunk into signed 16-bit PCM samples.
//! Gordan Ugarkovic, "VQA_INFO.TXT" (2004); Valery V. Anisimovsky;
//! community C&C Modding Wiki; MultimediaWiki VQA page.  Clean-room
//! implementation from publicly documented format specifications.

use super::snd_ima::ima_decode_nibble;
use crate::error::Error;
use crate::read::read_u16_le;
use std::borrow::Cow;

/// Decodes a single SND2 (IMA ADPCM) chunk into signed 16-bit PCM samples,
/// carrying IMA state across calls.
///
/// Per the VQA spec, IMA ADPCM state **carries across chunk boundaries**.
/// The caller is responsible for initialising state to `(0, 0, 0, 0)` at the
/// start of the stream and passing the same mutable references for every
/// subsequent chunk.
///
/// Stereo layout: the first half of the data is the left channel; the second
/// half is the right channel.  For mono, only `l_sample`/`l_index` are used;
/// `r_sample`/`r_index` are ignored.
/// The returned samples are interleaved: L0, R0, L1, R1, …
pub(super) fn decode_snd2_chunk_stateful(
    out: &mut Vec<i16>,
    data: &[u8],
    stereo: bool,
    l_sample: &mut i32,
    l_index: &mut usize,
    r_sample: &mut i32,
    r_index: &mut usize,
) {
    if stereo {
        let half = data.len() / 2;
        let left_data = data.get(..half).unwrap_or(&[]);
        let right_data = data.get(half..).unwrap_or(&[]);

        for (&lb, &rb) in left_data.iter().zip(right_data.iter()) {
            out.push(ima_decode_nibble(lb & 0x0F, l_sample, l_index));
            out.push(ima_decode_nibble(rb & 0x0F, r_sample, r_index));
            out.push(ima_decode_nibble(lb >> 4, l_sample, l_index));
            out.push(ima_decode_nibble(rb >> 4, r_sample, r_index));
        }
    } else {
        for &byte in data {
            out.push(ima_decode_nibble(byte & 0x0F, l_sample, l_index));
            out.push(ima_decode_nibble(byte >> 4, l_sample, l_index));
        }
    }
}

/// Decodes a single SND2 (IMA ADPCM) chunk into signed 16-bit PCM samples.
///
/// This is a convenience wrapper that starts from fresh IMA state `(0, 0)`.
/// Use [`decode_snd2_chunk_stateful`] to carry state across chunk boundaries.
fn decode_snd2_chunk(data: &[u8], stereo: bool) -> Vec<i16> {
    let mut ls: i32 = 0;
    let mut li: usize = 0;
    let mut rs: i32 = 0;
    let mut ri: usize = 0;
    let mut out = Vec::new();
    decode_snd2_chunk_stateful(&mut out, data, stereo, &mut ls, &mut li, &mut rs, &mut ri);
    out
}

/// Decodes a single SND1 (Westwood ADPCM) chunk, carrying `cur_sample`
/// across chunk boundaries.
///
/// `cur_sample` must be initialised to `0x80` (mid-scale) before the first
/// chunk of a stream and passed unchanged for every subsequent chunk.  The
/// caller updates it in-place with the final predictor value after each decode.
pub(super) fn decode_snd1_chunk_stateful(
    out: &mut Vec<i16>,
    data: &[u8],
    cur_sample: &mut i16,
) -> Result<(), Error> {
    if data.len() < 4 {
        return Err(Error::UnexpectedEof {
            needed: 4,
            available: data.len(),
        });
    }
    let out_size = read_u16_le(data, 0)? as usize;
    let in_size = read_u16_le(data, 2)? as usize;

    if out_size > MAX_AUDIO_CHUNK_SIZE {
        return Err(Error::InvalidSize {
            value: out_size,
            limit: MAX_AUDIO_CHUNK_SIZE,
            context: "SND1 output size",
        });
    }

    let payload_end = 4usize.saturating_add(in_size);
    if data.get(4..payload_end).is_none() {
        return Err(Error::UnexpectedEof {
            needed: payload_end,
            available: data.len(),
        });
    }

    let start = out.len();
    out.resize(start.saturating_add(out_size), 0);
    let mut pos = 4usize;
    let mut remaining_output = out_size;
    let mut mode = if in_size == out_size {
        Snd1Mode::RawCopy { remaining: out_size }
    } else {
        Snd1Mode::Idle
    };

    let dst = out.get_mut(start..).unwrap_or(&mut []);
    let written = read_snd1_samples(
        data,
        &mut pos,
        payload_end,
        &mut remaining_output,
        cur_sample,
        &mut mode,
        dst,
    )?;
    out.truncate(start.saturating_add(written));
    Ok(())
}

/// V38: maximum decompressed audio chunk size (1 MB).  Half a second of
/// 44.1kHz stereo 16-bit = ~176 KB; 1 MB is very generous.
const MAX_AUDIO_CHUNK_SIZE: usize = 1024 * 1024;
#[inline]
fn pcm8_to_i16(byte: u8) -> i16 {
    (byte as i16 - 128) * 256
}

#[derive(Debug, Clone)]
pub(super) enum Snd1Mode {
    Idle,
    Delta {
        delta: i16,
    },
    RawCopy {
        remaining: usize,
    },
    Adpcm4 {
        remaining: usize,
        current_byte: Option<u8>,
        stage: u8,
    },
    Adpcm2 {
        remaining: usize,
        current_byte: Option<u8>,
        shift: u8,
    },
    Repeat {
        remaining: usize,
    },
}
#[derive(Debug, Clone)]
pub(super) enum VqaAudioChunkDecoder<'data> {
    Snd0Pcm16 {
        data: Cow<'data, [u8]>,
        pos: usize,
    },
    Snd0Pcm8 {
        data: Cow<'data, [u8]>,
        pos: usize,
    },
    Snd1 {
        data: Cow<'data, [u8]>,
        pos: usize,
        payload_end: usize,
        remaining_output: usize,
        cur_sample: i16,
        mode: Snd1Mode,
    },
    /// Pre-decoded PCM samples stored directly as `i16`.
    ///
    /// Used for SND2 (IMA ADPCM) and stateful SND1 paths where the entire
    /// chunk is decoded upfront into a `Vec<i16>`.  Avoids the
    /// `i16 → bytes → i16` round-trip that the `Snd0Pcm16` variant would
    /// require.
    PcmDirect {
        samples: Vec<i16>,
        pos: usize,
    },
}

impl VqaAudioChunkDecoder<'_> {
    pub(super) fn open_borrowed<'data>(
        fourcc: &[u8; 4],
        data: &'data [u8],
        bits: u8,
        stereo: bool,
    ) -> Result<Option<VqaAudioChunkDecoder<'data>>, Error> {
        Self::open_with_data(fourcc, Cow::Borrowed(data), bits, stereo)
    }

    fn open_with_data<'data>(
        fourcc: &[u8; 4],
        data: Cow<'data, [u8]>,
        bits: u8,
        stereo: bool,
    ) -> Result<Option<VqaAudioChunkDecoder<'data>>, Error> {
        match fourcc {
            b"SND0" => {
                if bits == 16 {
                    Ok(Some(VqaAudioChunkDecoder::Snd0Pcm16 { data, pos: 0 }))
                } else {
                    Ok(Some(VqaAudioChunkDecoder::Snd0Pcm8 { data, pos: 0 }))
                }
            }
            b"SND1" => Ok(Some(Self::open_snd1(data)?)),
            b"SND2" => {
                // Decode the entire IMA ADPCM chunk upfront.
                // State resets to (0, 0) per chunk; stereo uses split-half layout.
                let samples = decode_snd2_chunk(data.as_ref(), stereo);
                Ok(Some(VqaAudioChunkDecoder::PcmDirect { samples, pos: 0 }))
            }
            _ => Ok(None),
        }
    }

    fn open_snd1(data: Cow<'_, [u8]>) -> Result<VqaAudioChunkDecoder<'_>, Error> {
        Self::open_snd1_with_state(data, 0x80)
    }

    pub(super) fn open_snd1_with_state(
        data: Cow<'_, [u8]>,
        initial_cur_sample: i16,
    ) -> Result<VqaAudioChunkDecoder<'_>, Error> {
        if data.len() < 4 {
            return Err(Error::UnexpectedEof {
                needed: 4,
                available: data.len(),
            });
        }
        let out_size = read_u16_le(data.as_ref(), 0)? as usize;
        let size = read_u16_le(data.as_ref(), 2)? as usize;

        if out_size > MAX_AUDIO_CHUNK_SIZE {
            return Err(Error::InvalidSize {
                value: out_size,
                limit: MAX_AUDIO_CHUNK_SIZE,
                context: "SND1 output size",
            });
        }

        let payload_end = 4usize.saturating_add(size);
        if data.get(4..payload_end).is_none() {
            return Err(Error::UnexpectedEof {
                needed: payload_end,
                available: data.len(),
            });
        }

        Ok(VqaAudioChunkDecoder::Snd1 {
            data,
            pos: 4,
            payload_end,
            remaining_output: out_size,
            cur_sample: initial_cur_sample,
            mode: if size == out_size {
                Snd1Mode::RawCopy {
                    remaining: out_size,
                }
            } else {
                Snd1Mode::Idle
            },
        })
    }
}

impl VqaAudioChunkDecoder<'static> {
    pub(super) fn open_owned(
        fourcc: &[u8; 4],
        data: Vec<u8>,
        bits: u8,
        stereo: bool,
    ) -> Result<Option<Self>, Error> {
        Self::open_with_data(fourcc, Cow::Owned(data), bits, stereo)
    }
}

impl VqaAudioChunkDecoder<'_> {
    pub(super) fn is_finished(&self) -> bool {
        match self {
            Self::Snd0Pcm16 { data, pos } | Self::Snd0Pcm8 { data, pos } => *pos >= data.len(),
            Self::Snd1 {
                remaining_output,
                mode,
                ..
            } => *remaining_output == 0 && matches!(mode, Snd1Mode::Idle),
            Self::PcmDirect { samples, pos } => *pos >= samples.len(),
        }
    }

    pub(super) fn remaining_sample_count(&self) -> usize {
        match self {
            Self::Snd0Pcm16 { data, pos } => (data.len().saturating_sub(*pos)) / 2,
            Self::Snd0Pcm8 { data, pos } => data.len().saturating_sub(*pos),
            Self::Snd1 {
                remaining_output, ..
            } => *remaining_output,
            Self::PcmDirect { samples, pos } => samples.len().saturating_sub(*pos),
        }
    }

    pub(super) fn read_samples(
        &mut self,
        out: &mut [i16],
    ) -> Result<usize, Error> {
        match self {
            Self::Snd0Pcm16 { data, pos } => read_snd0_pcm16(data, pos, out),
            Self::Snd0Pcm8 { data, pos } => read_snd0_pcm8(data, pos, out),
            Self::Snd1 {
                data,
                pos,
                payload_end,
                remaining_output,
                cur_sample,
                mode,
            } => read_snd1_samples(
                data,
                pos,
                *payload_end,
                remaining_output,
                cur_sample,
                mode,
                out,
            ),
            Self::PcmDirect { samples, pos } => read_pcm_direct(samples, pos, out),
        }
    }

    pub(super) fn into_owned_data(self) -> Option<Vec<u8>> {
        match self {
            Self::Snd0Pcm16 { data, .. }
            | Self::Snd0Pcm8 { data, .. }
            | Self::Snd1 { data, .. } => match data {
                Cow::Owned(data) => Some(data),
                Cow::Borrowed(_) => None,
            },
            // PcmDirect holds Vec<i16>, not Vec<u8> — no raw byte buffer to return.
            Self::PcmDirect { .. } => None,
        }
    }
}

/// Reads already-decoded `i16` samples directly — no conversion needed.
fn read_pcm_direct(samples: &[i16], pos: &mut usize, out: &mut [i16]) -> Result<usize, Error> {
    let available = samples.get(*pos..).unwrap_or(&[]);
    let count = available.len().min(out.len());
    if let (Some(src), Some(dst)) = (available.get(..count), out.get_mut(..count)) {
        dst.copy_from_slice(src);
    }
    *pos = pos.saturating_add(count);
    Ok(count)
}

/// Reads SND0 raw 16-bit LE PCM samples using a single bounds check per call.
fn read_snd0_pcm16(data: &[u8], pos: &mut usize, out: &mut [i16]) -> Result<usize, Error> {
    let available = data.get(*pos..).unwrap_or(&[]);
    let pairs = available.len() / 2;
    let count = pairs.min(out.len());
    for (slot, chunk) in out
        .get_mut(..count)
        .unwrap_or(&mut [])
        .iter_mut()
        .zip(available.chunks_exact(2))
    {
        let mut buf = [0u8; 2];
        buf.copy_from_slice(chunk);
        *slot = i16::from_le_bytes(buf);
    }
    *pos = pos.saturating_add(count.saturating_mul(2));
    Ok(count)
}

/// Reads SND0 raw 8-bit PCM samples, up-converting to 16-bit in a single pass.
fn read_snd0_pcm8(data: &[u8], pos: &mut usize, out: &mut [i16]) -> Result<usize, Error> {
    let available = data.get(*pos..).unwrap_or(&[]);
    let count = available.len().min(out.len());
    for (slot, &byte) in out
        .get_mut(..count)
        .unwrap_or(&mut [])
        .iter_mut()
        .zip(available.iter())
    {
        *slot = pcm8_to_i16(byte);
    }
    *pos = pos.saturating_add(count);
    Ok(count)
}

fn read_snd1_samples(
    data: &[u8],
    pos: &mut usize,
    payload_end: usize,
    remaining_output: &mut usize,
    cur_sample: &mut i16,
    mode: &mut Snd1Mode,
    out: &mut [i16],
) -> Result<usize, Error> {
    let mut written = 0usize;
    let out_len = out.len();

    while written < out.len() && *remaining_output > 0 {
        match mode {
            Snd1Mode::Idle => {
                if *pos >= payload_end {
                    return Err(Error::UnexpectedEof {
                        needed: pos.saturating_add(1),
                        available: data.len(),
                    });
                }
                let input_byte = data.get(*pos).copied().ok_or(Error::UnexpectedEof {
                    needed: pos.saturating_add(1),
                    available: data.len(),
                })?;
                *pos = pos.saturating_add(1);
                let input = (input_byte as u16) << 2;
                let code = (input >> 8) as u8;
                let count_raw = ((input & 0xFF) >> 2) as i8;

                *mode = match code {
                    2 if count_raw & 0x20 != 0 => Snd1Mode::Delta {
                        delta: ((count_raw << 3) >> 3) as i16,
                    },
                    2 => Snd1Mode::RawCopy {
                        remaining: (count_raw as u8).saturating_add(1) as usize,
                    },
                    1 => Snd1Mode::Adpcm4 {
                        remaining: (count_raw as u8).saturating_add(1) as usize,
                        current_byte: None,
                        stage: 0,
                    },
                    0 => Snd1Mode::Adpcm2 {
                        remaining: (count_raw as u8).saturating_add(1) as usize,
                        current_byte: None,
                        shift: 0,
                    },
                    _ => Snd1Mode::Repeat {
                        remaining: (count_raw as u8).saturating_add(1) as usize,
                    },
                };
            }
            Snd1Mode::Delta { delta } => {
                *cur_sample = cur_sample.saturating_add(*delta).clamp(0, 255);
                let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                    needed: written.saturating_add(1),
                    available: out_len,
                })?;
                *slot = pcm8_to_i16(*cur_sample as u8);
                *remaining_output = remaining_output.saturating_sub(1);
                written = written.saturating_add(1);
                *mode = Snd1Mode::Idle;
            }
            Snd1Mode::RawCopy { remaining } => {
                // Batch: copy as many bytes as fit in out[] in one slice copy.
                let can_write = (*remaining).min(out.len().saturating_sub(written));
                let can_read = payload_end.saturating_sub(*pos);
                let n = can_write.min(can_read);
                if n == 0 {
                    if *pos >= payload_end {
                        return Err(Error::UnexpectedEof {
                            needed: pos.saturating_add(1),
                            available: data.len(),
                        });
                    }
                    break;
                }
                let src = data.get(*pos..*pos + n).unwrap_or(&[]);
                let dst = out.get_mut(written..written + src.len()).unwrap_or(&mut []);
                for (d, &b) in dst.iter_mut().zip(src.iter()) {
                    *d = pcm8_to_i16(b);
                    *cur_sample = b as i16;
                }
                let actual = src.len();
                *pos = pos.saturating_add(actual);
                *remaining = remaining.saturating_sub(actual);
                *remaining_output = remaining_output.saturating_sub(actual);
                written = written.saturating_add(actual);
                if *remaining == 0 {
                    *mode = Snd1Mode::Idle;
                }
            }
            Snd1Mode::Adpcm4 {
                remaining,
                current_byte,
                stage,
            } => {
                if current_byte.is_none() {
                    if *remaining == 0 {
                        *mode = Snd1Mode::Idle;
                        continue;
                    }
                    if *pos >= payload_end {
                        return Err(Error::UnexpectedEof {
                            needed: pos.saturating_add(1),
                            available: data.len(),
                        });
                    }
                    *current_byte = data.get(*pos).copied();
                    *pos = pos.saturating_add(1);
                    *remaining = remaining.saturating_sub(1);
                    *stage = 0;
                }

                let byte = current_byte.unwrap_or(0);
                let nibble = if *stage == 0 { byte & 0x0F } else { byte >> 4 };
                let delta = WS_TABLE_4BIT.get(nibble as usize).copied().unwrap_or(0) as i16;
                *cur_sample = cur_sample.saturating_add(delta).clamp(0, 255);
                let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                    needed: written.saturating_add(1),
                    available: out_len,
                })?;
                *slot = pcm8_to_i16(*cur_sample as u8);
                *remaining_output = remaining_output.saturating_sub(1);
                written = written.saturating_add(1);

                if *stage == 0 {
                    *stage = 1;
                } else {
                    *current_byte = None;
                    *stage = 0;
                    if *remaining == 0 {
                        *mode = Snd1Mode::Idle;
                    }
                }
            }
            Snd1Mode::Adpcm2 {
                remaining,
                current_byte,
                shift,
            } => {
                if current_byte.is_none() {
                    if *remaining == 0 {
                        *mode = Snd1Mode::Idle;
                        continue;
                    }
                    if *pos >= payload_end {
                        return Err(Error::UnexpectedEof {
                            needed: pos.saturating_add(1),
                            available: data.len(),
                        });
                    }
                    *current_byte = data.get(*pos).copied();
                    *pos = pos.saturating_add(1);
                    *remaining = remaining.saturating_sub(1);
                    *shift = 0;
                }

                let byte = current_byte.unwrap_or(0);
                let nibble = ((byte >> *shift) & 0x03) as usize;
                let delta = WS_TABLE_2BIT.get(nibble).copied().unwrap_or(0) as i16;
                *cur_sample = cur_sample.saturating_add(delta).clamp(0, 255);
                let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                    needed: written.saturating_add(1),
                    available: out_len,
                })?;
                *slot = pcm8_to_i16(*cur_sample as u8);
                *remaining_output = remaining_output.saturating_sub(1);
                written = written.saturating_add(1);

                *shift = shift.saturating_add(2);
                if *shift >= 8 {
                    *current_byte = None;
                    *shift = 0;
                    if *remaining == 0 {
                        *mode = Snd1Mode::Idle;
                    }
                }
            }
            Snd1Mode::Repeat { remaining } => {
                // Batch: fill as many output slots as possible with the current sample.
                let n = (*remaining)
                    .min(*remaining_output)
                    .min(out.len().saturating_sub(written));
                let value = pcm8_to_i16(*cur_sample as u8);
                if let Some(dst) = out.get_mut(written..written.saturating_add(n)) {
                    dst.fill(value);
                }
                *remaining = remaining.saturating_sub(n);
                *remaining_output = remaining_output.saturating_sub(n);
                written = written.saturating_add(n);
                if *remaining == 0 {
                    *mode = Snd1Mode::Idle;
                }
            }
        }
    }

    Ok(written)
}

// ─── SND1: Westwood ADPCM ───────────────────────────────────────────────────

/// Westwood ADPCM (SND1) delta tables.
const WS_TABLE_2BIT: [i8; 4] = [-2, -1, 0, 1];
const WS_TABLE_4BIT: [i8; 16] = [-9, -8, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 8];
