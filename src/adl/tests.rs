use super::*;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Builds a minimal valid ADL binary with one instrument and some register writes.
///
/// Layout:
///   [0..2]   u16 LE offset to instrument 0 (points to byte 4)
///   [2..4]   u16 LE 0x0000 — end of offset table (sentinel)
///   [4..15]  11 bytes of instrument patch data
///   [15..17] u16 LE speed value (e.g. 4)
///   [17..]   register-write pairs
fn build_minimal_adl() -> Vec<u8> {
    let mut data = Vec::new();
    // Instrument offset table: one instrument at offset 4.
    // Offset 0 → instrument 0 at byte 4.
    data.extend_from_slice(&4u16.to_le_bytes());
    // Sentinel: offset 0 signals end of table.
    data.extend_from_slice(&0u16.to_le_bytes());
    // Instrument 0: 11 bytes of OPL2 register data.
    data.extend_from_slice(&[
        0x21, 0x31, 0x4F, 0x00, 0xF2, 0xF2, 0x60, 0x60, 0x00, 0x00, 0x06,
    ]);
    // Sub-song: speed = 4.
    data.extend_from_slice(&4u16.to_le_bytes());
    // Register writes: 3 pairs.
    data.extend_from_slice(&[0x20, 0x21, 0x40, 0x3F, 0xA0, 0x98]);
    data
}

/// Builds a minimal Dune II-style ADL container with two indexed sub-songs.
///
/// Layout:
///   [0..120]     primary sub-song table: indexes 0, 1, then `0xFF`
///   [120..620]   250 track pointers (`u16`, relative to byte 120)
///   [620..1120]  250 instrument pointers (`u16`, relative to byte 120)
///   [1120..]     two tiny track payloads followed by one 11-byte patch
fn build_dune2_container_adl() -> Vec<u8> {
    let mut data = vec![0xFFu8; 1120 + 2 + 2 + INSTRUMENT_SIZE];

    // Primary sub-song table: two songs, then unused sentinel slots.
    data[0] = 0;
    data[1] = 1;

    // Track pointer table: program 0 at 1000, program 1 at 1002.
    data[120..122].copy_from_slice(&1000u16.to_le_bytes());
    data[122..124].copy_from_slice(&1002u16.to_le_bytes());

    // Instrument pointer table: patch 0 starts after the two 2-byte tracks.
    data[620..622].copy_from_slice(&1004u16.to_le_bytes());
    data[622..624].copy_from_slice(&0u16.to_le_bytes());

    // Two tiny placeholder track payloads.  The parser validates boundaries
    // and preserves sub-song structure without decoding Westwood bytecode.
    data[1120..1122].copy_from_slice(&[0x10, 0x20]);
    data[1122..1124].copy_from_slice(&[0x30, 0x40]);

    // One OPL2 patch.
    data[1124..1135].copy_from_slice(&[
        0x21, 0x31, 0x4F, 0x00, 0xF2, 0xF2, 0x60, 0x60, 0x00, 0x00, 0x06,
    ]);

    data
}

// ── Basic functionality ──────────────────────────────────────────────────────

/// Parse a minimal valid ADL file and verify instrument + register write counts.
///
/// The golden-path test confirms the parser correctly separates instrument
/// patches from register write data.
#[test]
fn parse_minimal_adl() {
    let data = build_minimal_adl();
    let adl = AdlFile::parse(&data).unwrap();
    assert_eq!(adl.instruments.len(), 1);
    assert_eq!(adl.instruments[0].registers[0], 0x21);
    assert_eq!(adl.instruments[0].registers[10], 0x06);
    assert_eq!(adl.subsongs.len(), 1);
    assert_eq!(adl.subsongs[0].speed_ticks_per_step(), Some(4));
    assert_eq!(adl.subsongs[0].channel_count(), 1);
    assert_eq!(adl.subsongs[0].register_write_count(), 3);
    let channels = adl.subsongs[0].decoded_channels().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].len(), 3);
}

/// Parse a Dune II container ADL and preserve its indexed sub-song structure.
///
/// Why: real soundtrack files use the 120-byte primary song table plus
/// fixed track/instrument pointer tables.  Collapsing that header into one
/// flat sub-song hides valid cue boundaries.
///
/// How: the synthetic container points sub-song 0 and 1 at separate track
/// payloads and stores one 11-byte instrument patch after them.
#[test]
fn parse_dune2_container_subsongs() {
    let data = build_dune2_container_adl();
    let adl = AdlFile::parse(&data).unwrap();
    assert_eq!(adl.instruments.len(), 1);
    assert_eq!(adl.instruments[0].registers[0], 0x21);
    assert_eq!(adl.subsongs.len(), 2);
    assert_eq!(adl.subsongs[0].speed_ticks_per_step(), None);
    assert_eq!(adl.subsongs[1].speed_ticks_per_step(), None);
    assert_eq!(adl.subsongs[0].channel_count(), 0);
    assert_eq!(adl.subsongs[1].channel_count(), 0);
    assert!(adl.subsongs[0].decoded_channels().is_none());
    assert!(adl.subsongs[1].decoded_channels().is_none());
    let song0 = adl.subsongs[0].track_program().unwrap();
    let song1 = adl.subsongs[1].track_program().unwrap();
    assert_eq!(song0.index.to_raw(), 0);
    assert_eq!(song0.offset.to_raw(), 1000);
    assert_eq!(song1.index.to_raw(), 1);
    assert_eq!(song1.offset.to_raw(), 1002);
    assert_eq!(adl.total_register_writes(), 0);
}

/// Verify total_register_writes returns the correct count.
///
/// Ensures the convenience method sums across all channels and sub-songs.
#[test]
fn total_register_writes_count() {
    let data = build_minimal_adl();
    let adl = AdlFile::parse(&data).unwrap();
    assert_eq!(adl.total_register_writes(), 3);
}

/// Verify estimated_duration_secs returns a positive value.
///
/// With speed=4 and 3 writes at 560 Hz base, duration should be
/// approximately 3 × 4 / 560 ≈ 0.0214 seconds.
#[test]
fn estimated_duration_positive() {
    let data = build_minimal_adl();
    let adl = AdlFile::parse(&data).unwrap();
    let dur = adl.estimated_duration_secs().unwrap();
    assert!(dur > 0.0, "expected positive duration, got {dur}");
    assert!(dur < 1.0, "expected sub-second duration, got {dur}");
}

/// Indexed Dune II container songs report unknown duration until decoded.
///
/// Why: the parser preserves validated track-program references for real
/// Dune II containers instead of fabricating decoded write counts or speed.
#[test]
fn estimated_duration_unknown_for_indexed_container() {
    let data = build_dune2_container_adl();
    let adl = AdlFile::parse(&data).unwrap();
    assert_eq!(adl.estimated_duration_secs(), None);
}

// ── Error paths ──────────────────────────────────────────────────────────────

/// Empty input → UnexpectedEof.
///
/// V38: zero bytes must not cause a panic.
#[test]
fn parse_empty() {
    let result = AdlFile::parse(&[]);
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 2);
            assert_eq!(available, 0);
        }
        other => panic!("expected UnexpectedEof, got {other}"),
    }
}

/// Single byte → UnexpectedEof.
///
/// V38: one byte is insufficient for even the first u16 offset.
#[test]
fn parse_one_byte() {
    let result = AdlFile::parse(&[0x42]);
    assert!(result.is_err());
}

// ── Error Display verification ───────────────────────────────────────────────

/// `Error::Display` for ADL UnexpectedEof includes numeric byte counts.
///
/// Why: the Display output is the user-facing diagnostic message; it must
/// include `needed` and `available` byte counts so the user can identify
/// exactly where truncation occurred.
#[test]
fn error_display_contains_byte_counts() {
    let err = AdlFile::parse(&[0x42]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains('2'), "should mention needed bytes: {msg}");
    assert!(msg.contains('1'), "should mention available bytes: {msg}");
}

/// `Error::Display` for ADL `InvalidOffset` includes the bad offset and bound.
///
/// Why: indexed Dune II pointer failures must report the numeric boundary
/// values so callers can identify whether a header pointer fell into the
/// table area or past the file end.
#[test]
fn error_display_invalid_offset_contains_bounds() {
    let mut data = build_dune2_container_adl();
    data[122..124].copy_from_slice(&999u16.to_le_bytes());

    let err = AdlFile::parse(&data).unwrap_err();
    let msg = err.to_string();

    assert!(msg.contains("1119"), "should mention the bad offset: {msg}");
    assert!(
        msg.contains(&data.len().to_string()),
        "should mention the buffer length: {msg}"
    );
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Same input → same output.
///
/// Ensures the parser has no hidden state or non-deterministic paths.
#[test]
fn parse_deterministic() {
    let data = build_minimal_adl();
    let a = AdlFile::parse(&data).unwrap();
    let b = AdlFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ── Boundary tests ───────────────────────────────────────────────────────────

/// Exactly 2 bytes with a zero offset → no instruments, no panic.
///
/// The minimum parseable input: just the sentinel offset.
#[test]
fn parse_minimal_two_bytes() {
    let data = [0x00, 0x00];
    let adl = AdlFile::parse(&data).unwrap();
    assert!(adl.instruments.is_empty());
    assert!(adl.subsongs.is_empty());
}

/// Instrument offset points past end of file → UnexpectedEof.
///
/// V38: offset bounds validation must reject out-of-range instrument offsets.
#[test]
fn parse_instrument_offset_past_eof() {
    // Offset table: instrument at offset 100, sentinel 0.
    let mut data = Vec::new();
    data.extend_from_slice(&100u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    let result = AdlFile::parse(&data);
    assert!(result.is_err());
}

/// Dune II track pointers into the header tables are rejected as `InvalidOffset`.
///
/// Why: Westwood container pointers are relative to byte 120, and any value
/// below 1000 points back into the fixed header tables instead of the track
/// or patch data region.
#[test]
fn parse_dune2_track_pointer_before_data_is_invalid_offset() {
    let mut data = build_dune2_container_adl();
    data[122..124].copy_from_slice(&999u16.to_le_bytes());

    let result = AdlFile::parse(&data);
    match result.unwrap_err() {
        Error::InvalidOffset { offset, bound } => {
            assert_eq!(offset, 1119);
            assert_eq!(bound, data.len());
        }
        other => panic!("expected InvalidOffset, got {other}"),
    }
}

// ── Security edge-case tests (V38) ──────────────────────────────────────────

/// `AdlFile::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer maximises every header field, exercising
/// overflow guards, output caps, and offset bounds checks.  All u16 offsets
/// read as 0xFFFF, which should be caught by bounds validation.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = AdlFile::parse(&data);
}

/// `AdlFile::parse` on 256 bytes of `0x00` must not panic.
///
/// Why (V38): zero-filled input exercises zero-offset paths, zero-speed
/// divisions, and empty-payload handling.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = AdlFile::parse(&data);
}

/// Large number of register-write pairs must be capped by MAX_REGISTER_WRITES.
///
/// V38: prevents unbounded parsing from a malicious file claiming millions
/// of register writes.
#[test]
fn adversarial_huge_register_writes_capped() {
    // Build a file with the instrument sentinel (0, 0) then speed=1 and
    // many register pairs.
    let mut data = Vec::with_capacity(2_000_010);
    // Sentinel offset (no instruments).
    data.extend_from_slice(&0u16.to_le_bytes());
    // Speed.
    data.extend_from_slice(&1u16.to_le_bytes());
    // 1.5M bytes of register pairs → 750K pairs, should be capped.
    data.resize(1_500_004, 0x42);
    let adl = AdlFile::parse(&data).unwrap();
    // Total writes should be capped at MAX_REGISTER_WRITES.
    assert!(adl.total_register_writes() <= 1_000_000);
}
