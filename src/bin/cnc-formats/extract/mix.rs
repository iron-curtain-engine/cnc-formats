// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

use std::io::Write;

pub(super) trait MixExtractSource {
    fn entries_vec(&self) -> Vec<cnc_formats::mix::MixEntry>;
    fn indices_by_offset(&self) -> Vec<usize>;
    fn read_by_index_owned(&mut self, index: usize) -> Result<Option<Vec<u8>>, cnc_formats::Error>;
    fn copy_by_index_to<W: Write>(
        &mut self,
        index: usize,
        writer: &mut W,
    ) -> Result<bool, cnc_formats::Error>;
}

impl<R: std::io::Read + std::io::Seek> MixExtractSource for cnc_formats::mix::MixArchiveReader<R> {
    fn entries_vec(&self) -> Vec<cnc_formats::mix::MixEntry> {
        self.entries().to_vec()
    }

    fn indices_by_offset(&self) -> Vec<usize> {
        self.indices_by_offset()
    }

    fn read_by_index_owned(&mut self, index: usize) -> Result<Option<Vec<u8>>, cnc_formats::Error> {
        self.read_by_index(index)
    }

    fn copy_by_index_to<W: Write>(
        &mut self,
        index: usize,
        writer: &mut W,
    ) -> Result<bool, cnc_formats::Error> {
        self.copy_by_index(index, writer)
    }
}

impl MixExtractSource for cnc_formats::mix::MixArchive<'_> {
    fn entries_vec(&self) -> Vec<cnc_formats::mix::MixEntry> {
        self.entries().to_vec()
    }

    fn indices_by_offset(&self) -> Vec<usize> {
        let entries = self.entries();
        let mut indices: Vec<usize> = (0..entries.len()).collect();
        indices.sort_by_key(|&i| entries.get(i).map_or(u32::MAX, |e| e.offset));
        indices
    }

    fn read_by_index_owned(&mut self, index: usize) -> Result<Option<Vec<u8>>, cnc_formats::Error> {
        Ok(self.get_by_index(index).map(|data| data.to_vec()))
    }

    fn copy_by_index_to<W: Write>(
        &mut self,
        index: usize,
        writer: &mut W,
    ) -> Result<bool, cnc_formats::Error> {
        match self.get_by_index(index) {
            Some(data) => {
                writer
                    .write_all(data)
                    .map_err(|error| cnc_formats::Error::Io {
                        context: "writing MIX entry data",
                        kind: error.kind(),
                    })?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

/// Parse a MIX archive and extract matching entries to `out_dir`.
pub(super) fn extract_mix<A: MixExtractSource>(
    archive: &mut A,
    out_dir: &Path,
    name_map: &HashMap<MixCrc, String>,
    filter: Option<&str>,
) -> i32 {
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

    let entries = archive.entries_vec();
    let extraction_order = archive.indices_by_offset();
    let filter_lower = filter.map(|f| f.to_ascii_lowercase());

    eprintln!(
        "Extracting from MIX archive ({} entries) to {}",
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
        let resolved = name_map.get(&entry.crc).map(|s| s.as_str());
        let display_name = resolved.unwrap_or("(unknown)");
        let file_data = if resolved.is_none() {
            match archive.read_by_index_owned(i) {
                Ok(Some(data)) => Some(data),
                Ok(None) => {
                    eprintln!(
                        "  Warning: could not read 0x{:08X}, skipping",
                        entry.crc.to_raw()
                    );
                    continue;
                }
                Err(e) => {
                    eprintln!("  Error reading 0x{:08X}: {e}", entry.crc.to_raw());
                    return 1;
                }
            }
        } else {
            None
        };
        let fallback_name = fallback_filename(entry.crc, resolved, file_data.as_deref());
        let (strict_path, relative_name, warning) =
            match resolve_output_name(&out_boundary, resolved, entry.crc, file_data.as_deref()) {
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

        if let Some(out_path) = strict_path {
            if let Some(data) = file_data.as_deref() {
                if let Err(e) = out_path
                    .create_parent_dir_all()
                    .and_then(|_| out_path.write(data))
                {
                    eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                    return 1;
                }
            } else {
                let mut out_file = match create_strict_output_file(&out_path) {
                    Ok(file) => file,
                    Err(e) => {
                        eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                        return 1;
                    }
                };
                match archive.copy_by_index_to(i, &mut out_file) {
                    Ok(true) => {}
                    Ok(false) => {
                        eprintln!(
                            "  Warning: could not read 0x{:08X}, skipping",
                            entry.crc.to_raw()
                        );
                        continue;
                    }
                    Err(e) => {
                        eprintln!("  Error writing {}: {e}", out_path.strictpath_display());
                        return 1;
                    }
                }
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

            if let Some(data) = file_data.as_deref() {
                if let Err(e) = std::fs::write(&out_path, data) {
                    eprintln!("  Error writing {}: {e}", out_path.display());
                    return 1;
                }
            } else {
                let mut out_file = match create_generated_output_file(&out_path) {
                    Ok(file) => file,
                    Err(e) => {
                        eprintln!("  Error writing {}: {e}", out_path.display());
                        return 1;
                    }
                };
                match archive.copy_by_index_to(i, &mut out_file) {
                    Ok(true) => {}
                    Ok(false) => {
                        eprintln!(
                            "  Warning: could not read 0x{:08X}, skipping",
                            entry.crc.to_raw()
                        );
                        continue;
                    }
                    Err(e) => {
                        eprintln!("  Error writing {}: {e}", out_path.display());
                        return 1;
                    }
                }
            }
        }

        eprintln!("  {} ({} bytes)", relative_name, entry.size);
        extracted = extracted.saturating_add(1);
        bytes_total = bytes_total.saturating_add(u64::from(entry.size));
    }

    eprintln!("\nExtracted {extracted} files ({bytes_total} bytes total)");
    0
}
