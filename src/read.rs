// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Safe binary reading helpers for C&C format parsers.
//!
//! These functions eliminate direct `data[offset]` indexing in production code
//! by using `.get()` with bounds-checked slicing.  Every read returns
//! [`Error::UnexpectedEof`] on failure instead of panicking.
//!
//! ## Why
//!
//! All parsers already perform upfront bounds checks before accessing fields.
//! These helpers provide **defense-in-depth**: if a bounds check is ever
//! modified incorrectly, the helper prevents a panic.  They also centralise
//! the read-and-convert pattern so each parser does not reimplement it.

use crate::error::Error;

/// Reads a single byte at `offset`.
///
/// Returns [`Error::UnexpectedEof`] if `offset` is out of bounds.
#[inline]
pub(crate) fn read_u8(data: &[u8], offset: usize) -> Result<u8, Error> {
    data.get(offset).copied().ok_or(Error::UnexpectedEof {
        needed: offset.saturating_add(1),
        available: data.len(),
    })
}

/// Reads a little-endian `u16` starting at `offset`.
///
/// Returns [`Error::UnexpectedEof`] if fewer than 2 bytes remain at `offset`.
#[inline]
pub(crate) fn read_u16_le(data: &[u8], offset: usize) -> Result<u16, Error> {
    let end = offset.checked_add(2).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available: data.len(),
    })?;
    let slice = data.get(offset..end).ok_or(Error::UnexpectedEof {
        needed: end,
        available: data.len(),
    })?;
    // Safe: .get(offset..offset+2) guarantees exactly 2 bytes.
    let mut buf = [0u8; 2];
    buf.copy_from_slice(slice);
    Ok(u16::from_le_bytes(buf))
}

/// Reads a little-endian `u32` starting at `offset`.
///
/// Returns [`Error::UnexpectedEof`] if fewer than 4 bytes remain at `offset`.
#[inline]
pub(crate) fn read_u32_le(data: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available: data.len(),
    })?;
    let slice = data.get(offset..end).ok_or(Error::UnexpectedEof {
        needed: end,
        available: data.len(),
    })?;
    // Safe: .get(offset..offset+4) guarantees exactly 4 bytes.
    let mut buf = [0u8; 4];
    buf.copy_from_slice(slice);
    Ok(u32::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── read_u8 ──────────────────────────────────────────────────────────

    /// Reading a single byte at a valid offset succeeds.
    #[test]
    fn read_u8_valid() {
        assert_eq!(read_u8(&[0xAB], 0).unwrap(), 0xAB);
    }

    /// Reading one past the end returns UnexpectedEof.
    #[test]
    fn read_u8_one_past_end() {
        let err = read_u8(&[0xFF], 1).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                needed: 2,
                available: 1
            }
        ));
    }

    /// Reading from an empty slice returns UnexpectedEof.
    #[test]
    fn read_u8_empty() {
        let err = read_u8(&[], 0).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                needed: 1,
                available: 0
            }
        ));
    }

    /// Offset at `usize::MAX` does not panic (saturating_add guards overflow).
    #[test]
    fn read_u8_usize_max_offset() {
        let err = read_u8(&[0], usize::MAX).unwrap_err();
        assert!(matches!(err, Error::UnexpectedEof { .. }));
    }

    // ── read_u16_le ──────────────────────────────────────────────────────

    /// Reading a little-endian u16 from a valid position.
    #[test]
    fn read_u16_valid() {
        assert_eq!(read_u16_le(&[0x34, 0x12], 0).unwrap(), 0x1234);
    }

    /// Reading u16 when only 1 byte remains returns UnexpectedEof.
    #[test]
    fn read_u16_one_byte_short() {
        let err = read_u16_le(&[0xFF], 0).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                needed: 2,
                available: 1
            }
        ));
    }

    /// Offset that would overflow `usize` via `offset + 2` returns error
    /// (checked_add returns None, reported as `needed: usize::MAX`).
    #[test]
    fn read_u16_offset_overflow() {
        let err = read_u16_le(&[0; 4], usize::MAX).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                needed: usize::MAX,
                ..
            }
        ));
    }

    /// Reading u16 at a non-zero valid offset.
    #[test]
    fn read_u16_at_offset() {
        assert_eq!(read_u16_le(&[0x00, 0x78, 0x56], 1).unwrap(), 0x5678);
    }

    // ── read_u32_le ──────────────────────────────────────────────────────

    /// Reading a little-endian u32 from a valid position.
    #[test]
    fn read_u32_valid() {
        assert_eq!(
            read_u32_le(&[0x78, 0x56, 0x34, 0x12], 0).unwrap(),
            0x12345678
        );
    }

    /// Reading u32 when only 3 bytes remain returns UnexpectedEof.
    #[test]
    fn read_u32_three_bytes_short() {
        let err = read_u32_le(&[0xFF; 3], 0).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                needed: 4,
                available: 3
            }
        ));
    }

    /// Offset that would overflow `usize` via `offset + 4` returns error.
    #[test]
    fn read_u32_offset_overflow() {
        let err = read_u32_le(&[0; 8], usize::MAX).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                needed: usize::MAX,
                ..
            }
        ));
    }

    /// Reading u32 at exact last valid position.
    #[test]
    fn read_u32_at_end() {
        let data = [0x00, 0x00, 0x01, 0x02, 0x03, 0x04];
        assert_eq!(read_u32_le(&data, 2).unwrap(), 0x04030201);
    }

    /// One byte past the last valid u32 position returns UnexpectedEof.
    #[test]
    fn read_u32_one_past_end() {
        let data = [0x00; 6];
        let err = read_u32_le(&data, 3).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                needed: 7,
                available: 6
            }
        ));
    }
}
