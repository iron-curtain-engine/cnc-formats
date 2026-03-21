// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::decode::{VqaDecodeState, VqaFrame};
use super::{parse_finf, parse_vqhd, VqaHeader, VqaStream};
use crate::error::Error;
use crate::stream_io::io_error;

use std::collections::VecDeque;
use std::io::{Read, Seek};
use std::time::Duration;

mod audio;
mod buffer;
mod control;
mod info;
use audio::{
    audio_channels_usize, queue_stream_audio_chunk, recycle_audio_chunk_buffer,
    suggested_audio_chunk_capacity, VqaAudioDecodeState, VqaQueuedAudio,
};
pub use buffer::VqaFrameBuffer;

/// One incrementally decoded VQA frame.
///
/// This wrapper carries the decoded frame data plus its zero-based playback
/// index. Downstream runtimes typically upload `frame.pixels` into a persistent
/// texture and use `index` to coordinate queueing and presentation.
#[derive(Debug, Clone)]
pub struct VqaDecodedFrame {
    /// Zero-based decoded-frame index in playback order.
    pub index: u16,
    /// The decoded frame pixels and active palette snapshot.
    pub frame: VqaFrame,
}

/// One incrementally decoded VQA audio chunk.
///
/// The chunk payload is signed 16-bit PCM. `start_sample_frame` counts audio
/// sample frames, not individual interleaved `i16` values, so stereo timing is
/// stable regardless of channel count.
#[derive(Debug, Clone)]
pub struct VqaAudioChunk {
    /// Start position of this chunk in decoded sample frames.
    pub start_sample_frame: u64,
    /// Signed 16-bit PCM samples. Stereo is interleaved `[L, R, L, R, …]`.
    pub samples: Vec<i16>,
    /// Playback sample rate in Hz.
    pub sample_rate: u16,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u8,
}

impl VqaAudioChunk {
    /// Returns the number of decoded sample frames in this chunk.
    #[inline]
    pub fn sample_frames(&self) -> usize {
        let channels = usize::from(self.channels.max(1));
        self.samples.len() / channels
    }

    /// Returns the playback duration of this chunk if the sample rate is known.
    #[inline]
    pub fn duration(&self) -> Option<Duration> {
        if self.sample_rate == 0 {
            return None;
        }
        let frames = self.sample_frames() as u64;
        let secs = frames / u64::from(self.sample_rate);
        let nanos = ((frames % u64::from(self.sample_rate)) * 1_000_000_000u64)
            / u64::from(self.sample_rate);
        Some(Duration::new(secs, nanos as u32))
    }
}

/// Incremental VQA decoder for low-latency playback.
///
/// This session parses container metadata once, then advances through the
/// underlying stream incrementally. Callers can:
///
/// - inspect width, height, fps, frame count, and audio metadata up front
/// - decode frames one at a time
/// - decode raw audio chunks as they appear
/// - decode enough audio to cover one video-frame interval
/// - rewind back to the start when the input is seekable
///
/// The decoder is engine-agnostic: it does not own clocks, queues, textures,
/// or audio devices. Downstream runtimes are expected to use the exposed
/// metadata and incremental outputs to implement bounded preroll, audio-master
/// clocks, and presentation scheduling externally.
///
/// `rewind()` is supported. Arbitrary random seek is intentionally out of
/// scope for now because VQA playback depends on evolving codebook and palette
/// state; callers that need mid-stream seek should currently restart and fast-
/// forward from the beginning.
#[derive(Debug)]
pub struct VqaDecoder<R> {
    stream: Option<VqaStream<R>>,
    stream_start: u64,
    header: VqaHeader,
    frame_index: Option<Vec<u32>>,
    frame_decoder: VqaDecodeState,
    audio_decoder: VqaAudioDecodeState,
    frame_queue: VecDeque<VqaDecodedFrame>,
    audio_queue: VecDeque<VqaQueuedAudio>,
    audio_chunk_pool: Vec<Vec<u8>>,
    next_frame_index: u16,
    audio_sample_frames_delivered: u64,
    frame_audio_remainder: u64,
    ended: bool,
}

type ReopenedVqaStream<R> = (VqaStream<R>, VqaHeader, Option<Vec<u32>>);

fn preallocate_audio_chunk_pool(header: &VqaHeader) -> Vec<Vec<u8>> {
    let capacity = suggested_audio_chunk_capacity(header);
    if capacity == 0 {
        Vec::new()
    } else {
        vec![Vec::with_capacity(capacity)]
    }
}

fn suggested_stream_chunk_capacity(header: &VqaHeader) -> usize {
    usize::from(header.max_frame_size).max(suggested_audio_chunk_capacity(header))
}

impl<R: Read + Seek> VqaDecoder<R> {
    /// Opens a VQA decoder from a seekable reader.
    ///
    /// Metadata is parsed before the first frame or audio decode, so downstream
    /// code can size buffers, choose queue lengths, and start preroll without
    /// materializing the whole movie.
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let stream_start = reader
            .stream_position()
            .map_err(|err| io_error("capturing VQA decoder start position", err))?;
        let (stream, header, frame_index) = Self::reopen_primed_stream(reader, stream_start)?;
        let frame_decoder = VqaDecodeState::new(header.clone());
        let audio_decoder = VqaAudioDecodeState::new(&header);
        let audio_chunk_pool = preallocate_audio_chunk_pool(&header);

        Ok(Self {
            stream: Some(stream),
            stream_start,
            header,
            frame_index,
            frame_decoder,
            audio_decoder,
            frame_queue: VecDeque::new(),
            audio_queue: VecDeque::new(),
            audio_chunk_pool,
            next_frame_index: 0,
            audio_sample_frames_delivered: 0,
            frame_audio_remainder: 0,
            ended: false,
        })
    }

    /// Returns the decoded frame width in pixels.
    #[inline]
    pub fn width(&self) -> u16 {
        self.header.width
    }

    /// Returns the decoded frame height in pixels.
    #[inline]
    pub fn height(&self) -> u16 {
        self.header.height
    }

    /// Returns the nominal playback rate in frames per second.
    #[inline]
    pub fn fps(&self) -> u8 {
        self.header.fps.max(1)
    }

    /// Returns the declared frame count from the VQHD header.
    #[inline]
    pub fn frame_count(&self) -> u16 {
        self.header.num_frames
    }

    /// Returns `true` when the VQHD header declares audio.
    #[inline]
    pub fn has_audio(&self) -> bool {
        self.header.has_audio()
    }

    /// Returns the audio sample rate when the file declares audio.
    #[inline]
    pub fn audio_sample_rate(&self) -> Option<u16> {
        if self.has_audio() {
            Some(self.audio_decoder.sample_rate)
        } else {
            None
        }
    }

    /// Returns the audio channel count when the file declares audio.
    #[inline]
    pub fn audio_channels(&self) -> Option<u8> {
        if self.has_audio() {
            Some(self.audio_decoder.channels)
        } else {
            None
        }
    }

    /// Returns the parsed FINF frame index when it was available up front.
    #[inline]
    pub fn frame_index(&self) -> Option<&[u32]> {
        self.frame_index.as_deref()
    }

    /// Decodes the next video frame in playback order.
    ///
    /// Audio chunks encountered before that frame are preserved internally and
    /// can be read later with [`Self::next_audio_chunk`] or
    /// [`Self::next_audio_for_frame_interval`].
    pub fn next_frame(&mut self) -> Result<Option<VqaDecodedFrame>, Error> {
        loop {
            if let Some(frame) = self.frame_queue.pop_front() {
                return Ok(Some(frame));
            }
            if !self.pump_once()? {
                return Ok(None);
            }
        }
    }

    /// Decodes the next audio chunk in playback order.
    ///
    /// Video frames encountered before that audio chunk are preserved
    /// internally for later retrieval via [`Self::next_frame`].
    pub fn next_audio_chunk(&mut self) -> Result<Option<VqaAudioChunk>, Error> {
        if !self.has_audio() {
            return Ok(None);
        }

        loop {
            if let Some(audio) = self.audio_queue.pop_front() {
                let (audio, backing) = audio.into_audio_chunk(&mut self.audio_decoder)?;
                if let Some(buf) = backing {
                    recycle_audio_chunk_buffer(&mut self.audio_chunk_pool, buf);
                }
                let audio = match audio {
                    Some(audio) => audio,
                    None => continue,
                };
                self.audio_sample_frames_delivered = self
                    .audio_sample_frames_delivered
                    .saturating_add(audio.sample_frames() as u64);
                return Ok(Some(audio));
            }
            if !self.pump_once()? {
                return Ok(None);
            }
        }
    }

    /// Decodes enough audio PCM to cover one nominal video-frame interval.
    ///
    /// This method carries fractional `sample_rate / fps` remainder across
    /// calls, so downstream runtimes can feed audio-driven playback with small,
    /// bounded preroll instead of extracting the full soundtrack up front.
    pub fn next_audio_for_frame_interval(&mut self) -> Result<Option<VqaAudioChunk>, Error> {
        if !self.has_audio() {
            return Ok(None);
        }

        let target_frames = self.next_frame_audio_target();
        if target_frames == 0 {
            return Ok(None);
        }

        while self.queued_audio_frames() < target_frames && !self.ended {
            if !self.pump_once()? {
                break;
            }
        }

        self.take_audio_frames(target_frames)
    }

    fn reopen_primed_stream(reader: R, stream_start: u64) -> Result<ReopenedVqaStream<R>, Error> {
        let (header, frame_index, mut reader) = Self::prime_stream(reader)?;
        reader
            .seek(std::io::SeekFrom::Start(stream_start))
            .map_err(|err| io_error("rewinding primed VQA stream", err))?;
        let mut stream = VqaStream::open(reader)?;
        stream.reserve_chunk_capacity(suggested_stream_chunk_capacity(&header));
        Ok((stream, header, frame_index))
    }

    fn prime_stream(reader: R) -> Result<(VqaHeader, Option<Vec<u32>>, R), Error> {
        let mut stream = VqaStream::open(reader)?;
        let mut header: Option<VqaHeader> = None;
        let mut frame_index: Option<Vec<u32>> = None;

        while let Some(chunk) = stream.next_chunk()? {
            if chunk.fourcc == *b"VQHD" && header.is_none() {
                header = Some(parse_vqhd(chunk.data)?);
            }
            if chunk.fourcc == *b"FINF" && frame_index.is_none() {
                if let Some(ref parsed_header) = header {
                    frame_index = Some(parse_finf(chunk.data, parsed_header.num_frames)?);
                }
            }

            let is_metadata = matches!(&chunk.fourcc, b"VQHD" | b"FINF");
            if header.is_some() && !is_metadata {
                break;
            }
        }

        let final_header = header.ok_or(Error::InvalidMagic {
            context: "VQA VQHD header",
        })?;
        let reader = stream.into_inner();
        Ok((final_header, frame_index, reader))
    }

    fn pump_once(&mut self) -> Result<bool, Error> {
        let Self {
            stream,
            frame_decoder,
            audio_decoder,
            frame_queue,
            audio_queue,
            audio_chunk_pool,
            next_frame_index,
            ended,
            ..
        } = self;
        let chunk = match stream
            .as_mut()
            .ok_or(Error::DecompressionError {
                reason: "VQA decoder stream is unavailable",
            })?
            .next_chunk()?
        {
            Some(chunk) => chunk,
            None => {
                *ended = true;
                return Ok(false);
            }
        };

        if let Some(frame) = frame_decoder.apply_chunk(&chunk.fourcc, chunk.data)? {
            frame_queue.push_back(VqaDecodedFrame {
                index: *next_frame_index,
                frame,
            });
            *next_frame_index = next_frame_index.saturating_add(1);
            return Ok(true);
        }

        if queue_stream_audio_chunk(
            audio_decoder,
            audio_queue,
            audio_chunk_pool,
            &chunk.fourcc,
            chunk.data,
        )? {
            return Ok(true);
        }

        Ok(true)
    }

    #[inline]
    fn queued_audio_frames(&self) -> usize {
        self.audio_queue
            .iter()
            .map(|chunk| chunk.sample_frames(self.audio_decoder.channels))
            .sum()
    }

    #[inline]
    fn next_frame_audio_target(&mut self) -> usize {
        let numerator = self
            .frame_audio_remainder
            .saturating_add(u64::from(self.audio_decoder.sample_rate));
        let fps = u64::from(self.fps().max(1));
        let frames = numerator / fps;
        self.frame_audio_remainder = numerator % fps;
        frames as usize
    }

    fn take_audio_frames(&mut self, target_frames: usize) -> Result<Option<VqaAudioChunk>, Error> {
        if target_frames == 0 || self.audio_queue.is_empty() {
            return Ok(None);
        }

        let first = self.audio_queue.front().ok_or(Error::UnexpectedEof {
            needed: 1,
            available: 0,
        })?;
        let mut remaining = target_frames;
        let start_sample_frame = first.start_sample_frame;
        let sample_rate = self.audio_decoder.sample_rate;
        let channels = self.audio_decoder.channels.max(1);
        let channels_usize = audio_channels_usize(channels);
        let mut combined = Vec::with_capacity(target_frames.saturating_mul(channels_usize));

        while remaining > 0 {
            let mut front = match self.audio_queue.pop_front() {
                Some(chunk) => chunk,
                None => break,
            };
            let chunk_frames = front.sample_frames(channels);
            if chunk_frames == 0 {
                if !front.is_drained() {
                    self.audio_queue.push_front(front);
                    break;
                }
                continue;
            }

            let take_frames = chunk_frames.min(remaining);
            let take_samples = take_frames.saturating_mul(channels_usize);
            let start = combined.len();
            let end = start.saturating_add(take_samples);
            combined.resize(end, 0);
            let combined_len = combined.len();
            let dst = combined.get_mut(start..end).ok_or(Error::UnexpectedEof {
                needed: end,
                available: combined_len,
            })?;
            let read = front.read_samples(&mut self.audio_decoder, dst)?;
            combined.truncate(start.saturating_add(read));
            remaining = remaining.saturating_sub(read / channels_usize);

            if !front.is_drained() {
                self.audio_queue.push_front(front);
            } else if let Some(buf) = front.into_backing_buffer() {
                recycle_audio_chunk_buffer(&mut self.audio_chunk_pool, buf);
            }
            if read == 0 {
                break;
            }
        }

        if combined.is_empty() {
            return Ok(None);
        }

        let audio = VqaAudioChunk {
            start_sample_frame,
            samples: combined,
            sample_rate,
            channels,
        };
        self.audio_sample_frames_delivered = self
            .audio_sample_frames_delivered
            .saturating_add(audio.sample_frames() as u64);
        Ok(Some(audio))
    }
}
