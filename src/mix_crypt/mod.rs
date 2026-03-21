// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Blowfish key derivation and header decryption for encrypted MIX archives.
//!
//! ## Algorithm Overview
//!
//! Encrypted RA/TS MIX archives embed an 80-byte `key_source` block after the
//! flags word.  A 56-byte Blowfish key is derived from this block using
//! RSA-like modular exponentiation with a publicly known public key, then the
//! MIX header (count + index) is decrypted with Blowfish in ECB mode.
//!
//! ## Key Derivation Steps
//!
//! 1. Decode the base-64 public key string into a ~40 byte big-integer (the
//!    RSA modulus `n`).  The public exponent `e` is fixed at `0x10001`.
//! 2. Split the 80-byte `key_source` into chunks of `(bitlen(n) - 1) / 8 + 1`
//!    bytes.  Each chunk is treated as a little-endian big-integer.
//! 3. Compute `chunk^e mod n` for each chunk (modular exponentiation).
//! 4. Concatenate the `(bitlen(n) - 1) / 8` low bytes of each result.
//! 5. The first 56 bytes of the concatenation form the Blowfish key.
//!
//! ## Blowfish ECB Decryption
//!
//! The encrypted header is decrypted in 8-byte blocks.  Each block is read as
//! two little-endian `u32` words, deciphered, and written back in little-endian
//! order.  The first decrypted block reveals the file count, from which we
//! calculate how many more blocks to decrypt (enough for the full index).
//!
//! ## Public Key Source
//!
//! The public key string `"AihRvNoIbTn85FZRYNZRcT+i6KpU+maCsEqr3Q5q+LDB5tH7Tz2qQ38V"`
//! and the exponent `0x10001` are public knowledge, documented by XCC Utilities
//! (Olaf van der Spek, 2000), OpenRA, and numerous community tools for over
//! two decades.  These values are numbers, not copyrightable expression.
//! The Blowfish algorithm itself is public domain.
//!
//! ## References
//!
//! - XCC Utilities MIX format documentation (Olaf van der Spek, 2000–2005)
//! - `binary-codecs.md` in the Iron Curtain design-docs repository

use crate::error::Error;
use crate::read::{read_u16_le, read_u8};

// ─── Public Key Constants ────────────────────────────────────────────────────

/// Base-64 encoded RSA public key modulus used by Westwood's MIX encryption.
///
/// Source: XCC Utilities (Olaf van der Spek, 2000).  This string decodes to a
/// ~40 byte (320-bit) big-integer modulus.  Uses standard RFC 4648 base64
/// encoding (no padding).
const PUBKEY_STR: &str = "AihRvNoIbTn85FZRYNZRcT+i6KpU+maCsEqr3Q5q+LDB5tH7Tz2qQ38V";

/// RSA public exponent (0x10001 = 65537), the standard Fermat prime F4.
const PUBLIC_EXPONENT: u32 = 0x10001;

/// Size of the key_source block in encrypted MIX archives (bytes).
pub(crate) const KEY_SOURCE_LEN: usize = 80;

/// Size of the derived Blowfish key (bytes).
const BLOWFISH_KEY_LEN: usize = 56;

// ─── Big-Integer Arithmetic ──────────────────────────────────────────────────
//
// Extracted into `bignum.rs` — a self-contained minimal big-integer library
// for RSA modular exponentiation.  Kept in a separate file to stay under the
// ~600-line LLM context budget.

mod bignum;
use bignum::{
    bn_bitlen, bn_from_be_bytes, bn_from_le_bytes, bn_from_u32, bn_mod_exp, bn_to_le_bytes, BigNum,
};
// Re-export additional bignum functions for use by tests.
#[cfg(test)]
use bignum::{bn_cmp, bn_len, bn_mul, bn_sub, bn_zero, BN_WORDS};

// ─── Public Key Initialization ───────────────────────────────────────────────

/// Decoded public key: modulus `n` and its bit-length.
struct PubKey {
    modulus: BigNum,
    exponent: BigNum,
    /// Bit length of the modulus minus 1, used to determine chunk sizes.
    mod_bitlen: usize,
}

/// Decodes the hardcoded public key string into a `PubKey`.
///
/// The base-64 string encodes a DER-like structure: one tag byte (`0x02`),
/// a length byte, then the raw modulus bytes in big-endian order.
fn init_pubkey() -> Result<PubKey, Error> {
    use base64::prelude::*;

    // Decode the standard base-64 public key string.
    let raw = BASE64_STANDARD_NO_PAD
        .decode(PUBKEY_STR)
        .map_err(|_| Error::InvalidMagic {
            context: "MIX public key base64",
        })?;

    // The decoded bytes start with a DER-like tag: 0x02 (INTEGER), then length.
    // XCC's key_to_bignum skips the tag byte and reads the length.
    let modulus = if raw.len() >= 2 {
        // Safe reads via helpers (defense-in-depth).
        let tag = read_u8(&raw, 0).unwrap_or(0);
        if tag == 0x02 {
            // Simple length encoding (single byte, no high-bit flag for our key).
            let key_len = read_u8(&raw, 1).unwrap_or(0) as usize;
            let end = 2 + key_len.min(raw.len().saturating_sub(2));
            let key_bytes = raw.get(2..end).unwrap_or(&[]);
            bn_from_be_bytes(key_bytes)
        } else {
            bn_from_be_bytes(&raw)
        }
    } else {
        // Fallback: treat entire decoded output as big-endian modulus.
        bn_from_be_bytes(&raw)
    };

    let exponent = bn_from_u32(PUBLIC_EXPONENT);

    let mod_bitlen = bn_bitlen(&modulus);

    Ok(PubKey {
        modulus,
        exponent,
        mod_bitlen,
    })
}

// ─── Key Derivation ──────────────────────────────────────────────────────────

/// Derives a 56-byte Blowfish key from the 80-byte `key_source` block.
///
/// This implements the `get_blowfish_key` function from XCC Utilities:
/// 1. Split `key_source` into RSA-sized chunks.
/// 2. For each chunk, compute `chunk^e mod n`.
/// 3. Concatenate the low bytes of each result.
/// 4. Return the first 56 bytes as the Blowfish key.
///
/// The chunk size for input is `a + 1` where `a = (bitlen(n) - 1) / 8`.
/// The output per chunk is `a` bytes.
pub(crate) fn derive_blowfish_key(key_source: &[u8]) -> Result<[u8; BLOWFISH_KEY_LEN], Error> {
    if key_source.len() < KEY_SOURCE_LEN {
        return Err(Error::UnexpectedEof {
            needed: KEY_SOURCE_LEN,
            available: key_source.len(),
        });
    }

    let pubkey = init_pubkey()?;

    // `a` = number of output bytes per RSA chunk.
    // `a + 1` = number of input bytes consumed per chunk.
    // These match XCC's `len_predata()`: input_len = (55 / a + 1) * (a + 1).
    let a = (pubkey.mod_bitlen - 1) / 8;
    let chunk_in = a + 1; // input bytes per chunk
    let chunk_out = a; // output bytes per chunk

    // Stack-allocated key material buffer.  We need at most BLOWFISH_KEY_LEN
    // + chunk_out bytes; 128 bytes is generous for any RSA key size ≤ 1024 bits.
    let mut key_buf = [0u8; 128];
    let mut key_len = 0usize;
    let mut offset = 0;

    // Process chunks until we have enough key material or exhaust input.
    while offset + chunk_in <= key_source.len() && key_len < BLOWFISH_KEY_LEN {
        // Load chunk as little-endian big-integer (matches XCC's memmove).
        let chunk_bytes =
            key_source
                .get(offset..offset + chunk_in)
                .ok_or(Error::UnexpectedEof {
                    needed: offset + chunk_in,
                    available: key_source.len(),
                })?;
        let chunk_bn = bn_from_le_bytes(chunk_bytes);

        // RSA: result = chunk ^ e mod n
        let result = bn_mod_exp(&chunk_bn, &pubkey.exponent, &pubkey.modulus);

        // Extract the low `a` bytes in little-endian order into key_buf.
        let write_end = (key_len + chunk_out).min(key_buf.len());
        if let Some(dst) = key_buf.get_mut(key_len..write_end) {
            bn_to_le_bytes(&result, dst);
        }
        key_len = write_end;

        offset += chunk_in;
    }

    // Copy the first 56 bytes into the fixed-size key array.
    if key_len < BLOWFISH_KEY_LEN {
        return Err(Error::DecompressionError {
            reason: "key derivation produced insufficient key material",
        });
    }

    let mut key = [0u8; BLOWFISH_KEY_LEN];
    let src = key_buf
        .get(..BLOWFISH_KEY_LEN)
        .ok_or(Error::DecompressionError {
            reason: "key buffer too short for Blowfish key",
        })?;
    key.copy_from_slice(src);
    Ok(key)
}

// ─── Blowfish ECB Decryption ─────────────────────────────────────────────────

/// Decrypts the encrypted MIX header using Blowfish ECB mode.
///
/// The encrypted region starts immediately after the 80-byte `key_source`.
/// We decrypt in 8-byte blocks.  Each block is read as two little-endian
/// `u32` words (matching the Westwood `reverse()` + decipher pattern from
/// XCC Utilities).
///
/// The first 8 bytes, once decrypted, contain the MIX `FileHeader`:
/// `count` (u16) + padding (u16, ignored) + `data_size` (u32).
/// Wait — actually the first 2 bytes are `count` (u16) and the next 4 are
/// `data_size` (u32), totalling 6 bytes.  The encrypted block is 8 bytes,
/// so we decrypt the first block and read the header from it.
///
/// Then we calculate how many total 8-byte blocks are needed for the full
/// header + index (6 + count × 12), decrypt that many, and return the
/// plaintext.
///
/// Returns the decrypted header bytes (FileHeader + SubBlock index).
pub(crate) fn decrypt_mix_header(
    encrypted_data: &[u8],
    key: &[u8; BLOWFISH_KEY_LEN],
) -> Result<Vec<u8>, Error> {
    use blowfish::cipher::BlockDecrypt;
    use blowfish::cipher::KeyInit;
    // Standard (big-endian) Blowfish — the default generic parameter is BE.
    type BlowfishBE = blowfish::Blowfish;

    // Initialize standard (big-endian) Blowfish with the derived 56-byte key.
    // Westwood's implementation (and OpenRA's port) byte-swaps each u32 half
    // to big-endian before the Feistel rounds and back to little-endian after.
    // The `blowfish` crate's standard `Blowfish` type does exactly this.
    let cipher = BlowfishBE::new_from_slice(key).map_err(|_| Error::DecompressionError {
        reason: "failed to initialize Blowfish cipher",
    })?;

    // Need at least 8 bytes to decrypt the first block (FileHeader).
    if encrypted_data.len() < 8 {
        return Err(Error::UnexpectedEof {
            needed: 8,
            available: encrypted_data.len(),
        });
    }

    // ── Decrypt first block to read the file count ───────────────────────
    let mut first_block = [0u8; 8];
    let first_slice = encrypted_data.get(..8).ok_or(Error::UnexpectedEof {
        needed: 8,
        available: encrypted_data.len(),
    })?;
    first_block.copy_from_slice(first_slice);
    // Standard Blowfish byte-swaps the u32 halves internally, matching
    // Westwood's swap-before-rounds convention.
    cipher.decrypt_block(
        blowfish::cipher::generic_array::GenericArray::from_mut_slice(&mut first_block),
    );

    // Read count (u16) from the decrypted first block.
    let count = read_u16_le(&first_block, 0)? as usize;

    // V38 safety cap: reject unreasonable entry counts.
    if count > super::mix::MAX_MIX_ENTRIES {
        return Err(Error::InvalidSize {
            value: count,
            limit: super::mix::MAX_MIX_ENTRIES,
            context: "encrypted MIX entry count",
        });
    }

    // ── Calculate total encrypted header size ────────────────────────────
    // FileHeader = 6 bytes (count u16 + data_size u32)
    // SubBlock index = count * 12 bytes
    // Total = 6 + count * 12, rounded up to the next 8-byte block boundary.
    let header_size = 6usize.saturating_add(count.saturating_mul(12));
    let num_blocks = header_size.div_ceil(8);
    let encrypted_len = num_blocks * 8;

    if encrypted_data.len() < encrypted_len {
        return Err(Error::UnexpectedEof {
            needed: encrypted_len,
            available: encrypted_data.len(),
        });
    }

    // ── Decrypt all header blocks ────────────────────────────────────────
    let mut decrypted = vec![0u8; encrypted_len];
    for block_idx in 0..num_blocks {
        let src_off = block_idx * 8;
        let mut block = [0u8; 8];
        // Safe: upfront check guarantees encrypted_data.len() >= encrypted_len;
        // .get() is defense-in-depth.
        let src_slice = encrypted_data
            .get(src_off..src_off + 8)
            .ok_or(Error::UnexpectedEof {
                needed: src_off + 8,
                available: encrypted_data.len(),
            })?;
        block.copy_from_slice(src_slice);
        cipher.decrypt_block(
            blowfish::cipher::generic_array::GenericArray::from_mut_slice(&mut block),
        );
        let dec_len = decrypted.len();
        let dst = decrypted
            .get_mut(src_off..src_off + 8)
            .ok_or(Error::UnexpectedEof {
                needed: src_off + 8,
                available: dec_len,
            })?;
        dst.copy_from_slice(&block);
    }

    // Return only the meaningful bytes (not the padding).
    decrypted.truncate(header_size);
    Ok(decrypted)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_vectors;
