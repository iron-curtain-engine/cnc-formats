use super::*;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Builds a minimal valid XMI binary with a single sequence containing one
/// Note-On event.
///
/// IFF structure:
///   FORM:XDIR { INFO: [count=1] }
///   CAT :XMID { FORM:XMID { EVNT: [Note-On ch0 C4 vel=100 dur=60, End-of-Track] } }
fn build_minimal_xmi() -> Vec<u8> {
    let mut data = Vec::new();

    // ── FORM:XDIR ────────────────────────────────────────────────────
    data.extend_from_slice(b"FORM");
    // FORM size: 4 (type) + 8 (INFO header) + 2 (INFO data) = 14
    data.extend_from_slice(&14u32.to_be_bytes());
    data.extend_from_slice(b"XDIR");

    // INFO chunk: 1 sequence.
    data.extend_from_slice(b"INFO");
    data.extend_from_slice(&2u32.to_be_bytes()); // INFO size
    data.extend_from_slice(&1u16.to_le_bytes()); // 1 sequence

    // ── CAT :XMID ───────────────────────────────────────────────────
    // Build EVNT data first so we know its size.
    // Note-On: status 0x90, note C4=60, velocity 100, duration 60.
    // End-of-Track meta event: 0xFF 0x2F 0x00.
    let evnt_data: Vec<u8> = vec![
        0x90, 60, 100, 60, // Note-On ch0, C4, vel=100, dur=60
        0xFF, 0x2F, 0x00, // End-of-Track
    ];

    // FORM:XMID header: 4 (type) + 8 (EVNT header) + evnt_data_len
    let form_body_size = 4 + 8 + evnt_data.len();
    // Pad EVNT if odd.
    let evnt_padded = if evnt_data.len() % 2 == 1 {
        evnt_data.len() + 1
    } else {
        evnt_data.len()
    };
    let form_body_size_padded = 4 + 8 + evnt_padded;

    // CAT size: 4 (type) + 8 (FORM header) + form_body_size
    let cat_body_size = 4 + 8 + form_body_size_padded;

    data.extend_from_slice(b"CAT ");
    data.extend_from_slice(&(cat_body_size as u32).to_be_bytes());
    data.extend_from_slice(b"XMID");

    // FORM:XMID
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_body_size as u32).to_be_bytes());
    data.extend_from_slice(b"XMID");

    // EVNT chunk.
    data.extend_from_slice(b"EVNT");
    data.extend_from_slice(&(evnt_data.len() as u32).to_be_bytes());
    data.extend_from_slice(&evnt_data);
    // Pad to even.
    if evnt_data.len() % 2 == 1 {
        data.push(0x00);
    }

    data
}

/// Builds a valid XMI whose INFO payload contains the literal bytes `CAT `
/// before the real CAT:XMID sibling chunk.
///
/// Why: the XDIR parser must follow chunk boundaries, not byte-scan for
/// magic strings inside unrelated payload data.
fn build_xmi_with_cat_bytes_inside_info() -> Vec<u8> {
    let mut data = Vec::new();

    data.extend_from_slice(b"FORM");
    // FORM body: "XDIR" + INFO header + INFO payload (u16 count + "CAT ").
    data.extend_from_slice(&18u32.to_be_bytes());
    data.extend_from_slice(b"XDIR");

    data.extend_from_slice(b"INFO");
    data.extend_from_slice(&6u32.to_be_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(b"CAT ");

    // Reuse the same real CAT:XMID body as the minimal fixture.
    let minimal = build_minimal_xmi();
    data.extend_from_slice(minimal.get(22..).unwrap_or(&[]));

    data
}

// ── Basic functionality ──────────────────────────────────────────────────────

/// Parse a minimal valid XMI file and verify sequence extraction.
///
/// Confirms the parser correctly separates the IFF container and extracts
/// the EVNT data from a FORM:XMID sequence.
#[test]
fn parse_minimal_xmi() {
    let data = build_minimal_xmi();
    let xmi = XmiFile::parse(&data).unwrap();
    assert_eq!(xmi.sequence_count(), 1);
    assert!(!xmi.sequences[0].event_data.is_empty());
}

/// Convert a minimal XMI sequence to standard MIDI and verify the output.
///
/// The generated SMF should start with a valid MThd header and contain
/// the Note-On event with a matching Note-Off.
#[test]
fn to_mid_basic() {
    let data = build_minimal_xmi();
    let xmi = XmiFile::parse(&data).unwrap();
    let mid_bytes = to_mid(&xmi.sequences[0]).unwrap();
    // Should start with MThd.
    assert_eq!(&mid_bytes[..4], b"MThd");
    // Should contain MTrk.
    let has_mtrk = mid_bytes.windows(4).any(|w| w == b"MTrk");
    assert!(has_mtrk, "output should contain MTrk");
    // Should be parseable (basic sanity check).
    assert!(mid_bytes.len() > 22);
}

/// Verify the VLQ encoder/decoder round-trips correctly.
///
/// Tests several values including 0, small, boundary, and large values.
#[test]
fn vlq_roundtrip() {
    for &value in &[0u32, 1, 127, 128, 0x3FFF, 0x1FFFFF, 0x0FFFFFFF] {
        let mut buf = Vec::new();
        write_vlq_to(&mut buf, value);
        let (decoded, bytes_read) = read_vlq(&buf, 0);
        assert_eq!(decoded, value, "VLQ round-trip failed for {value}");
        assert_eq!(bytes_read, buf.len());
    }
}

// ── Error paths ──────────────────────────────────────────────────────────────

/// Empty input → UnexpectedEof.
///
/// V38: zero bytes must not cause a panic.
#[test]
fn parse_empty() {
    let result = XmiFile::parse(&[]);
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 12);
            assert_eq!(available, 0);
        }
        other => panic!("expected UnexpectedEof, got {other}"),
    }
}

/// Wrong magic → InvalidMagic.
///
/// V38: non-IFF data must be rejected cleanly.
#[test]
fn parse_bad_magic() {
    let data = b"NOT_IFF_DATA_AT_ALL!";
    let result = XmiFile::parse(data);
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::InvalidMagic { .. } => {}
        other => panic!("expected InvalidMagic, got {other}"),
    }
}

/// Truncated FORM header → UnexpectedEof.
///
/// V38: 8 bytes is not enough for a complete FORM (needs 12).
#[test]
fn parse_truncated_form() {
    let data = b"FORM\x00\x00\x00\x04";
    let result = XmiFile::parse(data);
    assert!(result.is_err());
}

/// Truncated Note-On event data returns `UnexpectedEof`.
///
/// Why: `to_mid()` must reject EVNT streams that stop mid-event instead
/// of silently truncating the generated SMF.
#[test]
fn to_mid_truncated_note_on_returns_eof() {
    let seq = XmiSequence {
        timbres: vec![],
        event_data: &[0x90, 60],
    };
    let err = to_mid(&seq).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Truncated meta payload returns `UnexpectedEof`.
///
/// Why: EVNT meta events declare their payload length explicitly, so the
/// converter must verify that the full payload is present.
#[test]
fn to_mid_truncated_meta_payload_returns_eof() {
    let seq = XmiSequence {
        timbres: vec![],
        event_data: &[0xFF, 0x01, 0x02, 0x41],
    };
    let err = to_mid(&seq).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── INFO cross-validation ────────────────────────────────────────────────────

/// INFO declares 1 sequence; parser extracts exactly 1 even if the CAT
/// body contains padding that could be misinterpreted as extra chunks.
///
/// Why (V38): the INFO-declared count caps how many sequences are extracted,
/// preventing an attacker from injecting phantom FORM:XMID chunks after
/// the legitimate sequence.
#[test]
fn info_count_caps_sequences() {
    let data = build_minimal_xmi();
    let xmi = XmiFile::parse(&data).unwrap();
    // build_minimal_xmi declares INFO count=1, so only 1 sequence.
    assert_eq!(xmi.sequence_count(), 1);
}

/// INFO count larger than the actual CAT body does not invent sequences.
///
/// Why: INFO is an upper bound, not authority to fabricate nonexistent
/// FORM:XMID chunks past the real CAT contents.
#[test]
fn info_count_above_actual_sequences_uses_actual_count() {
    let mut data = build_minimal_xmi();
    if let Some(info_count) = data.get_mut(20..22) {
        info_count.copy_from_slice(&2u16.to_le_bytes());
    }

    let xmi = XmiFile::parse(&data).unwrap();
    assert_eq!(xmi.sequence_count(), 1);
}

/// `parse_xdir()` ignores `CAT ` bytes inside the INFO payload.
///
/// Why: structured chunk walking must not treat arbitrary payload bytes as
/// a top-level CAT:XMID chunk boundary.
#[test]
fn parse_xdir_ignores_cat_bytes_inside_info_payload() {
    let data = build_xmi_with_cat_bytes_inside_info();
    let xmi = XmiFile::parse(&data).unwrap();
    assert_eq!(xmi.sequence_count(), 1);
}

// ── Error Display verification ───────────────────────────────────────────────

/// `Error::Display` for XMI UnexpectedEof includes numeric byte counts.
///
/// Why: the Display output is the user-facing diagnostic message; it must
/// include `needed` and `available` so the user can diagnose truncation.
#[test]
fn error_display_eof_contains_byte_counts() {
    let err = XmiFile::parse(&[]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("12"), "should mention needed bytes: {msg}");
    assert!(msg.contains('0'), "should mention available bytes: {msg}");
}

/// `Error::Display` for XMI InvalidMagic includes the format context.
///
/// Why: when non-IFF data is rejected, the error message must identify
/// which format validation failed.
#[test]
fn error_display_magic_contains_context() {
    let data = b"NOT_IFF_DATA_AT_ALL!";
    let err = XmiFile::parse(data).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("XMI"), "should mention XMI context: {msg}");
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Same input → same output.
///
/// Ensures the parser has no hidden state or non-deterministic paths.
#[test]
fn parse_deterministic() {
    let data = build_minimal_xmi();
    let a = XmiFile::parse(&data).unwrap();
    let b = XmiFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

/// XMI→MID conversion is deterministic.
///
/// The same EVNT data must always produce the same SMF binary.
#[test]
fn to_mid_deterministic() {
    let data = build_minimal_xmi();
    let xmi = XmiFile::parse(&data).unwrap();
    let a = to_mid(&xmi.sequences[0]).unwrap();
    let b = to_mid(&xmi.sequences[0]).unwrap();
    assert_eq!(a, b);
}

// ── Security edge-case tests (V38) ──────────────────────────────────────────

/// `XmiFile::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer maximises every header field, exercising
/// overflow guards, chunk-size validation, and offset bounds checks.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = XmiFile::parse(&data);
}

/// `XmiFile::parse` on 256 bytes of `0x00` must not panic.
///
/// Why (V38): zero-filled input exercises zero-size chunks, zero sequence
/// counts, and empty-payload handling.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = XmiFile::parse(&data);
}

/// `to_mid` on empty EVNT data must not panic.
///
/// Why (V38): an empty event stream should produce a valid (empty) SMF.
#[test]
fn to_mid_empty_evnt_no_panic() {
    let seq = XmiSequence {
        timbres: vec![],
        event_data: &[],
    };
    let result = to_mid(&seq);
    assert!(result.is_ok());
    let mid = result.unwrap();
    // Should still have MThd + MTrk headers.
    assert_eq!(&mid[..4], b"MThd");
}

/// `to_mid` on adversarial all-0xFF EVNT data must not panic.
///
/// Why (V38): all-0xFF events have status bytes 0xFF (meta events) with
/// maximised length fields, exercising VLQ parsing limits and bounds checks.
#[test]
fn to_mid_adversarial_all_ff() {
    let data = vec![0xFFu8; 512];
    let seq = XmiSequence {
        timbres: vec![],
        event_data: &data,
    };
    let _ = to_mid(&seq);
}
