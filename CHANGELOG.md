# Changelog

All notable changes to the `cnc-formats` crate and `cncf` CLI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
