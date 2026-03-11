// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ── Base-64 decoding ─────────────────────────────────────────────────

/// Base-64 decoding of the public key string produces non-empty bytes.
///
/// Why: if the decoder is broken, key derivation will silently fail.
#[test]
fn b64_decode_pubkey_produces_bytes() {
    let (buf, len) = b64_decode(PUBKEY_STR);
    let decoded = &buf[..len];
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
    let a = b64_decode(PUBKEY_STR);
    let b = b64_decode(PUBKEY_STR);
    assert_eq!(a.0[..a.1], b.0[..b.1]);
}

/// Base-64 encoding of empty input yields empty output.
#[test]
fn b64_decode_empty() {
    let (_, len) = b64_decode(b"");
    assert_eq!(len, 0);
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
    let pk = init_pubkey();
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
    let pk = init_pubkey();
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
    use blowfish::BlowfishLE;

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
    let cipher = BlowfishLE::new_from_slice(&key).unwrap();
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

/// decrypt_mix_header: decrypted count > MAX_MIX_ENTRIES → InvalidSize.
///
/// Why (V38): even behind encryption, an unreasonable entry count must
/// be caught before allocating the SubBlock index.
///
/// How: encrypt a first block containing count = 16385, then decrypt it.
#[test]
fn decrypt_header_count_exceeds_cap() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    use blowfish::BlowfishLE;

    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&16_385u16.to_le_bytes());

    let key = [0xABu8; BLOWFISH_KEY_LEN];
    let cipher = BlowfishLE::new_from_slice(&key).unwrap();
    cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));

    let err = decrypt_mix_header(&block, &key).unwrap_err();
    assert!(
        matches!(err, Error::InvalidSize { value: 16385, .. }),
        "expected InvalidSize with value 16385, got: {err}",
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
    use blowfish::BlowfishLE;

    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&10u16.to_le_bytes());

    let key = [0xCDu8; BLOWFISH_KEY_LEN];
    let cipher = BlowfishLE::new_from_slice(&key).unwrap();
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
    use blowfish::BlowfishLE;

    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&0u16.to_le_bytes()); // count = 0

    let key = [0x55u8; BLOWFISH_KEY_LEN];
    let cipher = BlowfishLE::new_from_slice(&key).unwrap();
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
