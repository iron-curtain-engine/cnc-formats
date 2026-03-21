// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::{crc, known_names, MixCrc};
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::sync::OnceLock;

const RA2_FILENAME_CANDIDATES: &str = include_str!("known_names_ra2.txt");

#[derive(Debug)]
struct BuiltinNameDb {
    names: HashMap<MixCrc, &'static str>,
    ambiguous_crc_count: usize,
}

static BUILTIN_NAME_DB: OnceLock<BuiltinNameDb> = OnceLock::new();

/// Summary of the built-in MIX filename resolver corpus.
///
/// Built-in names come from candidate filename corpora, not authoritative
/// archive metadata.  Any CRC that maps to multiple candidates is treated as
/// ambiguous and omitted from [`builtin_name_map()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinNameStats {
    /// Number of CRCs that resolve to a single built-in filename candidate.
    pub resolved_crc_count: usize,
    /// Number of CRCs omitted because the candidate corpus collided.
    pub ambiguous_crc_count: usize,
}

/// Builds a CRC→filename lookup map from the built-in filename databases.
///
/// Built-in names are sourced from vendored TD/RA1 and RA2 filename corpora.
/// The corpora are candidate lists, not authoritative archive metadata, so the
/// resolver only keeps CRCs that map to exactly one candidate name.  Any
/// ambiguous CRC collision is omitted instead of depending on list order.
///
/// This is useful as a default name map when the user doesn't supply
/// `--names`.
pub fn builtin_name_map() -> HashMap<MixCrc, String> {
    builtin_name_db()
        .names
        .iter()
        .map(|(&crc, &name)| (crc, name.to_string()))
        .collect()
}

/// Returns counts for the built-in MIX filename resolver.
pub fn builtin_name_stats() -> BuiltinNameStats {
    let db = builtin_name_db();
    BuiltinNameStats {
        resolved_crc_count: db.names.len(),
        ambiguous_crc_count: db.ambiguous_crc_count,
    }
}

fn builtin_name_db() -> &'static BuiltinNameDb {
    BUILTIN_NAME_DB.get_or_init(build_builtin_name_db)
}

fn build_builtin_name_db() -> BuiltinNameDb {
    let mut names = HashMap::new();
    let mut ambiguous = HashSet::new();

    for line in known_names::TD_RA1_FILENAME_CANDIDATES.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        insert_builtin_candidate(&mut names, &mut ambiguous, trimmed);
    }

    for line in RA2_FILENAME_CANDIDATES.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        insert_builtin_candidate(&mut names, &mut ambiguous, trimmed);
    }

    BuiltinNameDb {
        names,
        ambiguous_crc_count: ambiguous.len(),
    }
}

fn insert_builtin_candidate(
    names: &mut HashMap<MixCrc, &'static str>,
    ambiguous: &mut HashSet<MixCrc>,
    name: &'static str,
) {
    let key = crc(name);
    if ambiguous.contains(&key) {
        return;
    }

    match names.entry(key) {
        Entry::Vacant(slot) => {
            slot.insert(name);
        }
        Entry::Occupied(slot) => {
            if slot.get() != &name {
                slot.remove();
                ambiguous.insert(key);
            }
        }
    }
}
