// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::{crc, MixCrc, MixEntry};

use std::collections::HashMap;

/// Resolved MIX entry location inside a mounted overlay set.
///
/// `source` is caller-defined metadata identifying which mounted archive owns
/// the selected entry. Keep it small and cheap to clone, such as an integer
/// archive ID or `Arc<PathBuf>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixOverlayRecord<S> {
    /// Caller-defined archive/source identifier.
    pub source: S,
    /// Index of the winning entry inside that archive's entry table.
    pub entry_index: usize,
    /// Stored byte size of the winning entry.
    pub size: u32,
}

/// Overlay-resolution index spanning multiple mounted MIX archives.
///
/// This type caches the final winning source for each CRC according to mount
/// order. Later mounts override earlier mounts, mirroring classic archive
/// overlay behavior and the package-resolution strategy used by downstream
/// engines.
///
/// The model is lookup-based, not stream-based: callers mount archive entry
/// tables once, then resolve `MixCrc` values or filenames without rescanning
/// every archive. The index allocates only when archives are mounted.
#[derive(Debug, Clone, Default)]
pub struct MixOverlayIndex<S> {
    resolved: HashMap<MixCrc, MixOverlayRecord<S>>,
}

impl<S: Clone> MixOverlayIndex<S> {
    /// Creates an empty overlay index.
    #[inline]
    pub fn new() -> Self {
        Self {
            resolved: HashMap::new(),
        }
    }

    /// Mounts one archive's entry table.
    ///
    /// Later calls override earlier mounts for duplicate CRCs.
    pub fn mount_archive(&mut self, source: S, entries: &[MixEntry]) {
        for (entry_index, entry) in entries.iter().enumerate() {
            self.resolved.insert(
                entry.crc,
                MixOverlayRecord {
                    source: source.clone(),
                    entry_index,
                    size: entry.size,
                },
            );
        }
    }

    /// Returns the winning entry for a known CRC.
    #[inline]
    pub fn resolve_crc(&self, key: MixCrc) -> Option<&MixOverlayRecord<S>> {
        self.resolved.get(&key)
    }

    /// Returns the winning entry for a filename.
    #[inline]
    pub fn resolve_name(&self, filename: &str) -> Option<&MixOverlayRecord<S>> {
        self.resolve_crc(crc(filename))
    }

    /// Returns the number of resolved CRCs in the overlay set.
    #[inline]
    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    /// Returns `true` when the overlay set contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }

    /// Returns an iterator over the resolved winning entries.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&MixCrc, &MixOverlayRecord<S>)> {
        self.resolved.iter()
    }
}
