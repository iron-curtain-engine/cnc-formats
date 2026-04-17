// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

#[test]
fn bn_mod_exp_known_vector_westwood_key() {
    let mod_be = hex_to_bytes(
        "51bcda086d39fce4565160d651713fa2e8aa54fa6682b04aabdd0e6af8b0c1e6d1fb4f3daa437f15",
    );
    let modulus = bn_from_be_bytes(&mod_be);
    let exp = bn_from_u32(0x10001);
    let base = bn_from_u32(42);

    let result = bn_mod_exp(&base, &exp, &modulus);

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

#[test]
fn derive_key_known_vector() {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(b"cnc-formats-test-keysource");
    let mut ks = [0u8; 80];
    ks[..32].copy_from_slice(&hash);
    ks[32..64].copy_from_slice(&hash);
    ks[64..80].copy_from_slice(&hash[..16]);

    let key = derive_blowfish_key(&ks).unwrap();

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
    let encrypted_start: [u8; 24] = [
        0x3B, 0xA7, 0xD6, 0xA0, 0x94, 0x9D, 0x5E, 0xE5, 0x1C, 0x6C, 0x4C, 0x72, 0x8C, 0x4D, 0x34,
        0x2D, 0x34, 0x71, 0x41, 0x16, 0x16, 0x0F, 0x3C, 0x2B,
    ];

    let bf_key = derive_blowfish_key(&key_source).unwrap();

    use blowfish::cipher::{Block, BlockCipherDecrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    let cipher = BlowfishBE::new_from_slice(&bf_key).unwrap();
    let mut block = [0u8; 8];
    block.copy_from_slice(&encrypted_start[..8]);
    let mut blk = Block::<BlowfishBE>::try_from(block.as_slice()).unwrap();
    cipher.decrypt_blocks(std::slice::from_mut(&mut blk));
    block.copy_from_slice(&blk);

    let count = u16::from_le_bytes([block[0], block[1]]);
    let data_size = u32::from_le_bytes([block[2], block[3], block[4], block[5]]);

    let file_size: usize = 25_046_328;
    let header_overhead = 4 + 80 + (6 + count as usize * 12).div_ceil(8) * 8;
    let expected_size = header_overhead + data_size as usize;

    assert!(count > 0 && count < 10000);
    assert!(expected_size <= file_size + 20 && expected_size >= file_size.saturating_sub(20));
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}
