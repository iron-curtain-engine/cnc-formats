# Changelog

All notable changes to the `cnc-formats` crate and `cncf` CLI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha.5] - 2026-03-26

### Added
- **`xor_delta` module**: New public `xor_delta::apply_xor_delta` function implementing the full Format40 XOR-delta command set (small/big skip, small/big XOR from stream, repeated XOR, end-of-stream), extracted from `shp/mod.rs` with complete command-table documentation and `## References` section.
- **WSA**: Expose `loop_frame` field on `WsaFile` carrying the loop-back XOR-delta data; auto-detect raw XOR vs Format40 command stream by comparing decompressed size.
- **TMP**: Add `trans_flags` and `remap_data` fields to `TdTmpFile` for per-icon transparency flags and color remap table access.
- **VQA playback**: Add `read_queued_audio_samples` for bounded-memory audio drain that does not pump additional chunks or buffer future video frames.
- **LCW**: Add `decompress_into(src, dst, max)` to reuse an existing `Vec` allocation across calls; add `LcwDecoder::with_buffer` constructor with capacity pre-reserve.
- **AUD**: Add `encode_adpcm_stateful` for encoding that preserves predictor/step-index state across calls, used by the VQA encode path.
- **VQA playback**: Add `VqaAudioChunkDecoder::PcmDirect` variant to hold already-decoded `Vec<i16>` directly, eliminating the `i16 → bytes → i16` round-trip previously required for decoded-upfront SND1/SND2 chunks.
- **SHP**: `ShpFrame` gains a `file_offset` field to support `ref_offset`-based keyframe lookup in `XorLcw` decode.

### Fixed
- **SHP**: `XorLcw` and `XorPrev` delta frames are raw XOR masks, not LCW-compressed streams; `XorLcw` now resolves the reference keyframe by `ref_offset` lookup instead of always using the sequential previous frame.
- **SHP**: Zero-dimension frames (e.g. `SIDEBAR.SHP`) return empty pixel buffers instead of attempting LCW with a zero output cap.
- **SHP**: Padding slot validation relaxed — only rejects the slot when `format_byte` carries a valid frame code (`0x20`/`0x40`/`0x80`); non-zero garbage in other fields (as written by original Westwood tools) is accepted.
- **SHP-D2**: Detect and promote relative-offset table entries (used by RA1 cursor sprites such as `MOUSE.SHP` and `EDMOUSE.SHP`) to absolute offsets; clamp LCW input to the file boundary for last-frame entries whose `data_size` overshoots available bytes.
- **VQA audio (SND2)**: IMA ADPCM predictor/step-index state is now carried across SND2 chunk boundaries; the previous per-chunk reset to initial state corrupted audio after the first chunk.
- **VQA audio (SND1)**: Westwood ADPCM `cur_sample` predictor is now carried across SND1 chunk boundaries; the previous per-chunk reset to `0x80` corrupted multi-chunk streams.
- **CPS**: 6-bit to 8-bit palette conversion uses `(v << 2) | (v >> 4)` so that a full-intensity 63 maps to 255, matching VGA DAC behavior and the `pal` module convention.
- **`build_aud`**: Include the 4-byte per-chunk frame header in the reported compressed size field.
- **`perf_alloc` test**: Add a throwaway allocator warmup call before assertions to avoid false positives from glibc per-thread arena initialisation on first real allocation inside the measurement window.

### Performance
- **IMA ADPCM** (`snd_ima`): Replace per-nibble branch tree with a precomputed `const DELTA_TABLE[89][16]` lookup; `ima_decode_nibble` reduces to a single table load + saturating clamp.
- **SND2 stereo decode**: Decode left/right nibbles in lockstep (`zip`) to produce correctly interleaved `L0 R0 L1 R1` output, replacing the two-pass split-half approach that required an extra allocation and a separate interleave loop.
- **VQA codebook** (`build_compact_codebook`): Before each frame, count VPT reference frequencies and pack only referenced entries hottest-first into a ~1–2 KB compact buffer, keeping the working set in L1 cache instead of scattering across a 32–64 KB codebook. Falls back to the full codebook for all-fill frames, malformed input, or fill-marker aliasing.
- **VQA renderer**: Row-level `copy_from_slice`/`fill` replaces per-pixel inner loops; pre-computed edge clamps eliminate redundant per-pixel bounds checks.
- **LCW / VQA decode**: `decompress_into` reuses the codebook `Vec` allocation across CBFZ/CBPZ updates; raw CBP path swaps buffers (`mem::swap`) instead of `mem::take` + replace.

### Refactored
- **File splits (AGENTS.md compliance)**: Seven files exceeding the ~600-line context target were split into focused submodules: `convert/mkv.rs` → `mkv/mod.rs` + `mkv/ebml.rs`; `aud/mod.rs` → `mod.rs` + `encode.rs`; `tmp/mod.rs` → `mod.rs` + `encode.rs`; `sniff.rs` → `sniff/mod.rs` + `sniff/tests.rs`; `bin/.../inspect/extra.rs` → `extra.rs` + `extra2.rs`; `vqa/tests_playback.rs` split; `vqa/playback.rs` → `playback/mod.rs`.
- **Clippy / rustdoc**: Fixed `empty_line_after_doc_comment`, `needless_pass_by_ref_mut`, `manual_repeat_n`, `dead_code`, `doc_lazy_continuation` warnings; resolved `private_intra_doc_links` and `unresolved_link` rustdoc errors introduced by the file splits.

## [0.1.0-alpha.4] - 2026-03-23

### Fixed
- **SHP Parser**: Relaxed EOF-sentinel and zero-padding entry validation to accept non-zero `ref_offset`/`ref_format` fields. Original Westwood tools wrote garbage into those positions on some RA1 assets; only `format_byte` (and `file_offset` for the padding slot) carry semantic meaning and are validated.
- **SAGE-STR Parser**: Replaced direct string slice `[1..len-1]` with bounds-safe `.get()` access (AGENTS.md P1 compliance).
- **Module Documentation**: Added missing `## References` section to nine format modules (`bag_idx`, `bin_td`, `d2_map`, `icn`, `mpr`, `pak`, `sage_str`, `voc`, `wnd`) to satisfy AGENTS.md P5 requirements.

## [0.1.0-alpha.3] - 2026-03-23

### Added
- **New Format Parsers**: Added complete parsers for fifteen additional C&C/SAGE/Dune formats:
  - **APT** — Red Alert 2 audio track index files.
  - **BAG/IDX** — Red Alert 2 / Yuri's Revenge audio archives (`.bag` data + `.idx` index pairs).
  - **BIN-TD** — Tiberian Dawn / Red Alert 1 binary tileset maps.
  - **D2 Map** — Dune II binary scenario files.
  - **DDS** — DirectDraw Surface compressed textures used in Generals and Zero Hour.
  - **ICN** — Dune II / Tiberian Dawn icon tile and ICON.MAP files.
  - **MAP-RA2** — Red Alert 2 / Yuri's Revenge INI-based map files.
  - **MAP-SAGE** — Generals / Zero Hour binary chunk-based map files (`.map`).
  - **MPR** — Tiberian Dawn / Red Alert 1 binary map files.
  - **PAK** — Dune II resource archive files.
  - **SAGE-STR** — Generals / Zero Hour binary string stream files.
  - **SHP-D2** — Dune II sprite files (distinct format from the C&C SHP variant).
  - **TGA** — Truevision Targa image files used in various C&C titles.
  - **VOC** — Creative Voice audio files used in Dune II and Tiberian Dawn.
  - **WND** — Generals / Zero Hour UI window layout files.
- **CLI `check` Subcommand**: New command that validates a file's format and reports any parse errors.
- **CLI `inspect` Extras**: Extended `cncf inspect` with per-format supplementary metadata for deeper file introspection.
- **CLI `extract` Stored-File Support**: `cncf extract` can now extract stored (non-archive) files directly.
- **AGENTS.md**: Added coding-principle reference document (`P1`–`P7`) for contributor and AI-agent guidance.

### Fixed
- **CPS Parser**: `buffer_size` field corrected from `u16` to `u32` to match the actual 4-byte on-disk layout; `palette_size` read offset adjusted accordingly.
- **CSF Parser**: Accept ` RTS` as a valid string entry marker (used in Red Alert 2 and Generals `.csf` files alongside ` STR` and `STRW`).
- **WND Parser**: Replaced the incorrect `CHILD`/`ENDCHILD` block grammar with the real Generals format: individual `CHILD` keywords before each nested `WINDOW` block, closed by `ENDALLCHILDREN`.
- **MAP-SAGE Parser**: Recognize the 18-byte outer `EAR\0` header present in real Generals / Zero Hour `.map` files and skip to the inner `CkMp` chunk stream.
- **BAG/IDX Parser**: Replaced direct slice indexing with bounds-checked `get` access (AGENTS.md P1 compliance).
- **Documentation**: Removed private intra-doc links (`[`CONSTANT`]`) from public doc comments in `bin_td`, `d2_map`, `icn`, `map_ra2`, and `voc` that caused `RUSTDOCFLAGS=-D warnings` build failures.

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
