// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Cap output size to prevent OOM during fuzzing.
    let _ = cnc_formats::lcw::decompress(data, 1024 * 1024);
});
