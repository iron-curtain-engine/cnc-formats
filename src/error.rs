// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Error types for all cnc-formats parsers.

use core::fmt;

/// Errors that can occur while parsing C&C format files.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Error {
    /// Input data ended before the parser expected.
    UnexpectedEof,
    /// A format-specific magic number or identifier was wrong.
    InvalidMagic,
    /// A size or count field contained an out-of-range value.
    InvalidSize,
    /// An offset field pointed outside the data buffer.
    InvalidOffset,
    /// LCW decompression encountered a bad command or output overrun.
    DecompressionError,
    /// The archive uses Blowfish encryption, which is not yet supported.
    EncryptedArchive,
    /// A computed or stored CRC did not match.
    CrcMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEof => f.write_str("unexpected end of input"),
            Error::InvalidMagic => f.write_str("invalid magic number / format identifier"),
            Error::InvalidSize => f.write_str("invalid size or count field"),
            Error::InvalidOffset => f.write_str("offset points outside the data buffer"),
            Error::DecompressionError => f.write_str("LCW decompression error"),
            Error::EncryptedArchive => {
                f.write_str("encrypted MIX archive (Blowfish) is not yet supported")
            }
            Error::CrcMismatch => f.write_str("CRC mismatch"),
        }
    }
}
