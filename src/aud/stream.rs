// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{
    info::media_info_with_seek, parse_header_bytes, AdpcmChannel, AudHeader, AudMediaInfo,
    AudSeekSupport, AUD_HEADER_SIZE, SCOMP_NONE, SCOMP_SONARC, SCOMP_SOS, SCOMP_WESTWOOD,
};
use crate::error::Error;
use crate::stream_io::{io_error, read_exact_array};

use std::io::{BufReader, Read, Seek, SeekFrom};
use std::time::Duration;

const SCOMP99_CHUNK_MAGIC: u32 = 0x0000_DEAF;

/// One owned chunk of decoded AUD PCM output.
///
/// This is the queue-friendly counterpart to [`AudStream::read_samples`].
/// Downstream runtimes that prefer bounded owned buffers over caller-managed
/// scratch slices can request one chunk at a time and enqueue the returned PCM
/// directly in their audio pipeline.
#[derive(Debug, Clone)]
pub struct AudPcmChunk {
    /// Start position of the chunk in decoded sample frames.
    pub start_sample_frame: u64,
    /// Signed 16-bit PCM samples. Stereo is interleaved `[L, R, L, R, …]`.
    pub samples: Vec<i16>,
    /// Playback sample rate in Hz.
    pub sample_rate: u16,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u8,
}

impl AudPcmChunk {
    /// Returns the number of decoded sample frames in this chunk.
    #[inline]
    pub fn sample_frames(&self) -> usize {
        let channels = usize::from(self.channels.max(1));
        self.samples.len() / channels
    }

    /// Returns the playback duration of this chunk.
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

    /// Returns the playback timestamp of the first sample frame in this chunk.
    #[inline]
    pub fn start_time(&self) -> Option<Duration> {
        if self.sample_rate == 0 {
            return None;
        }
        let secs = self.start_sample_frame / u64::from(self.sample_rate);
        let nanos = ((self.start_sample_frame % u64::from(self.sample_rate)) * 1_000_000_000u64)
            / u64::from(self.sample_rate);
        Some(Duration::new(secs, nanos as u32))
    }
}

/// Streaming AUD reader and incremental PCM decoder.
///
/// This type reads an AUD file from any [`Read`] source, parses the 12-byte
/// header once, then decodes audio incrementally into caller-provided sample
/// buffers. It avoids loading the full compressed file or decoded PCM output
/// into memory.
///
/// Timing is sample-based: [`Self::read_samples`] fills exactly as many PCM
/// samples as fit in the caller-owned output buffer. The stream holds only the
/// current ADPCM state plus a tiny amount of pending data, so it is suitable
/// for both short sound effects and long-form streaming playback.
///
/// Truncated payloads return [`Error::UnexpectedEof`]. Unsupported compression
/// modes return [`Error::DecompressionError`]. Use [`Self::open_seekable`] when
/// downstream playback needs reliable rewind semantics.
#[derive(Debug)]
pub struct AudStream<R> {
    reader: BufReader<R>,
    header: AudHeader,
    seek_support: AudSeekSupport,
    rewind_offset: u64,
    remaining_payload: usize,
    sample_limit: usize,
    samples_emitted: usize,
    left: AdpcmChannel,
    right: AdpcmChannel,
    next_adpcm_left: bool,
    pending_sample: Option<i16>,
    scomp99_chunk_remaining: usize,
}

impl<R: Read> AudStream<R> {
    /// Opens an AUD stream by reading the fixed-size 12-byte header.
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let header_bytes =
            read_exact_array::<_, AUD_HEADER_SIZE>(&mut reader, "reading AUD header")?;
        let header = parse_header_bytes(&header_bytes)?;
        Ok(Self::from_parts(
            header,
            reader,
            AudSeekSupport::None,
            AUD_HEADER_SIZE as u64,
        ))
    }

    /// Builds a stream from a known header plus a payload reader.
    ///
    /// This is useful when the caller already parsed the header from an
    /// in-memory [`super::AudFile`] but still wants incremental sample decode.
    pub fn from_payload(header: AudHeader, reader: R) -> Self {
        Self::from_parts(header, reader, AudSeekSupport::None, 0)
    }

    fn from_parts(
        header: AudHeader,
        reader: R,
        seek_support: AudSeekSupport,
        rewind_offset: u64,
    ) -> Self {
        let sample_limit = sample_limit(&header);
        let remaining_payload = header.compressed_size as usize;
        Self {
            reader: BufReader::new(reader),
            header,
            seek_support,
            rewind_offset,
            remaining_payload,
            sample_limit,
            samples_emitted: 0,
            left: AdpcmChannel::default(),
            right: AdpcmChannel::default(),
            next_adpcm_left: true,
            pending_sample: None,
            scomp99_chunk_remaining: 0,
        }
    }

    /// Returns the parsed AUD header.
    #[inline]
    pub fn header(&self) -> &AudHeader {
        &self.header
    }

    /// Returns the playback sample rate in Hz.
    #[inline]
    pub fn sample_rate(&self) -> u16 {
        self.header.sample_rate
    }

    /// Returns the decoded channel count.
    #[inline]
    pub fn channels(&self) -> u8 {
        self.header.channel_count()
    }

    /// Returns the decoded sample-frame count implied by the AUD header.
    #[inline]
    pub fn sample_frames(&self) -> usize {
        self.header.sample_frames()
    }

    /// Returns the nominal playback duration implied by the AUD header.
    #[inline]
    pub fn duration(&self) -> Option<Duration> {
        self.header.duration()
    }

    /// Returns first-class playback metadata for this session.
    #[inline]
    pub fn media_info(&self) -> AudMediaInfo {
        media_info_with_seek(&self.header, self.seek_support)
    }

    /// Returns whether this session can restart from the beginning.
    #[inline]
    pub fn seek_support(&self) -> AudSeekSupport {
        self.seek_support
    }

    /// Returns the number of decoded sample frames already emitted.
    #[inline]
    pub fn decoded_sample_frames(&self) -> usize {
        self.samples_emitted / usize::from(self.channels().max(1))
    }

    /// Returns the number of sample frames still available in the stream.
    #[inline]
    pub fn remaining_sample_frames(&self) -> usize {
        self.sample_frames()
            .saturating_sub(self.decoded_sample_frames())
    }

    /// Returns the playback timestamp of the next sample frame to be decoded.
    #[inline]
    pub fn decoded_duration(&self) -> Option<Duration> {
        self.header
            .sample_frame_timestamp(self.decoded_sample_frames() as u64)
    }

    /// Returns the remaining playback duration implied by header metadata.
    #[inline]
    pub fn remaining_duration(&self) -> Option<Duration> {
        let total = self.duration()?;
        let decoded = self.decoded_duration()?;
        total.checked_sub(decoded).or(Some(Duration::ZERO))
    }

    /// Reads and decodes as many PCM samples as will fit in `out`.
    ///
    /// `out` is caller-owned scratch storage; this method does not allocate on
    /// the hot path. The return value is the number of interleaved `i16`
    /// samples written. `0` means end of stream.
    pub fn read_samples(&mut self, out: &mut [i16]) -> Result<usize, Error> {
        let mut written = 0usize;

        while written < out.len() {
            if self.samples_emitted >= self.sample_limit {
                return Ok(written);
            }

            if let Some(sample) = self.pending_sample.take() {
                let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                    needed: written.saturating_add(1),
                    available: written,
                })?;
                *slot = sample;
                written = written.saturating_add(1);
                self.samples_emitted = self.samples_emitted.saturating_add(1);
                continue;
            }

            match self.header.compression {
                SCOMP_NONE => {
                    let sample = if self.header.is_16bit() {
                        match self.next_pcm16_sample()? {
                            Some(sample) => sample,
                            None => break,
                        }
                    } else {
                        match self.next_payload_byte()? {
                            Some(byte) => ((byte as i16) - 128) << 8,
                            None => break,
                        }
                    };
                    let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                        needed: written.saturating_add(1),
                        available: written,
                    })?;
                    *slot = sample;
                    written = written.saturating_add(1);
                    self.samples_emitted = self.samples_emitted.saturating_add(1);
                }
                SCOMP_WESTWOOD | SCOMP_SOS => {
                    let byte = match self.next_adpcm_byte()? {
                        Some(byte) => byte,
                        None => break,
                    };
                    let (low, high) = {
                        let channel = self.next_adpcm_channel();
                        (
                            channel.decode_nibble(byte & 0x0F),
                            channel.decode_nibble(byte >> 4),
                        )
                    };
                    let slot = out.get_mut(written).ok_or(Error::UnexpectedEof {
                        needed: written.saturating_add(1),
                        available: written,
                    })?;
                    *slot = low;
                    written = written.saturating_add(1);
                    self.samples_emitted = self.samples_emitted.saturating_add(1);

                    if self.samples_emitted >= self.sample_limit {
                        continue;
                    }

                    if let Some(slot) = out.get_mut(written) {
                        *slot = high;
                        written = written.saturating_add(1);
                        self.samples_emitted = self.samples_emitted.saturating_add(1);
                    } else {
                        self.pending_sample = Some(high);
                    }
                }
                SCOMP_SONARC => {
                    return Err(Error::DecompressionError {
                        reason: "AUD Sonarc compression is not supported",
                    });
                }
                _ => {
                    return Err(Error::DecompressionError {
                        reason: "AUD compression type is not supported",
                    });
                }
            }
        }

        Ok(written)
    }

    /// Decodes and returns one owned PCM chunk with at most `max_sample_frames`.
    ///
    /// This method is intended for queue-based playback code that wants
    /// bounded owned buffers instead of managing reusable scratch slices.
    /// `max_sample_frames` counts interleaved stereo pairs as one frame.
    pub fn next_chunk(&mut self, max_sample_frames: usize) -> Result<Option<AudPcmChunk>, Error> {
        if max_sample_frames == 0 {
            return Err(Error::InvalidSize {
                value: 0,
                limit: usize::MAX,
                context: "AUD chunk sample-frame count",
            });
        }

        let channels = usize::from(self.channels().max(1));
        let max_samples = max_sample_frames.saturating_mul(channels);
        let start_sample_frame = self.decoded_sample_frames() as u64;
        let mut samples = vec![0i16; max_samples];
        let read = self.read_samples(&mut samples)?;
        if read == 0 {
            return Ok(None);
        }
        samples.truncate(read);
        Ok(Some(AudPcmChunk {
            start_sample_frame,
            samples,
            sample_rate: self.sample_rate(),
            channels: self.channels(),
        }))
    }

    /// Returns the underlying reader.
    #[inline]
    pub fn into_inner(self) -> R {
        self.reader.into_inner()
    }

    fn reset_decode_state(&mut self) {
        self.remaining_payload = self.header.compressed_size as usize;
        self.samples_emitted = 0;
        self.left = AdpcmChannel::default();
        self.right = AdpcmChannel::default();
        self.next_adpcm_left = true;
        self.pending_sample = None;
        self.scomp99_chunk_remaining = 0;
    }

    #[inline]
    fn next_pcm16_sample(&mut self) -> Result<Option<i16>, Error> {
        if self.remaining_payload == 0 {
            return Ok(None);
        }
        if self.remaining_payload < 2 {
            return Err(Error::UnexpectedEof {
                needed: 2,
                available: self.remaining_payload,
            });
        }

        let [lo, hi] = read_exact_array::<_, 2>(&mut self.reader, "reading AUD PCM16 sample")?;
        self.remaining_payload = self.remaining_payload.saturating_sub(2);
        Ok(Some(i16::from_le_bytes([lo, hi])))
    }

    #[inline]
    fn next_adpcm_channel(&mut self) -> &mut AdpcmChannel {
        if !self.header.is_stereo() {
            return &mut self.left;
        }

        let use_left = self.next_adpcm_left;
        self.next_adpcm_left = !self.next_adpcm_left;
        if use_left {
            &mut self.left
        } else {
            &mut self.right
        }
    }

    #[inline]
    fn next_adpcm_byte(&mut self) -> Result<Option<u8>, Error> {
        if self.header.compression == SCOMP_SOS {
            return self.next_scomp99_byte();
        }
        self.next_payload_byte()
    }

    fn next_scomp99_byte(&mut self) -> Result<Option<u8>, Error> {
        loop {
            if self.scomp99_chunk_remaining > 0 {
                let [byte] =
                    read_exact_array::<_, 1>(&mut self.reader, "reading AUD chunk payload")?;
                self.remaining_payload = self.remaining_payload.saturating_sub(1);
                self.scomp99_chunk_remaining = self.scomp99_chunk_remaining.saturating_sub(1);
                return Ok(Some(byte));
            }

            if self.remaining_payload == 0 {
                return Ok(None);
            }

            if self.remaining_payload < 8 {
                return Err(Error::UnexpectedEof {
                    needed: 8,
                    available: self.remaining_payload,
                });
            }

            let header = read_exact_array::<_, 8>(&mut self.reader, "reading AUD chunk header")?;
            self.remaining_payload = self.remaining_payload.saturating_sub(8);
            let [c0, c1, _, _, m0, m1, m2, m3] = header;
            let compressed_size = u16::from_le_bytes([c0, c1]) as usize;
            let magic = u32::from_le_bytes([m0, m1, m2, m3]);

            if magic != SCOMP99_CHUNK_MAGIC {
                return Err(Error::InvalidMagic {
                    context: "AUD SCOMP=99 chunk header",
                });
            }
            if compressed_size > self.remaining_payload {
                return Err(Error::UnexpectedEof {
                    needed: compressed_size,
                    available: self.remaining_payload,
                });
            }

            self.scomp99_chunk_remaining = compressed_size;
        }
    }

    #[inline]
    fn next_payload_byte(&mut self) -> Result<Option<u8>, Error> {
        if self.remaining_payload == 0 {
            return Ok(None);
        }

        let [byte] = read_exact_array::<_, 1>(&mut self.reader, "reading AUD payload byte")?;
        self.remaining_payload = self.remaining_payload.saturating_sub(1);
        Ok(Some(byte))
    }
}

/// Maximum number of bytes to scan when attempting resync.
const RESYNC_SCAN_LIMIT: usize = 64 * 1024;

impl<R: Read + Seek> AudStream<R> {
    /// Opens a seekable AUD stream and records the absolute rewind position.
    ///
    /// Prefer this constructor when the reader may not start at offset 0 or
    /// when downstream playback code intends to call [`Self::rewind`].
    pub fn open_seekable(mut reader: R) -> Result<Self, Error> {
        let start = reader
            .stream_position()
            .map_err(|err| io_error("capturing AUD stream start position", err))?;
        let header_bytes =
            read_exact_array::<_, AUD_HEADER_SIZE>(&mut reader, "reading AUD header")?;
        let header = parse_header_bytes(&header_bytes)?;
        Ok(Self::from_parts(
            header,
            reader,
            AudSeekSupport::Restart,
            start.saturating_add(AUD_HEADER_SIZE as u64),
        ))
    }

    /// Rewinds the stream back to the start of the compressed payload.
    ///
    /// Header metadata stays available and decoder state is reset, so the next
    /// call to [`Self::read_samples`] reproduces the original sample stream.
    pub fn rewind(&mut self) -> Result<(), Error> {
        self.reader
            .seek(SeekFrom::Start(self.rewind_offset))
            .map_err(|err| io_error("rewinding AUD stream", err))?;
        self.reset_decode_state();
        Ok(())
    }

    /// Alias for [`Self::rewind`] when callers prefer restart terminology.
    #[inline]
    pub fn restart(&mut self) -> Result<(), Error> {
        self.rewind()
    }

    /// Attempts to recover from a corrupted SCOMP=99 chunk stream.
    ///
    /// Scans forward up to 64 KB looking for the next valid `0x0000DEAF` chunk
    /// header. If found, resets the ADPCM state and positions the reader just
    /// after the header so the next [`Self::read_samples`] call resumes
    /// decoding from the new chunk.
    ///
    /// Returns `true` if a valid chunk header was found, `false` if the scan
    /// limit was reached without finding one.
    ///
    /// This method is only useful for SCOMP=99 (SOS) streams that use chunked
    /// framing. For flat ADPCM streams (SCOMP_WESTWOOD), there are no sync
    /// markers and this method always returns `false`.
    pub fn try_resync(&mut self) -> Result<bool, Error> {
        if self.header.compression != SCOMP_SOS {
            return Ok(false);
        }

        let mut window = [0u8; 4];
        let mut scanned = 0usize;

        while scanned < RESYNC_SCAN_LIMIT {
            let byte = match read_exact_array::<_, 1>(&mut self.reader, "scanning for AUD resync") {
                Ok([b]) => b,
                Err(_) => return Ok(false),
            };

            window[0] = window[1];
            window[1] = window[2];
            window[2] = window[3];
            window[3] = byte;
            scanned = scanned.saturating_add(1);

            if scanned >= 4 && u32::from_le_bytes(window) == SCOMP99_CHUNK_MAGIC {
                // Found the magic. Read the compressed_size + output_size that
                // precedes the magic in the 8-byte chunk header. We consumed
                // the magic itself, so the reader is now at the start of the
                // chunk's ADPCM payload.  We need the 4 bytes *before* the
                // magic (compressed_size u16 + output_size u16) to know the
                // chunk length, but we've already passed them.
                //
                // Strategy: seek back 8 bytes (to chunk header start), re-read
                // the full 8-byte header, and let the normal decode path handle it.
                self.reader
                    .seek(SeekFrom::Current(-8))
                    .map_err(|err| io_error("seeking to AUD chunk header during resync", err))?;

                // Reset ADPCM state — the decoder will accumulate fresh state
                // from the new chunk.
                self.left = AdpcmChannel::default();
                self.right = AdpcmChannel::default();
                self.next_adpcm_left = true;
                self.pending_sample = None;
                self.scomp99_chunk_remaining = 0;

                return Ok(true);
            }
        }

        Ok(false)
    }
}

#[inline]
fn sample_limit(header: &AudHeader) -> usize {
    let uncompressed_size = header.uncompressed_size as usize;
    if header.compression == SCOMP_NONE && !header.is_16bit() {
        return uncompressed_size;
    }
    uncompressed_size / 2
}
