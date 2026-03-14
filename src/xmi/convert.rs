// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! XMI EVNT stream conversion helpers.
//!
//! This file holds the XMI-specific timing conversion logic so `mod.rs`
//! stays focused on IFF container parsing and remains under the repo's
//! per-file context cap.

use super::{XmiSequence, XMI_TICKS_PER_BEAT};
use crate::error::Error;
use crate::read::read_u8;

/// Converts an XMI sequence to a Standard MIDI File (Type 0) byte vector.
///
/// The conversion:
/// 1. Writes the SMF MThd header (Type 0, 1 track, 120 tpb)
/// 2. Converts XMI IFTHEN delay opcodes to MIDI variable-length delta-times
/// 3. Generates explicit Note-Off events from XMI note durations
/// 4. Appends an End-of-Track meta event
///
/// # Errors
///
/// - [`Error::UnexpectedEof`] — EVNT data is truncated mid-event
pub fn to_mid(sequence: &XmiSequence<'_>) -> Result<Vec<u8>, Error> {
    let evnt = sequence.event_data;

    // ── Convert EVNT to MIDI track events ────────────────────────────
    //
    // XMI timing: delay bytes appear as 0x00–0x7F between events.
    // Multiple consecutive delay bytes sum their values.
    // MIDI status bytes have bit 7 set (0x80–0xFF).
    //
    // XMI Note-On (0x90–0x9F) is followed by:
    //   - note number (1 byte)
    //   - velocity (1 byte)
    //   - duration (variable-length, same encoding as MIDI VLQ)
    //
    // We convert Note-On + duration into separate Note-On and Note-Off
    // events with appropriate delta-times.

    // Pending Note-Off events: (absolute_tick, channel, note).
    let mut pending_offs: Vec<(u32, u8, u8)> = Vec::new();
    let mut track_events: Vec<(u32, Vec<u8>)> = Vec::new(); // (abs_tick, raw_bytes)
    let mut abs_tick: u32 = 0;
    let mut pos: usize = 0;
    let mut running_status: u8 = 0;

    while pos < evnt.len() {
        let delay_start = pos;

        // Read delay bytes (0x00–0x7F).
        while pos < evnt.len() {
            let b = match read_u8(evnt, pos) {
                Ok(v) => v,
                Err(_) => break,
            };
            if b >= 0x80 {
                break;
            }
            abs_tick = abs_tick.saturating_add(u32::from(b));
            pos = pos.saturating_add(1);
        }
        if pos >= evnt.len() {
            // Empty EVNT is valid, but a stream ending after one or more
            // delay bytes is truncated because a status byte never follows.
            if delay_start != pos {
                return Err(Error::UnexpectedEof {
                    needed: pos.saturating_add(1),
                    available: evnt.len(),
                });
            }
            break;
        }

        // Insert any pending Note-Off events that are due.
        flush_pending_offs(&mut pending_offs, abs_tick, &mut track_events);

        let status = read_required_u8(evnt, pos)?;
        pos = pos.saturating_add(1);

        if status == 0xFF {
            // Meta event.
            let meta_type = read_required_u8(evnt, pos)?;
            pos = pos.saturating_add(1);
            let (meta_len, bytes_read) = read_vlq_required(evnt, pos)?;
            pos = pos.saturating_add(bytes_read);
            let meta_end = pos.saturating_add(meta_len as usize);
            let meta_data = evnt.get(pos..meta_end).ok_or(Error::UnexpectedEof {
                needed: meta_end,
                available: evnt.len(),
            })?;
            pos = meta_end;

            // End-of-track: stop processing.
            if meta_type == 0x2F {
                break;
            }

            // Emit meta event.
            let mut ev = vec![0xFF, meta_type];
            write_vlq_to(&mut ev, meta_len);
            ev.extend_from_slice(meta_data);
            track_events.push((abs_tick, ev));
        } else if status == 0xF0 || status == 0xF7 {
            // SysEx event — read length and skip.
            let (sysex_len, bytes_read) = read_vlq_required(evnt, pos)?;
            pos = pos.saturating_add(bytes_read);
            let sysex_end = pos.saturating_add(sysex_len as usize);
            let sysex_data = evnt.get(pos..sysex_end).ok_or(Error::UnexpectedEof {
                needed: sysex_end,
                available: evnt.len(),
            })?;
            pos = sysex_end;
            let mut ev = vec![status];
            write_vlq_to(&mut ev, sysex_len);
            ev.extend_from_slice(sysex_data);
            track_events.push((abs_tick, ev));
        } else if status >= 0x80 {
            // Channel message.
            running_status = status;
            let channel = status & 0x0F;
            let msg_type = status & 0xF0;

            match msg_type {
                0x90 => {
                    // Note-On with duration.
                    let note = read_required_u8(evnt, pos)?;
                    pos = pos.saturating_add(1);
                    let velocity = read_required_u8(evnt, pos)?;
                    pos = pos.saturating_add(1);

                    // XMI encodes note duration as a VLQ after velocity.
                    let (duration, bytes_read) = read_vlq_required(evnt, pos)?;
                    pos = pos.saturating_add(bytes_read);

                    // Emit Note-On.
                    track_events.push((abs_tick, vec![status, note, velocity]));

                    // Schedule Note-Off.
                    let off_tick = abs_tick.saturating_add(duration);
                    pending_offs.push((off_tick, channel, note));
                }
                0x80 => {
                    // Explicit Note-Off (unusual in XMI but possible).
                    let note = read_required_u8(evnt, pos)?;
                    pos = pos.saturating_add(1);
                    let velocity = read_required_u8(evnt, pos)?;
                    pos = pos.saturating_add(1);
                    track_events.push((abs_tick, vec![status, note, velocity]));
                }
                0xA0 | 0xB0 | 0xE0 => {
                    // Polyphonic aftertouch, Control Change, Pitch Bend — 2 data bytes.
                    let d1 = read_required_u8(evnt, pos)?;
                    pos = pos.saturating_add(1);
                    let d2 = read_required_u8(evnt, pos)?;
                    pos = pos.saturating_add(1);
                    track_events.push((abs_tick, vec![status, d1, d2]));
                }
                0xC0 | 0xD0 => {
                    // Program Change, Channel Pressure — 1 data byte.
                    let d1 = read_required_u8(evnt, pos)?;
                    pos = pos.saturating_add(1);
                    track_events.push((abs_tick, vec![status, d1]));
                }
                _ => {
                    // Unknown status byte — skip (permissive).
                    break;
                }
            }
        }
    }

    // Flush remaining Note-Off events.
    flush_pending_offs(&mut pending_offs, u32::MAX, &mut track_events);

    // Sort all events by absolute tick (stable for same-tick ordering).
    track_events.sort_by_key(|(tick, _)| *tick);

    // ── Build SMF binary ─────────────────────────────────────────────
    let mut track_data = Vec::new();

    // Convert absolute ticks to delta-times and serialise.
    let mut prev_tick: u32 = 0;
    for (tick, event_bytes) in &track_events {
        let delta = tick.saturating_sub(prev_tick);
        write_vlq_to(&mut track_data, delta);
        track_data.extend_from_slice(event_bytes);
        prev_tick = *tick;
    }

    // Append End-of-Track meta event.
    track_data.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);

    // Build the complete SMF.
    let mut smf = Vec::with_capacity(22 + track_data.len());

    // MThd header: 14 bytes.
    smf.extend_from_slice(b"MThd");
    smf.extend_from_slice(&6u32.to_be_bytes()); // header length
    smf.extend_from_slice(&0u16.to_be_bytes()); // format 0
    smf.extend_from_slice(&1u16.to_be_bytes()); // 1 track
    smf.extend_from_slice(&XMI_TICKS_PER_BEAT.to_be_bytes()); // timing

    // MTrk header.
    smf.extend_from_slice(b"MTrk");
    smf.extend_from_slice(&(track_data.len() as u32).to_be_bytes());
    smf.extend_from_slice(&track_data);

    let _ = running_status; // suppress unused warning

    Ok(smf)
}

/// Reads one byte from `data` or returns a structured EOF error.
fn read_required_u8(data: &[u8], offset: usize) -> Result<u8, Error> {
    read_u8(data, offset).map_err(|_| Error::UnexpectedEof {
        needed: offset.saturating_add(1),
        available: data.len(),
    })
}

/// Flushes pending Note-Off events that are due at or before `current_tick`.
fn flush_pending_offs(
    pending: &mut Vec<(u32, u8, u8)>,
    current_tick: u32,
    events: &mut Vec<(u32, Vec<u8>)>,
) {
    // Extract events due now, keeping later ones pending.
    let mut remaining = Vec::new();
    for &(tick, channel, note) in pending.iter() {
        if tick <= current_tick {
            events.push((tick, vec![0x80 | channel, note, 0x00]));
        } else {
            remaining.push((tick, channel, note));
        }
    }
    *pending = remaining;
}

/// Reads a MIDI variable-length quantity from `data` at `offset`.
///
/// Returns `(value, bytes_consumed)`.  If the data is truncated, returns
/// the partial value and the bytes that were available.  The parser caps
/// at 4 bytes (28-bit value) per the MIDI spec.
pub(crate) fn read_vlq(data: &[u8], offset: usize) -> (u32, usize) {
    let mut value: u32 = 0;
    let mut bytes_read: usize = 0;
    while bytes_read < 4 {
        let pos = offset.saturating_add(bytes_read);
        let b = match data.get(pos) {
            Some(&v) => v,
            None => return (value, bytes_read),
        };
        value = (value << 7) | u32::from(b & 0x7F);
        bytes_read = bytes_read.saturating_add(1);
        if b & 0x80 == 0 {
            break;
        }
    }
    (value, bytes_read)
}

/// Reads a complete MIDI VLQ and errors when the value is truncated.
fn read_vlq_required(data: &[u8], offset: usize) -> Result<(u32, usize), Error> {
    let (value, bytes_read) = read_vlq(data, offset);
    if bytes_read == 0 {
        return Err(Error::UnexpectedEof {
            needed: offset.saturating_add(1),
            available: data.len(),
        });
    }

    let last_pos = offset.saturating_add(bytes_read.saturating_sub(1));
    let last_byte = data.get(last_pos).copied().ok_or(Error::UnexpectedEof {
        needed: last_pos.saturating_add(1),
        available: data.len(),
    })?;

    // A continuation bit on the final available byte means the caller hit
    // EOF before the VLQ terminated.
    if last_byte & 0x80 != 0 && bytes_read < 4 {
        return Err(Error::UnexpectedEof {
            needed: offset.saturating_add(bytes_read).saturating_add(1),
            available: data.len(),
        });
    }

    Ok((value, bytes_read))
}

/// Writes a MIDI variable-length quantity to a byte vector.
pub(crate) fn write_vlq_to(buf: &mut Vec<u8>, value: u32) {
    if value == 0 {
        buf.push(0);
        return;
    }

    // Encode in reverse order into a fixed stack buffer, then emit the
    // bytes most-significant first.  `.get_mut()` avoids panic-prone
    // direct indexing in production code.
    let mut bytes = [0u8; 4];
    let mut count = 0usize;
    let mut v = value;

    if let Some(slot) = bytes.get_mut(count) {
        *slot = (v & 0x7F) as u8;
    }
    v >>= 7;
    count = count.saturating_add(1);

    while v > 0 {
        if let Some(slot) = bytes.get_mut(count) {
            *slot = 0x80 | (v & 0x7F) as u8;
        }
        v >>= 7;
        count = count.saturating_add(1);
    }

    while count > 0 {
        count = count.saturating_sub(1);
        if let Some(byte) = bytes.get(count) {
            buf.push(*byte);
        }
    }
}
