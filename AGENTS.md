# AGENTS.md — cnc-formats

> Local implementation rules for the `cnc-formats` crate.
> Canonical design authority lives in the Iron Curtain design-doc repository.

## Canonical Design Authority (Do Not Override Locally)

- Design docs repo: `https://github.com/iron-curtain-engine/iron-curtain-design-docs`
- Design-doc baseline revision: `HEAD`

Primary canonical references:

- `src/05-FORMATS.md` — file format specifications
- `src/decisions/09a/D076-standalone-crates.md` — standalone crate extraction strategy
- `src/18-PROJECT-TRACKER.md` — execution overlay, milestone ordering

## Non-Negotiable Rule: No Silent Design Divergence

If implementation reveals a missing detail, contradiction, or infeasible design path:

- do **not** silently invent a new canonical behavior
- open a design-gap/design-change request in the design-doc repo
- mark local work as `implementation placeholder` or `blocked on Pxxx`

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

## Local Rules

- **Language:** Rust (2021 edition)
- **Build:** `cargo build`
- **Test:** `cargo test`
- **Lint:** `cargo clippy -- -D warnings`
- **Format:** `cargo fmt --check`
- **License check:** `cargo deny check licenses`

## Current Implementation Target

- Active milestone: `M1`
- Active `G*` steps: `G1` (RA asset parsing)
- Current blockers: none known

## Execution Overlay Mapping

- **Milestone:** `M1` (Resource Fidelity)
- **Priority:** `P-Core`
- **Feature Cluster:** D076 Tier 1
- **Depends on:** none (standalone from inception)
