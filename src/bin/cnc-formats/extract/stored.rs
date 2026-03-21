// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use std::io::Write;

/// Parse a BIG archive and extract matching entries to `out_dir`.
///
/// BIG archives store filenames directly, typically with Windows path
/// separators. Those separators are normalised before boundary validation so
/// extraction creates nested directories correctly on non-Windows hosts.
pub(super) fn extract_big<R: std::io::Read + std::io::Seek>(
    file: R,
    out_dir: &Path,
    filter: Option<&str>,
) -> i32 {
    let mut archive = match cnc_formats::big::BigArchiveReader::open(file) {
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

    let entries = archive.entries().to_vec();
    let extraction_order = archive.indices_by_offset();
    let filter_lower = filter.map(|f| f.to_ascii_lowercase());

    eprintln!(
        "Extracting from BIG archive ({} entries) to {}",
        entries.len(),
        out_dir.display()
    );

    let mut extracted = 0u64;
    let mut bytes_total = 0u64;
    let mut used_names = HashSet::new();

    for &i in &extraction_order {
        let entry = match entries.get(i) {
            Some(e) => e,
            None => continue,
        };
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

        let mut out_file = match create_strict_output_file(&out_path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                return 1;
            }
        };

        match archive.copy_by_index(i, &mut out_file) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("  Warning: could not read \"{}\", skipping", entry.name);
                continue;
            }
            Err(e) => {
                eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                return 1;
            }
        }

        if let Err(e) = out_file.flush() {
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

/// Parse a MEG archive and extract matching entries to `out_dir`.
///
/// MEG archives store filenames directly, so no `--names` file is needed.
/// Filenames are validated against the extraction boundary to prevent
/// path traversal attacks from malicious archive contents.
#[cfg(feature = "meg")]
pub(super) fn extract_meg<R: std::io::Read + std::io::Seek>(
    file: R,
    out_dir: &Path,
    filter: Option<&str>,
) -> i32 {
    let mut archive = match cnc_formats::meg::MegArchiveReader::open(file) {
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

    let entries = archive.entries().to_vec();
    let extraction_order = archive.indices_by_offset();
    let filter_lower = filter.map(|f| f.to_ascii_lowercase());

    eprintln!(
        "Extracting from MEG archive ({} entries) to {}",
        entries.len(),
        out_dir.display()
    );

    let mut extracted = 0u64;
    let mut bytes_total = 0u64;
    let mut used_names = HashSet::new();

    for &i in &extraction_order {
        let entry = match entries.get(i) {
            Some(e) => e,
            None => continue,
        };
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
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "  Warning: refusing unsafe path \"{}\": {e}, skipping",
                    entry.name
                );
                continue;
            }
        };

        let mut out_file = match create_strict_output_file(&out_path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                return 1;
            }
        };

        match archive.copy_by_index(i, &mut out_file) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("  Warning: could not read \"{}\", skipping", entry.name);
                continue;
            }
            Err(e) => {
                eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                return 1;
            }
        }

        if let Err(e) = out_file.flush() {
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
