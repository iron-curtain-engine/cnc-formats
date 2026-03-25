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
    data: &[u8],
    stereo: bool,
    l_sample: &mut i32,
    l_index: &mut usize,
    r_sample: &mut i32,
    r_index: &mut usize,
) -> Vec<i16> {
    if stereo {
        let half = data.len() / 2;
        let left_data = data.get(..half).unwrap_or(&[]);
        let right_data = data.get(half..).unwrap_or(&[]);

        let mut left_pcm: Vec<i16> = Vec::with_capacity(left_data.len() * 2);
        for &byte in left_data {
            left_pcm.push(ima_decode_nibble(byte & 0x0F, l_sample, l_index));
            left_pcm.push(ima_decode_nibble(byte >> 4, l_sample, l_index));
        }

        let mut right_pcm: Vec<i16> = Vec::with_capacity(right_data.len() * 2);
        for &byte in right_data {
            right_pcm.push(ima_decode_nibble(byte & 0x0F, r_sample, r_index));
            right_pcm.push(ima_decode_nibble(byte >> 4, r_sample, r_index));
        }

        // Interleave L/R.
        let n = left_pcm.len().min(right_pcm.len());
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            out.push(left_pcm[i]);
            out.push(right_pcm[i]);
        }
        out
    } else {
        let mut out = Vec::with_capacity(data.len() * 2);
        for &byte in data {
            out.push(ima_decode_nibble(byte & 0x0F, l_sample, l_index));
            out.push(ima_decode_nibble(byte >> 4, l_sample, l_index));
        }
        out
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
    decode_snd2_chunk_stateful(data, stereo, &mut ls, &mut li, &mut rs, &mut ri)
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
                let pcm = decode_snd2_chunk(data.as_ref(), stereo);
                let mut bytes = Vec::with_capacity(pcm.len() * 2);
                for s in pcm {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                Ok(Some(VqaAudioChunkDecoder::Snd0Pcm16 {
                    data: Cow::Owned(bytes),
                    pos: 0,
                }))
            }
            _ => Ok(None),
        }
    }

    fn open_snd1(data: Cow<'_, [u8]>) -> Result<VqaAudioChunkDecoder<'_>, Error> {
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
            cur_sample: 0x80,
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
        }
    }

    pub(super) fn remaining_sample_count(&self) -> usize {
        match self {
            Self::Snd0Pcm16 { data, pos } => (data.len().saturating_sub(*pos)) / 2,
            Self::Snd0Pcm8 { data, pos } => data.len().saturating_sub(*pos),
            Self::Snd1 {
                remaining_output, ..
            } => *remaining_output,
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
        }
    }
}

fn read_snd0_pcm16(data: &[u8], pos: &mut usize, out: &mut [i16]) -> Result<usize, Error> {
    let mut written = 0usize;
    let out_len = out.len();
    while written < out.len() {
        let end = pos.saturating_add(2);
        let sample_bytes = match data.get(*pos..end) {
            Some(bytes) => bytes,
            None => break,
        };
        let mut buf = [0u8; 2];
        buf.copy_from_slice(sample_bytes);
        let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
            needed: written.saturating_add(1),
            available: out_len,
        })?;
        *slot = i16::from_le_bytes(buf);
        *pos = end;
        written = written.saturating_add(1);
    }
    Ok(written)
}

fn read_snd0_pcm8(data: &[u8], pos: &mut usize, out: &mut [i16]) -> Result<usize, Error> {
    let mut written = 0usize;
    let out_len = out.len();
    while written < out.len() {
        let byte = match data.get(*pos).copied() {
            Some(byte) => byte,
            None => break,
        };
        let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
            needed: written.saturating_add(1),
            available: out_len,
        })?;
        *slot = pcm8_to_i16(byte);
        *pos = pos.saturating_add(1);
        written = written.saturating_add(1);
    }
    Ok(written)
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
                if *pos >= payload_end {
                    return Err(Error::UnexpectedEof {
                        needed: pos.saturating_add(1),
                        available: data.len(),
                    });
                }
                let byte = data.get(*pos).copied().ok_or(Error::UnexpectedEof {
                    needed: pos.saturating_add(1),
                    available: data.len(),
                })?;
                let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                    needed: written.saturating_add(1),
                    available: out_len,
                })?;
                *slot = pcm8_to_i16(byte);
                *cur_sample = byte as i16;
                *pos = pos.saturating_add(1);
                *remaining = remaining.saturating_sub(1);
                *remaining_output = remaining_output.saturating_sub(1);
                written = written.saturating_add(1);
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
                let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                    needed: written.saturating_add(1),
                    available: out_len,
                })?;
                *slot = pcm8_to_i16(*cur_sample as u8);
                *remaining = remaining.saturating_sub(1);
                *remaining_output = remaining_output.saturating_sub(1);
                written = written.saturating_add(1);
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
