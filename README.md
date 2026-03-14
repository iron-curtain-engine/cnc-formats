# cnc-formats

[![CI](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/ci.yml/badge.svg)](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/ci.yml)
[![Fuzz](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/fuzz.yml/badge.svg)](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/fuzz.yml)
[![Security Audit](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/audit.yml/badge.svg)](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/audit.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![No GPL](https://img.shields.io/badge/no_GPL_deps-enforced-brightgreen.svg)](deny.toml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

Clean-room binary format parsers for Command & Conquer game files.

Parses `.mix` archives, `.shp` sprites, `.pal` palettes, `.aud` audio,
`.vqa` video, `.tmp` terrain tiles, `.wsa` animations, `.fnt` bitmap fonts,
`.ini` rules files, and LCW-compressed data used by Red Alert, Tiberian Dawn,
and related C&C titles. Optional feature flags add MiniYAML, MIDI, ADL,
XMIDI, PCM-to-MIDI transcription, and Petroglyph MEG/PGM archive support.

## Status

> **Pre-1.0** — all format modules are implemented and tested.
> The API may change before 1.0 as the Iron Curtain engine matures.

## Modules

| Module      | Format | Description                                                                     |
| ----------- | ------ | ------------------------------------------------------------------------------- |
| `mix`       | `.mix` | Flat archive with CRC-based file lookup                                         |
| `shp`       | `.shp` | Keyframe sprite animation frames                                                |
| `pal`       | `.pal` | 256-color 6-bit VGA palette                                                     |
| `aud`       | `.aud` | Westwood IMA ADPCM audio                                                        |
| `lcw`       | —      | LCW decompression (used by SHP, VQA, WSA)                                       |
| `tmp`       | `.tmp` | Terrain tile sets (TD + RA variants)                                            |
| `vqa`       | `.vqa` | VQ video container (IFF chunk-based)                                            |
| `wsa`       | `.wsa` | LCW + XOR-delta animation                                                       |
| `fnt`       | `.fnt` | Bitmap fonts (variable character count, 4bpp nibble-packed)                     |
| `ini`       | `.ini` | Classic C&C rules file parser                                                   |
| `mix_crypt` | —      | Blowfish key derivation for encrypted `.mix` (requires `encrypted-mix` feature) |

### Feature-gated modules

| Module       | Format      | Description                                                        |
| ------------ | ----------- | ------------------------------------------------------------------ |
| `miniyaml`   | MiniYAML    | OpenRA configuration file parser (`miniyaml` feature)              |
| `mid`        | `.mid`      | Standard MIDI file parser/writer (`midi` feature)                  |
| `adl`        | `.adl`      | AdLib OPL2 music parser (`adl` feature)                            |
| `xmi`        | `.xmi`      | XMIDI parser + XMI→MID converter (`xmi` feature)                   |
| `transcribe` | WAV→MIDI    | PCM audio transcription pipeline (`transcribe` feature)            |
| `meg`        | `.meg/.pgm` | Petroglyph archive parser (`meg` feature)                          |
| `convert`    | PNG/GIF/etc | Import/export codecs and AVI container support (`convert` feature) |

### CLI tool

The `cnc-formats` binary provides seven subcommands:

```text
cnc-formats validate <file>                                  # Parse and report structural validity
cnc-formats inspect  <file>                                  # Dump metadata (entries, dimensions, etc.)
cnc-formats list     <file>                                  # List archive entries
cnc-formats extract  <file>                                  # Extract archive entries to individual files
cnc-formats convert  <file> --format miniyaml --to yaml      # .yaml is ambiguous — explicit --format
cnc-formats convert  rules.miniyaml --to yaml                # .miniyaml auto-detects
cnc-formats check    <file>                                  # Deep structural integrity verification
cnc-formats fingerprint <file>                               # SHA-256 of raw file bytes
```

`validate` and `inspect` work on all formats.  `list` and `extract` operate
on archive formats: MIX always, plus MEG/PGM when built with the `meg`
feature. Use `--format <fmt>` when the file extension is ambiguous
(e.g. `--format tmp-ra` for Red Alert terrain, or `--format miniyaml`
for `.yaml` files that are MiniYAML).

`list` displays a tabular inventory of archive entries (CRC, size, and
optionally resolved filenames via `--names <file>` for MIX archives).

`extract` writes each archive entry to a separate file.  Use `--output <dir>`
to set the destination, `--names <file>` to resolve MIX filenames, and
`--filter <substring>` to extract only matching entries. MEG/PGM archives
store filenames directly.

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
use cnc_formats::{mix, pal, shp};

// Parse a MIX archive from a byte slice
let archive = mix::MixArchive::parse(&mix_data)?;

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
