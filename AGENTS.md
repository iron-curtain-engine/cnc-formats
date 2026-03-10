# AGENTS.md — cnc-formats

> Local implementation rules for the `cnc-formats` crate.
> Canonical design authority lives in the Iron Curtain design-doc repository.

## Canonical Design Authority (Do Not Override Locally)

- Design docs repo: `https://github.com/iron-curtain-engine/iron-curtain-design-docs`
- Design-doc baseline revision: `HEAD`

Primary canonical references:

- `src/05-FORMATS.md` — file format specifications
- `src/formats/binary-codecs.md` — struct definitions, codec specs, EA source insights
- `src/decisions/09a/D076-standalone-crates.md` — standalone crate extraction strategy
- `src/18-PROJECT-TRACKER.md` — execution overlay, milestone ordering

## Non-Negotiable Rule: No Silent Design Divergence

If implementation reveals a missing detail, contradiction, or infeasible design path:

- do **not** silently invent a new canonical behavior
- open a design-gap/design-change request in the design-doc repo
- mark local work as `implementation placeholder` or `blocked on Pxxx`

## Engine Architecture Context

Iron Curtain enforces ten non-negotiable invariants. The ones that directly
govern this crate are:

- **Invariant 8 — Full resource compatibility:** the engine must load `.mix`,
  `.shp`, `.pal`, `.aud`, and `.oramap` formats. `cnc-formats` is the
  permissive-licensed half of this responsibility; `ra-formats` (GPL v3)
  covers the EA-derived half.
- **Invariant 10 — Platform-agnostic design:** parsers must not assume a
  filesystem, OS threading model, or allocator. Prefer `&[u8]` input and
  `#![no_std]` + `alloc` output over `std::fs`-coupled APIs.

The other eight invariants (determinism, networking, modding, Bevy, YAML,
OpenRA compat, game-agnostic core, performance) are engine-layer concerns
and do not impose direct obligations on this crate.

## Critical Rules for This Crate

### 1. Clean-Room Only — No EA-Derived Code

This crate is licensed under MIT OR Apache-2.0. It must **never** contain code
derived from EA's GPL-licensed C&C source code releases. All parsing logic must
be implemented from:

- Publicly available format documentation
- Binary analysis of game files
- Clean-room reverse engineering

EA-derived parsing logic belongs in `ra-formats` (GPL v3) in the main engine repo.

### 2. No IC Dependencies

This crate must **never** depend on any `ic-*` crate. It is a standalone library
usable by any project regardless of license.

### 3. `#![no_std]` Where Possible

Maximize portability. Use `#![no_std]` with optional `alloc` feature for
heap-dependent functionality.

### 4. No GPL Dependencies

`cargo deny check licenses` must pass. The `deny.toml` rejects GPL dependencies.

### 5. Parser Security (V38)

All decompressors and format parsers must enforce:

- **Decompression ratio cap:** reject output exceeding `256 × compressed_size`
- **Absolute output size limit:** honour the `max_output` / `uncompressed_size`
  field from the file header; never allocate unbounded buffers
- **Loop iteration guard:** decompression loops must make forward progress on
  every iteration (no infinite spin on malformed input)
- **Offset bounds check:** every absolute or relative copy must be validated
  against the current output length before reading
- **`cargo-fuzz` target:** each format module (`.mix`, `.shp`, `.pal`, `.aud`,
  `.lcw`) must have a corresponding fuzz target (tracked in `fuzz/`)

These requirements implement V38 from `src/06-SECURITY.md`.

## Legal & Affiliation Boundaries

- Iron Curtain is **not** affiliated with Electronic Arts.
- This crate ships **zero** copyrighted EA content. It is a parser library only.
- Users supply their own legally-obtained game assets; the crate never bundles,
  redistributes, or downloads them.
- Theme art, test fixtures, and documentation must use original or
  permissively-licensed material only.

## Local Rules

- **Language:** Rust (2021 edition)
- **Build:** `cargo build`
- **Test:** `cargo test`
- **Lint:** `cargo clippy --tests -- -D warnings`
- **Format:** `cargo fmt --check`
- **License check:** `cargo deny check licenses`

## Current Implementation Target

- Active milestone: `M1`
- Active `G*` steps: `G1` (RA asset parsing) — **complete**
- Current blockers: none known

## Execution Overlay Mapping

- **Milestone:** `M1` (Resource Fidelity)
- **Priority:** `P-Core`
- **Feature Cluster:** D076 Tier 1
- **Depends on:** none (standalone from inception)
