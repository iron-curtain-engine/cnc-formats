// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(aud) = cnc_formats::aud::AudFile::parse(data) {
        if aud.header.compression == cnc_formats::aud::SCOMP_WESTWOOD {
            let _ = cnc_formats::aud::decode_adpcm(aud.compressed_data, aud.header.is_stereo(), 0);
        }
    }
});
