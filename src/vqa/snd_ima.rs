// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

/// Standard IMA ADPCM step table (89 entries, indices 0–88).
const IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// IMA ADPCM step-index adjustment table (16 entries).
const IMA_INDEX_ADJ: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Decodes one IMA ADPCM nibble, updating state in place.
#[inline]
pub(super) fn ima_decode_nibble(nibble: u8, sample: &mut i32, index: &mut usize) -> i16 {
    let step = IMA_STEP_TABLE.get(*index).copied().unwrap_or(7);
    let code = (nibble & 0x07) as i32;

    let mut delta = step >> 3;
    if code & 4 != 0 {
        delta = delta.saturating_add(step);
    }
    if code & 2 != 0 {
        delta = delta.saturating_add(step >> 1);
    }
    if code & 1 != 0 {
        delta = delta.saturating_add(step >> 2);
    }
    if nibble & 0x08 != 0 {
        delta = -delta;
    }

    *sample = (*sample).saturating_add(delta).clamp(-32768, 32767);
    let adj = IMA_INDEX_ADJ
        .get((nibble & 0x0F) as usize)
        .copied()
        .unwrap_or(-1);
    *index = ((*index as i32).saturating_add(adj)).clamp(0, 88) as usize;

    *sample as i16
}
