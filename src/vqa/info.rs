// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{VqaFile, VqaFrameIndexEntry, VqaHeader};

use std::time::Duration;

/// Declares how a VQA session can position playback.
///
/// This type describes seek behavior, not whether a player should seek.
/// Downstream runtimes use it to choose between direct restart, linear
/// decode-forward, or future index-aware heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VqaSeekSupport {
    /// Playback can rewind and decode forward from the beginning.
    LinearFromStart,
    /// Playback can rewind/decode forward and exposes FINF index metadata.
    IndexedLinearFromStart,
}

impl VqaSeekSupport {
    /// Returns `true` when FINF index metadata is available.
    #[inline]
    pub fn has_index(self) -> bool {
        matches!(self, Self::IndexedLinearFromStart)
    }
}

/// One timing-aware seek point derived from a FINF entry.
///
/// VQA does not expose Matroska-style cue richness, so this type keeps the raw
/// FINF entry plus the nominal presentation timestamp and byte offset implied
/// by that entry. Callers should treat it as an index/diagnostic surface, not
/// a promise of true random-access decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VqaSeekPoint {
    /// The decoded FINF entry.
    pub entry: VqaFrameIndexEntry,
    /// Nominal presentation timestamp of the indexed frame.
    pub timestamp: Duration,
    /// Raw FINF offset converted from 16-bit words to bytes.
    pub byte_offset: u64,
}

impl VqaSeekPoint {
    /// Returns `true` when the underlying FINF entry has any flag bits set.
    #[inline]
    pub fn has_flags(&self) -> bool {
        self.entry.has_flags()
    }
}

/// Timing/index metadata for a VQA stream.
///
/// This is the VQA-specific analogue of a lightweight container cue table.
/// It is intentionally narrower than Matroska Cues: VQA still requires
/// decoder state reconstruction, so these entries are best used for seek UI,
/// diagnostics, and future anchor-selection heuristics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaSeekIndex {
    entries: Vec<VqaSeekPoint>,
    seek_support: VqaSeekSupport,
}

impl VqaSeekIndex {
    /// Returns the derived seek points in playback order.
    #[inline]
    pub fn entries(&self) -> &[VqaSeekPoint] {
        &self.entries
    }

    /// Returns the number of indexed frames.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the index contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the seek contract associated with this index.
    #[inline]
    pub fn seek_support(&self) -> VqaSeekSupport {
        self.seek_support
    }
}

/// First-class playback metadata for a VQA asset.
///
/// This keeps timing, audio, and index availability explicit so downstream
/// runtimes can inspect a movie before deciding whether to queue, preroll,
/// seek, or reject it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaMediaInfo {
    /// Display width in pixels.
    pub width: u16,
    /// Display height in pixels.
    pub height: u16,
    /// Nominal playback rate in frames per second.
    pub fps: u8,
    /// Declared number of video frames.
    pub frame_count: u16,
    /// Nominal duration of one video frame.
    pub frame_duration: Duration,
    /// Nominal total playback duration implied by `frame_count / fps`.
    pub duration: Duration,
    /// Whether the stream declares audio.
    pub has_audio: bool,
    /// Audio sample rate when audio is declared.
    pub audio_sample_rate: Option<u16>,
    /// Audio channel count when audio is declared.
    pub audio_channels: Option<u8>,
    /// Audio bit depth when audio is declared.
    pub audio_bits: Option<u8>,
    /// Number of FINF entries available up front.
    pub index_entry_count: usize,
    /// How the session can position playback.
    pub seek_support: VqaSeekSupport,
}

#[inline]
pub(crate) fn build_seek_support(index_entry_count: usize) -> VqaSeekSupport {
    if index_entry_count > 0 {
        VqaSeekSupport::IndexedLinearFromStart
    } else {
        VqaSeekSupport::LinearFromStart
    }
}

#[inline]
pub(crate) fn build_media_info(header: &VqaHeader, index_entry_count: usize) -> VqaMediaInfo {
    let has_audio = header.has_audio();
    VqaMediaInfo {
        width: header.width,
        height: header.height,
        fps: header.fps.max(1),
        frame_count: header.num_frames,
        frame_duration: header.frame_duration(),
        duration: header.duration(),
        has_audio,
        audio_sample_rate: if has_audio { Some(header.freq) } else { None },
        audio_channels: if has_audio {
            Some(header.channels)
        } else {
            None
        },
        audio_bits: if has_audio { Some(header.bits) } else { None },
        index_entry_count,
        seek_support: build_seek_support(index_entry_count),
    }
}

pub(crate) fn build_seek_index(
    entries: Option<&[u32]>,
    header: &VqaHeader,
) -> Option<VqaSeekIndex> {
    let raw_entries = entries?;
    let points = raw_entries
        .iter()
        .enumerate()
        .map(|(index, raw_entry)| {
            let frame_index = index as u16;
            let entry = super::timing::frame_index_entry_from_raw(frame_index, *raw_entry);
            VqaSeekPoint {
                entry,
                timestamp: header
                    .frame_timestamp(frame_index)
                    .unwrap_or(Duration::ZERO),
                byte_offset: u64::from(entry.raw_offset).saturating_mul(2),
            }
        })
        .collect();
    Some(VqaSeekIndex {
        entries: points,
        seek_support: build_seek_support(raw_entries.len()),
    })
}

impl VqaHeader {
    /// Returns first-class playback metadata derived from the VQHD header.
    ///
    /// This header-only view does not imply the presence of FINF index data.
    #[inline]
    pub fn media_info(&self) -> VqaMediaInfo {
        build_media_info(self, 0)
    }
}

impl VqaFile<'_> {
    /// Returns first-class playback metadata for the parsed VQA container.
    #[inline]
    pub fn media_info(&self) -> VqaMediaInfo {
        build_media_info(&self.header, self.frame_index.as_ref().map_or(0, Vec::len))
    }

    /// Returns a timing-aware seek/index view when FINF metadata was present.
    #[inline]
    pub fn seek_index(&self) -> Option<VqaSeekIndex> {
        build_seek_index(self.frame_index.as_deref(), &self.header)
    }
}
