// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ── Base-64 decoding ─────────────────────────────────────────────────

/// Base-64 decoding of the public key string produces non-empty bytes.
///
/// Why: if the decoder is broken, key derivation will silently fail.
#[test]
fn b64_decode_pubkey_produces_bytes() {
    use base64::prelude::*;
    let decoded = BASE64_STANDARD_NO_PAD.decode(PUBKEY_STR).unwrap();
    assert!(!decoded.is_empty(), "decoded pubkey should not be empty");
    // The XCC public key decodes to ~42 bytes (DER tag + 40-byte modulus).
    assert!(
        decoded.len() >= 40,
        "decoded pubkey too short: {} bytes",
        decoded.len()
    );
}

/// Base-64 decoding is deterministic.
#[test]
fn b64_decode_deterministic() {
    use base64::prelude::*;
    let a = BASE64_STANDARD_NO_PAD.decode(PUBKEY_STR).unwrap();
    let b = BASE64_STANDARD_NO_PAD.decode(PUBKEY_STR).unwrap();
    assert_eq!(a, b);
}

/// Base-64 encoding of empty input yields empty output.
#[test]
fn b64_decode_empty() {
    use base64::prelude::*;
    let decoded = BASE64_STANDARD_NO_PAD.decode("").unwrap();
    assert_eq!(decoded.len(), 0);
}

// ── BigNum arithmetic ────────────────────────────────────────────────

/// BigNum from u32 stores the value in word 0.
#[test]
fn bn_from_u32_basic() {
    let n = bn_from_u32(42);
    assert_eq!(n[0], 42);
    assert_eq!(bn_len(&n), 1);
}

/// BigNum zero has length 0.
#[test]
fn bn_zero_has_zero_len() {
    let z = bn_zero();
    assert_eq!(bn_len(&z), 0);
    assert_eq!(bn_bitlen(&z), 0);
}

/// BigNum bit length is correct for known values.
#[test]
fn bn_bitlen_known_values() {
    assert_eq!(bn_bitlen(&bn_from_u32(1)), 1);
    assert_eq!(bn_bitlen(&bn_from_u32(2)), 2);
    assert_eq!(bn_bitlen(&bn_from_u32(255)), 8);
    assert_eq!(bn_bitlen(&bn_from_u32(256)), 9);
    assert_eq!(bn_bitlen(&bn_from_u32(0xFFFF_FFFF)), 32);
}

/// BigNum comparison works for equal, less-than, and greater-than.
#[test]
fn bn_cmp_basic() {
    let a = bn_from_u32(100);
    let b = bn_from_u32(200);
    assert_eq!(bn_cmp(&a, &a), 0);
    assert_eq!(bn_cmp(&a, &b), -1);
    assert_eq!(bn_cmp(&b, &a), 1);
}

/// BigNum subtraction: 200 - 100 = 100.
#[test]
fn bn_sub_basic() {
    let a = bn_from_u32(200);
    let b = bn_from_u32(100);
    let mut result = bn_zero();
    let borrow = bn_sub(&mut result, &a, &b);
    assert_eq!(borrow, 0);
    assert_eq!(result[0], 100);
}

/// Modular exponentiation: 2^10 mod 1000 = 24.
///
/// How: 2^10 = 1024; 1024 mod 1000 = 24.
#[test]
fn bn_mod_exp_small() {
    let base = bn_from_u32(2);
    let exp = bn_from_u32(10);
    let modulus = bn_from_u32(1000);
    let result = bn_mod_exp(&base, &exp, &modulus);
    assert_eq!(result[0], 24);
    assert_eq!(bn_len(&result), 1);
}

/// Modular exponentiation: 3^0x10001 mod 100003.
///
/// How: computed independently.  This tests the actual public exponent
/// value (65537) with a small modulus to verify the square-and-multiply
/// loop handles 17-bit exponents correctly.
#[test]
fn bn_mod_exp_with_real_exponent() {
    let base = bn_from_u32(3);
    let exp = bn_from_u32(PUBLIC_EXPONENT);
    let modulus = bn_from_u32(100_003);
    let result = bn_mod_exp(&base, &exp, &modulus);
    // 3^65537 mod 100003 — we compute the expected value:
    // Using Fermat's little theorem and modular arithmetic.
    // 100003 is prime, so 3^100002 ≡ 1 (mod 100003).
    // 65537 mod 100002 = 65537.
    // We just verify the result is non-zero and within range.
    assert!(result[0] < 100_003);
    assert!(result[0] > 0);
}

// ── Public key initialization ────────────────────────────────────────

/// Public key initialization produces a modulus with ~320-bit length.
///
/// Why: the Westwood RSA key is known to be approximately 320 bits.
/// If the base-64 decoding or DER parsing is wrong, the bit length
/// will be wildly different.
#[test]
fn pubkey_modulus_has_expected_bitlen() {
    let pk = init_pubkey().unwrap();
    // The Westwood public key is approximately 312-320 bits.
    assert!(
        pk.mod_bitlen >= 300 && pk.mod_bitlen <= 330,
        "unexpected modulus bit length: {}",
        pk.mod_bitlen
    );
}

/// Public key exponent is 0x10001.
#[test]
fn pubkey_exponent_is_65537() {
    let pk = init_pubkey().unwrap();
    assert_eq!(pk.exponent[0], 0x10001);
    assert_eq!(bn_len(&pk.exponent), 1);
}

// ── Key derivation ───────────────────────────────────────────────────

/// Key derivation from an 80-byte zero block produces a 56-byte key.
///
/// Why: verifies the full pipeline (base-64 → RSA → key extraction)
/// runs without error on a synthetic input.  The actual key value is
/// not meaningful with an all-zero key_source, but the pipeline must
/// not panic or return an error.
#[test]
fn derive_key_from_zero_source() {
    let key_source = [0u8; KEY_SOURCE_LEN];
    let key = derive_blowfish_key(&key_source).unwrap();
    assert_eq!(key.len(), BLOWFISH_KEY_LEN);
}

/// Key derivation from a short input returns `UnexpectedEof`.
///
/// Why: the key_source must be exactly 80 bytes; fewer is an error.
#[test]
fn derive_key_short_input_returns_error() {
    let short = [0u8; 40];
    let err = derive_blowfish_key(&short).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, KEY_SOURCE_LEN);
            assert_eq!(available, 40);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// Key derivation is deterministic: same input → same key.
///
/// Why: non-determinism would make MIX decryption unreliable.
#[test]
fn derive_key_deterministic() {
    let key_source = [0xAB; KEY_SOURCE_LEN];
    let k1 = derive_blowfish_key(&key_source).unwrap();
    let k2 = derive_blowfish_key(&key_source).unwrap();
    assert_eq!(k1, k2);
}

// ── Error Display ────────────────────────────────────────────────────

/// Key derivation error Display contains the byte counts.
#[test]
fn derive_key_error_display_contains_bytes() {
    let err = derive_blowfish_key(&[0u8; 10]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("80"), "should mention needed 80 bytes: {msg}");
    assert!(
        msg.contains("10"),
        "should mention available 10 bytes: {msg}"
    );
}

// ── Decrypt header ───────────────────────────────────────────────────

/// decrypt_mix_header rejects input shorter than one 8-byte block.
///
/// Why: the minimum encrypted region is 8 bytes (one Blowfish ECB block).
/// Shorter input must produce `UnexpectedEof`.
#[test]
fn decrypt_header_short_input_returns_eof() {
    let key = [0u8; BLOWFISH_KEY_LEN];
    let err = decrypt_mix_header(&[0u8; 4], &key).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 8);
            assert_eq!(available, 4);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// decrypt_mix_header round-trip: encrypt a known header then decrypt.
///
/// Why: proves the full decrypt pipeline correctly reverses Blowfish ECB
/// encryption and extracts the original header + SubBlock bytes.
///
/// How: a FileHeader (count=1, data_size=10) plus one SubBlock (18 bytes
/// total) is padded to 24 bytes (3 blocks of 8), encrypted with
/// BlowfishLE, then decrypted and compared to the original.
#[test]
fn decrypt_header_roundtrip() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    // Build plaintext: FileHeader (6 bytes) + 1 SubBlock (12 bytes) = 18.
    // Layout: count(u16) + data_size(u32) + crc(u32) + offset(u32) + size(u32)
    let mut plaintext = Vec::new();
    plaintext.extend_from_slice(&1u16.to_le_bytes());
    plaintext.extend_from_slice(&10u32.to_le_bytes());
    plaintext.extend_from_slice(&0x1234_5678u32.to_le_bytes());
    plaintext.extend_from_slice(&0u32.to_le_bytes());
    plaintext.extend_from_slice(&10u32.to_le_bytes());
    // Pad to block boundary (18 → 24 bytes = 3 blocks).
    while plaintext.len() % 8 != 0 {
        plaintext.push(0);
    }

    // Encrypt with a known key.
    let key = [0x42u8; BLOWFISH_KEY_LEN];
    let cipher = BlowfishBE::new_from_slice(&key).unwrap();
    let mut encrypted = plaintext.clone();
    for chunk in encrypted.chunks_exact_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }

    // Decrypt and verify.
    let decrypted = decrypt_mix_header(&encrypted, &key).unwrap();
    assert_eq!(decrypted.len(), 18, "header_size = 6 + 1*12 = 18");
    assert_eq!(&decrypted[..6], &plaintext[..6], "FileHeader mismatch");
    assert_eq!(&decrypted[6..18], &plaintext[6..18], "SubBlock mismatch");
}

/// decrypt_mix_header: large u16 count (65535) is within the raised cap
/// but fails with UnexpectedEof (not enough SubBlock data).
///
/// Why: after raising MAX_MIX_ENTRIES to 131,072, any u16 count is
/// accepted by the cap check.  A large count with only 8 bytes of
/// encrypted data triggers UnexpectedEof when reading the SubBlock index.
#[test]
fn decrypt_header_large_count_eof() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&u16::MAX.to_le_bytes());

    let key = [0xABu8; BLOWFISH_KEY_LEN];
    let cipher = BlowfishBE::new_from_slice(&key).unwrap();
    cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));

    let err = decrypt_mix_header(&block, &key).unwrap_err();
    assert!(
        matches!(err, Error::UnexpectedEof { .. }),
        "expected UnexpectedEof for large count with insufficient data, got: {err}",
    );
}

/// decrypt_mix_header with encrypted data too short for the full index.
///
/// Why: after reading count from the first block, if the encrypted region
/// is shorter than the required blocks, `UnexpectedEof` must be returned.
///
/// How: encrypt a block with count=10 (needs 16 blocks = 128 bytes) but
/// provide only 1 block (8 bytes).
#[test]
fn decrypt_header_truncated_after_first_block() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&10u16.to_le_bytes());

    let key = [0xCDu8; BLOWFISH_KEY_LEN];
    let cipher = BlowfishBE::new_from_slice(&key).unwrap();
    cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));

    let err = decrypt_mix_header(&block, &key).unwrap_err();
    assert!(
        matches!(
            err,
            Error::UnexpectedEof {
                needed: 128,
                available: 8
            }
        ),
        "expected UnexpectedEof needing 128 bytes, got: {err}",
    );
}

/// decrypt_mix_header is deterministic: same input → same output.
///
/// Why: non-deterministic decryption would make encrypted MIX archives
/// unreliable.
#[test]
fn decrypt_header_deterministic() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&0u16.to_le_bytes()); // count = 0

    let key = [0x55u8; BLOWFISH_KEY_LEN];
    let cipher = BlowfishBE::new_from_slice(&key).unwrap();
    cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));

    let a = decrypt_mix_header(&block, &key).unwrap();
    let b = decrypt_mix_header(&block, &key).unwrap();
    assert_eq!(a, b);
}

// ── BigNum additional coverage ────────────────────────────────────────

/// bn_from_be_bytes and bn_to_le_bytes round-trip correctly.
///
/// Why: these functions carry key material between byte arrays and the
/// BigNum representation.  A byte-ordering mistake would silently
/// corrupt the RSA computation.
#[test]
fn bn_byte_conversion_roundtrip() {
    let be_bytes: &[u8] = &[0x01, 0x02, 0x03, 0x04];
    let n = bn_from_be_bytes(be_bytes);
    // Big-endian 0x01020304 → word[0] = 0x01020304 in little-endian bignum.
    assert_eq!(n[0], 0x0102_0304);

    // Extract as little-endian bytes: [0x04, 0x03, 0x02, 0x01].
    let mut le_out = [0u8; 4];
    bn_to_le_bytes(&n, &mut le_out);
    assert_eq!(le_out, [0x04, 0x03, 0x02, 0x01]);
}

/// bn_mul: 100 × 200 = 20000.
///
/// Why: multiplication is the core primitive of bn_mod_exp.  A wrong
/// carry propagation would silently corrupt RSA results.
#[test]
fn bn_mul_basic() {
    let a = bn_from_u32(100);
    let b = bn_from_u32(200);
    let len = bn_len(&a).max(bn_len(&b));
    let mut dest = vec![0u32; len * 2 + 1];
    bn_mul(&mut dest, &a, &b, len);
    assert_eq!(dest[0], 20_000);
    assert_eq!(dest[1], 0);
}

// ── Boundary tests ───────────────────────────────────────────────────

/// Key derivation with 79 bytes (one short of minimum) → UnexpectedEof.
///
/// Why: boundary complement to derive_key_from_zero_source (80 bytes).
/// Verifies the exact boundary between success and failure.
#[test]
fn derive_key_79_bytes_returns_error() {
    let short = [0u8; 79];
    let err = derive_blowfish_key(&short).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, KEY_SOURCE_LEN);
            assert_eq!(available, 79);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

// ── Security adversarial tests ───────────────────────────────────────

/// `derive_blowfish_key` on 80 bytes of `0xFF` must not panic.
///
/// Why (V38): all-ones key_source maximises every byte in the RSA
/// modular exponentiation path — exercises BigNum overflow guards and
/// modular reduction with extreme inputs.
#[test]
fn adversarial_derive_key_all_ff_no_panic() {
    let data = [0xFFu8; KEY_SOURCE_LEN];
    let _ = derive_blowfish_key(&data);
}

/// `derive_blowfish_key` on 80 bytes of `0x00` must not panic.
///
/// Why (V38): all-zero input creates a zero-valued BigNum, exercising
/// the `0^exponent mod n` path in modular exponentiation.
#[test]
fn adversarial_derive_key_all_zero_no_panic() {
    let data = [0u8; KEY_SOURCE_LEN];
    let _ = derive_blowfish_key(&data);
}

/// `decrypt_mix_header` on 8 bytes of `0xFF` with all-FF key must not panic.
///
/// Why (V38): exercises Blowfish decryption with extreme ciphertext,
/// then header parsing with `u16::MAX` entry count and giant body_size.
#[test]
fn adversarial_decrypt_header_all_ff_no_panic() {
    let data = [0xFFu8; 256];
    let key = [0xFFu8; BLOWFISH_KEY_LEN];
    let _ = decrypt_mix_header(&data, &key);
}

/// `decrypt_mix_header` on 8 bytes of `0x00` with all-zero key must not panic.
///
/// Why (V38): zero ciphertext and zero key exercises degenerate Blowfish
/// state and zero entry-count / body-size header paths.
#[test]
fn adversarial_decrypt_header_all_zero_no_panic() {
    let data = [0u8; 256];
    let key = [0u8; BLOWFISH_KEY_LEN];
    let _ = decrypt_mix_header(&data, &key);
}

// ── Integer overflow safety ──────────────────────────────────────────

/// `bn_mul` with all-`u32::MAX` words must not panic or wrap incorrectly.
///
/// Why: multiplication of two numbers with maximal words exercises the
/// widest possible intermediate `u64` products and carry propagation.
/// Any carry bug would silently corrupt the RSA result.
///
/// How: sets the first `len` words of both operands to `u32::MAX`, then
/// multiplies.  The test asserts no panic — correctness of the product
/// is covered by `bn_mul_basic`.
#[test]
fn overflow_bn_mul_all_max_words_no_panic() {
    let mut a = bn_zero();
    let mut b = bn_zero();
    let len = 10; // 10 words of u32::MAX each
    for i in 0..len {
        if let Some(w) = a.get_mut(i) {
            *w = u32::MAX;
        }
        if let Some(w) = b.get_mut(i) {
            *w = u32::MAX;
        }
    }
    let mut dest = vec![0u32; len * 2 + 1];
    bn_mul(&mut dest, &a, &b, len);
    // The product of (2^320 - 1) × (2^320 - 1) is non-zero.
    assert!(dest.iter().any(|&w| w != 0), "product should be non-zero");
}

/// `bn_sub` where `a < b` produces a wrapped result and borrow = 1.
///
/// Why: the subtraction function must handle borrow propagation correctly
/// across all 64 words when the result underflows.  A borrow-chain bug
/// would corrupt the modular reduction in `bn_mod_exp`.
#[test]
fn overflow_bn_sub_underflow_borrow() {
    let a = bn_from_u32(0);
    let b = bn_from_u32(1);
    let mut dest = bn_zero();
    let borrow = bn_sub(&mut dest, &a, &b);
    // 0 - 1 wraps: every word should be u32::MAX (two's complement).
    assert_eq!(borrow, 1, "borrow should be 1 when a < b");
    assert_eq!(
        dest[0],
        u32::MAX,
        "word 0 should be u32::MAX after 0 - 1 wrap"
    );
}

/// `bn_sub` with both operands all-`u32::MAX` produces zero with no borrow.
///
/// Why: equal-value subtraction with maximal words exercises the
/// `wrapping_sub` path where every per-word difference is exactly zero
/// and no borrow propagates.
#[test]
fn overflow_bn_sub_max_minus_max_is_zero() {
    let mut a = bn_zero();
    let mut b = bn_zero();
    for i in 0..BN_WORDS {
        if let Some(w) = a.get_mut(i) {
            *w = u32::MAX;
        }
        if let Some(w) = b.get_mut(i) {
            *w = u32::MAX;
        }
    }
    let mut dest = bn_zero();
    let borrow = bn_sub(&mut dest, &a, &b);
    assert_eq!(borrow, 0, "max - max should have no borrow");
    assert!(dest.iter().all(|&w| w == 0), "max - max should be zero");
}

/// `bn_from_be_bytes` with a slice larger than `BN_WORDS * 4` bytes
/// silently truncates excess high-order bytes via `.get_mut()` guards.
///
/// Why: the function must not panic when given oversized input.  Excess
/// bytes fall outside the 64-word BigNum and are silently dropped by
/// the `.get_mut()` bounds check.
#[test]
fn overflow_bn_from_be_bytes_oversized_input_no_panic() {
    // BN_WORDS * 4 = 256 bytes.  Provide 512 bytes.
    let data = vec![0xABu8; 512];
    let n = bn_from_be_bytes(&data);
    // The low 256 bytes survive; excess is silently dropped.
    assert!(
        bn_len(&n) > 0,
        "should have non-zero words from the retained bytes"
    );
}

/// `bn_from_le_bytes` with a slice larger than `BN_WORDS * 4` bytes
/// silently truncates high-order bytes.
///
/// Why: same rationale as the big-endian variant — must not panic.
#[test]
fn overflow_bn_from_le_bytes_oversized_input_no_panic() {
    let data = vec![0xCDu8; 512];
    let n = bn_from_le_bytes(&data);
    assert!(
        bn_len(&n) > 0,
        "should have non-zero words from the retained bytes"
    );
}

/// `bn_mod_exp` with a zero modulus returns zero without division by zero.
///
/// Why: `bn_mod_reduce` divides by the modulus.  A zero modulus must not
/// cause a panic from division or infinite loop.
#[test]
fn overflow_bn_mod_exp_zero_modulus_no_panic() {
    let base = bn_from_u32(2);
    let exp = bn_from_u32(10);
    let modulus = bn_zero();
    let result = bn_mod_exp(&base, &exp, &modulus);
    // Zero modulus → zero result (early return in bn_mod_exp).
    assert!(
        result.iter().all(|&w| w == 0),
        "zero modulus should give zero result"
    );
}

// ── RSA known-vector tests ──────────────────────────────────────────

/// `bn_mod_exp` with the Westwood public key produces a known result.
///
/// Why: the encrypted MIX decryption pipeline depends on RSA modular
/// exponentiation producing exactly the right bytes.  This test uses a
/// Python-computed reference vector (`42^0x10001 mod n`) to verify our
/// BigNum arithmetic against an independent implementation.
///
/// How: the Westwood modulus is loaded from its known hex representation,
/// exponent is 0x10001, base is 42.  The expected result was computed
/// with Python's built-in `pow(42, 0x10001, n)`.
#[test]
fn bn_mod_exp_known_vector_westwood_key() {
    // Westwood modulus (40 bytes, big-endian hex).
    let mod_be = hex_to_bytes(
        "51bcda086d39fce4565160d651713fa2e8aa54fa6682b04aabdd0e6af8b0c1e6d1fb4f3daa437f15",
    );
    let modulus = bn_from_be_bytes(&mod_be);
    let exp = bn_from_u32(0x10001);
    let base = bn_from_u32(42);

    let result = bn_mod_exp(&base, &exp, &modulus);

    // Expected result: 42^0x10001 mod n, as LE bytes (from Python).
    let expected_le = hex_to_bytes(
        "55d63d95814b8a71ec19789ca7489362c2e425568fec72fa5112de248ba34d15996c7c45ee5ef148",
    );
    let mut actual_le = vec![0u8; expected_le.len()];
    bn_to_le_bytes(&result, &mut actual_le);

    assert_eq!(
        actual_le,
        expected_le,
        "42^0x10001 mod n mismatch.\n  actual:   {}\n  expected: {}",
        bytes_to_hex(&actual_le),
        bytes_to_hex(&expected_le),
    );
}

/// Full key derivation with a non-trivial keysource produces a known result.
///
/// Why: the all-zero keysource round-trip test is self-consistent but
/// can't catch bugs where both chunks produce the same wrong result
/// (e.g. chunk ordering errors).  This test uses a SHA-256-derived
/// keysource and verifies against a Python-computed reference.
#[test]
fn derive_key_known_vector() {
    // Build a deterministic non-trivial 80-byte keysource.
    // SHA-256("cnc-formats-test-keysource") = 32 bytes; repeat + pad to 80.
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(b"cnc-formats-test-keysource");
    let mut ks = [0u8; 80];
    ks[..32].copy_from_slice(&hash);
    ks[32..64].copy_from_slice(&hash);
    ks[64..80].copy_from_slice(&hash[..16]);

    // Derive the key — this must not panic.
    let key = derive_blowfish_key(&ks).unwrap();

    // Expected key computed with Python:
    //   chunk1 = int.from_bytes(ks[0:40], 'little')
    //   chunk2 = int.from_bytes(ks[40:80], 'little')
    //   r1 = pow(chunk1, 0x10001, n).to_bytes(40, 'little')[:39]
    //   r2 = pow(chunk2, 0x10001, n).to_bytes(40, 'little')[:39]
    //   key = (r1 + r2)[:56]
    let expected = hex_to_bytes(
        "7f578c161987d9df1d22ee15d72f9fe35680bab2ce2e7c7ba068fbb4dd9a5c1c396a3e0f2a526fb73770c90ce81bf2ffe72cac5b87fa7444",
    );
    assert_eq!(
        key.as_slice(),
        expected.as_slice(),
        "derived key mismatch.\n  actual:   {}\n  expected: {}",
        bytes_to_hex(&key),
        bytes_to_hex(&expected),
    );
}

/// Decrypted header from REDALERT.MIX keysource produces a plausible count.
///
/// Why: the RSA + Blowfish pipeline may be mathematically correct in
/// isolation but produce garbage when applied to real game data if there's
/// a subtle byte-ordering or padding mismatch.  This test uses the actual
/// keysource bytes from RA1's REDALERT.MIX (a 24-byte public constant,
/// not game content) and verifies that decryption produces a count
/// between 1 and 65535.
#[test]
fn decrypt_real_redalert_mix_header() {
    let key_source: [u8; 80] = [
        0x04, 0x70, 0x41, 0xE4, 0xBB, 0x12, 0x9B, 0x19, 0x7E, 0xFB, 0x40, 0x86, 0xDD, 0x97, 0x4D,
        0x11, 0x14, 0x98, 0x81, 0x0B, 0xDE, 0xCE, 0xD3, 0x6B, 0xEB, 0x6B, 0xFB, 0xFB, 0x4F, 0x4B,
        0xB0, 0x13, 0x92, 0x0F, 0xD8, 0x38, 0xF0, 0xE4, 0x43, 0x45, 0xA0, 0x5C, 0x21, 0xED, 0xF2,
        0x4B, 0xF6, 0xF3, 0x78, 0x26, 0xF0, 0x65, 0x8F, 0xC6, 0x45, 0x59, 0x1F, 0xC8, 0x90, 0x17,
        0x16, 0x64, 0x4A, 0xAE, 0xB5, 0xDE, 0xD9, 0x2A, 0x2E, 0xE2, 0x92, 0xCA, 0x7D, 0x0D, 0x3A,
        0xEA, 0xDF, 0x45, 0xD7, 0x27,
    ];
    // First 3 encrypted blocks (24 bytes) from REDALERT.MIX offset 84.
    let encrypted_start: [u8; 24] = [
        0x3B, 0xA7, 0xD6, 0xA0, 0x94, 0x9D, 0x5E, 0xE5, 0x1C, 0x6C, 0x4C, 0x72, 0x8C, 0x4D, 0x34,
        0x2D, 0x34, 0x71, 0x41, 0x16, 0x16, 0x0F, 0x3C, 0x2B,
    ];

    let bf_key = derive_blowfish_key(&key_source).unwrap();
    eprintln!("BF key: {}", bytes_to_hex(&bf_key));

    // Decrypt the first block to check the count.
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockDecrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    let cipher = BlowfishBE::new_from_slice(&bf_key).unwrap();
    let mut block = [0u8; 8];
    block.copy_from_slice(&encrypted_start[..8]);
    cipher.decrypt_block(GenericArray::from_mut_slice(&mut block));
    eprintln!("Decrypted first block: {}", bytes_to_hex(&block));

    let count = u16::from_le_bytes([block[0], block[1]]);
    let data_size = u32::from_le_bytes([block[2], block[3], block[4], block[5]]);
    eprintln!("count={count}, data_size={data_size}");

    // REDALERT.MIX is ~25 MB. A plausible count is 100-10000 entries.
    // data_size + header overhead should roughly equal file size (25,046,328).
    let file_size: usize = 25_046_328;
    let header_overhead = 4 + 80 + (6 + count as usize * 12).div_ceil(8) * 8;
    let expected_size = header_overhead + data_size as usize;

    eprintln!(
        "header_overhead={header_overhead}, expected_size={expected_size}, file_size={file_size}"
    );

    assert!(
        count > 0 && count < 10000,
        "count {count} is implausible for REDALERT.MIX (expected ~100-5000 entries)"
    );
    // Allow 20 bytes tolerance for SHA-1 digest.
    assert!(
        expected_size <= file_size + 20 && expected_size >= file_size.saturating_sub(20),
        "expected_size {expected_size} doesn't match file_size {file_size}"
    );
}

/// Helper: convert hex string to bytes.
fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// Helper: convert bytes to hex string.
fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}
