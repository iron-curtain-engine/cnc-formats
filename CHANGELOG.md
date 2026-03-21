# Changelog

All notable changes to the `cnc-formats` crate and `cncf` CLI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha.2] - 2026-03-20

### Added
- **New Format Parsers**: Added complete parsers for seven additional C&C/SAGE formats:
  - **CPS** — compressed screen pictures used in Tiberian Dawn and Red Alert.
  - **CSF** — compiled string tables used in Tiberian Sun, Red Alert 2, and Generals.
  - **HVA** — voxel animation sequences for Red Alert 2 unit turrets and barrels.
  - **SHP-TS** — the Tiberian Sun / Red Alert 2 sprite variant with per-frame crop offsets.
  - **TMP-TS** — isometric terrain tiles for Tiberian Sun and Red Alert 2 (`TsTmpFile`).
  - **VXL** — voxel models for Red Alert 2 and Tiberian Sun 3-D units.
  - **W3D** — the SAGE-era 3-D model/mesh format used in Generals and Zero Hour.
- **SHP Encoding**: Added `shp::encode_frames` to produce SHP keyframe binaries from indexed-color frame slices, completing the SHP round-trip.
- **VQA → MKV Conversion**: Added Matroska container export (`vqa_to_mkv`) as an alternative to AVI, preserving the original video and audio streams in a modern container.
- **`cncf identify` Command**: New CLI subcommand that probes file contents and reports the most likely format without relying on the file extension.
- **Format Auto-Detection Library API**: Added `sniff_format` to `src/sniff.rs` for programmatic content-based format identification.
- **MIX Overlay Index**: Added `MixOverlayIndex` for mounting multiple MIX archives into a single logical namespace and resolving entries by CRC or filename across the overlay stack.
- **VQA Timing API**: Added `VqaTiming` with frame-accurate timestamp queries, total-duration calculation, seek-by-wall-clock-time, and audio/video sync ratio helpers.
- **Streaming Archive Readers**: Added reader-based archive APIs for `.MIX`, `.BIG`, and `.MEG`/`.PGM` containers so callers can inspect and extract large archives without loading the whole file into memory first.
- **Incremental VQA Playback API**: Added reusable incremental VQA playback surfaces with metadata-first open, frame-by-frame decode, chunked audio decode, bounded per-frame audio decode, and rewind/restart support.
- **Streaming AUD Decode API**: Added `AudStream` for chunked AUD PCM decode with early metadata access and seekable rewind support for long-form playback.
- **Performance Benchmarks**: Added Criterion throughput benchmarks for format parsing and streaming paths, and Callgrind hot-path benchmarks for zero-allocation verification.
- **Media Streaming Tests**: Added synthetic fixture coverage for incremental VQA playback, incremental AUD decode, rewind behavior, malformed/truncated media, and whole-file alignment checks.

### Changed
- **CLI Streaming Behavior**: `cncf list`, `cncf extract`, and archive fingerprinting now use streaming archive readers instead of eager whole-file reads where practical.
- **AUD to WAV Conversion**: The CLI and conversion helpers now stream AUD decoding into WAV output instead of materializing the full decoded sample buffer first.
- **Codebase Structure**: Split oversized production and test files into focused directory modules to keep the crate aligned with the AGENTS file-size and context-efficiency rules.
- **MIX Built-in Names**: Moved the large built-in TD/RA1 MIX filename corpus out of Rust source into a text asset and focused resolver module.

### Fixed
- **Prerelease Installation Docs**: Corrected installation instructions so prerelease versions use explicit `cargo install --version ...` guidance.
- **CLI/Test Coverage Preservation**: Expanded CLI integration coverage after the file splits so prior command behavior stayed intact across `validate`, `inspect`, `convert`, `list`, `extract`, `check`, `identify`, and `fingerprint`.
- **Parser/CLI Panic Surfaces**: Tightened several parser and CLI error paths so malformed input returns structured errors instead of relying on panic-prone behavior.
- **AGENTS.md Compliance**: Renamed all single-letter lifetime parameters to meaningful names (`'input`, `'reader`, `'data`); added `#[inline]` to all hot-path accessors; ensured every production and test file stays within the 600-line RAG/LLM context target.

### Documentation
- **Streaming Media Guidance**: Updated `README.md` and `skills/cnc-formats/SKILL.md` to document the new streaming archive, VQA, and AUD APIs.
- **Agent Guidance**: Refreshed `AGENTS.md` to match the current module layout, CLI surface, and maintenance rules.

## [0.1.0-alpha.1] - 2026-03-18

### Added
- **VQP Format Support**: Added full support for parsing VQP (palette interpolation tables) files.
  - The crate now provides `VqpFile` and `VqpTable` structs to parse and manage packed interpolation table data and metadata.
- **LUT, ENG, and DIP Formats**: Added complete parsers for Red Alert's Chrono Vortex lookup tables (`.LUT`), game engine strings (`.ENG`), and special effects palettes (`.DIP`).
- **Red Alert 2 Dictionary Support**: Added `known_names_ra2.txt` and updated the `mix` module with a massive dictionary of known filenames for Red Alert 2 to help with `MIX` archive filename hashing and extraction.
- **Agent Integration**: Added an LM skill file to the repository (`skills/cnc-formats/SKILL.md`) to help AI agents understand how to use the `cncf` CLI and Rust API.

### Changed
- **BIG Archive Duplicates**: The CLI now properly handles duplicate entries when extracting `.BIG` archives. It automatically generates unique fallback names to prevent extracted files from being overwritten.
- **Archive Extraction Fallbacks**: Enhanced the `cncf extract` CLI logic with strict path validation to prevent boundary escapes and added robust fallback handling for corrupted filenames within archives.

### Fixed
- **Validation**: Added enhanced validation and inspection CLI logic and tests for `LUT`, `ENG`, and `DIP` file formats.
- **VQP Error Handling**: Added rigorous error handling on malformed VQP files, protecting against size mismatches and excessive table counts, complete with comprehensive unit tests for metadata inspection.

### Documentation
- **CLI Tooling**: Updated CLI tool documentation (`--help` annotations) for better clarity and alignment.
- **Badges**: Added the LM Ready badge and image to the project's `README.md`.
