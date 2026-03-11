// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Error types for all cnc-formats parsers.
//!
//! ## Design Rationale
//!
//! A single shared `Error` enum serves every module.  This keeps the public
//! API surface small and lets callers use one `match` to handle errors from
//! any parser or decoder.
//!
//! Every variant carries **structured fields** (named, not positional) so
//! that callers can produce precise diagnostics without reaching for a
//! debugger.  Human-readable messages are built via `Display`, which embeds
//! the numeric context (byte counts, offsets, limits) directly into the
//! output text.
//!
//! Stringly-typed errors are avoided: context tags use `&'static str` to
//! prevent heap allocation in error paths.

use core::fmt;

/// Errors that can occur while parsing C&C format files.
///
/// All variants use named fields for diagnostic precision.  The intent is
/// that any error can be rendered into a human-readable message containing
/// every value the caller needs to diagnose the problem.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Error {
    /// Input data ended before the parser expected.
    UnexpectedEof {
        /// Minimum number of bytes needed to continue parsing.
        needed: usize,
        /// Number of bytes actually available in the input.
        available: usize,
    },
    /// A format-specific magic number or identifier was wrong.
    InvalidMagic {
        /// Which format or field was being validated.
        context: &'static str,
    },
    /// A size or count field contained an out-of-range value.
    InvalidSize {
        /// The value that was read from the input.
        value: usize,
        /// The maximum allowed value.
        limit: usize,
        /// Which field the size came from.
        context: &'static str,
    },
    /// An offset field pointed outside the data buffer.
    InvalidOffset {
        /// The computed end position (offset + size).
        offset: usize,
        /// The length of the buffer the offset is relative to.
        bound: usize,
    },
    /// LCW decompression encountered invalid data.
    ///
    /// The `reason` tag is a `&'static str` (not `String`) to keep the error
    /// type allocation-free — important because decompression runs on hot
    /// paths and the error type must be `no_std`-compatible.
    DecompressionError {
        /// What went wrong during decompression.
        reason: &'static str,
    },
    /// The archive uses Blowfish encryption and the `encrypted-mix` feature
    /// is not enabled, or decryption support is otherwise unavailable.
    EncryptedArchive,
    /// A computed or stored CRC did not match.
    CrcMismatch {
        /// The expected CRC value.
        expected: u32,
        /// The CRC value that was actually computed.
        found: u32,
    },
}

impl fmt::Display for Error {
    /// Renders a human-readable diagnostic message.
    ///
    /// Every message embeds the structured field values so the user sees
    /// exactly what went wrong (e.g. "needed 14 bytes but only 10 were
    /// available") without needing to attach a debugger.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEof { needed, available } => write!(
                f,
                "Unexpected end of input: needed at least {needed} bytes \
                 but only {available} were available.",
            ),
            Error::InvalidMagic { context } => {
                write!(f, "Invalid magic number or format identifier in {context}.")
            }
            Error::InvalidSize {
                value,
                limit,
                context,
            } => write!(
                f,
                "Size field out of range in {context}: \
                 value {value} exceeds the maximum of {limit}.",
            ),
            Error::InvalidOffset { offset, bound } => write!(
                f,
                "Offset out of bounds: computed end position {offset} \
                 exceeds buffer length {bound}.",
            ),
            Error::DecompressionError { reason } => {
                write!(f, "LCW decompression failed: {reason}.")
            }
            Error::EncryptedArchive => f.write_str(
                "Encrypted MIX archive (Blowfish): enable the \
                 `encrypted-mix` feature to decrypt automatically.",
            ),
            Error::CrcMismatch { expected, found } => write!(
                f,
                "CRC mismatch: expected 0x{expected:08X} but computed 0x{found:08X}.",
            ),
        }
    }
}
