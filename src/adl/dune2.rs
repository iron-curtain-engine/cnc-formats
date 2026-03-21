// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

/// Detects the documented Dune II container shape.
pub(super) fn looks_like_dune2_container(data: &[u8]) -> bool {
    if data.len() < DUNE2_DATA_START_ABS {
        return false;
    }

    if read_u16_le(data, DUNE2_TRACK_POINTERS_START).ok() != Some(DUNE2_DATA_START_REL as u16) {
        return false;
    }

    match data.get(..DUNE2_SUBSONG_INDEX_COUNT) {
        Some(indexes) => indexes.iter().any(|&index| {
            index != DUNE2_UNUSED_SUBSONG && usize::from(index) < DUNE2_POINTER_COUNT
        }),
        None => false,
    }
}

/// Parses a documented Dune II container ADL file.
pub(super) fn parse_dune2_container(data: &[u8]) -> Result<AdlFile, Error> {
    let mut instruments = Vec::new();
    for instrument_index in 0..MAX_INSTRUMENTS.min(DUNE2_POINTER_COUNT) {
        let pointer_offset = DUNE2_INSTRUMENT_POINTERS_START + instrument_index.saturating_mul(2);
        let relative = AdlDataOffset::from_raw(read_u16_le(data, pointer_offset)?);
        if relative.to_raw() == 0 {
            break;
        }

        let start = dune2_absolute_offset(relative, data.len())?;
        let end = start.saturating_add(INSTRUMENT_SIZE);
        let patch_data = data.get(start..end).ok_or(Error::InvalidOffset {
            offset: end,
            bound: data.len(),
        })?;

        let mut registers = [0u8; INSTRUMENT_SIZE];
        registers.copy_from_slice(patch_data);
        instruments.push(AdlInstrument { registers });
    }

    let mut subsongs = Vec::new();
    let subsong_indexes = data
        .get(..DUNE2_SUBSONG_INDEX_COUNT)
        .ok_or(Error::UnexpectedEof {
            needed: DUNE2_SUBSONG_INDEX_COUNT,
            available: data.len(),
        })?;

    for &track_index in subsong_indexes {
        if track_index == DUNE2_UNUSED_SUBSONG {
            break;
        }
        if usize::from(track_index) >= DUNE2_POINTER_COUNT {
            return Err(Error::InvalidSize {
                value: usize::from(track_index),
                limit: DUNE2_POINTER_COUNT.saturating_sub(1),
                context: "ADL primary sub-song index",
            });
        }
        if subsongs.len() >= MAX_SUBSONGS {
            break;
        }

        let pointer_offset =
            DUNE2_TRACK_POINTERS_START + usize::from(track_index).saturating_mul(2);
        let track_index = AdlTrackIndex::from_raw(track_index);
        let relative = AdlDataOffset::from_raw(read_u16_le(data, pointer_offset)?);
        let _track_start = dune2_absolute_offset(relative, data.len())?;

        subsongs.push(AdlSubSong {
            speed: None,
            data: AdlSubSongData::IndexedTrackProgram {
                program: AdlTrackProgramRef {
                    index: track_index,
                    offset: relative,
                },
            },
        });
    }

    Ok(AdlFile {
        instruments,
        subsongs,
    })
}

/// Converts a Dune II relative pointer into an absolute file offset.
fn dune2_absolute_offset(relative: AdlDataOffset, data_len: usize) -> Result<usize, Error> {
    let relative = usize::from(relative.to_raw());
    if relative < DUNE2_DATA_START_REL {
        return Err(Error::InvalidOffset {
            offset: DUNE2_SUBSONG_INDEX_COUNT.saturating_add(relative),
            bound: data_len,
        });
    }

    let absolute = DUNE2_SUBSONG_INDEX_COUNT.saturating_add(relative);
    if absolute > data_len {
        return Err(Error::InvalidOffset {
            offset: absolute,
            bound: data_len,
        });
    }

    Ok(absolute)
}
