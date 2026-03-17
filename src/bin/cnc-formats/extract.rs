// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `extract` subcommand — decompose MIX archives into individual files.
//!
//! Replaces XCC Mixer for the most common modding operation: pulling
//! individual assets out of a `.mix` archive.
//!
//! ## Output naming
//!
//! MIX archives store CRC hashes, not filenames.  Without a `--names`
//! file, extracted entries are named `{CRC:08X}.bin`.  With `--names`,
//! resolved entries use their real filename.
//!
//! ## Progress output
//!
//! All progress and summary output goes to stderr so stdout stays clean
//! for piping.

use super::{
    build_mix_name_map, load_name_map, read_file, resolve_format, supported_archive_list, Format,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use cnc_formats::mix::MixCrc;
use strict_path::{PathBoundary, StrictPath};

// ── extract ──────────────────────────────────────────────────────────────────

/// Parse an archive and write each entry to an individual file.
pub(crate) fn cmd_extract(
    path: &str,
    explicit: Option<Format>,
    output: Option<&str>,
    names: Option<&str>,
    filter: Option<&str>,
) -> i32 {
    let fmt = resolve_format(path, explicit);

    if !is_archive_format(&fmt) {
        eprintln!(
            "Error: `extract` only supports archive formats ({}).\n  \
             To convert non-archive files, use `cncf convert`.",
            supported_archive_list()
        );
        return 1;
    }

    let data = read_file(path);

    // Determine output directory.
    let out_dir = match output {
        Some(d) => PathBuf::from(d),
        None => {
            // Default: input filename stem + "_extracted".
            let stem = Path::new(path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            PathBuf::from(format!("{stem}_extracted"))
        }
    };

    match fmt {
        Format::Mix => {
            // Name resolution priority:
            // 1. Explicit --names file (user override)
            // 2. Embedded XCC "local mix database.dat" from inside the archive
            // 3. Built-in TD/RA1 database (~3,900 known filenames)
            let name_map = match names {
                Some(path) => match load_name_map(path) {
                    Ok(m) => {
                        eprintln!("Loaded {} names from {path}", m.len());
                        m
                    }
                    Err(e) => {
                        eprintln!("Error loading names file: {e}");
                        return 1;
                    }
                },
                None => build_mix_name_map(&data),
            };
            extract_mix(&data, &out_dir, &name_map, filter)
        }
        Format::Big => {
            if names.is_some() {
                eprintln!(
                    "Warning: --names is ignored for BIG archives; filenames are stored in the archive."
                );
            }
            extract_big(&data, &out_dir, filter)
        }
        #[cfg(feature = "meg")]
        Format::Meg => {
            if names.is_some() {
                eprintln!(
                    "Warning: --names is ignored for MEG/PGM archives; filenames are stored in the archive."
                );
            }
            extract_meg(&data, &out_dir, filter)
        }
        _ => {
            eprintln!("Error: unsupported archive format for `extract`.");
            1
        }
    }
}

/// Returns `true` if the format is an archive type that `extract` can handle.
fn is_archive_format(fmt: &Format) -> bool {
    if matches!(fmt, Format::Big) {
        return true;
    }
    #[cfg(feature = "meg")]
    if matches!(fmt, Format::Meg) {
        return true;
    }
    matches!(fmt, Format::Mix)
}

// ── MIX extraction ───────────────────────────────────────────────────────────

/// Parse a MIX archive and extract matching entries to `out_dir`.
fn extract_mix(
    data: &[u8],
    out_dir: &Path,
    name_map: &HashMap<MixCrc, String>,
    filter: Option<&str>,
) -> i32 {
    let archive = match cnc_formats::mix::MixArchive::parse(data) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    // Validate the extraction root once, then validate every archive path
    // against that boundary.  `strict-path` handles traversal, symlink, and
    // platform-specific path tricks that ad-hoc sanitizers routinely miss.
    let out_boundary = match PathBoundary::try_new_create(out_dir) {
        Ok(boundary) => boundary,
        Err(e) => {
            eprintln!(
                "Error creating extraction boundary {}: {e}",
                out_dir.display()
            );
            return 1;
        }
    };

    let entries = archive.entries();
    let filter_lower = filter.map(|f| f.to_ascii_lowercase());

    eprintln!(
        "Extracting from MIX archive ({} entries) to {}",
        entries.len(),
        out_dir.display()
    );

    let mut extracted = 0u64;
    let mut bytes_total = 0u64;
    let mut used_names = HashSet::new();

    for (i, entry) in entries.iter().enumerate() {
        // Read entry data first so we can sniff format for unnamed files.
        let file_data = match archive.get_by_index(i) {
            Some(d) => d,
            None => {
                eprintln!(
                    "  Warning: could not read 0x{:08X}, skipping",
                    entry.crc.to_raw()
                );
                continue;
            }
        };

        // Resolve filename: real name if available, else CRC hex with
        // sniffed extension.
        let resolved = name_map.get(&entry.crc).map(|s| s.as_str());
        let display_name = resolved.unwrap_or("(unknown)");
        let fallback_name = fallback_filename(entry.crc, Some(file_data));
        let (strict_path, relative_name, warning) =
            match resolve_output_name(&out_boundary, resolved, entry.crc, Some(file_data)) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!(
                        "  Error: could not resolve output path for 0x{:08X} ({display_name}): {e}",
                        entry.crc.to_raw()
                    );
                    return 1;
                }
            };

        if let Some(message) = warning {
            eprintln!("  Warning: {message}");
        }

        // Apply filter to the logical archive name before any collision-driven
        // fallback rewrite so duplicate entries are not accidentally skipped.
        if let Some(ref fl) = filter_lower {
            if !relative_name.to_ascii_lowercase().contains(fl.as_str()) {
                continue;
            }
        }

        let (strict_path, relative_name, collision_warning) =
            make_output_name_unique(strict_path, relative_name, &fallback_name, &mut used_names);

        if let Some(message) = collision_warning {
            eprintln!("  Warning: {message}");
        }

        // Preserve a validated metadata path when one exists; otherwise use the
        // deterministic flat fallback name under the extraction root.
        if let Some(out_path) = strict_path {
            if let Err(e) = out_path
                .create_parent_dir_all()
                .and_then(|_| out_path.write(file_data))
            {
                eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                return 1;
            }
        } else {
            let out_path = match generated_flat_output_path(&out_boundary, &relative_name) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!(
                        "  Error: could not build fallback output path for 0x{:08X} ({display_name}): {e}",
                        entry.crc.to_raw()
                    );
                    return 1;
                }
            };

            if let Some(parent) = out_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("  Error creating {}: {e}", parent.display());
                    return 1;
                }
            }

            if let Err(e) = std::fs::write(&out_path, file_data) {
                eprintln!("  Error writing {}: {e}", out_path.display());
                return 1;
            }
        }

        eprintln!("  {} ({} bytes)", relative_name, entry.size);
        extracted = extracted.saturating_add(1);
        bytes_total = bytes_total.saturating_add(u64::from(entry.size));
    }

    eprintln!("\nExtracted {extracted} files ({bytes_total} bytes total)");
    0
}

// ── Output-path validation ───────────────────────────────────────────────────

/// Resolve the archive entry's output name and optional validated path.
///
/// Real filenames from `--names` are preserved when they are safe.  Unsafe or
/// malformed names fall back to a deterministic CRC-based filename so the
/// extraction still succeeds without trusting hostile path text.  The returned
/// `StrictPath` is only present when an untrusted metadata path survives.
fn resolve_output_name(
    out_boundary: &PathBoundary,
    resolved_name: Option<&str>,
    crc: MixCrc,
    file_data: Option<&[u8]>,
) -> Result<(Option<StrictPath>, String, Option<String>), String> {
    if let Some(name) = resolved_name {
        match validate_candidate_path(out_boundary, name) {
            Ok(path) => return Ok((Some(path), name.to_string(), None)),
            Err(reason) => {
                let fallback = fallback_filename(crc, file_data);
                let warning = format!(
                    "refusing unsafe resolved name `{name}` for 0x{:08X}: {reason}; using `{fallback}`",
                    crc.to_raw()
                );
                return Ok((None, fallback, Some(warning)));
            }
        }
    }

    let fallback = fallback_filename(crc, file_data);
    Ok((None, fallback, None))
}

/// Validate one candidate relative path under the extraction boundary.
///
/// The path must stay inside `out_boundary` and must resolve to a file-like
/// path rather than the boundary root or a directory.
fn validate_candidate_path(
    out_boundary: &PathBoundary,
    candidate: &str,
) -> Result<StrictPath, String> {
    let path = out_boundary
        .strict_join(candidate)
        .map_err(|e| e.to_string())?;

    if path.strictpath_file_name().is_none() {
        return Err("path resolves to the extraction root or a directory".to_string());
    }

    Ok(path)
}

/// Build a flat fallback output path under the extraction boundary.
///
/// The fallback filename is generated by this crate (`CRC.ext`), so it is not
/// treated as untrusted metadata.  Keep this path flat on purpose: if a future
/// change ever adds separators, fail loudly instead of silently widening the
/// trust boundary.
fn generated_flat_output_path(
    out_boundary: &PathBoundary,
    generated_name: &str,
) -> Result<PathBuf, String> {
    if generated_name.contains('/') || generated_name.contains('\\') {
        return Err("generated fallback name must be a single flat filename".to_string());
    }

    Ok(PathBuf::from(out_boundary.interop_path()).join(generated_name))
}

/// Deterministic fallback filename for entries without a safe resolved path.
///
/// If `file_data` is provided, attempts format detection by content
/// inspection to assign a meaningful extension instead of `.bin`.
fn fallback_filename(crc: MixCrc, file_data: Option<&[u8]>) -> String {
    let ext = file_data
        .and_then(cnc_formats::sniff::sniff_format)
        .unwrap_or("bin");
    format!("{:08X}.{ext}", crc.to_raw())
}

/// Ensure one extraction run never writes two entries to the same path.
///
/// The preferred archive-derived name is used when available. If that name has
/// already been claimed by an earlier entry, the extractor falls back to a
/// deterministic flat filename and, if needed, appends a numeric suffix.
fn make_output_name_unique(
    strict_path: Option<StrictPath>,
    relative_name: String,
    fallback_name: &str,
    used_names: &mut HashSet<String>,
) -> (Option<StrictPath>, String, Option<String>) {
    let key = relative_name.to_ascii_lowercase();
    if used_names.insert(key) {
        return (strict_path, relative_name, None);
    }

    let mut suffix = 2usize;
    let mut candidate = if relative_name != fallback_name {
        fallback_name.to_string()
    } else {
        suffixed_flat_name(fallback_name, suffix)
    };
    while !used_names.insert(candidate.to_ascii_lowercase()) {
        candidate = suffixed_flat_name(fallback_name, suffix);
        suffix = suffix.saturating_add(1);
    }

    let warning = format!("duplicate output name `{relative_name}`; using `{candidate}` instead");
    (None, candidate, Some(warning))
}

/// Insert a numeric suffix before the extension of a generated flat filename.
fn suffixed_flat_name(base: &str, suffix: usize) -> String {
    let path = Path::new(base);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| base.to_string());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());

    match ext {
        Some(ext) if !ext.is_empty() => format!("{stem}__{suffix}.{ext}"),
        _ => format!("{stem}__{suffix}"),
    }
}

/// Keep duplicate metadata-derived paths distinct without flattening them.
///
/// Archive formats with stored filenames (BIG, MEG) should preserve those
/// relative paths when safe. If an archive repeats the same logical path,
/// append a numeric suffix before the extension instead of overwriting the
/// earlier payload.
fn make_stored_name_unique(
    relative_name: &str,
    used_names: &mut HashSet<String>,
) -> (String, Option<String>) {
    let key = relative_name.to_ascii_lowercase();
    if used_names.insert(key) {
        return (relative_name.to_string(), None);
    }

    let mut suffix = 2usize;
    let mut candidate = suffixed_relative_name(relative_name, suffix);
    while !used_names.insert(candidate.to_ascii_lowercase()) {
        suffix = suffix.saturating_add(1);
        candidate = suffixed_relative_name(relative_name, suffix);
    }

    let warning = format!("duplicate output name `{relative_name}`; using `{candidate}` instead");
    (candidate, Some(warning))
}

/// Insert a numeric suffix before the last path component's extension while
/// preserving parent directories.
fn suffixed_relative_name(base: &str, suffix: usize) -> String {
    let path = Path::new(base);
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| base.to_string());
    let suffixed = suffixed_flat_name(&file_name, suffix);

    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            format!("{}/{}", parent.to_string_lossy(), suffixed)
        }
        _ => suffixed,
    }
}

// ── BIG extraction ───────────────────────────────────────────────────────────

/// Parse a BIG archive and extract matching entries to `out_dir`.
///
/// BIG archives store filenames directly, typically with Windows path
/// separators. Those separators are normalised before boundary validation so
/// extraction creates nested directories correctly on non-Windows hosts.
fn extract_big(data: &[u8], out_dir: &Path, filter: Option<&str>) -> i32 {
    let archive = match cnc_formats::big::BigArchive::parse(data) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let out_boundary: PathBoundary = match PathBoundary::try_new_create(out_dir) {
        Ok(boundary) => boundary,
        Err(e) => {
            eprintln!(
                "Error creating extraction boundary {}: {e}",
                out_dir.display()
            );
            return 1;
        }
    };

    let entries = archive.entries();
    let filter_lower = filter.map(|f| f.to_ascii_lowercase());

    eprintln!(
        "Extracting from BIG archive ({} entries) to {}",
        entries.len(),
        out_dir.display()
    );

    let mut extracted = 0u64;
    let mut bytes_total = 0u64;
    let mut used_names = HashSet::new();

    for (i, entry) in entries.iter().enumerate() {
        let normalized_name = entry.name.replace('\\', "/");

        if let Some(ref fl) = filter_lower {
            if !normalized_name.to_ascii_lowercase().contains(fl.as_str()) {
                continue;
            }
        }

        let (relative_name, collision_warning) =
            make_stored_name_unique(&normalized_name, &mut used_names);
        if let Some(message) = collision_warning {
            eprintln!("  Warning: {message}");
        }

        let out_path = match validate_candidate_path(&out_boundary, &relative_name) {
            Ok(path) => path,
            Err(e) => {
                eprintln!(
                    "  Warning: refusing unsafe path \"{}\": {e}, skipping",
                    entry.name
                );
                continue;
            }
        };

        let file_data = match archive.get_by_index(i) {
            Some(d) => d,
            None => {
                eprintln!("  Warning: could not read \"{}\", skipping", entry.name);
                continue;
            }
        };

        if let Err(e) = out_path
            .create_parent_dir_all()
            .and_then(|_| out_path.write(file_data))
        {
            eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
            return 1;
        }

        eprintln!("  {} ({} bytes)", relative_name, entry.size);
        extracted = extracted.saturating_add(1);
        bytes_total = bytes_total.saturating_add(entry.size);
    }

    eprintln!("\nExtracted {extracted} files ({bytes_total} bytes total)");
    0
}

// ── MEG extraction ───────────────────────────────────────────────────────────

/// Parse a MEG archive and extract matching entries to `out_dir`.
///
/// MEG archives store filenames directly, so no `--names` file is needed.
/// Filenames are validated against the extraction boundary to prevent
/// path traversal attacks from malicious archive contents.
#[cfg(feature = "meg")]
fn extract_meg(data: &[u8], out_dir: &Path, filter: Option<&str>) -> i32 {
    let archive = match cnc_formats::meg::MegArchive::parse(data) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let out_boundary: PathBoundary = match PathBoundary::try_new_create(out_dir) {
        Ok(boundary) => boundary,
        Err(e) => {
            eprintln!(
                "Error creating extraction boundary {}: {e}",
                out_dir.display()
            );
            return 1;
        }
    };

    let entries = archive.entries();
    let filter_lower = filter.map(|f| f.to_ascii_lowercase());

    eprintln!(
        "Extracting from MEG archive ({} entries) to {}",
        entries.len(),
        out_dir.display()
    );

    let mut extracted = 0u64;
    let mut bytes_total = 0u64;
    let mut used_names = HashSet::new();

    for (i, entry) in entries.iter().enumerate() {
        let normalized_name = entry.name.replace('\\', "/");

        // Apply filter: case-insensitive substring match on the filename.
        if let Some(ref fl) = filter_lower {
            if !normalized_name.to_ascii_lowercase().contains(fl.as_str()) {
                continue;
            }
        }

        let (relative_name, collision_warning) =
            make_stored_name_unique(&normalized_name, &mut used_names);
        if let Some(message) = collision_warning {
            eprintln!("  Warning: {message}");
        }

        // Validate the filename against the extraction boundary.
        let out_path = match validate_candidate_path(&out_boundary, &relative_name) {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "  Warning: refusing unsafe path \"{}\": {e}, skipping",
                    entry.name
                );
                continue;
            }
        };

        // Read entry data by index (not by name) so that duplicate or
        // case-colliding filenames each get their own payload.
        let file_data = match archive.get_by_index(i) {
            Some(d) => d,
            None => {
                eprintln!("  Warning: could not read \"{}\", skipping", entry.name);
                continue;
            }
        };

        if let Err(e) = out_path
            .create_parent_dir_all()
            .and_then(|_| out_path.write(file_data))
        {
            eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
            return 1;
        }

        eprintln!("  {} ({} bytes)", relative_name, entry.size);
        extracted = extracted.saturating_add(1);
        bytes_total = bytes_total.saturating_add(entry.size);
    }

    eprintln!("\nExtracted {extracted} files ({bytes_total} bytes total)");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Create a unique temporary directory for extraction-path tests.
    ///
    /// Why: each test needs an isolated filesystem boundary so path
    /// validation and file creation do not interfere with parallel runs.
    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cnc_formats_{prefix}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Safe nested filenames keep their relative subdirectories.
    ///
    /// Why: MIX name maps may contain legitimate path components.  The
    /// extractor should preserve them instead of flattening everything.
    #[test]
    fn resolve_output_path_preserves_nested_relative_name() {
        let dir = temp_dir("extract_nested");
        let boundary = PathBoundary::try_new(&dir).unwrap();

        let (strict_path, relative, warning) = resolve_output_name(
            &boundary,
            Some("tiles/desert/unit.shp"),
            MixCrc::from_raw(0x1234_5678),
            None,
        )
        .unwrap();

        assert_eq!(relative, "tiles/desert/unit.shp");
        assert!(warning.is_none());
        let path = strict_path.expect("safe metadata path should stay strict");
        path.create_parent_dir_all().unwrap();
        path.write([0xAA, 0xBB]).unwrap();
        assert!(dir.join("tiles").join("desert").join("unit.shp").exists());

        fs::remove_dir_all(&dir).ok();
    }

    /// Path traversal names fall back to a deterministic CRC filename.
    ///
    /// Why: hostile `--names` input must not escape the extraction
    /// boundary, but it also should not abort the whole archive dump.
    #[test]
    fn resolve_output_path_traversal_falls_back_to_crc_name() {
        let dir = temp_dir("extract_traversal");
        let boundary = PathBoundary::try_new(&dir).unwrap();
        let crc = MixCrc::from_raw(0xDEAD_BEEF);

        let (strict_path, relative, warning) =
            resolve_output_name(&boundary, Some("../../evil.shp"), crc, None).unwrap();

        assert_eq!(relative, "DEADBEEF.bin");
        assert!(warning.is_some());
        assert!(strict_path.is_none());
        let path = generated_flat_output_path(&boundary, &relative).unwrap();
        fs::write(&path, [0xCC]).unwrap();
        assert!(dir.join("DEADBEEF.bin").exists());
        assert!(!dir.join("evil.shp").exists());

        fs::remove_dir_all(&dir).ok();
    }

    /// Duplicate logical filenames are rewritten instead of overwriting.
    #[test]
    fn duplicate_output_name_uses_unique_fallback() {
        let mut used = HashSet::new();
        let first =
            make_output_name_unique(None, "ALLY1.VQP".to_string(), "1234ABCD.vqp", &mut used);
        let second =
            make_output_name_unique(None, "ALLY1.VQP".to_string(), "1234ABCD.vqp", &mut used);

        assert_eq!(first.1, "ALLY1.VQP");
        assert!(first.2.is_none());
        assert_eq!(second.1, "1234ABCD.vqp");
        assert!(second.2.is_some());

        let third =
            make_output_name_unique(None, "ALLY1.VQP".to_string(), "1234ABCD.vqp", &mut used);
        assert_eq!(third.1, "1234ABCD__2.vqp");
    }

    /// Duplicate stored archive paths get suffixed in place instead of
    /// flattening to unrelated fallback names.
    #[test]
    fn duplicate_stored_name_preserves_parent_dirs() {
        let mut used = HashSet::new();
        let first = make_stored_name_unique("Data/Audio/test.wav", &mut used);
        let second = make_stored_name_unique("Data/Audio/test.wav", &mut used);

        assert_eq!(first.0, "Data/Audio/test.wav");
        assert!(first.1.is_none());
        assert_eq!(second.0, "Data/Audio/test__2.wav");
        assert!(second.1.is_some());
    }

    /// Sniffed LUT content gets a `.lut` fallback extension.
    #[test]
    fn fallback_filename_uses_lut_extension() {
        let mut data = Vec::with_capacity(cnc_formats::lut::LUT_FILE_SIZE);
        for i in 0..cnc_formats::lut::LUT_ENTRY_COUNT {
            data.push((i % 64) as u8);
            data.push(((i / 64) % 64) as u8);
            data.push(((i / 256) % 16) as u8);
        }

        let name = fallback_filename(MixCrc::from_raw(0xDEAD_BEEF), Some(&data));
        assert_eq!(name, "DEADBEEF.lut");
    }

    /// Segmented setup-data DIPs get a `.dip` fallback extension.
    #[test]
    fn fallback_filename_uses_dip_extension() {
        let data = [
            0x02, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00,
            0x3C, 0x3C, 0x01, 0x80, 0x00, 0x00, 0x0B, 0x80,
        ];

        let name = fallback_filename(MixCrc::from_raw(0xDEAD_BEEF), Some(&data));
        assert_eq!(name, "DEADBEEF.dip");
    }
}
