# cnc-formats

[![CI](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/ci.yml/badge.svg)](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/ci.yml)
[![Fuzz](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/fuzz.yml/badge.svg)](https://github.com/iron-curtain-engine/cnc-formats/actions/workflows/fuzz.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![No GPL](https://img.shields.io/badge/no_GPL_deps-enforced-brightgreen.svg)](deny.toml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

Clean-room binary format parsers for Command & Conquer game files.

Parses `.mix` archives, `.shp` sprites, `.pal` palettes, `.aud` audio,
`.vqa` video, `.tmp` terrain tiles, `.wsa` animations, `.fnt` bitmap fonts,
`.ini` rules files, and LCW-compressed data used by Red Alert, Tiberian Dawn,
and related C&C titles.  Optional MiniYAML support for OpenRA mod files.

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
| `lcw`       | —      | LCW decompression (used by SHP, VQA, TMP, WSA)                                  |
| `tmp`       | `.tmp` | Terrain tile sets (TD + RA variants)                                            |
| `vqa`       | `.vqa` | VQ video container (IFF chunk-based)                                            |
| `wsa`       | `.wsa` | LCW + XOR-delta animation                                                       |
| `fnt`       | `.fnt` | Bitmap fonts (256-glyph fixed-height)                                           |
| `ini`       | `.ini` | Classic C&C rules file parser                                                   |
| `mix_crypt` | —      | Blowfish key derivation for encrypted `.mix` (requires `encrypted-mix` feature) |

### Feature-gated modules

| Module     | Format   | Description                                                    |
| ---------- | -------- | -------------------------------------------------------------- |
| `miniyaml` | MiniYAML | OpenRA configuration file parser (requires `miniyaml` feature) |

The `miniyaml2yaml` CLI tool (behind `miniyaml` feature) converts MiniYAML
files to standard YAML.

## Design

This crate is a **clean-room implementation** — no EA-derived code.
All parsing logic is written from publicly available format documentation
and binary analysis. This is what allows the MIT/Apache-2.0 licensing.

For EA GPL-derived parsing (e.g., game-specific rule interpretation),
see the `ra-formats` crate in the [Iron Curtain engine](https://github.com/iron-curtain-engine/iron-curtain).

### Key properties

- **Zero-copy parsing** — parsed structures borrow from the input `&[u8]`
- **Security hardened** — bounds-checked reads, decompression ratio caps,
  output size limits, fuzz targets for every module

## Usage

```rust
use cnc_formats::{mix, pal, shp};

// Parse a MIX archive from a byte slice
let archive = mix::MixArchive::parse(&mix_data)?;

// Look up a file by name
if let Some(entry_data) = archive.get("palette.pal") {
    let palette = pal::Palette::parse(entry_data)?;
    // Each color is 6-bit VGA (0–63); convert to 8-bit:
    let rgb8 = palette.colors[0].to_rgb8();
}

// Parse SHP sprites
let shp_file = shp::ShpFile::parse(&shp_data)?;
let frame = &shp_file.frames[0];
let pixel_count = shp_file.header.width as usize * shp_file.header.height as usize;
let pixels = frame.pixels(pixel_count)?; // LCW-decompressed pixel data
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
