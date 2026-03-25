// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::super::snd::{decode_snd2_chunk_stateful, VqaAudioChunkDecoder};
use super::super::VqaHeader;
use super::VqaAudioChunk;
use crate::error::Error;

use std::borrow::Cow;
use std::collections::VecDeque;

#[derive(Debug)]
pub(super) struct VqaQueuedAudio {
    pub(super) start_sample_frame: u64,
    decoder: VqaAudioChunkDecoder<'static>,
}

impl VqaQueuedAudio {
    #[inline]
    pub(super) fn sample_frames(&self, channels: u8) -> usize {
        self.decoder.remaining_sample_count() / audio_channels_usize(channels)
    }

    #[inline]
    pub(super) fn is_drained(&self) -> bool {
        self.decoder.is_finished()
    }

    pub(super) fn read_samples(
        &mut self,
        audio: &VqaAudioDecodeState,
        out: &mut [i16],
    ) -> Result<usize, Error> {
        let written = self.decoder.read_samples(out)?;
        self.start_sample_frame = self
            .start_sample_frame
            .saturating_add((written / audio_channels_usize(audio.channels)) as u64);
        Ok(written)
    }

    pub(super) fn into_audio_chunk(
        mut self,
        audio: &VqaAudioDecodeState,
    ) -> Result<(Option<VqaAudioChunk>, Option<Vec<u8>>), Error> {
        let sample_count = self.decoder.remaining_sample_count();
        if sample_count == 0 {
            return Ok((None, self.decoder.into_owned_data()));
        }

        let start_sample_frame = self.start_sample_frame;
        let mut samples = vec![0i16; sample_count];
        let read = self.read_samples(audio, &mut samples)?;
        let backing = self.decoder.into_owned_data();
        samples.truncate(read);
        if samples.is_empty() {
            return Ok((None, backing));
        }

        Ok((
            Some(VqaAudioChunk {
                start_sample_frame,
                samples,
                sample_rate: audio.sample_rate,
                channels: audio.channels,
            }),
            backing,
        ))
    }

    pub(super) fn into_backing_buffer(self) -> Option<Vec<u8>> {
        self.decoder.into_owned_data()
    }
}

#[derive(Debug)]
pub(super) struct VqaAudioDecodeState {
    has_audio: bool,
    pub(super) sample_rate: u16,
    pub(super) channels: u8,
    bits: u8,
    next_start_sample_frame: u64,
    /// IMA ADPCM left-channel sample state, carried across SND2 chunk boundaries.
    ima_l_sample: i32,
    /// IMA ADPCM left-channel step-index state, carried across SND2 chunk boundaries.
    ima_l_index: usize,
    /// IMA ADPCM right-channel sample state, carried across SND2 chunk boundaries.
    ima_r_sample: i32,
    /// IMA ADPCM right-channel step-index state, carried across SND2 chunk boundaries.
    ima_r_index: usize,
}

impl VqaAudioDecodeState {
    pub(super) fn new(header: &VqaHeader) -> Self {
        Self {
            has_audio: header.has_audio(),
            sample_rate: if header.freq == 0 { 22050 } else { header.freq },
            channels: if header.channels == 0 {
                1
            } else {
                header.channels
            },
            bits: header.bits,
            next_start_sample_frame: 0,
            ima_l_sample: 0,
            ima_l_index: 0,
            ima_r_sample: 0,
            ima_r_index: 0,
        }
    }

    pub(super) fn queue_chunk(
        &mut self,
        fourcc: &[u8; 4],
        data: Vec<u8>,
    ) -> Result<(Option<VqaQueuedAudio>, Option<Vec<u8>>), Error> {
        if !self.has_audio {
            return Ok((None, Some(data)));
        }

        // SND2 (IMA ADPCM): decode directly, carrying state across chunk boundaries.
        if fourcc == b"SND2" {
            let stereo = self.channels >= 2;
            let pcm = decode_snd2_chunk_stateful(
                &data,
                stereo,
                &mut self.ima_l_sample,
                &mut self.ima_l_index,
                &mut self.ima_r_sample,
                &mut self.ima_r_index,
            );
            // Recycle the raw compressed buffer back to the caller.
            let backing = Some(data);
            let sample_count = pcm.len();
            let sample_frames = sample_count / audio_channels_usize(self.channels);
            if sample_frames == 0 {
                return Ok((None, backing));
            }
            // Pack decoded PCM into bytes for Snd0Pcm16.
            let mut bytes = Vec::with_capacity(sample_count * 2);
            for s in &pcm {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            let decoder = VqaAudioChunkDecoder::Snd0Pcm16 {
                data: Cow::Owned(bytes),
                pos: 0,
            };
            let chunk = VqaQueuedAudio {
                start_sample_frame: self.next_start_sample_frame,
                decoder,
            };
            self.next_start_sample_frame = self
                .next_start_sample_frame
                .saturating_add(sample_frames as u64);
            return Ok((Some(chunk), backing));
        }

        let decoder =
            match VqaAudioChunkDecoder::open_owned(fourcc, data, self.bits, self.channels >= 2)? {
                Some(decoder) => decoder,
                None => return Ok((None, None)),
            };
        let sample_frames = decoder.remaining_sample_count() / audio_channels_usize(self.channels);
        if sample_frames == 0 {
            return Ok((None, decoder.into_owned_data()));
        }

        let chunk = VqaQueuedAudio {
            start_sample_frame: self.next_start_sample_frame,
            decoder,
        };
        self.next_start_sample_frame = self
            .next_start_sample_frame
            .saturating_add(sample_frames as u64);
        Ok((Some(chunk), None))
    }
}

#[inline]
pub(super) fn audio_channels_usize(channels: u8) -> usize {
    usize::from(channels.max(1))
}

#[inline]
fn is_vqa_audio_chunk(fourcc: &[u8; 4]) -> bool {
    matches!(fourcc, b"SND0" | b"SND1" | b"SND2")
}

fn take_audio_chunk_buffer(pool: &mut Vec<Vec<u8>>, data: &[u8]) -> Vec<u8> {
    let mut buf = pool.pop().unwrap_or_default();
    buf.clear();
    buf.extend_from_slice(data);
    buf
}

pub(super) fn recycle_audio_chunk_buffer(pool: &mut Vec<Vec<u8>>, mut buf: Vec<u8>) {
    buf.clear();
    pool.push(buf);
}

pub(super) fn queue_stream_audio_chunk(
    audio_decoder: &mut VqaAudioDecodeState,
    audio_queue: &mut VecDeque<VqaQueuedAudio>,
    audio_chunk_pool: &mut Vec<Vec<u8>>,
    fourcc: &[u8; 4],
    data: &[u8],
) -> Result<bool, Error> {
    if !is_vqa_audio_chunk(fourcc) {
        return Ok(false);
    }

    let owned = take_audio_chunk_buffer(audio_chunk_pool, data);
    let (chunk, unused) = audio_decoder.queue_chunk(fourcc, owned)?;
    if let Some(buf) = unused {
        recycle_audio_chunk_buffer(audio_chunk_pool, buf);
    }
    if let Some(chunk) = chunk {
        audio_queue.push_back(chunk);
        return Ok(true);
    }

    Ok(false)
}

pub(super) fn suggested_audio_chunk_capacity(header: &VqaHeader) -> usize {
    if !header.has_audio() {
        return 0;
    }

    let fps = u64::from(header.fps.max(1));
    let sample_rate = u64::from(if header.freq == 0 { 22050 } else { header.freq });
    let channels = u64::from(header.channels.max(1));
    let bytes_per_sample = if header.bits == 16 { 2u64 } else { 1u64 };
    let sample_frames = sample_rate
        .saturating_add(fps.saturating_sub(1))
        .saturating_div(fps);
    let payload_bytes = sample_frames
        .saturating_mul(channels)
        .saturating_mul(bytes_per_sample);
    payload_bytes.saturating_add(4).min(usize::MAX as u64) as usize
}
