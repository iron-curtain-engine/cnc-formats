// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026-present Iron Curtain contributors

//! Parser for the C&C Compiled String Format (`.csf`).
//!
//! CSF files are used in Tiberian Sun, Red Alert 2, and Generals to store
//! localized string tables. Note that while Tiberian Sun nominally introduced
//! them, they became heavily utilized in RA2 and Generals.

use crate::error::Error;
use std::collections::HashMap;

/// Maximum number of labels to permit (V38 bound).
const MAX_LABELS: usize = 100_000;

/// Maximum number of characters in a single string (V38 bound).
const MAX_STRING_CHARS: usize = 65_536;

#[inline]
fn read_slice<'input>(
    data: &'input [u8],
    offset: &mut usize,
    len: usize,
) -> Result<&'input [u8], Error> {
    let end = offset.checked_add(len).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available: data.len(),
    })?;
    let slice = data.get(*offset..end).ok_or(Error::UnexpectedEof {
        needed: end,
        available: data.len(),
    })?;
    *offset = end;
    Ok(slice)
}

#[inline]
fn advance_u32(data: &[u8], offset: &mut usize) -> Result<u32, Error> {
    let val = crate::read::read_u32_le(data, *offset)?;
    *offset += 4;
    Ok(val)
}

/// Represents a single CSF string entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsfString {
    /// The decoded text string.
    pub value: String,
    /// Optional extra ASCII data, typically an audio filename or macro name.
    /// Only present if the string magic was `STRW`.
    pub extra: Option<String>,
}

/// A parsed Compiled String Format file.
#[derive(Debug, Clone)]
pub struct CsfFile {
    /// The format version (typically 3).
    pub version: u32,
    /// The language ID code.
    pub language: u32,
    /// The mapping of string identifiers (labels) to their string data.
    pub labels: HashMap<String, Vec<CsfString>>,
}

impl CsfFile {
    /// Parses a CSF file from a byte slice.
    ///
    /// Evaluates the `FSC ` header, iterates over ` LBL` entries, and decodes
    /// the bitwise-NOT (XOR `0xFF`) UTF-16LE characters in ` STR` or ` STRW` payloads.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut offset: usize = 0;

        let magic = read_slice(data, &mut offset, 4)?;
        if magic != b" FSC" {
            return Err(Error::InvalidMagic {
                context: "CSF file header (expected ' FSC')",
            });
        }

        let version = advance_u32(data, &mut offset)?;
        let num_labels = advance_u32(data, &mut offset)?;
        let _num_strings = advance_u32(data, &mut offset)?; // Ignored in favor of actual label loops
        let _unused = advance_u32(data, &mut offset)?;
        let language = advance_u32(data, &mut offset)?;

        let num_labels_usize = num_labels as usize;
        if num_labels_usize > MAX_LABELS {
            return Err(Error::InvalidSize {
                value: num_labels_usize,
                limit: MAX_LABELS,
                context: "CSF labels count",
            });
        }

        let mut labels = HashMap::with_capacity(num_labels_usize);

        for _ in 0..num_labels_usize {
            let lbl_magic = read_slice(data, &mut offset, 4)?;
            if lbl_magic != b" LBL" {
                return Err(Error::InvalidMagic {
                    context: "CSF label entry (expected ' LBL')",
                });
            }

            let num_lbl_strings = advance_u32(data, &mut offset)?;
            let lbl_len = advance_u32(data, &mut offset)?;
            let lbl_len_usize = lbl_len as usize;

            if lbl_len_usize > MAX_STRING_CHARS {
                return Err(Error::InvalidSize {
                    value: lbl_len_usize,
                    limit: MAX_STRING_CHARS,
                    context: "CSF label name length",
                });
            }

            let lbl_bytes = read_slice(data, &mut offset, lbl_len_usize)?;
            let lbl_name = String::from_utf8_lossy(lbl_bytes).into_owned();

            let mut strings = Vec::with_capacity(num_lbl_strings as usize);

            for _ in 0..num_lbl_strings {
                let str_magic = read_slice(data, &mut offset, 4)?;
                let has_extra = match str_magic {
                    b" STR" => false,
                    b"STRW" => true,
                    _ => {
                        return Err(Error::InvalidMagic {
                            context: "CSF string entry (expected ' STR' or 'STRW')",
                        })
                    }
                };

                let value_len_chars = advance_u32(data, &mut offset)?;
                let value_len_usize = value_len_chars as usize;

                if value_len_usize > MAX_STRING_CHARS {
                    return Err(Error::InvalidSize {
                        value: value_len_usize,
                        limit: MAX_STRING_CHARS,
                        context: "CSF string character count",
                    });
                }

                let value_len_bytes = value_len_usize * 2;
                let value_bytes_raw = read_slice(data, &mut offset, value_len_bytes)?;

                // Decode UTF-16LE, XORing each byte via bitwise NOT
                let mut decoded_chars = Vec::with_capacity(value_len_usize);
                for i in 0..value_len_usize {
                    let lo = !value_bytes_raw[i * 2];
                    let hi = !value_bytes_raw[i * 2 + 1];
                    let ch = u16::from_le_bytes([lo, hi]);
                    decoded_chars.push(ch);
                }

                let value_str = String::from_utf16_lossy(&decoded_chars);

                let extra_str = if has_extra {
                    let extra_len = advance_u32(data, &mut offset)?;
                    let extra_len_usize = extra_len as usize;

                    if extra_len_usize > MAX_STRING_CHARS {
                        return Err(Error::InvalidSize {
                            value: extra_len_usize,
                            limit: MAX_STRING_CHARS,
                            context: "CSF extra value length",
                        });
                    }

                    let extra_bytes = read_slice(data, &mut offset, extra_len_usize)?;
                    Some(String::from_utf8_lossy(extra_bytes).into_owned())
                } else {
                    None
                };

                strings.push(CsfString {
                    value: value_str,
                    extra: extra_str,
                });
            }

            labels.insert(lbl_name, strings);
        }

        Ok(CsfFile {
            version,
            language,
            labels,
        })
    }
}

#[cfg(test)]
mod tests;
