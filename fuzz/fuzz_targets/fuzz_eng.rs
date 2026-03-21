// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(eng) = cnc_formats::eng::EngFile::parse(data) {
        for s in &eng.strings {
            let _ = s.bytes.len();
        }
    }
});
