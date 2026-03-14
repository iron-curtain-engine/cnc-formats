// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Minimal big-integer library for RSA modular exponentiation.
//!
//! Numbers are stored as little-endian arrays of `u32` words.  The fixed
//! size (`BN_WORDS`) is large enough for the 320-bit Westwood public key.
//!
//! This module is internal to `mix_crypt` — it exists solely to keep file
//! sizes under the ~600-line LLM context budget.
//!
//! All indexing uses `.get()` / `.get_mut()` per the project's safe-indexing
//! rule — no direct `[]` indexing in production code.

/// Number of u32 words in our big-integer representation.
///
/// 64 words × 4 bytes = 256 bytes = 2048 bits — more than enough for the
/// 320-bit Westwood RSA key.  This matches the XCC Utilities implementation.
pub(super) const BN_WORDS: usize = 64;

/// Double-width buffer size for multiplication results: `BN_WORDS * 2 + 1`.
///
/// Used as a stack-allocated scratch buffer in `bn_mod_exp` and
/// `bn_mod_reduce`, avoiding heap allocation in the inner RSA loop.
const BN_DOUBLE: usize = BN_WORDS * 2 + 1;

/// A fixed-size big-integer stored as `BN_WORDS` little-endian `u32` words.
pub(super) type BigNum = [u32; BN_WORDS];

/// Returns a zeroed big-integer.
pub(super) const fn bn_zero() -> BigNum {
    [0u32; BN_WORDS]
}

/// Creates a big-integer from a single `u32` value.
pub(super) fn bn_from_u32(val: u32) -> BigNum {
    let mut n = bn_zero();
    if let Some(w) = n.get_mut(0) {
        *w = val;
    }
    n
}

/// Returns the effective length (number of significant words, min 0).
pub(super) fn bn_len(n: &BigNum) -> usize {
    let mut i = BN_WORDS;
    while i > 0 && n.get(i - 1) == Some(&0) {
        i -= 1;
    }
    i
}

/// Returns the bit length of the big-integer (0 for zero).
pub(super) fn bn_bitlen(n: &BigNum) -> usize {
    let len = bn_len(n);
    if len == 0 {
        return 0;
    }
    let top = n.get(len - 1).copied().unwrap_or(0);
    // 32 - leading_zeros gives the position of the highest set bit.
    len * 32 - top.leading_zeros() as usize
}

/// Loads a big-endian byte slice into a little-endian BigNum.
///
/// The XCC key_to_bignum function loads a DER-like key.  Our version is
/// simpler: it takes raw big-endian bytes and distributes them into the
/// little-endian word array.
pub(super) fn bn_from_be_bytes(bytes: &[u8]) -> BigNum {
    let mut n = bn_zero();
    // Process bytes from least significant to most significant.
    for (i, &b) in bytes.iter().rev().enumerate() {
        let word_idx = i / 4;
        let byte_idx = i % 4;
        if let Some(word) = n.get_mut(word_idx) {
            *word |= (b as u32) << (byte_idx * 8);
        }
    }
    n
}

/// Loads a little-endian byte slice into a BigNum.
///
/// Used to load key_source chunks which are stored in little-endian order
/// in the MIX file (matching the XCC implementation's memmove into bignum).
pub(super) fn bn_from_le_bytes(bytes: &[u8]) -> BigNum {
    let mut n = bn_zero();
    for (i, &b) in bytes.iter().enumerate() {
        let word_idx = i / 4;
        let byte_idx = i % 4;
        if let Some(word) = n.get_mut(word_idx) {
            *word |= (b as u32) << (byte_idx * 8);
        }
    }
    n
}

/// Writes the low `count` bytes of a BigNum in little-endian order into `out`.
///
/// The caller provides a pre-allocated buffer.  Only `count` bytes are
/// written (where `count = out.len()`).  This avoids heap allocation.
pub(super) fn bn_to_le_bytes(n: &BigNum, out: &mut [u8]) {
    for (i, byte) in out.iter_mut().enumerate() {
        let word_idx = i / 4;
        let byte_idx = i % 4;
        *byte = n.get(word_idx).map_or(0, |w| (*w >> (byte_idx * 8)) as u8);
    }
}

/// Compares two big-integers.  Returns -1, 0, or 1.
pub(super) fn bn_cmp(a: &BigNum, b: &BigNum) -> i32 {
    let mut i = BN_WORDS;
    while i > 0 {
        i -= 1;
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        if av < bv {
            return -1;
        }
        if av > bv {
            return 1;
        }
    }
    0
}

/// Subtracts `b` from `a`, storing result in `dest`.  Returns the borrow bit.
///
/// All operands are treated as unsigned.  If `a < b` the result wraps and
/// borrow = 1.
pub(super) fn bn_sub(dest: &mut BigNum, a: &BigNum, b: &BigNum) -> u32 {
    let mut borrow: u64 = 0;
    for i in 0..BN_WORDS {
        let ai = a.get(i).copied().unwrap_or(0) as u64;
        let bi = b.get(i).copied().unwrap_or(0) as u64;
        let diff = ai.wrapping_sub(bi).wrapping_sub(borrow);
        if let Some(d) = dest.get_mut(i) {
            *d = diff as u32;
        }
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
pub(super) fn bn_mul(dest: &mut [u32], a: &BigNum, b: &BigNum, len: usize) {
    // Zero the destination (2*len words).
    for w in dest.iter_mut().take(len * 2) {
        *w = 0;
    }
    for i in 0..len {
        let mut carry: u64 = 0;
        let ai = a.get(i).copied().unwrap_or(0) as u64;
        for j in 0..len {
            let bj = b.get(j).copied().unwrap_or(0) as u64;
            let dij = dest.get(i + j).copied().unwrap_or(0) as u64;
            let prod = ai * bj + dij + carry;
            if let Some(d) = dest.get_mut(i + j) {
                *d = prod as u32;
            }
            carry = prod >> 32;
        }
        if let Some(d) = dest.get_mut(i + len) {
            *d = carry as u32;
        }
    }
}

/// Computes `base^exp mod modulus` using square-and-multiply.
///
/// This is the core RSA operation.  For the Westwood key, `exp = 0x10001`
/// and `modulus` is a 320-bit number, so performance is not a concern
/// (only ~17 squarings and 2 multiplications needed for exponent 65537).
pub(super) fn bn_mod_exp(base: &BigNum, exp: &BigNum, modulus: &BigNum) -> BigNum {
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
        let exp_word = exp.get(word_idx).copied().unwrap_or(0);
        if (exp_word >> bit_idx) & 1 == 1 {
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
    if let (Some(dst), Some(src)) = (rem.get_mut(..copy_len), product.get(..copy_len)) {
        dst.copy_from_slice(src);
    }

    // Find effective length of remainder.
    let mut rem_len = prod_len;
    while rem_len > 0 && rem.get(rem_len - 1) == Some(&0) {
        rem_len -= 1;
    }

    // Repeated subtraction with alignment.
    // We align the modulus to the top of the remainder and subtract downward.
    let mod_bits = bn_bitlen(modulus);
    let rem_bits = {
        if rem_len == 0 {
            0
        } else {
            let top = rem.get(rem_len - 1).copied().unwrap_or(0);
            (rem_len - 1) * 32 + (32 - top.leading_zeros() as usize)
        }
    };

    if rem_bits <= mod_bits {
        // Already smaller than modulus — just copy.
        let mut result = bn_zero();
        let copy = mod_len.min(rem_len);
        if let (Some(dst), Some(src)) = (result.get_mut(..copy), rem.get(..copy)) {
            dst.copy_from_slice(src);
        }
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
        let rem_word = rem.get(word_idx).copied().unwrap_or(0);
        if let Some(r0) = result.get_mut(0) {
            *r0 |= (rem_word >> bit_idx) & 1;
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
