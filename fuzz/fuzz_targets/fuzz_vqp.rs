// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(vqp) = cnc_formats::vqp::VqpFile::parse(data) {
        for table in &vqp.tables {
            let _ = table.packed.len();
        }
    }
});
