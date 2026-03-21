// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(csf) = cnc_formats::csf::CsfFile::parse(data) {
        // Walk all labels to touch parsed strings.
        for (_key, strings) in &csf.labels {
            for s in strings {
                let _ = s.value.len();
            }
        }
    }
});
