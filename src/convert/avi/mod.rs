// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Minimal AVI (RIFF) writer and reader for uncompressed video + PCM audio.
//!
//! AVI is a RIFF-based container (same family as WAV).  This module implements
//! just enough of the AVI spec to store uncompressed RGB24 video frames and
//! PCM audio — no codecs, no dependencies.
//!
//! ## AVI Structure
//!
//! ```text
//! RIFF 'AVI '
//!   LIST 'hdrl'
//!     'avih' — main AVI header
//!     LIST 'strl' (video stream)
//!       'strh' — stream header
//!       'strf' — BITMAPINFOHEADER
//!     LIST 'strl' (audio stream, optional)
//!       'strh' — stream header
//!       'strf' — WAVEFORMATEX
//!   LIST 'movi'
//!     '00dc' — video frames (DIB, bottom-up BGR24)
//!     '01wb' — audio chunks (PCM)
//!   'idx1' — index
//! ```
//!
//! ## References
//!
//! Microsoft AVI RIFF specification (1992); OpenDML AVI extensions.
//! This is a clean-room implementation from the publicly documented spec.

mod decode;
mod encode;

pub use decode::{decode_avi, AviContent};
pub use encode::encode_avi;

use crate::error::Error;

// ─── Constants ───────────────────────────────────────────────────────────────

/// V38: maximum video frame count.
pub(super) const MAX_FRAME_COUNT: usize = 65536;

/// V38: maximum video dimensions.
pub(super) const MAX_DIMENSION: u32 = 4096;

/// V38: maximum AVI file size for reading (2 GB, AVI 1.0 limit).
pub(super) const MAX_AVI_SIZE: usize = 2 * 1024 * 1024 * 1024;

/// Bytes per pixel for uncompressed BGR24 video.
pub(super) const BYTES_PER_PIXEL: usize = 3;
