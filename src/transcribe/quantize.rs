// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Note-to-MIDI event conversion and SMF binary assembly.
//!
//! Takes a list of [`DetectedNote`] and produces a valid Standard MIDI File
//! (Type 0, single track) binary.  Reuses the VLQ encoding from the XMI
//! converter module.

use super::onset::DetectedNote;

/// Writes a MIDI variable-length quantity to a byte vector.
///
/// Same encoding as `xmi::convert::write_vlq_to`, duplicated here to avoid
/// a cross-feature dependency (transcribe does not require xmi).
fn write_vlq(buf: &mut Vec<u8>, value: u32) {
    if value == 0 {
        buf.push(0);
        return;
    }

    let mut bytes = [0u8; 4];
    let mut count = 0usize;

    if let Some(slot) = bytes.get_mut(count) {
        *slot = (value & 0x7F) as u8;
    }
    let mut v = value >> 7;
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

/// A MIDI event at an absolute tick position.
struct MidiEvent {
    /// Absolute tick from the start of the track.
    abs_tick: u32,
    /// Raw MIDI bytes (status + data bytes).
    bytes: Vec<u8>,
}

/// Converts detected notes to a complete Standard MIDI File (Type 0).
///
/// # Arguments
///
/// - `notes` — detected notes sorted by onset time
/// - `ticks_per_beat` — MIDI resolution (e.g. 480)
/// - `tempo_bpm` — beats per minute (e.g. 120)
/// - `channel` — MIDI channel for note events (0–15)
///
/// Returns the complete SMF binary as `Vec<u8>`.
pub fn notes_to_smf(
    notes: &[DetectedNote],
    ticks_per_beat: u16,
    tempo_bpm: u16,
    channel: u8,
) -> Vec<u8> {
    let channel = channel & 0x0F;
    let tempo_bpm = tempo_bpm.max(1);
    let ticks_per_beat = ticks_per_beat.max(1);

    // Microseconds per beat = 60_000_000 / BPM.
    let us_per_beat: u32 = 60_000_000u32
        .checked_div(u32::from(tempo_bpm))
        .unwrap_or(500_000);

    // Seconds per tick.
    let secs_per_tick = (us_per_beat as f64 / 1_000_000.0) / f64::from(ticks_per_beat);

    // Build event list: Note-On and Note-Off for each detected note.
    let mut events: Vec<MidiEvent> = Vec::with_capacity(notes.len().saturating_mul(2));

    for note in notes {
        let onset_tick = if secs_per_tick > 0.0 {
            (note.onset_secs / secs_per_tick).round() as u32
        } else {
            0
        };
        let duration_ticks = if secs_per_tick > 0.0 {
            ((note.duration_secs / secs_per_tick).round() as u32).max(1)
        } else {
            1
        };
        let off_tick = onset_tick.saturating_add(duration_ticks);

        // Note-On.
        events.push(MidiEvent {
            abs_tick: onset_tick,
            bytes: vec![0x90 | channel, note.note, note.velocity],
        });
        // Note-Off.
        events.push(MidiEvent {
            abs_tick: off_tick,
            bytes: vec![0x80 | channel, note.note, 0],
        });
    }

    // Sort by absolute tick; Note-Off before Note-On at the same tick.
    events.sort_by(|a, b| {
        a.abs_tick.cmp(&b.abs_tick).then_with(|| {
            let a_is_off = a.bytes.first().copied().unwrap_or(0) & 0xF0 == 0x80;
            let b_is_off = b.bytes.first().copied().unwrap_or(0) & 0xF0 == 0x80;
            b_is_off.cmp(&a_is_off) // Note-Off first (true > false)
        })
    });

    // ── Build MTrk data ──────────────────────────────────────────────
    let mut track_data = Vec::new();

    // Tempo meta-event at tick 0: FF 51 03 <3-byte tempo>.
    write_vlq(&mut track_data, 0); // delta = 0
    track_data.push(0xFF);
    track_data.push(0x51);
    track_data.push(0x03);
    track_data.push((us_per_beat >> 16) as u8);
    track_data.push((us_per_beat >> 8) as u8);
    track_data.push(us_per_beat as u8);

    // Convert absolute ticks to delta-times.
    let mut prev_tick: u32 = 0;
    for event in &events {
        let delta = event.abs_tick.saturating_sub(prev_tick);
        write_vlq(&mut track_data, delta);
        track_data.extend_from_slice(&event.bytes);
        prev_tick = event.abs_tick;
    }

    // End-of-track meta-event.
    track_data.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);

    // ── Build SMF ────────────────────────────────────────────────────
    let mut smf = Vec::with_capacity(22 + track_data.len());

    // MThd header: 14 bytes.
    smf.extend_from_slice(b"MThd");
    smf.extend_from_slice(&6u32.to_be_bytes()); // header length
    smf.extend_from_slice(&0u16.to_be_bytes()); // format 0
    smf.extend_from_slice(&1u16.to_be_bytes()); // 1 track
    smf.extend_from_slice(&ticks_per_beat.to_be_bytes());

    // MTrk header.
    smf.extend_from_slice(b"MTrk");
    smf.extend_from_slice(&(track_data.len() as u32).to_be_bytes());
    smf.extend_from_slice(&track_data);

    smf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_notes_produces_valid_smf() {
        let smf = notes_to_smf(&[], 480, 120, 0);
        assert_eq!(smf.get(..4), Some(b"MThd".as_slice()));
        // Should have MThd (14 bytes) + MTrk header (8 bytes) + track data.
        assert!(smf.len() >= 22, "SMF too short: {} bytes", smf.len());
        // Track data should at least contain tempo + end-of-track.
        let track_data_len = u32::from_be_bytes([smf[18], smf[19], smf[20], smf[21]]) as usize;
        assert!(track_data_len > 0);
    }

    #[test]
    fn single_note_event_count() {
        let notes = vec![DetectedNote {
            note: 69,
            onset_secs: 0.0,
            duration_secs: 0.5,
            velocity: 100,
        }];
        let smf = notes_to_smf(&notes, 480, 120, 0);
        // Should contain Note-On (0x90) and Note-Off (0x80).
        let has_note_on = smf.windows(2).any(|w| w[0] == 0x90 && w[1] == 69);
        let has_note_off = smf.windows(2).any(|w| w[0] == 0x80 && w[1] == 69);
        assert!(has_note_on, "missing Note-On");
        assert!(has_note_off, "missing Note-Off");
    }

    #[test]
    fn tempo_event_present() {
        let smf = notes_to_smf(&[], 480, 120, 0);
        // Tempo meta-event: FF 51 03.
        let has_tempo = smf
            .windows(3)
            .any(|w| w[0] == 0xFF && w[1] == 0x51 && w[2] == 0x03);
        assert!(has_tempo, "missing tempo meta-event");
    }

    #[test]
    fn end_of_track_present() {
        let smf = notes_to_smf(&[], 480, 120, 0);
        // End-of-track: FF 2F 00.
        let has_eot = smf
            .windows(3)
            .any(|w| w[0] == 0xFF && w[1] == 0x2F && w[2] == 0x00);
        assert!(has_eot, "missing End-of-Track meta-event");
    }

    #[test]
    fn multiple_notes_sorted() {
        let notes = vec![
            DetectedNote {
                note: 60,
                onset_secs: 0.5,
                duration_secs: 0.25,
                velocity: 80,
            },
            DetectedNote {
                note: 69,
                onset_secs: 0.0,
                duration_secs: 0.25,
                velocity: 100,
            },
        ];
        let smf = notes_to_smf(&notes, 480, 120, 0);
        // First Note-On should be note 69 (onset 0.0), then note 60 (onset 0.5).
        let note_ons: Vec<u8> = smf
            .windows(3)
            .filter(|w| w[0] & 0xF0 == 0x90)
            .map(|w| w[1])
            .collect();
        assert_eq!(note_ons, vec![69, 60], "notes should be time-ordered");
    }

    #[test]
    fn channel_encoded_correctly() {
        let notes = vec![DetectedNote {
            note: 60,
            onset_secs: 0.0,
            duration_secs: 0.5,
            velocity: 100,
        }];
        let smf = notes_to_smf(&notes, 480, 120, 5);
        // Note-On should be 0x95 (channel 5).
        assert!(
            smf.windows(2).any(|w| w[0] == 0x95 && w[1] == 60),
            "Note-On should use channel 5"
        );
    }

    #[test]
    fn write_vlq_known_values() {
        let mut buf = Vec::new();

        write_vlq(&mut buf, 0);
        assert_eq!(buf, vec![0x00]);

        buf.clear();
        write_vlq(&mut buf, 127);
        assert_eq!(buf, vec![0x7F]);

        buf.clear();
        write_vlq(&mut buf, 128);
        assert_eq!(buf, vec![0x81, 0x00]);

        buf.clear();
        write_vlq(&mut buf, 0x3FFF);
        assert_eq!(buf, vec![0xFF, 0x7F]);
    }
}
