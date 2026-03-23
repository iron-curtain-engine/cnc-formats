---
name: cnc-formats
description: >
  Parse, inspect, convert, and extract Command & Conquer game files using the
  cnc-formats Rust library and cncf CLI tool. Use when working with C&C binary
  formats: .mix archives, .shp sprites, .pal palettes, .aud audio, .vqa video,
  .lut Chrono Vortex tables, .vqp palette tables, .tmp tiles, .wsa animations, .fnt fonts, .eng string tables, .ini rules, .meg Petroglyph archives,
  .adl AdLib music, .xmi XMIDI, and .mid MIDI files.
---

# cnc-formats — Command & Conquer Game File Skill

Parse, inspect, convert, and extract classic Command & Conquer game assets
using the `cnc-formats` Rust library and the `cncf` CLI tool.

Covers Tiberian Dawn, Red Alert 1, Tiberian Sun, Red Alert 2, and
Petroglyph Remastered titles.

## Installation

### CLI tool

```bash
cargo install cnc-formats --version 0.1.0-alpha.3
```

This installs the `cncf` binary with all default format support enabled.

### As a library dependency

```toml
# Cargo.toml — while the crate is prerelease, specify the explicit prerelease version
[dependencies]
cnc-formats = "0.1.0-alpha.3"

# Common feature combinations:
# Parse MIX archives including encrypted RA1/TS files:
cnc-formats = { version = "0.1.0-alpha.3", features = ["encrypted-mix"] }

# Full conversion support (PNG, WAV, AVI, GIF):
cnc-formats = { version = "0.1.0-alpha.3", features = ["convert", "encrypted-mix"] }

# Everything:
cnc-formats = { version = "0.1.0-alpha.3", features = ["convert", "encrypted-mix", "miniyaml", "midi", "adl", "xmi", "transcribe", "meg"] }
```

## Format Reference

| Format   | Extension(s)  | Description                       | Notes                                       |
|----------|---------------|-----------------------------------|---------------------------------------------|
| MIX      | `.mix`        | Flat archive, CRC-hashed entries  | Encrypted variant needs `encrypted-mix` feat |
| SHP      | `.shp`        | Keyframe sprite frames            | LCW-compressed; needs PAL for rendering     |
| PAL      | `.pal`        | 256-color VGA palette             | 6-bit values (0-63); x4 for 8-bit RGB      |
| AUD      | `.aud`        | Westwood IMA ADPCM audio          | SCOMP=99 has chunk headers to strip         |
| LUT      | `.lut`        | Chrono Vortex lookup table        | Red Alert `HOLE0000.LUT`-style assets       |
| VQA      | `.vqa`        | VQ video (IFF chunk container)    | CBP codebook deferred to next frame group   |
| VQP      | `.vqp`        | VQA palette interpolation tables  | Packed lower-triangle lookup tables         |
| TMP      | `.tmp`        | Terrain tiles                     | TD and RA formats are INCOMPATIBLE          |
| WSA      | `.wsa`        | LCW + XOR-delta animation         | Frame 0 keyframe, rest are deltas           |
| FNT      | `.fnt`        | Bitmap font glyphs                | 4bpp nibble-packed, variable char count     |
| ENG      | `.eng`/`.ger`/`.fre` | Westwood string tables     | Language packs share the same offset-table layout |
| INI      | `.ini`        | C&C rules/config files            | Semicolon comments, permissive parsing      |
| MiniYAML | `.miniyaml`   | OpenRA config format              | Feature: `miniyaml`                         |
| MID      | `.mid`        | Standard MIDI file                | Feature: `midi`                             |
| ADL      | `.adl`        | AdLib OPL2 music (Dune II era)    | Feature: `adl`                              |
| XMI      | `.xmi`        | XMIDI (IFF-wrapped MIDI)          | Feature: `xmi`; convertible to Standard MIDI|
| MEG      | `.meg`/`.pgm` | Petroglyph archive (Remastered)   | Feature: `meg`; stores real filenames       |

## cncf CLI Reference

### Subcommands

| Command       | Purpose                                    | Works on          |
|---------------|--------------------------------------------|-------------------|
| `validate`    | Parse and report structural validity       | All formats       |
| `inspect`     | Dump metadata (entries, dimensions, FPS)   | All formats       |
| `list`        | Quick archive entry inventory              | MIX, BIG, MEG/PGM |
| `extract`     | Extract archive entries to files           | MIX, BIG, MEG/PGM |
| `convert`     | Bidirectional format conversion            | See matrix below  |
| `check`       | Deep integrity verification                | All (archives get extra checks) |
| `fingerprint` | SHA-256 hash (sha256sum-compatible)        | Any file          |

### Common Flags

- `--format <fmt>` — override auto-detected format (REQUIRED for `.tmp`)
- `--palette <file>` — palette `.pal` file (REQUIRED for SHP/TMP/WSA/FNT visual export)
- `--output <path>` — output file or directory
- `--mix-access <stream|eager>` — MIX loading policy for `list` and `extract`
- `--names <file>` — filename list for MIX CRC resolution
- `--filter <str>` — extract only matching entries (case-insensitive substring)

### Format Detection Rules

| Extension    | Auto-detected as | Notes                                    |
|--------------|------------------|------------------------------------------|
| `.mix`       | MIX              |                                          |
| `.shp`       | SHP              |                                          |
| `.pal`       | PAL              |                                          |
| `.aud`       | AUD              |                                          |
| `.lut`       | LUT              |                                          |
| `.vqa`       | VQA              |                                          |
| `.vqp`       | VQP              |                                          |
| `.wsa`       | WSA              |                                          |
| `.fnt`       | FNT              |                                          |
| `.eng`/`.ger`/`.fre` | ENG       |                                          |
| `.ini`       | INI              |                                          |
| `.miniyaml`  | MiniYAML         |                                          |
| `.meg`/`.pgm`| MEG              | Requires `meg` feature                   |
| `.mid`       | MIDI             | Requires `midi` feature                  |
| `.adl`       | ADL              | Requires `adl` feature                   |
| `.xmi`       | XMI              | Requires `xmi` feature                   |
| `.avi`       | AVI              | Requires `convert` feature               |
| `.tmp`       | **AMBIGUOUS**    | MUST use `--format tmp` or `--format tmp-ra` |
| `.yaml`/`.yml`| **NOT detected** | Use `--format miniyaml` if it's MiniYAML |

### Conversion Matrix

**Export (C&C -> common format):**

```
cncf convert units.shp   --to png --palette temperat.pal
cncf convert units.shp   --to gif --palette temperat.pal
cncf convert anim.wsa    --to png --palette temperat.pal
cncf convert anim.wsa    --to gif --palette temperat.pal
cncf convert desert.tmp  --to png --palette temperat.pal --format tmp
cncf convert font.fnt    --to png --palette temperat.pal
cncf convert temperat.pal --to png
cncf convert speech.aud  --to wav
cncf convert intro.vqa   --to avi
```

**Import (common format -> C&C):**

```
cncf convert frame_00.png --to shp --palette temperat.pal
cncf convert anim.gif     --to shp --palette temperat.pal
cncf convert frame_00.png --to wsa --palette temperat.pal
cncf convert anim.gif     --to wsa --palette temperat.pal
cncf convert tile_00.png  --to tmp
cncf convert swatch.png   --to pal
cncf convert sound.wav    --to aud
cncf convert video.avi    --to vqa
```

**Text conversion:**

```
cncf convert rules.miniyaml --to yaml
```

### MIX Filename Resolution

MIX archives use CRC hashes instead of filenames. Three resolution sources
(checked in priority order):

1. **`--names <file>`** — user-supplied text file (one filename per line,
   `#` comments allowed)
2. **Embedded XCC database** — `local mix database.dat` entry inside the
   MIX (CRC `0x54C2D545`), placed by XCC Mixer
3. **Built-in resolver** — built-in TD/RA1/RA2 candidate corpus compiled into
   the binary; only unique CRC mappings are kept, collisions are omitted

The MIX index stores `CRC(filename)`, offset, and size. The CRC is a hash of
the filename text, not a checksum of file contents.

Without any resolution, entries are extracted as `{CRC:08X}.bin`.

`--mix-access stream` is the default. It keeps MIX entry payloads on disk until
they are needed. `--mix-access eager` loads the full archive into RAM first.
That choice belongs to the caller's workflow: lower startup memory and less
up-front waiting, or fewer later disk reads.

## Rust API Quick Reference

### Core Pattern: Parse from `&[u8]` or stream from readers

All parsers follow the same pattern — `parse(&[u8])` returns a
`Result<T, cnc_formats::Error>`:

```rust
use cnc_formats::{mix, pal, shp, aud, lut, vqa, vqp, tmp, wsa, fnt, eng, ini, Error};

let archive  = mix::MixArchive::parse(&data)?;
let palette  = pal::Palette::parse(&data)?;
let sprites  = shp::ShpFile::parse(&data)?;
let audio    = aud::AudFile::parse(&data)?;
let vortex   = lut::LutFile::parse(&data)?;
let video    = vqa::VqaFile::parse(&data)?;
let interp   = vqp::VqpFile::parse(&data)?;
let tiles_td = tmp::TdTmpFile::parse(&data)?;
let tiles_ra = tmp::RaTmpFile::parse(&data)?;
let anim     = wsa::WsaFile::parse(&data)?;
let font     = fnt::FntFile::parse(&data)?;
let strings  = eng::EngFile::parse(&data)?;
let config   = ini::IniFile::parse(&data)?;
```

Large containers and classic media also have reader-based incremental APIs so
callers can preroll small buffers instead of holding a whole movie or long
audio decode in memory:

```rust
use cnc_formats::{aud, mix, vqa};

let file = std::fs::File::open("conquer.mix")?;
let mut archive = mix::MixArchiveReader::open(file)?;
if let Some(bytes) = archive.read("RULES.INI")? {
    let ini = cnc_formats::ini::IniFile::parse(&bytes)?;
}

if let Some(mut entry_reader) = archive.open_entry("CONQUER.ENG")? {
    let mut eng_bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry_reader, &mut eng_bytes)?;
    let strings = cnc_formats::eng::EngFile::parse(&eng_bytes)?;
    assert!(strings.string_count() > 0);
}

let file = std::fs::File::open("speech.aud")?;
let mut audio = aud::AudStream::open_seekable(file)?;
let info = audio.media_info();
assert_eq!(info.channels, 1);

let mut pcm = [0i16; 2048];
let read = audio.read_samples(&mut pcm)?;
assert!(read <= pcm.len());

let file = std::fs::File::open("intro.vqa")?;
let mut video = vqa::VqaDecoder::open(file)?;
let info = video.media_info();
assert_eq!(info.fps, 15);
assert_eq!(video.frame_timestamp(0), Some(std::time::Duration::ZERO));

let mut frame = vqa::VqaFrameBuffer::from_media_info(&info);
if let Some(index) = video.next_frame_into(&mut frame)? {
    assert_eq!(index, 0);
}

let mut audio_buf = [0i16; 2048];
let samples = video.read_audio_samples(&mut audio_buf)?;
assert!(samples <= audio_buf.len());

video.seek_to_time(std::time::Duration::from_millis(500))?;
```

### MIX Archive Operations

```rust
use cnc_formats::mix;

let archive = mix::MixArchive::parse(&mix_data)?;

// Lookup by filename (computes CRC internally)
if let Some(data) = archive.get("RULES.INI") {
    let ini = cnc_formats::ini::IniFile::parse(data)?;
}

// Lookup by CRC
let crc = mix::crc("CONQUER.SHP");
if let Some(data) = archive.get_by_crc(crc) { /* ... */ }

// Iterate entries with built-in name resolution
let names = mix::builtin_name_map();
for entry in archive.entries() {
    let name = names.get(&entry.crc)
        .map(|s| s.as_str())
        .unwrap_or("unknown");
    let data = archive.get_by_crc(entry.crc);
    println!("{}: {} bytes", name, entry.size);
}

// Check for embedded XCC filename database
let embedded = archive.embedded_names();

// Build a mounted overlay index once instead of rescanning every archive.
let mut overlay = mix::MixOverlayIndex::new();
overlay.mount_archive("base", archive.entries());
```

### Format Sniffing (unknown files)

```rust
use cnc_formats::sniff;

match sniff::sniff_format(&unknown_bytes) {
    Some("mix") => { /* handle MIX */ }
    Some("shp") => { /* handle SHP */ }
    Some("pal") => { /* handle PAL */ }
    Some("aud") => { /* handle AUD */ }
    Some("vqa") => { /* handle VQA */ }
    Some(other) => { println!("Detected: {other}"); }
    None        => { println!("Unknown format"); }
}
```

### Conversion API (feature: `convert`)

```rust
use cnc_formats::convert;

// Visual exports (need palette for indexed-color formats)
let pngs = convert::shp_frames_to_png(&shp_file, &palette)?;
let wav  = convert::aud_to_wav(&aud_file)?;
let avi  = convert::vqa_to_avi(&vqa_file)?;

// Imports
let aud_file = convert::wav_to_aud(&wav_bytes)?;
let vqa_file = convert::avi_to_vqa(&avi_bytes)?;
let shp_data = convert::png_to_shp(&png_bytes, &palette)?;
```

### Feature-Gated APIs

```rust
// miniyaml feature
use cnc_formats::miniyaml;
let doc = miniyaml::MiniYamlDoc::parse(&data)?;
let yaml_string = miniyaml::to_yaml(&doc);

// midi feature
use cnc_formats::mid;
let midi = mid::MidFile::parse(&data)?;

// xmi feature
use cnc_formats::xmi;
let xmi = xmi::XmiFile::parse(&data)?;
let standard_midi = xmi::to_mid(&xmi, 0)?; // sequence index

// meg feature
use cnc_formats::meg;
let archive = meg::MegArchive::parse(&data)?;
let file_data = archive.get("DATA/ART/UNIT.TGA"); // case-insensitive
// Or iterate by index (preferred for archives with duplicate names)
for (i, entry) in archive.entries().iter().enumerate() {
    let data = archive.get_by_index(i);
    println!("{}: {} bytes", entry.name, entry.size);
}

// transcribe feature (PCM -> MIDI)
use cnc_formats::transcribe::{TranscribeConfig, pcm_to_mid};
let config = TranscribeConfig::default();
let midi_bytes = pcm_to_mid(&pcm_samples, 44100, &config)?;
```

### Error Handling

```rust
use cnc_formats::Error;

match cnc_formats::mix::MixArchive::parse(&data) {
    Ok(archive) => { /* success */ }
    Err(Error::UnexpectedEof { needed, available }) => {
        // File truncated: needed N bytes but only M available
    }
    Err(Error::InvalidMagic { context }) => {
        // Wrong file format or corrupted header
    }
    Err(Error::InvalidSize { value, limit, context }) => {
        // V38 security cap exceeded (e.g., entry count too large)
    }
    Err(Error::InvalidOffset { offset, bound }) => {
        // Offset points outside file bounds
    }
    Err(e) => {
        // Display impl includes all diagnostic values
        eprintln!("Parse error: {e}");
    }
}
```

## Critical Gotchas

These are the most common pitfalls when working with C&C formats:

1. **`.tmp` is always ambiguous.** Tiberian Dawn and Red Alert use
   incompatible tile formats with the same extension. Always specify
   `--format tmp` (TD) or `--format tmp-ra` (RA) in CLI. In code, use
   `TdTmpFile::parse` or `RaTmpFile::parse` explicitly.

2. **Palette is required for visual exports.** SHP, TMP, WSA, and FNT are
   indexed-color. You need a `.pal` file to render them. Common palettes:
   `temperat.pal`, `snow.pal`, `desert.pal`, `interior.pal`.

3. **Encrypted MIX needs the `encrypted-mix` feature.** RA1 and Tiberian
   Sun use Blowfish+RSA encryption on some MIX archives. Without this
   feature, the parser sees `count=0` and treats it as extended format,
   returning incorrect results.

4. **AUD SCOMP=99 has chunk headers.** RA1 IMA ADPCM voice files wrap
   every ~512 bytes of ADPCM data in 8-byte headers (magic `0xDEAF`). The
   `convert` module strips these automatically, but custom ADPCM decoders
   must handle them or the audio will be garbled.

5. **VQA codebook timing matters.** Partial codebook (CBP) chunks
   accumulate over `groupsize` frames. The completed codebook takes effect
   on the NEXT frame group. Applying it immediately causes a visible flash.

6. **LCW copies from unwritten positions return zero.** The original EA
   engine pre-zeroes the output buffer. Absolute and relative copies that
   reference positions not yet written produce zeros, not errors.

7. **MEG archives store real filenames.** Unlike MIX (CRC hashes), MEG
   entries have actual filenames. The `--names` flag is ignored for MEG
   archives.

8. **MIX CRC is case-insensitive.** The Westwood CRC algorithm uppercases
   filenames before hashing. `mix::crc("rules.ini")` and
   `mix::crc("RULES.INI")` produce the same hash.

## Common Workflows

### Extract and identify files from a MIX archive

```bash
# Extract with filename resolution
cncf extract CONQUER.MIX --output ./extracted/

# The tool auto-detects embedded XCC databases and falls back to
# the built-in TD/RA1/RA2 filename database. Files are named by
# their resolved filename where possible, CRC hex otherwise.
```

### Batch convert all SHP sprites to PNG

```bash
# Extract sprites from archive
cncf extract LOCAL.MIX --output ./sprites/ --filter .shp

# Convert each one (needs a palette from the same game)
for f in ./sprites/*.shp; do
    cncf convert "$f" --to png --palette temperat.pal
done
```

### Parse a nested MIX-inside-MIX structure

```rust
use cnc_formats::mix::MixArchive;

let outer = MixArchive::parse(&outer_data)?;
if let Some(inner_data) = outer.get("LOCAL.MIX") {
    let inner = MixArchive::parse(inner_data)?;
    if let Some(rules) = inner.get("RULES.INI") {
        let ini = cnc_formats::ini::IniFile::parse(rules)?;
    }
}
```

### Identify an unknown extracted file

```rust
use cnc_formats::sniff;

let data = std::fs::read("unknown.bin")?;
match sniff::sniff_format(&data) {
    Some(fmt) => println!("Detected format: {fmt}"),
    None => println!("Unknown format — try manual inspection"),
}
```

## Building and Testing (for contributors)

```bash
# Build the CLI with all features
cargo build --features cli

# Run all tests
cargo test

# Run tests for a specific module
cargo test --test integration mix

# Run with all features
cargo test --all-features

# Lint
cargo clippy --tests -- -D warnings

# Full local CI (mirrors GitHub Actions)
bash ci-local.sh      # or ./ci-local.ps1 on Windows
```
