// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

/// Reusable caller-owned VQA frame storage.
///
/// Create this once from decoder metadata, then pass it to
/// [`VqaDecoder::next_frame_into`] on every playback step. This avoids
/// allocating a fresh pixel buffer per frame while preserving the source
/// boundary: indexed pixels plus the active palette snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaFrameBuffer {
    width: u16,
    height: u16,
    pixels: Vec<u8>,
    palette: [u8; 768],
}

impl VqaFrameBuffer {
    /// Allocates one reusable frame buffer for the given dimensions.
    pub fn new(width: u16, height: u16) -> Self {
        let pixel_count = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            pixels: vec![0; pixel_count],
            palette: [0; 768],
        }
    }

    /// Allocates a reusable frame buffer sized for the given decoder metadata.
    #[inline]
    pub fn from_media_info(info: &super::super::VqaMediaInfo) -> Self {
        Self::new(info.width, info.height)
    }

    /// Returns the configured frame width.
    #[inline]
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Returns the configured frame height.
    #[inline]
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Returns the most recently decoded indexed pixels.
    #[inline]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns the most recently decoded palette snapshot.
    #[inline]
    pub fn palette(&self) -> &[u8; 768] {
        &self.palette
    }

    fn copy_from_frame(&mut self, frame: &VqaFrame) -> Result<(), Error> {
        self.ensure_dimensions(self.width, self.height)?;
        if frame.pixels.len() != self.pixels.len() {
            return Err(Error::InvalidSize {
                value: frame.pixels.len(),
                limit: self.pixels.len(),
                context: "VQA frame pixel count",
            });
        }
        self.pixels.copy_from_slice(&frame.pixels);
        self.palette = frame.palette;
        Ok(())
    }

    fn ensure_dimensions(&self, width: u16, height: u16) -> Result<(), Error> {
        if self.width != width || self.height != height {
            return Err(Error::InvalidSize {
                value: (self.width as usize).saturating_mul(self.height as usize),
                limit: (width as usize).saturating_mul(height as usize),
                context: "VQA frame buffer dimensions",
            });
        }
        Ok(())
    }
}

impl<R: Read + Seek> VqaDecoder<R> {
    /// Decodes the next frame into a caller-owned reusable buffer.
    ///
    /// This is the low-allocation playback path for downstream engines that
    /// keep one persistent texture and update it frame-by-frame. After the
    /// initial `VqaFrameBuffer` allocation, repeated calls do not allocate a
    /// new pixel buffer per frame.
    pub fn next_frame_into(&mut self, buffer: &mut VqaFrameBuffer) -> Result<Option<u16>, Error> {
        buffer.ensure_dimensions(self.width(), self.height())?;

        if let Some(frame) = self.frame_queue.pop_front() {
            buffer.copy_from_frame(&frame.frame)?;
            return Ok(Some(frame.index));
        }

        loop {
            let Self {
                stream,
                frame_decoder,
                audio_decoder,
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
                    return Ok(None);
                }
            };

            if frame_decoder.apply_chunk_into(&chunk.fourcc, chunk.data, &mut buffer.pixels)? {
                buffer.palette = *frame_decoder.palette();
                let frame_index = *next_frame_index;
                *next_frame_index = next_frame_index.saturating_add(1);
                return Ok(Some(frame_index));
            }

            let _ = queue_stream_audio_chunk(
                audio_decoder,
                audio_queue,
                audio_chunk_pool,
                &chunk.fourcc,
                chunk.data,
            )?;
        }
    }

    /// Reads as many decoded audio samples as fit in `out`.
    ///
    /// `out` is caller-owned scratch storage. The return value counts
    /// interleaved `i16` samples, matching [`crate::aud::AudStream::read_samples`].
    /// Downstream runtimes should consult [`Self::decoded_audio_sample_frames`]
    /// before the call when they need the exact starting timestamp.
    pub fn read_audio_samples(&mut self, out: &mut [i16]) -> Result<usize, Error> {
        if !self.has_audio() || out.is_empty() {
            return Ok(0);
        }

        let channels = audio_channels_usize(self.audio_decoder.channels);
        let target_len = out.len().saturating_sub(out.len() % channels);
        let mut written = 0usize;

        while written < target_len {
            if let Some(mut front) = self.audio_queue.pop_front() {
                if front.sample_frames(self.audio_decoder.channels) == 0 && front.is_drained() {
                    continue;
                }

                let out_len = out.len();
                let dst = out
                    .get_mut(written..target_len)
                    .ok_or(Error::UnexpectedEof {
                        needed: target_len,
                        available: out_len,
                    })?;
                let read = front.read_samples(&mut self.audio_decoder, dst)?;
                let copied_frames = read / channels;
                written = written.saturating_add(read);
                self.audio_sample_frames_delivered = self
                    .audio_sample_frames_delivered
                    .saturating_add(copied_frames as u64);

                if !front.is_drained() {
                    self.audio_queue.push_front(front);
                } else if let Some(buf) = front.into_backing_buffer() {
                    recycle_audio_chunk_buffer(&mut self.audio_chunk_pool, buf);
                }
                if read == 0 {
                    break;
                }
                continue;
            }

            if self.ended {
                break;
            }

            if !self.pump_once()? {
                break;
            }
        }

        Ok(written)
    }
}
