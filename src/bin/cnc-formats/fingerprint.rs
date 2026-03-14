// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `fingerprint` subcommand — SHA-256 hash of a file.
//!
//! Computes and prints the SHA-256 digest of the raw file bytes in a
//! format compatible with `sha256sum`:
//!
//! ```text
//! a1b2c3d4...  filename.mix
//! ```

use sha2::{Digest, Sha256};

use super::read_file;

// ── fingerprint ──────────────────────────────────────────────────────────

/// Compute SHA-256 of the file and print the result.
pub(crate) fn cmd_fingerprint(path: &str) -> i32 {
    let data = read_file(path);
    let hash = Sha256::digest(&data);
    // Print in sha256sum-compatible format: <hex>  <filename>
    println!("{}  {path}", hex_encode(&hash));
    0
}

/// Encode a byte slice as lowercase hexadecimal.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX_CHARS[(b >> 4) as usize]);
        s.push(HEX_CHARS[(b & 0x0F) as usize]);
    }
    s
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];
