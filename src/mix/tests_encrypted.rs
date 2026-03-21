// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

#[test]
fn parse_extended_encrypted_with_sha1_returns_error() {
    let data = [0x00u8, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let result = MixArchive::parse(&data);
    let err = result.unwrap_err();
    #[cfg(feature = "encrypted-mix")]
    assert!(
        matches!(err, Error::UnexpectedEof { .. }),
        "expected UnexpectedEof, got: {err}",
    );
    #[cfg(not(feature = "encrypted-mix"))]
    assert_eq!(err, Error::EncryptedArchive);
}

#[cfg(feature = "encrypted-mix")]
#[test]
fn parse_encrypted_mix_end_to_end() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    let key_source = [0u8; 80];
    let bf_key = crate::mix_crypt::derive_blowfish_key(&key_source).unwrap();

    let file_data = b"HELLO";
    let file_crc = crc("TEST.DAT");
    let mut plaintext = Vec::new();
    plaintext.extend_from_slice(&1u16.to_le_bytes());
    plaintext.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    plaintext.extend_from_slice(&file_crc.to_raw().to_le_bytes());
    plaintext.extend_from_slice(&0u32.to_le_bytes());
    plaintext.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    while plaintext.len() % 8 != 0 {
        plaintext.push(0);
    }

    let cipher = BlowfishBE::new_from_slice(&bf_key).unwrap();
    let mut encrypted_header = plaintext.clone();
    for chunk in encrypted_header.chunks_exact_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }

    let mut archive_bytes = Vec::new();
    archive_bytes.extend_from_slice(&0u16.to_le_bytes());
    archive_bytes.extend_from_slice(&0x0002u16.to_le_bytes());
    archive_bytes.extend_from_slice(&key_source);
    archive_bytes.extend_from_slice(&encrypted_header);
    archive_bytes.extend_from_slice(file_data);

    let archive = MixArchive::parse(&archive_bytes).unwrap();
    assert_eq!(archive.file_count(), 1);
    let extracted = archive.get("TEST.DAT").expect("file should exist");
    assert_eq!(extracted, file_data);
}
