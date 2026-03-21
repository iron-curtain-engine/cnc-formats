// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{VqaFile, VqaHeader};

use std::time::Duration;

const FRAME_INDEX_FLAG_SHIFT: u32 = 28;
const FRAME_INDEX_OFFSET_MASK: u32 = 0x0FFF_FFFF;
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// One raw FINF entry decoded into stable metadata fields.
///
/// VQA variants encode frame index entries slightly differently across games,
/// so this type intentionally preserves the raw FINF flag nibble and offset
/// value without trying to over-normalize them. Downstream tools can use the
/// structured fields for diagnostics, indexing, or future seek heuristics
/// while the crate continues to preserve deterministic parse behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VqaFrameIndexEntry {
    /// Zero-based playback frame index.
    pub frame_index: u16,
    /// Raw 32-bit FINF entry as stored in the file.
    pub raw_entry: u32,
    /// Raw high-nibble flag bits from the FINF entry.
    pub raw_flags: u8,
    /// Raw low-28-bit offset payload from the FINF entry.
    pub raw_offset: u32,
}

impl VqaFrameIndexEntry {
    /// Returns `true` when the FINF entry carries any non-zero flag bits.
    #[inline]
    pub fn has_flags(&self) -> bool {
        self.raw_flags != 0
    }
}

#[inline]
pub(crate) fn frame_index_entry_from_raw(frame_index: u16, raw_entry: u32) -> VqaFrameIndexEntry {
    VqaFrameIndexEntry {
        frame_index,
        raw_entry,
        raw_flags: (raw_entry >> FRAME_INDEX_FLAG_SHIFT) as u8,
        raw_offset: raw_entry & FRAME_INDEX_OFFSET_MASK,
    }
}

#[inline]
fn fps_or_one(fps: u8) -> u128 {
    u128::from(fps.max(1))
}

#[inline]
fn duration_from_frame_count(frame_count: u64, fps: u8) -> Duration {
    let fps = fps_or_one(fps);
    let total_nanos = u128::from(frame_count).saturating_mul(NANOS_PER_SECOND) / fps;
    let secs = (total_nanos / NANOS_PER_SECOND).min(u128::from(u64::MAX)) as u64;
    let nanos = (total_nanos % NANOS_PER_SECOND) as u32;
    Duration::new(secs, nanos)
}

/// Returns the nominal presentation duration of one video frame.
#[inline]
pub(crate) fn frame_duration_for_fps(fps: u8) -> Duration {
    duration_from_frame_count(1, fps)
}

impl VqaHeader {
    /// Returns the nominal duration of one video frame.
    ///
    /// Timing is frame-based. A zero FPS header is treated as `1 fps` so
    /// malformed inputs still have deterministic timing semantics.
    #[inline]
    pub fn frame_duration(&self) -> Duration {
        frame_duration_for_fps(self.fps)
    }

    /// Returns the nominal presentation time of `frame_index`.
    ///
    /// The timestamp is relative to the start of playback and uses the header
    /// frame rate directly. Returns `None` when `frame_index` is out of range.
    #[inline]
    pub fn frame_timestamp(&self, frame_index: u16) -> Option<Duration> {
        if frame_index >= self.num_frames {
            return None;
        }
        Some(duration_from_frame_count(u64::from(frame_index), self.fps))
    }

    /// Returns the nominal playback duration from `num_frames / fps`.
    #[inline]
    pub fn duration(&self) -> Duration {
        duration_from_frame_count(u64::from(self.num_frames), self.fps)
    }

    /// Maps a playback time to the frame that should be presented then.
    ///
    /// The mapping is clamped to the last frame. Returns `None` when the file
    /// declares zero frames.
    pub fn frame_index_for_time(&self, time: Duration) -> Option<u16> {
        if self.num_frames == 0 {
            return None;
        }

        let frame = time.as_nanos().saturating_mul(fps_or_one(self.fps)) / NANOS_PER_SECOND;
        let last = u128::from(self.num_frames.saturating_sub(1));
        Some(frame.min(last) as u16)
    }

    /// Returns the exact rational relationship between audio sample frames and one video frame.
    ///
    /// The tuple is `(sample_rate, fps)`, meaning
    /// `sample_rate / fps = audio_sample_frames_per_video_frame`.
    #[inline]
    pub fn audio_sample_frames_per_video_frame(&self) -> Option<(u32, u32)> {
        if !self.has_audio() {
            return None;
        }
        Some((u32::from(self.freq), u32::from(self.fps.max(1))))
    }
}

impl VqaFile<'_> {
    /// Returns one decoded FINF entry for `frame_index`.
    ///
    /// The returned metadata is derived from the raw FINF table and allocated
    /// on demand; the parsed `VqaFile` continues to store the source entries
    /// as-is for round-trip fidelity.
    pub fn frame_index_entry(&self, frame_index: u16) -> Option<VqaFrameIndexEntry> {
        let raw = self
            .frame_index
            .as_ref()?
            .get(usize::from(frame_index))
            .copied()?;
        Some(frame_index_entry_from_raw(frame_index, raw))
    }

    /// Returns the decoded FINF frame index when one was present.
    ///
    /// This allocates a small `Vec` of metadata entries on demand rather than
    /// during parse so container parsing stays close to the source bytes.
    pub fn frame_index_entries(&self) -> Option<Vec<VqaFrameIndexEntry>> {
        let entries = self.frame_index.as_ref()?;
        Some(
            entries
                .iter()
                .enumerate()
                .map(|(index, raw)| frame_index_entry_from_raw(index as u16, *raw))
                .collect(),
        )
    }
}
