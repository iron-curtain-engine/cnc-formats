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

use alloc::vec;
use alloc::vec::Vec;

use crate::error::Error;
use crate::read::{read_u16_le, read_u8};

// ─── Public Key Constants ────────────────────────────────────────────────────

/// Base-64 encoded RSA public key modulus used by Westwood's MIX encryption.
///
/// Source: XCC Utilities (Olaf van der Spek, 2000).  This string decodes to a
/// ~40 byte (320-bit) big-integer modulus.
const PUBKEY_STR: &[u8] = b"AihRvNoIbTn85FZRYNZRcT+i6KpU+maCsEqr3Q5q+LDB5tH7Tz2qQ38V";

/// RSA public exponent (0x10001 = 65537), the standard Fermat prime F4.
const PUBLIC_EXPONENT: u32 = 0x10001;

/// Size of the key_source block in encrypted MIX archives (bytes).
pub(crate) const KEY_SOURCE_LEN: usize = 80;

/// Size of the derived Blowfish key (bytes).
const BLOWFISH_KEY_LEN: usize = 56;

// ─── Base-64 Decoding ────────────────────────────────────────────────────────

/// Maps ASCII characters to 6-bit values for the Westwood base-64 alphabet.
///
/// Standard base-64: A-Z=0..25, a-z=26..51, 0-9=52..61, +=62, /=63.
/// Returns 0xFF for invalid characters.
const fn b64_val(c: u8) -> u8 {
    match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0xFF,
    }
}

/// Maximum base-64 decoded output size.  The Westwood key is 57 chars;
/// `ceil(57 / 4) * 3 = 45` bytes.  We use 48 for alignment headroom.
const B64_MAX_OUT: usize = 48;

/// Decodes a base-64 string into a stack buffer.
///
/// Returns `(buffer, length)` where `length` is the number of valid bytes.
/// Processes 4 characters at a time, producing 3 bytes per group.
/// Trailing partial groups (2 or 3 chars) produce 1 or 2 bytes respectively.
///
/// Uses a fixed-size stack buffer to avoid heap allocation.
fn b64_decode(input: &[u8]) -> ([u8; B64_MAX_OUT], usize) {
    let mut out = [0u8; B64_MAX_OUT];
    let mut len = 0usize;
    // Process full 4-character groups using .get() for safe access.
    let mut i = 0;
    while i + 3 < input.len() {
        // Safe: loop condition guarantees i+3 < input.len().
        let a = b64_val(*input.get(i).unwrap_or(&0)) as u32;
        let b = b64_val(*input.get(i + 1).unwrap_or(&0)) as u32;
        let c = b64_val(*input.get(i + 2).unwrap_or(&0)) as u32;
        let d = b64_val(*input.get(i + 3).unwrap_or(&0)) as u32;
        let triple = (a << 18) | (b << 12) | (c << 6) | d;
        if len < B64_MAX_OUT {
            out[len] = (triple >> 16) as u8;
            len += 1;
        }
        if len < B64_MAX_OUT {
            out[len] = (triple >> 8) as u8;
            len += 1;
        }
        if len < B64_MAX_OUT {
            out[len] = triple as u8;
            len += 1;
        }
        i += 4;
    }
    // Handle trailing 2 or 3 characters.
    let remaining = input.len() - i;
    if remaining == 3 {
        let a = b64_val(*input.get(i).unwrap_or(&0)) as u32;
        let b = b64_val(*input.get(i + 1).unwrap_or(&0)) as u32;
        let c = b64_val(*input.get(i + 2).unwrap_or(&0)) as u32;
        let triple = (a << 18) | (b << 12) | (c << 6);
        if len < B64_MAX_OUT {
            out[len] = (triple >> 16) as u8;
            len += 1;
        }
        if len < B64_MAX_OUT {
            out[len] = (triple >> 8) as u8;
            len += 1;
        }
    } else if remaining == 2 {
        let a = b64_val(*input.get(i).unwrap_or(&0)) as u32;
        let b = b64_val(*input.get(i + 1).unwrap_or(&0)) as u32;
        let triple = (a << 18) | (b << 12);
        if len < B64_MAX_OUT {
            out[len] = (triple >> 16) as u8;
            len += 1;
        }
    }
    (out, len)
}

// ─── Big-Integer Arithmetic ──────────────────────────────────────────────────
//
// Minimal big-integer library for RSA modular exponentiation.  Numbers are
// stored as little-endian arrays of u32 words.  The fixed size (`BN_WORDS`)
// is large enough for the 320-bit Westwood public key.

/// Number of u32 words in our big-integer representation.
///
/// 64 words × 4 bytes = 256 bytes = 2048 bits — more than enough for the
/// 320-bit Westwood RSA key.  This matches the XCC Utilities implementation.
const BN_WORDS: usize = 64;

/// Double-width buffer size for multiplication results: `BN_WORDS * 2 + 1`.
///
/// Used as a stack-allocated scratch buffer in `bn_mod_exp` and
/// `bn_mod_reduce`, avoiding heap allocation in the inner RSA loop.
const BN_DOUBLE: usize = BN_WORDS * 2 + 1;

/// A fixed-size big-integer stored as `BN_WORDS` little-endian `u32` words.
type BigNum = [u32; BN_WORDS];

/// Returns a zeroed big-integer.
const fn bn_zero() -> BigNum {
    [0u32; BN_WORDS]
}

/// Creates a big-integer from a single `u32` value.
fn bn_from_u32(val: u32) -> BigNum {
    let mut n = bn_zero();
    n[0] = val;
    n
}

/// Returns the effective length (number of significant words, min 0).
fn bn_len(n: &BigNum) -> usize {
    let mut i = BN_WORDS;
    while i > 0 && n[i - 1] == 0 {
        i -= 1;
    }
    i
}

/// Returns the bit length of the big-integer (0 for zero).
fn bn_bitlen(n: &BigNum) -> usize {
    let len = bn_len(n);
    if len == 0 {
        return 0;
    }
    let top = n[len - 1];
    // 32 - leading_zeros gives the position of the highest set bit.
    len * 32 - top.leading_zeros() as usize
}

/// Loads a big-endian byte slice into a little-endian BigNum.
///
/// The XCC key_to_bignum function loads a DER-like key.  Our version is
/// simpler: it takes raw big-endian bytes and distributes them into the
/// little-endian word array.
fn bn_from_be_bytes(bytes: &[u8]) -> BigNum {
    let mut n = bn_zero();
    // Process bytes from least significant to most significant.
    for (i, &b) in bytes.iter().rev().enumerate() {
        let word_idx = i / 4;
        let byte_idx = i % 4;
        if word_idx < BN_WORDS {
            n[word_idx] |= (b as u32) << (byte_idx * 8);
        }
    }
    n
}

/// Loads a little-endian byte slice into a BigNum.
///
/// Used to load key_source chunks which are stored in little-endian order
/// in the MIX file (matching the XCC implementation's memmove into bignum).
fn bn_from_le_bytes(bytes: &[u8]) -> BigNum {
    let mut n = bn_zero();
    for (i, &b) in bytes.iter().enumerate() {
        let word_idx = i / 4;
        let byte_idx = i % 4;
        if word_idx < BN_WORDS {
            n[word_idx] |= (b as u32) << (byte_idx * 8);
        }
    }
    n
}

/// Writes the low `count` bytes of a BigNum in little-endian order into `out`.
///
/// The caller provides a pre-allocated buffer.  Only `count` bytes are
/// written (where `count = out.len()`).  This avoids heap allocation.
fn bn_to_le_bytes(n: &BigNum, out: &mut [u8]) {
    for (i, byte) in out.iter_mut().enumerate() {
        let word_idx = i / 4;
        let byte_idx = i % 4;
        *byte = if word_idx < BN_WORDS {
            (n[word_idx] >> (byte_idx * 8)) as u8
        } else {
            0
        };
    }
}

/// Compares two big-integers.  Returns -1, 0, or 1.
fn bn_cmp(a: &BigNum, b: &BigNum) -> i32 {
    let mut i = BN_WORDS;
    while i > 0 {
        i -= 1;
        if a[i] < b[i] {
            return -1;
        }
        if a[i] > b[i] {
            return 1;
        }
    }
    0
}

/// Subtracts `b` from `a`, storing result in `dest`.  Returns the borrow bit.
///
/// All operands are treated as unsigned.  If `a < b` the result wraps and
/// borrow = 1.
fn bn_sub(dest: &mut BigNum, a: &BigNum, b: &BigNum) -> u32 {
    let mut borrow: u64 = 0;
    for i in 0..BN_WORDS {
        let diff = (a[i] as u64).wrapping_sub(b[i] as u64).wrapping_sub(borrow);
        dest[i] = diff as u32;
        // If the subtraction underflowed, carry a borrow.
        borrow = if diff > u32::MAX as u64 { 1 } else { 0 };
    }
    borrow as u32
}

/// Multiplies two big-integers, storing the double-width result in `dest`.
///
/// `dest` must be at least `2 * BN_WORDS` words, but we use the same
/// `BigNum` type and only multiply numbers whose effective length fits.
/// This uses the schoolbook O(n²) algorithm, which is fine for 320-bit keys.
fn bn_mul(dest: &mut [u32], a: &BigNum, b: &BigNum, len: usize) {
    // Zero the destination (2*len words).
    for w in dest.iter_mut().take(len * 2) {
        *w = 0;
    }
    for i in 0..len {
        let mut carry: u64 = 0;
        for j in 0..len {
            let prod = (a[i] as u64) * (b[j] as u64) + dest[i + j] as u64 + carry;
            dest[i + j] = prod as u32;
            carry = prod >> 32;
        }
        if i + len < dest.len() {
            dest[i + len] = carry as u32;
        }
    }
}

/// Computes `base^exp mod modulus` using square-and-multiply.
///
/// This is the core RSA operation.  For the Westwood key, `exp = 0x10001`
/// and `modulus` is a 320-bit number, so performance is not a concern
/// (only ~17 squarings and 2 multiplications needed for exponent 65537).
fn bn_mod_exp(base: &BigNum, exp: &BigNum, modulus: &BigNum) -> BigNum {
    let mod_len = bn_len(modulus);
    if mod_len == 0 {
        return bn_zero();
    }
    let exp_bits = bn_bitlen(exp);
    if exp_bits == 0 {
        // x^0 = 1 (for any non-zero modulus)
        return bn_from_u32(1);
    }

    // Start with result = base mod modulus.
    let mut result = *base;
    // Stack-allocated scratch buffer for double-width multiplication results.
    // BN_DOUBLE = 129 words = 516 bytes — trivial for the stack.
    let mut tmp_buf = [0u32; BN_DOUBLE];

    // Square-and-multiply from the second-highest bit down to bit 0.
    for bit_pos in (0..exp_bits - 1).rev() {
        let word_idx = bit_pos / 32;
        let bit_idx = bit_pos % 32;

        // Square: result = result * result mod modulus
        bn_mul(&mut tmp_buf, &result, &result, mod_len);
        result = bn_mod_reduce(&tmp_buf, modulus, mod_len);

        // Multiply if bit is set: result = result * base mod modulus
        if (exp[word_idx] >> bit_idx) & 1 == 1 {
            bn_mul(&mut tmp_buf, &result, base, mod_len);
            result = bn_mod_reduce(&tmp_buf, modulus, mod_len);
        }
    }

    result
}

/// Reduces a double-width number modulo `modulus` using trial subtraction.
///
/// This is a simple shift-and-subtract algorithm.  For the small key sizes
/// used by Westwood (320-bit), this is fast enough.
fn bn_mod_reduce(product: &[u32], modulus: &BigNum, mod_len: usize) -> BigNum {
    // Copy product into a stack-allocated working buffer.
    // BN_DOUBLE = 129 words = 516 bytes — avoids heap allocation.
    let prod_len = mod_len * 2 + 1;
    let mut rem = [0u32; BN_DOUBLE];
    let copy_len = prod_len.min(product.len()).min(BN_DOUBLE);
    rem[..copy_len].copy_from_slice(&product[..copy_len]);

    // Find effective length of remainder.
    let mut rem_len = prod_len;
    while rem_len > 0 && rem[rem_len - 1] == 0 {
        rem_len -= 1;
    }

    // Repeated subtraction with alignment.
    // We align the modulus to the top of the remainder and subtract downward.
    let mod_bits = bn_bitlen(modulus);
    let rem_bits = {
        if rem_len == 0 {
            0
        } else {
            (rem_len - 1) * 32 + (32 - rem[rem_len - 1].leading_zeros() as usize)
        }
    };

    if rem_bits <= mod_bits {
        // Already smaller than modulus — just copy.
        let mut result = bn_zero();
        result[..mod_len.min(rem_len)].copy_from_slice(&rem[..mod_len.min(rem_len)]);
        // Final check: if result >= modulus, subtract once.
        if bn_cmp(&result, modulus) >= 0 {
            let mut tmp = bn_zero();
            bn_sub(&mut tmp, &result, modulus);
            return tmp;
        }
        return result;
    }

    // Shift-and-subtract: process one bit at a time from the top.
    // We build the remainder by shifting in one bit at a time from the
    // product, subtracting the modulus whenever the partial remainder
    // is >= modulus.
    let mut result = bn_zero();
    for bit in (0..rem_bits).rev() {
        // Shift result left by 1 bit.
        let mut carry = 0u32;
        for word in result.iter_mut().take(mod_len) {
            let new_carry = *word >> 31;
            *word = (*word << 1) | carry;
            carry = new_carry;
        }

        // Bring in the next bit from the product.
        let word_idx = bit / 32;
        let bit_idx = bit % 32;
        if word_idx < rem.len() {
            result[0] |= (rem[word_idx] >> bit_idx) & 1;
        }

        // If result >= modulus, subtract.
        if bn_cmp(&result, modulus) >= 0 {
            let mut tmp = bn_zero();
            bn_sub(&mut tmp, &result, modulus);
            result = tmp;
        }
    }

    result
}

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
fn init_pubkey() -> PubKey {
    let (raw, raw_len) = b64_decode(PUBKEY_STR);
    let raw = &raw[..raw_len];

    // The decoded bytes start with a DER-like tag: 0x02 (INTEGER), then length.
    // XCC's key_to_bignum skips the tag byte and reads the length.
    let modulus = if raw.len() >= 2 {
        // Safe reads via helpers (defense-in-depth).
        let tag = read_u8(raw, 0).unwrap_or(0);
        if tag == 0x02 {
            // Simple length encoding (single byte, no high-bit flag for our key).
            let key_len = read_u8(raw, 1).unwrap_or(0) as usize;
            let key_bytes = &raw[2..2 + key_len.min(raw.len() - 2)];
            bn_from_be_bytes(key_bytes)
        } else {
            bn_from_be_bytes(raw)
        }
    } else {
        // Fallback: treat entire decoded output as big-endian modulus.
        bn_from_be_bytes(raw)
    };

    let exponent = bn_from_u32(PUBLIC_EXPONENT);
    let mod_bitlen = bn_bitlen(&modulus);

    PubKey {
        modulus,
        exponent,
        mod_bitlen,
    }
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

    let pubkey = init_pubkey();

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
        bn_to_le_bytes(&result, &mut key_buf[key_len..write_end]);
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
    key.copy_from_slice(&key_buf[..BLOWFISH_KEY_LEN]);
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
    use blowfish::BlowfishLE;

    // Initialize Blowfish with the derived 56-byte key.
    // `BlowfishLE` reads/writes each u32 half in little-endian order,
    // matching the Westwood MIX encryption format.  This eliminates the
    // need for manual byte-swapping around each block operation.
    let cipher = BlowfishLE::new_from_slice(key).map_err(|_| Error::DecompressionError {
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
    // BlowfishLE operates directly on little-endian u32 pairs, matching
    // the Westwood on-disk format — no manual byte-swapping needed.
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
        decrypted[src_off..src_off + 8].copy_from_slice(&block);
    }

    // Return only the meaningful bytes (not the padding).
    decrypted.truncate(header_size);
    Ok(decrypted)
}

#[cfg(test)]
mod tests;
