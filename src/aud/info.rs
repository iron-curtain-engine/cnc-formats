// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{AudFile, AudHeader};

use std::time::Duration;

/// Declares whether an AUD session can restart from the beginning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudSeekSupport {
    /// The current session cannot rewind or restart the source.
    None,
    /// The current session can restart from the beginning of the payload.
    Restart,
}

/// First-class playback metadata for an AUD asset or session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudMediaInfo {
    /// Playback sample rate in Hz.
    pub sample_rate: u16,
    /// Number of decoded audio channels.
    pub channels: u8,
    /// Output PCM bit depth implied by the header flags.
    pub bits_per_sample: u8,
    /// Compression algorithm identifier from the AUD header.
    pub compression: u8,
    /// Total decoded sample frames implied by the header.
    pub sample_frames: usize,
    /// Nominal playback duration when the sample rate is known.
    pub duration: Option<Duration>,
    /// Whether the current session can restart from the beginning.
    pub seek_support: AudSeekSupport,
}

#[inline]
pub(crate) fn media_info_with_seek(
    header: &AudHeader,
    seek_support: AudSeekSupport,
) -> AudMediaInfo {
    AudMediaInfo {
        sample_rate: header.sample_rate,
        channels: header.channel_count(),
        bits_per_sample: if header.is_16bit() { 16 } else { 8 },
        compression: header.compression,
        sample_frames: header.sample_frames(),
        duration: header.duration(),
        seek_support,
    }
}

impl AudHeader {
    /// Returns first-class playback metadata derived from the AUD header.
    #[inline]
    pub fn media_info(&self) -> AudMediaInfo {
        media_info_with_seek(self, AudSeekSupport::None)
    }
}

impl AudFile<'_> {
    /// Returns first-class playback metadata for the parsed AUD file.
    #[inline]
    pub fn media_info(&self) -> AudMediaInfo {
        self.header.media_info()
    }
}
