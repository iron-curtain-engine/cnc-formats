// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

impl<R: Read + Seek> VqaDecoder<R> {
    /// Returns the nominal duration of one video frame.
    #[inline]
    pub fn frame_duration(&self) -> Duration {
        self.header.frame_duration()
    }

    /// Returns the nominal presentation time of `frame_index`.
    #[inline]
    pub fn frame_timestamp(&self, frame_index: u16) -> Option<Duration> {
        self.header.frame_timestamp(frame_index)
    }

    /// Maps playback time to the frame that should be presented then.
    #[inline]
    pub fn frame_index_for_time(&self, time: Duration) -> Option<u16> {
        self.header.frame_index_for_time(time)
    }

    /// Returns one decoded FINF entry for `frame_index`.
    #[inline]
    pub fn frame_index_entry(&self, frame_index: u16) -> Option<super::super::VqaFrameIndexEntry> {
        let raw = self
            .frame_index
            .as_ref()?
            .get(usize::from(frame_index))
            .copied()?;
        Some(super::super::timing::frame_index_entry_from_raw(
            frame_index,
            raw,
        ))
    }

    /// Returns the decoded FINF table when one was available up front.
    pub fn frame_index_entries(&self) -> Option<Vec<super::super::VqaFrameIndexEntry>> {
        let entries = self.frame_index.as_ref()?;
        Some(
            entries
                .iter()
                .enumerate()
                .map(|(index, raw)| {
                    super::super::timing::frame_index_entry_from_raw(index as u16, *raw)
                })
                .collect(),
        )
    }

    /// Returns the exact rational relationship between audio sample frames and one video frame.
    #[inline]
    pub fn audio_sample_frames_per_video_frame(&self) -> Option<(u32, u32)> {
        self.header.audio_sample_frames_per_video_frame()
    }

    /// Returns the nominal presentation duration implied by `frame_count / fps`.
    #[inline]
    pub fn duration(&self) -> Duration {
        self.header.duration()
    }

    /// Returns the number of decoded-but-not-yet-consumed video frames.
    #[inline]
    pub fn queued_frame_count(&self) -> usize {
        self.frame_queue.len()
    }

    /// Returns the number of decoded-but-not-yet-consumed audio sample frames.
    #[inline]
    pub fn queued_audio_sample_frames(&self) -> usize {
        self.queued_audio_frames()
    }

    /// Returns the duration of queued audio when the stream has audio.
    pub fn queued_audio_duration(&self) -> Option<Duration> {
        let sample_rate = self.audio_sample_rate()?;
        let frames = self.queued_audio_sample_frames() as u64;
        let secs = frames / u64::from(sample_rate);
        let nanos = ((frames % u64::from(sample_rate)) * 1_000_000_000u64) / u64::from(sample_rate);
        Some(Duration::new(secs, nanos as u32))
    }

    /// Returns `true` when no more container data or queued media remains.
    #[inline]
    pub fn is_drained(&self) -> bool {
        self.ended && self.frame_queue.is_empty() && self.audio_queue.is_empty()
    }

    /// Rewinds the decoder back to the start of the VQA stream.
    ///
    /// Decoder state, queued frames, queued audio, and fractional timing
    /// remainder are all reset. Metadata is re-read from the stream so the
    /// session behaves identically to a fresh open on the same input bytes.
    pub fn rewind(&mut self) -> Result<(), Error> {
        let stream = self.stream.take().ok_or(Error::DecompressionError {
            reason: "VQA decoder stream is unavailable",
        })?;
        let mut reader = stream.into_inner();
        reader
            .seek(std::io::SeekFrom::Start(self.stream_start))
            .map_err(|err| io_error("rewinding VQA decoder", err))?;
        let (stream, header, frame_index) = Self::reopen_primed_stream(reader, self.stream_start)?;

        self.stream = Some(stream);
        self.header = header.clone();
        self.frame_index = frame_index;
        self.frame_decoder = VqaDecodeState::new(header.clone());
        self.audio_decoder = VqaAudioDecodeState::new(&header);
        self.frame_queue.clear();
        self.audio_queue.clear();
        self.audio_chunk_pool = preallocate_audio_chunk_pool(&header);
        self.next_frame_index = 0;
        self.audio_sample_frames_delivered = 0;
        self.frame_audio_remainder = 0;
        self.ended = false;
        Ok(())
    }

    /// Alias for [`Self::rewind`] when callers prefer restart terminology.
    #[inline]
    pub fn restart(&mut self) -> Result<(), Error> {
        self.rewind()
    }

    /// Seeks to `target_frame` by restarting and decoding forward linearly.
    ///
    /// This decoder does not currently jump into the middle of the stream from
    /// FINF offsets because codebook and palette state evolve over time.
    /// Seeking is therefore deterministic but O(target_frame): prior frames are
    /// decoded and discarded, and pre-target audio is dropped.
    pub fn seek_to_frame(&mut self, target_frame: u16) -> Result<(), Error> {
        if target_frame > self.frame_count() {
            return Err(Error::InvalidOffset {
                offset: usize::from(target_frame),
                bound: usize::from(self.frame_count()),
            });
        }

        self.rewind()?;
        for _ in 0..target_frame {
            match self.next_frame()? {
                Some(_) => self.audio_queue.clear(),
                None => {
                    return Err(Error::InvalidOffset {
                        offset: usize::from(target_frame),
                        bound: usize::from(self.frame_count()),
                    });
                }
            }
        }
        self.audio_queue.clear();
        self.frame_audio_remainder = self.audio_remainder_for_completed_frames(target_frame);
        Ok(())
    }

    /// Seeks to the frame that should be presented at `time`.
    ///
    /// The mapping is frame-based and clamps to the last frame. When the file
    /// declares zero frames, this falls back to a plain rewind.
    pub fn seek_to_time(&mut self, time: Duration) -> Result<(), Error> {
        match self.frame_index_for_time(time) {
            Some(frame_index) => self.seek_to_frame(frame_index),
            None => self.rewind(),
        }
    }

    fn audio_remainder_for_completed_frames(&self, completed_frames: u16) -> u64 {
        if !self.has_audio() {
            return 0;
        }
        let fps = u64::from(self.fps().max(1));
        (u64::from(completed_frames) * u64::from(self.audio_decoder.sample_rate)) % fps
    }
}
