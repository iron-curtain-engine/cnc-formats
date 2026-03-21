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

use super::open_file;
use std::io::Read;

// ── fingerprint ──────────────────────────────────────────────────────────

/// Compute SHA-256 of the file and print the result.
pub(crate) fn cmd_fingerprint(path: &str) -> i32 {
    let mut file = open_file(path);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];

    loop {
        let read = match file.read(&mut buf) {
            Ok(read) => read,
            Err(e) => {
                eprintln!("Error reading {path}: {e}");
                return 1;
            }
        };
        if read == 0 {
            break;
        }

        if let Some(chunk) = buf.get(..read) {
            hasher.update(chunk);
        }
    }

    let hash = hasher.finalize();
    // Print in sha256sum-compatible format: <hex>  <filename>
    println!("{}  {path}", hex_encode(&hash));
    0
}

/// Encode a byte slice as lowercase hexadecimal.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let hi = HEX_CHARS.get((b >> 4) as usize).copied().unwrap_or('0');
        let lo = HEX_CHARS.get((b & 0x0F) as usize).copied().unwrap_or('0');
        s.push(hi);
        s.push(lo);
    }
    s
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];
