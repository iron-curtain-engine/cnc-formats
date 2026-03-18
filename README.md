# cnc-formats

<p align="center">
  <img src="images/logo.png" alt="Iron Curtain logo" width="280">
</p>

<p align="center">
  <a href="https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/ci.yml"><img src="https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/fuzz.yml"><img src="https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/fuzz.yml/badge.svg" alt="Fuzz"></a>
  <a href="https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/audit.yml"><img src="https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/audit.yml/badge.svg" alt="Security Audit"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg" alt="License"></a>
  <a href="deny.toml"><img src="https://img.shields.io/badge/no_GPL_deps-enforced-brightgreen.svg" alt="No GPL"></a>
  <a href="https://crates.io/crates/cnc-formats"><img src="https://img.shields.io/crates/v/cnc-formats.svg" alt="crates.io"></a>
</p>

<p align="center">
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.85%2B-orange.svg" alt="Rust"></a>
  &nbsp;&nbsp;
  <img src="https://img.shields.io/badge/LM-ready-blueviolet.svg" alt="LM Ready"><br>
  <img src="images/rust_inside.png" alt="Rust-based project" width="74">
  &nbsp;
  <img src="images/lm_ready.png" alt="LM Ready" width="74">
</p>

Clean-room binary format parsers for Command & Conquer game files, plus the
`cncf` command-line tool. Supports Red Alert, Tiberian Dawn, and related
C&C titles — see the full format list below.

<p align="center">
  <a href="https://github.com/iron-curtain-engine/cnc-formats/releases/latest">
    <img src="https://img.shields.io/github/v/release/iron-curtain-engine/cnc-formats?label=📥%20Download%20Latest%20Release&style=for-the-badge&color=brightgreen" alt="Download Latest Release">
  </a>
  <br>
  <sub>Pre-built binaries for Windows, macOS, and Linux — or <code>cargo install cnc-formats</code></sub>
</p>

## Status

> **Alpha / pre-1.0** — all format modules are implemented and tested.
> The API may change before 1.0 as the Iron Curtain engine matures.

## Modules

| Module      | Format | Description                                                                     |
| ----------- | ------ | ------------------------------------------------------------------------------- |
| `big`       | `.big` | Flat archive with filenames directly in records                                   |
| `mix`       | `.mix` | Flat archive with CRC-based file lookup                                         |
| `shp`       | `.shp` | Keyframe sprite animation frames                                                |
| `pal`       | `.pal` | 256-color 6-bit VGA palette                                                     |
| `aud`       | `.aud` | Westwood IMA ADPCM audio                                                        |
| `lcw`       | —      | LCW decompression (used by SHP, VQA, WSA)                                       |
| `lut`       | `.lut` | Red Alert Chrono Vortex lookup tables                                           |
| `tmp`       | `.tmp` | Terrain tile sets (TD + RA variants)                                            |
| `vqa`       | `.vqa` | VQ video container (IFF chunk-based)                                            |
| `vqp`       | `.vqp` | Packed VQA palette interpolation tables                                         |
| `wsa`       | `.wsa` | LCW + XOR-delta animation                                                       |
| `fnt`       | `.fnt` | Bitmap fonts (variable character count, 4bpp nibble-packed)                     |
| `eng`       | `.eng` | Westwood language string tables (`.eng`, `.ger`, `.fre`)                        |
| `dip`       | `.dip` | Special effects palette data                                                    |
| `ini`       | `.ini` | Classic C&C rules file parser                                                   |
| `mix_crypt` | —      | Blowfish key derivation for encrypted `.mix` (requires `encrypted-mix` feature) |
| `sniff`     | —      | Content-based format detection (`sniff::sniff_format`)                          |

### Feature-gated modules

| Module       | Format      | Description                                                        |
| ------------ | ----------- | ------------------------------------------------------------------ |
| `miniyaml`   | MiniYAML    | OpenRA configuration file parser (`miniyaml` feature)              |
| `mid`        | `.mid`      | Standard MIDI file parser/writer (`midi` feature)                  |
| `adl`        | `.adl`      | AdLib OPL2 music parser (`adl` feature)                            |
| `xmi`        | `.xmi`      | XMIDI parser + XMI→MID converter (`xmi` feature)                   |
| `transcribe` | WAV→MIDI    | PCM/WAV transcription helpers and MIDI/XMI generation              |
| `meg`        | `.meg/.pgm` | Petroglyph archive parser (`meg` feature)                          |
| `convert`    | PNG/GIF/etc | Import/export codecs re-exported from `cnc_formats::convert::*`    |

## Core Library API

Most modules follow the same pattern: parse from `&[u8]`, inspect the parsed
structure, then optionally run helper conversion or rendering APIs.

### Always available

| Area | Primary APIs |
| ---- | ------------ |
| Error handling | `cnc_formats::Error` re-exported at the crate root |
| Format detection | `sniff::sniff_format(&[u8]) -> Option<&'static str>` |
| MIX / BIG archives | `mix::crc`, `mix::builtin_name_map`, `mix::MixArchive::parse`, `get`, `get_by_crc`, `entries`, `file_count`, `big::BigArchive::parse`, `get`, `get_by_index` |
| AUD / LUT data | `aud::AudFile::parse`, `aud::decode_adpcm`, `aud::encode_adpcm`, `aud::build_aud`, `lut::LutFile::parse` |
| LCW codec | `lcw::decompress`, `lcw::compress` |
| SHP / WSA / TMP | `shp::ShpFile::parse`, `shp::encode_frames`, `wsa::WsaFile::parse`, `wsa::encode_frames`, `tmp::TdTmpFile::parse`, `tmp::RaTmpFile::parse`, `tmp::encode_td_tmp` |
| PAL / FNT / ENG / DIP / INI | `pal::Palette::parse`, `fnt::FntFile::parse`, `eng::EngFile::parse`, `dip::DipFile::parse`, `ini::IniFile::parse` |
| VQA / VQP | `vqa::VqaFile::parse`, `VqaFile::decode_frames`, `VqaFile::extract_audio`, `vqp::VqpFile::parse`, `VqpTable::get` |

### Feature-gated APIs

| Feature | Primary APIs |
| ------- | ------------ |
| `convert` | `convert::shp_frames_to_png`, `png_to_shp`, `aud_to_wav`, `wav_to_aud`, `vqa_to_avi`, `avi_to_vqa`, `decode_avi`, `encode_avi` |
| `miniyaml` | `miniyaml::MiniYamlDoc::parse`, `MiniYamlDoc::parse_str`, `miniyaml::to_yaml` |
| `midi` | `mid::MidFile::parse`, `mid::write`, `mid::load_soundfont`, `mid::render_to_pcm`, `mid::render_to_wav` |
| `adl` | `adl::AdlFile::parse`, `AdlFile::total_register_writes`, `AdlFile::estimated_duration_secs` |
| `xmi` | `xmi::XmiFile::parse`, `XmiFile::sequence_count`, `xmi::to_mid` |
| `transcribe` | `transcribe::TranscribeConfig`, `pcm_to_notes`, `pcm_to_mid`, `notes_to_mid`, `wav_to_mid`, `wav_to_xmi`, `mid_to_xmi` |
| `meg` | `meg::MegArchive::parse`, `get`, `get_by_index`, `entries`, `file_count` |

### CLI tool

The `cncf` binary provides seven subcommands:

```text
cncf validate <file>                                  # Parse and report structural validity
cncf inspect  <file>                                  # Dump metadata (entries, dimensions, etc.)
cncf list     <file>                                  # List archive entries
cncf extract  <file>                                  # Extract archive entries to individual files
cncf convert  <file> --format miniyaml --to yaml      # .yaml is ambiguous — explicit --format
cncf convert  rules.miniyaml --to yaml                # .miniyaml auto-detects
cncf check    <file>                                  # Deep structural integrity verification
cncf fingerprint <file>                               # SHA-256 of raw file bytes
```

`validate` and `inspect` work on all formats.  `list` and `extract` operate
on archive formats: MIX and BIG always, plus MEG/PGM when built with the `meg`
feature. Use `--format <fmt>` when the file extension is ambiguous
(e.g. `--format tmp-ra` for Red Alert terrain, or `--format miniyaml`
for `.yaml` files that are MiniYAML).

`list` displays a tabular inventory of archive entries (CRC, size, and
optionally resolved filenames via `--names <file>` or the built-in unique-CRC
resolver for MIX archives). BIG archives display their stored names directly.

`extract` writes each archive entry to a separate file.  Use `--output <dir>`
to set the destination, `--names <file>` to resolve MIX filenames, and
`--filter <substring>` to extract only matching entries. MEG/PGM archives store
filenames directly.

`convert` requires `--to` (target format).  `--format` overrides auto-detected
source format — `.miniyaml` auto-detects, but `.yaml`/`.yml` always require
explicit `--format miniyaml`.  Requires the `convert` and/or `miniyaml`
feature flags (not enabled by default).

`check` goes beyond parse success to verify internal consistency such as
overlapping archive entry ranges. `fingerprint` prints a `sha256sum`-compatible
hash of the raw file bytes.

Supported conversions (with `convert` feature):
- SHP + PAL ↔ PNG, GIF
- AUD ↔ WAV
- WSA + PAL ↔ PNG, GIF
- TMP + PAL ↔ PNG
- PAL ↔ PNG
- FNT + PAL → PNG
- VQA ↔ AVI

Supported conversions (with `miniyaml` feature):
- MiniYAML → YAML

## Design

This crate is a **clean-room implementation** — no EA-derived code.
All parsing logic is written from publicly available format documentation
and binary analysis. This is what allows the MIT/Apache-2.0 licensing.

For EA GPL-derived parsing (e.g., game-specific rule interpretation),
see the `ra-formats` crate in the [Iron Curtain engine](https://github.com/iron-curtain-engine/iron-curtain).

### Key properties

- **Minimal allocation** — binary format parsers borrow from the input
  `&[u8]` (zero-copy); text parsers (`.ini`, MiniYAML) allocate owned
  strings for transformed keys and values
- **Security hardened** — bounds-checked reads, decompression ratio caps,
  output size limits, fuzz targets for every module
- **Slice-based API** — all parsers take `&[u8]` or `&str`; callers provide
  the bytes directly

## Usage

```rust
use cnc_formats::{mix, pal, shp, sniff};

// Parse a MIX archive from a byte slice
let archive = mix::MixArchive::parse(&mix_data)?;

// Or sniff the file type first when the extension is missing.
let guessed = sniff::sniff_format(&mix_data);
assert_eq!(guessed, Some("mix"));

// Look up a file by name
if let Some(entry_data) = archive.get("palette.pal") {
    let palette = pal::Palette::parse(entry_data)?;
    // Each color is 6-bit VGA (0–63); convert to 8-bit:
    if let Some(first_color) = palette.colors.first() {
        let rgb8 = first_color.to_rgb8();
    }
}

// Parse SHP sprites
let shp_file = shp::ShpFile::parse(&shp_data)?;
if let Some(frame) = shp_file.frames.first() {
    let pixel_count = shp_file.header.width as usize * shp_file.header.height as usize;
    let pixels = frame.pixels(pixel_count)?; // LCW-decompressed pixel data
}
```

Feature-gated examples:

- `midi`: parse and inspect with `mid::MidFile::parse`, then render with
  `mid::load_soundfont` + `mid::render_to_pcm` / `mid::render_to_wav`
- `xmi`: parse with `xmi::XmiFile::parse`, convert to SMF with `xmi::to_mid`
- `transcribe`: build a `transcribe::TranscribeConfig`, then call
  `pcm_to_mid`, `wav_to_mid`, or `wav_to_xmi`
- `meg`: parse Petroglyph archives with `meg::MegArchive::parse`, then use
  `get` for name lookup or `get_by_index` when iterating entries

## Design Documents

Architecture and format specifications are maintained in the
[Iron Curtain Design Documentation](https://github.com/iron-curtain-engine/iron-curtain-design-docs).

Key references:
- [Format specifications](https://iron-curtain-engine.github.io/iron-curtain-design-docs/05-FORMATS.html)
- [D076 — Standalone crate extraction](https://iron-curtain-engine.github.io/iron-curtain-design-docs/decisions/09a/D076-standalone-crates.html)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

Contributions require a Developer Certificate of Origin (DCO) — add `Signed-off-by`
to your commit messages (`git commit -s`).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
