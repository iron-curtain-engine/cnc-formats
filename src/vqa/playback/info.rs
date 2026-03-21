// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

impl<R: Read + Seek> VqaDecoder<R> {
    /// Returns first-class playback metadata for the opened VQA session.
    ///
    /// This is the inspect-before-decode surface downstream runtimes should
    /// use when sizing buffers, deciding preroll depth, or exposing media
    /// details in tools and content browsers.
    #[inline]
    pub fn media_info(&self) -> super::super::VqaMediaInfo {
        super::super::info::build_media_info(
            &self.header,
            self.frame_index.as_ref().map_or(0, Vec::len),
        )
    }

    /// Returns the decoder's seek contract.
    #[inline]
    pub fn seek_support(&self) -> super::super::VqaSeekSupport {
        self.media_info().seek_support
    }

    /// Returns a timing-aware FINF seek/index view when one was available.
    #[inline]
    pub fn seek_index(&self) -> Option<super::super::VqaSeekIndex> {
        super::super::info::build_seek_index(self.frame_index.as_deref(), &self.header)
    }

    /// Returns the number of sample frames already delivered to the caller.
    #[inline]
    pub fn decoded_audio_sample_frames(&self) -> u64 {
        self.audio_sample_frames_delivered
    }

    /// Returns the playback timestamp of the next unread audio sample frame.
    pub fn decoded_audio_duration(&self) -> Option<Duration> {
        let sample_rate = self.audio_sample_rate()?;
        let frames = self.decoded_audio_sample_frames();
        let secs = frames / u64::from(sample_rate);
        let nanos = ((frames % u64::from(sample_rate)) * 1_000_000_000u64) / u64::from(sample_rate);
        Some(Duration::new(secs, nanos as u32))
    }
}
