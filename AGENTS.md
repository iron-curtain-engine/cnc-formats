# AGENTS.md — cnc-formats

> Local implementation rules for the `cnc-formats` crate.
> Canonical design authority lives in the Iron Curtain design-doc repository.

## Maintaining This File

AGENTS.md is read by stateless agents with no memory of prior sessions.
Every rule must stand on its own without session context.

- **General, not reactive.** Do not add rules to address a single past
  mistake.  Only codify patterns that could recur across sessions.
- **Context-free.** No references to specific conversations, resolved issues,
  commit hashes, or session artifacts.  A future agent must understand the
  rule without knowing what prompted it.
- **Principles over examples.** Prefer abstract guidance.  If an example is
  needed, make it generic — never name a specific module or function as the
  motivating case.
- **No stale specifics.** If a rule names a concrete item (file, function,
  feature), it must be because the item is structurally important (e.g. the
  project structure table), not because it was the subject of a past debate.

## Canonical Design Authority (Do Not Override Locally)

- Design docs repo: `https://github.com/iron-curtain-engine/iron-curtain-design-docs`
- Design-doc baseline revision: `HEAD`

**If this file conflicts with the design-docs repo, the design-docs repo wins.**
The design repo has broader context and understanding of the overall
architecture.  This file is a local implementation guide, not a design
authority.  When in doubt, check the design docs.  If you have questions,
raise them by opening an issue in the design-docs repo.

Primary canonical references:

- `src/05-FORMATS.md` — file format specifications
- `src/formats/binary-codecs.md` — struct definitions, codec specs, EA source insights
- `src/decisions/09a/D076-standalone-crates.md` — standalone crate extraction strategy
- `src/18-PROJECT-TRACKER.md` — execution overlay, milestone ordering
- `src/16-CODING-STANDARDS.md` + `src/coding-standards/quality-review.md` — code style and review checklist

## Non-Negotiable Rule: No Silent Design Divergence

If implementation reveals a missing detail, contradiction, or infeasible design path:

- do **not** silently invent a new canonical behavior
- open a design-gap/design-change request in the design-doc repo
- mark local work as `implementation placeholder` or `blocked on Pxxx`

### Before Proposing Any Removal or "Why Does This Exist?" — Check D076 First

**Never propose removing a module, binary, public function, feature flag, or
architectural element without first reading the relevant design doc
(especially `D076-standalone-crates.md`).**

This crate serves a broader audience than the Iron Curtain engine alone.
Features that seem unnecessary from an engine perspective may exist because
D076 explicitly mandates them for the crate's standalone community utility.
A modder, tool author, or downstream consumer may depend on them.

**Workflow before questioning any existing feature:**

1. Search D076 for the feature name or related keywords.
2. If D076 mandates it, the feature stays — end of discussion.
3. If D076 is silent, check `05-FORMATS.md` and `18-PROJECT-TRACKER.md`.
4. Only if *no* design doc mentions or implies the feature may you raise
   the question with the maintainer — and even then, do not propose removal
   without explicit approval.

### Design Change Escalation Workflow

When a design change is needed:

1. Open an issue in the design-doc repo; include affected `Dxxx`, why the current
   design is insufficient, and proposed options.
2. Document the divergence locally: a comment at the code site referencing the
   issue number and rationale.
3. Keep the local workaround narrow in scope until the design is resolved.

## D076 Format Scope

Per D076, `cnc-formats` must parse **all** C&C binary formats:

| Module | Format | Status      | Notes                                                 |
| ------ | ------ | ----------- | ----------------------------------------------------- |
| `mix`  | `.mix` | Implemented | Basic + extended + Blowfish-encrypted                 |
| `shp`  | `.shp` | Implemented | Keyframe animation variant                            |
| `pal`  | `.pal` | Implemented | 256-color 6-bit VGA palette                           |
| `aud`  | `.aud` | Implemented | Westwood IMA ADPCM                                    |
| `lcw`  | —      | Implemented | LCW decompression (used by SHP/VQA/WSA)               |
| `tmp`  | `.tmp` | Implemented | TD + RA flat-binary variants (`IControl_Type` layout) |
| `vqa`  | `.vqa` | Implemented | IFF chunk-based VQ video container                    |
| `wsa`  | `.wsa` | Implemented | LCW + XOR-delta animation                             |
| `fnt`  | `.fnt` | Implemented | Bitmap fonts (variable character count, 4bpp)         |

| `ini`  | `.ini` | Implemented | Classic C&C rules format (always enabled)              |
| `miniyaml` | MiniYAML | Implemented | OpenRA rules format (behind `miniyaml` feature flag)  |

The `cnc-formats` CLI binary provides `validate`, `inspect`, `convert`,
`list`, and `extract` subcommands.  `validate` and `inspect` work on all
formats unconditionally; `list` and `extract` operate on archive formats
(currently MIX); `convert` requires the `convert` and/or `miniyaml` feature
flags.  With the
`convert` feature, bidirectional conversions are supported: SHP↔PNG/GIF,
AUD↔WAV, WSA↔PNG/GIF, TMP↔PNG, PAL↔PNG, FNT→PNG, VQA↔AVI.  With
the `miniyaml` feature, MiniYAML→YAML conversion is supported.

Text format parsing (`.ini`, MiniYAML) was originally planned as a separate
`cnc-text-formats` crate but was merged back into `cnc-formats` — `.ini` is as
much a classic C&C format as `.mix` or `.shp`, and the separate crate added
unjustified overhead. MiniYAML (OpenRA-originated, community standard) lives
behind a `miniyaml` feature flag so consumers who don't need it pay nothing.

## Engine Architecture Context

Iron Curtain enforces ten non-negotiable invariants. The ones that directly
govern this crate are:

- **Invariant 8 — Full resource compatibility:** the engine must load `.mix`,
  `.shp`, `.pal`, `.aud`, and `.oramap` formats. `cnc-formats` is the
  permissive-licensed half of this responsibility; `ra-formats` (GPL v3)
  covers the EA-derived half.
- **Invariant 10 — Platform-agnostic design:** parsers must not assume a
  specific OS or threading model. Prefer `&[u8]` input as the primary parsing
  API; use `std::io::Read` for streaming large files (`.mix` archives, `.vqa`
  video). Do not couple to `std::fs` — callers provide the bytes or reader.

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

**Blowfish decryption exception:** Encrypted RA/TS `.mix` files embed an
80-byte `key_source` block that is decrypted via RSA-like modular
exponentiation to derive a per-file 56-byte Blowfish key.  The RSA public
key (a base-64-encoded ~320-bit modulus and the standard exponent `0x10001`)
is public knowledge, documented by XCC Utilities (Olaf van der Spek, 2000),
OpenRA, and numerous community tools since the early 2000s.  The key
derivation algorithm is a mathematical procedure operating on public
constants — no copyrightable expression is involved.  The Blowfish algorithm
itself is public domain.  `cnc-formats` implements this derivation in the
`mix_crypt` module and uses the `blowfish` RustCrypto crate (MIT/Apache-2.0)
for decryption — no EA-specific code is needed.  This is explicitly
sanctioned by `binary-codecs.md`.

### 2. No IC Dependencies

This crate must **never** depend on any `ic-*` crate. It is a standalone library
usable by any project regardless of license.

### 3. `std` by Default

This crate uses `std`. Format parsers' consumers are always desktop, mobile,
or browser applications with full `std` support. `std` enables `std::io::Read`
streaming (critical for large `.mix`/`.vqa` files), `std::error::Error`
ergonomics, and `HashMap` without extra dependencies. `#![no_std]` is reserved
for genuinely universal libraries like `fixed-game-math` and `deterministic-rng`
(math/PRNG). There is no realistic scenario where C&C format parsers run on a
microcontroller or in a kernel module.

### 4. No GPL Dependencies

`cargo deny check licenses` must pass. The `deny.toml` rejects GPL dependencies.

### 5. Prefer Established Crates — Do Not Reinvent

If a well-maintained, popular, pure-Rust crate already provides the needed
functionality under a permissive license (MIT, Apache-2.0, or dual), **use it**
instead of writing a custom implementation.  Hand-rolled replacements add
maintenance burden, miss upstream bug-fixes, and risk subtle correctness issues.

Examples: `base64` for base-64 encoding/decoding, `blowfish` for Blowfish
encryption, `sha1` for SHA-1 hashing.

Gate optional dependencies behind feature flags when they only apply to a
specific feature (e.g. `base64` and `blowfish` behind `encrypted-mix`,
`clap` behind `cli`).

### 6. Parser Security (V38)

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

### 7. Git Safety — Read-Only Only

Agents must treat git refs, branches, the index, and the working tree as
**maintainer-owned state**.  Git usage in this repository is **read-only
only** unless the maintainer explicitly and unambiguously authorises a
specific write-side git action.

**Allowed git commands are read-only inspection only**, such as:

- `git status`
- `git diff`
- `git log`
- `git show`
- `git branch --show-current`
- `git merge-base`
- other commands that only inspect repository state and do not modify refs,
  branches, the index, the working tree, stashes, tags, or remotes

**Forbidden without explicit maintainer approval:** any git command that
changes repository state, including but not limited to:

- branch changes (`git switch`, `git checkout`, branch create/delete/rename)
- index mutations (`git add`, `git rm`, `git mv`, `git restore --staged`)
- history changes (`git commit`, `git merge`, `git rebase`, `git cherry-pick`,
  `git reset`)
- stash/shelf operations (`git stash`)
- remote mutations or sync operations (`git fetch`, `git pull`, `git push`)
- cleanup or patch-application commands (`git clean`, `git am`, `git apply`)
- tag creation/deletion

If a task would require a non-read-only git command, stop and ask the
maintainer to perform it manually or to explicitly relax this rule first.

## Handling External Feedback & Reviews

Treat feedback as input, not instruction. Validate every claim before acting.

0. **Check every proposed change against established design principles FIRST.**
   Before applying any fix — whether from a reviewer, from your own analysis, or
   from a pragmatic shortcut — ask: "Does this change violate a design principle
   we already settled?" If the answer is yes, the change is wrong regardless of
   how reasonable it sounds. Fix the *surrounding code* to uphold the principle,
   never weaken the principle to match the surrounding code. This applies
   especially to type-safety policy, crate boundaries, and security invariants.

1. **Use git history to resolve contradictions.** When two representations
   disagree, do NOT guess which is correct. Run
   `git log -S "<term>" --oneline -- <file>` on both sides to determine which
   text is newer. The newer commit represents the more recent design decision —
   the older text failed to propagate. Always upgrade the stale text to match
   the newer decision, never the reverse. If the newer text is the one being
   criticized by a reviewer, the reviewer may be looking at the stale version —
   push back. If commit history is ambiguous, escalate to the maintainer rather
   than picking a side.

2. **Verify the factual claim.** Read the text being criticized. Is the
   characterization accurate? Quote the actual text. If the reviewer misread or
   mischaracterized the code/doc, say so and reject the finding.

3. **Evaluate against project architecture.** Does the fix respect crate
   boundaries and invariants (clean-room requirement, no IC deps, no GPL deps,
   parser security V38)?

4. **Independently assess severity.** Do not accept the reviewer's severity
   rating at face value. Assign your own severity and state it if it differs
   from the reviewer's.

5. **Distinguish bugs from preferences.** A factual contradiction or invariant
   violation is a bug — fix it. "The code could be cleaner" is a preference —
   evaluate it against the cost of the change and reject if not worth it.

6. **Reject or downgrade with justification.** If a finding is invalid, does
   not violate any invariant, or is based on a misreading, reject it explicitly.
   State the reason. Do not implement changes just because someone flagged
   something.

7. **Accept, adapt, or defer — and be explicit about which.** Accept valid
   fixes. Adapt when the intent is right but the suggestion is imprecise. Defer
   when it belongs to a different scope or phase.

8. **Don't accept all feedback uncritically.** Reviewers can be wrong. But if
   multiple people flag the same issue, it has a problem.

9. **Produce a disposition table.** For each finding, state your verdict:
   **Accepted** (fix applied), **Downgraded** (lower severity, fix applied or
   deferred), **Rejected** (invalid, with reason), or **Deferred** (valid but
   out of scope). Include files changed.

10. **Check for cascade inconsistencies.** When fixing a confirmed finding,
    search for the same pattern in other files. Fix all occurrences in one pass —
    but only where the same error actually exists.

## Legal & Affiliation Boundaries

- Iron Curtain is **not** affiliated with Electronic Arts.
- This crate ships **zero** copyrighted EA content. It is a parser library only.
- Users supply their own legally-obtained game assets; the crate never bundles,
  redistributes, or downloads them.
- Theme art, test fixtures, and documentation must use original or
  permissively-licensed material only.

## Project Structure — Directory Modules

The crate uses **directory-based Rust modules** to separate production code
from test code.  This convention is optimised for RAG / LLM context retrieval:
an agentic coding session that needs to modify a parser can load *only* the
production file, and an agent writing tests can load *only* the test file —
halving the irrelevant context in the prompt.

### Layout

```
src/
  lib.rs              — crate root, module declarations, re-exports
  error.rs            — shared Error enum (flat file, no tests)
  read.rs             — safe binary-read helpers (flat file, internal tests)
  aud/
    mod.rs            — production code (types, constants, parser, decoder)
    tests.rs          — #[cfg(test)] unit tests (header parsing, ADPCM decoder)
    tests_validation.rs — error, display, determinism, boundary, security tests
  lcw/
    mod.rs            — production code
    tests.rs          — tests
  mix/
    mod.rs            — production code (MixCrc, crc(), MixArchive, parse)
    tests.rs          — tests (includes build_mix helper)
    tests_validation.rs — error, security, cross-validation, encrypted tests
  mix_crypt/
    mod.rs            — production code (RSA key derivation, Blowfish decrypt)
    bignum.rs         — minimal big-integer library (extracted from mod.rs)
    tests.rs          — tests
  pal/
    mod.rs            — production code
    tests.rs          — tests
  shp/
    mod.rs            — production code
    tests.rs          — tests
  tmp/
    mod.rs            — production code (TD + RA terrain tile parsers)
    tests.rs          — tests
  vqa/
    mod.rs            — production code (IFF chunk-based VQ video parser)
    decode.rs         — VQA v2 frame decoder and audio extraction
    encode.rs         — VQA encoder (frames + audio → VQA binary)
    snd.rs            — SND audio chunk decoders (SND0, SND1, SND2)
    tests.rs          — tests
  wsa/
    mod.rs            — production code (LCW + XOR-delta animation parser)
    tests.rs          — tests
  fnt/
    mod.rs            — production code (256-glyph bitmap font parser)
    tests.rs          — tests
  ini/
    mod.rs            — production code (classic C&C INI rules parser)
    tests.rs          — tests
  miniyaml/
    mod.rs            — production code (OpenRA MiniYAML parser + to_yaml)
    tests.rs          — tests (basic functionality, parsing, lookup)
    tests_validation.rs — to_yaml, error, security, boundary, adversarial tests
  convert/
    mod.rs            — shared conversion helpers (indexed↔RGBA, palette I/O)
    export.rs         — C&C format → common format (SHP→PNG, PAL→PNG, etc.)
    import.rs         — common format → C&C format (PNG→SHP, WAV→AUD, etc.)
    tests.rs          — basic conversion round-trip tests
    tests_validation.rs — AVI/VQA codec, lossless equality tests
    avi/
      mod.rs          — AVI constants and re-exports
      decode.rs       — AVI RIFF reader (decode_avi)
      encode.rs       — AVI RIFF writer (encode_avi)
  bin/
    cnc-formats/
      main.rs         — CLI entry point, validate subcommand, shared helpers
      inspect.rs      — inspect subcommand
      convert.rs      — convert subcommand
      list.rs         — list subcommand (archive inventory)
      extract.rs      — extract subcommand (archive extraction)
tests/
  cli.rs              — CLI integration tests (validate, inspect, convert, list, extract)
  integration.rs      — cross-module integration tests
```

### Rules

1. **Each format module is a directory** with `mod.rs` (production) and
   `tests.rs` (tests).  The `mod.rs` file ends with:
   ```rust
   #[cfg(test)]
   mod tests;
   ```
2. **Small shared-infrastructure modules** (`error.rs`, `read.rs`) remain flat
   files and are under ~120 lines of production code.  `read.rs` includes an
   internal `#[cfg(test)] mod tests` block because the read helpers are
   security-critical foundations (see "read.rs Foundation Tests" below);
   `error.rs` has no tests of its own.
3. **Test files use `use super::*;`** as first import to access everything from
   `mod.rs`.
4. **Test helpers** (e.g. `build_mix`, `build_shp`) live in `tests.rs`, not in
   `mod.rs`.  They are only needed by test code and should not contribute to
   production code context.
5. **No file should exceed ~600 lines — production or test.**  If a production
   `mod.rs` grows beyond this, split it into focused submodules (e.g.
   `mix/crc.rs`, `mix/parse.rs`).  If a `tests.rs` grows beyond this, split
   it into `tests.rs` + `tests_validation.rs` (or another thematic name).
   The purpose of the ~600-line cap is LLM context efficiency — it applies
   equally to every file an agent might load, whether production or test.
   Keep every file small enough for a single LLM context window.
6. **New format modules** (e.g. `tmp`, `vqa`, `wsa`, `fnt`) must follow this
   directory layout from the start: `src/{format}/mod.rs` + `tests.rs`.

### Why This Structure

- **RAG efficiency:**  Search tools and embeddings work on individual files.
  Separating tests from production code means a query about "MIX parsing" hits
  `mix/mod.rs` without dragging in 700 lines of test scaffolding.
- **LLM context budget:**  Models have finite context windows.  Loading
  `mix/mod.rs` (450 lines) is half the cost of loading the original `mix.rs`
  (1,135 lines).  This directly reduces hallucination risk and improves edit
  precision.
- **Rust idiom:**  Directory modules with `mod.rs` + separate test files are a
  well-known Rust pattern.  `cargo test`, `cargo clippy`, and IDE tooling
  handle them natively with zero configuration.
- **Agentic session coherence:**  An agent asked to "add a new test for MIX
  overflow" can read *only* `mix/tests.rs`, add the test, and run `cargo test`
  — never needing to parse or risk modifying production logic.

## Coding Principles

### Error Design

- Use a **single shared `Error` enum** in `src/error.rs` for all modules.
- Every variant must carry **structured fields** (named, not positional) that
  provide enough context for callers to produce diagnostics without a debugger.
- Never use stringly-typed errors; prefer `&'static str` context tags over
  allocated `String`.
- Implement `Display` so the human-readable message embeds the numeric context
  (byte counts, offsets, limits).

### Integer Overflow Safety

- Use `saturating_add` (or `checked_add` where recovery is needed) at **every
  arithmetic boundary** where untrusted input influences the operands —
  especially `header_size + payload_size`, `offset + size`, and decompression
  output length calculations.
- This applies to both parsing paths and lookup/retrieval paths (e.g.
  `get_by_crc`).
- Never rely on Rust's debug-mode overflow panics as the safety mechanism;
  the code must be correct in release mode.

### Safe Indexing — No Direct Indexing in Production Code

Production code must **never** use direct indexing on **any type** —
`&[u8]`, `&str`, `Vec<T>`, or any other indexable container.  This applies
regardless of whether the index "feels safe" (e.g. derived from `.find()`
or bounded by a loop guard).  Direct indexing panics on out-of-bounds
access, which is a denial-of-service vector.

**Banned patterns (all of these panic on OOB):**

```rust
data[offset]           // byte slice indexing
data[start..end]       // byte slice range
line[pos..]            // string slicing
content[..colon_pos]   // string slicing with find()-derived index
entries[i].0           // vec/slice element access
bytes[i]               // byte array indexing
value.as_bytes()[0]    // first-byte access
```

**Required replacements:**

| Banned                | Replacement                                            |
| --------------------- | ------------------------------------------------------ |
| `data[offset]`        | `read_u8(data, offset)?` or `data.get(offset)`         |
| `data[start..end]`    | `data.get(start..end).ok_or(Error::…)?`                |
| `line[pos..]`         | `line.get(pos..).unwrap_or("")`                        |
| `&line[..pos]`        | `line.get(..pos).unwrap_or(line)`                      |
| `entries[i]`          | `entries.get(i).map(…)` or `entries.get_mut(i).map(…)` |
| `bytes[i]`            | `bytes.get(i) == Some(&val)`                           |
| `value.as_bytes()[0]` | `value.as_bytes().first()`                             |

**Binary parsers** should use the centralised safe-read helpers in
`src/read.rs`:

- `read_u8(data, offset)` — reads one byte via `.get()`
- `read_u16_le(data, offset)` — reads two bytes via `.get()`, little-endian
- `read_u32_le(data, offset)` — reads four bytes via `.get()`, little-endian

All helpers return `Result<_, Error::UnexpectedEof>` with structured context
(needed offset, available length).  They use `checked_add` internally to
prevent integer overflow on offset arithmetic.

**Text parsers** should use `.get()` with `.unwrap_or("")` (or
`.unwrap_or(original)` when the fallback is the unsliced source).
Even though `str::find()` returns valid UTF-8-aligned indices, the rule
is absolute — no reviewer should ever need to *reason* about whether an
index is safe.  If it compiles without `.get()`, it's wrong.

**Test code** (`#[cfg(test)]` blocks) may use direct indexing when the test
controls the input and panic-on-bug is acceptable.

### No `.unwrap()` in Production Code

Production code must **never** call `.unwrap()`, `.expect()`, or any method
that panics on `None`/`Err`. Use `?`, `.ok_or()`, `.map_err()`, or
`.unwrap_or()` instead.

**Test code** may use `.unwrap()` freely — a panic in a test is an acceptable
failure mode.

### Type Safety — Newtypes for Domain Identifiers

Use newtype wrappers for domain-specific integer identifiers to prevent
accidental mixing of semantically different values. The newtype should:

- Derive: `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`
- Provide `from_raw(value) -> Self` and `to_raw(self) -> inner` accessors
- Implement `Display` with a human-readable format

**Current newtypes:**

| Type     | Inner | Module | Purpose                        |
| -------- | ----- | ------ | ------------------------------ |
| `MixCrc` | `u32` | `mix`  | Westwood MIX filename CRC hash |

When adding new format modules, evaluate whether key identifiers (offsets,
indices, hashes) would benefit from newtype wrapping. Apply newtypes where
misuse could cause silent data corruption or security issues — not for every
integer.

### Parser Design Philosophy

- **Parsers are pure functions** of their input (`&[u8]`). No hidden state,
  no side effects, no filesystem access. Calling a parser twice on the same
  input must yield identical results.
- **Permissive on unknown values.** Parsers accept unrecognised enum values
  (e.g. compression IDs, flags) and store them as-is. Callers decide whether
  they can handle the value. This supports future and modded game files.
- **Strict on structural integrity.** Offsets, sizes, and counts must be
  validated against actual buffer lengths before any slice operation.

### `std` and Allocation

- The crate uses `std`. Use standard library types (`Vec`, `String`, `HashMap`)
  as appropriate.
- The `&[u8]` parsing API remains the primary interface (callers provide bytes).
  Streaming APIs are outside the current public contract for this crate.

### Heap Allocation Policy

This crate processes game assets in real-time contexts. Minimise heap
allocation to reduce allocator overhead, GC pauses, and memory fragmentation.

**Rules (in priority order):**

1. **Hot paths must not heap-allocate.** Any function called per-frame, per-lookup,
   or per-byte (e.g. `crc()`, LCW command handlers, ADPCM nibble decode) must be
   zero-allocation. Use stack buffers, byte-by-byte processing, or iterator
   patterns instead of `String`, `Vec`, or `Box`.

2. **Parsers should borrow, not copy.** When the parsed result can reference the
   input slice (via `&'a [u8]`), prefer borrowing over `.to_vec()`. This
   eliminates per-entry allocations during bulk parsing. Example: `ShpFrame<'a>`
   borrows frame data from the input; `MixArchive<'a>` borrows the data section.

3. **Fixed-size scratch buffers belong on the stack.** When the maximum size is
   bounded and small (≤ ~4 KB), use a `[T; N]` array instead of `Vec<T>`.
   Example: BigNum double-width multiplication buffers in `mix_crypt` use
   `[u32; BN_DOUBLE]` (516 bytes) instead of `vec![0u32; len]`.

4. **`Vec::with_capacity` for necessary allocations.** When a heap allocation
   is unavoidable (variable-length output like decompressed pixel data), always
   use `Vec::with_capacity(known_size)` to avoid reallocation.

5. **Prefer bulk operations over per-element loops.**
   - `Vec::extend_from_slice` over N × `push` for literal copies (memcpy).
   - `Vec::extend_from_within` over N × indexed-push for non-overlapping
     back-references (memcpy from self).
   - `Vec::resize(len + n, value)` over N × `push(value)` for fills (memset).
   These let the compiler emit SIMD/vectorised memory operations.

6. **`#[inline]` on small hot functions.** Trivial accessors
   (`from_raw`/`to_raw`, `is_stereo`, `has_embedded_palette`), CRC computation,
   binary-search lookup (`get`, `get_by_crc`), and the safe-read helpers must
   carry `#[inline]` to guarantee inlining across crate boundaries.

7. **Release profile optimisation.** `Cargo.toml` specifies `lto = true` and
   `codegen-units = 1` for release builds, enabling cross-crate inlining and
   whole-program dead-code elimination.

**Current allocation profile by module:**

| Module      | Parse-time allocs       | Runtime allocs       | Notes                       |
| ----------- | ----------------------- | -------------------- | --------------------------- |
| `mix`       | 1 (entry Vec)           | 0 per lookup         | `crc()` is zero-alloc       |
| `pal`       | 0                       | 0                    | Fixed `[PalColor; 256]`     |
| `shp`       | 2 (offset + frame Vecs) | 1 per `pixels()`     | Frame data borrows input    |
| `aud`       | 0 (borrows input)       | 1 per `decode_adpcm` | With-capacity Vec           |
| `lcw`       | 1 (output Vec)          | —                    | With-capacity, bulk ops     |
| `mix_crypt` | 1 (decrypt output)      | 0 in RSA loop        | BigNum is stack `[u32; 64]` |
| `tmp`       | 1 (tile Vec)            | 0                    | Tile data borrows input     |
| `vqa`       | 2 (chunk + frame Vecs)  | 0                    | Chunk data borrows input    |
| `wsa`       | 2 (offset + frame Vecs) | 0                    | Frame data borrows input    |
| `fnt`       | 1 (glyph Vec)           | 0                    | Glyph data borrows input    |
| `ini`       | 3 (HashMap + 2 Vecs)    | 0                    | String allocs per entry     |
| `miniyaml`  | N (node tree)           | 0                    | String allocs per node      |

### Implementation Comments (What / Why / How)

A reviewer should be able to learn and understand the entire design by reading
the source alone — without consulting external documentation, git history, or
the original author.

Every non-trivial block of implementation code must carry comments that answer
up to three questions:

1. **What** — what this code does (one-line summary above the block or method).
2. **Why** — the design decision, security invariant, or domain rationale that
   motivated this approach over alternatives.
3. **How** (when non-obvious) — algorithm steps, bit-level encoding, reference
   to the original format spec or EA source file name.

Specific guidance:

- **Constants and magic numbers:** document the origin and meaning.  If a
  constant derives from a V38 security cap, say so.  If it mirrors a value
  from the original game binary, name the source file.
- **Section headers:** use `// ── Section name ───…` comment bars to visually
  separate logical phases within a long function (e.g. header parsing, offset
  table, frame extraction).
- **Safety-critical paths:** every V38 guard (ratio cap, output limit,
  bounds check, forward-progress assertion) must have an inline comment
  explaining *what* it prevents and *why* the chosen limit is correct.
- **Algorithm steps:** multi-step algorithms (LCW commands, IMA ADPCM nibble
  decode, CRC accumulation) should have per-step inline comments so a reader
  can follow the logic without cross-referencing an external spec.
- **Permissive vs. strict:** where the parser intentionally accepts values it
  doesn't recognise (unknown compression IDs, out-of-range palette bytes),
  comment that the permissiveness is deliberate and why.

This standard applies equally to production code and test helpers (e.g.
`build_shp`, `build_aud`).  The same what/why/how structure used for `#[test]`
doc comments (see Testing Standards below) applies to implementation code via
`///` doc comments on public items and `//` inline comments on internal logic.

## Testing Standards

### Test Documentation

Every `#[test]` function must have a `///` doc comment with up to three
paragraphs:

1. **What** (first line) — the scenario being tested.
2. **Why** (second paragraph) — the security invariant, correctness guarantee,
   or edge-case rationale that motivates the test.
3. **How** (optional third paragraph) — non-obvious test construction details
   (byte encoding, overflow mechanics, manual binary layout).

Omit the "How" paragraph when the test body is self-explanatory.

### Doc Examples Must Compile and Pass

All `///` and `//!` code examples (doctests) must compile, run, and pass.
Never use `no_run`, `ignore`, or `compile_fail` annotations to skip execution.
If a code example requires filesystem access, network, or other unavailable
resources, rewrite it to use in-memory data so it runs in CI without external
dependencies.

### Test Organisation

Tests within each module are grouped under section-comment headers:

```rust
// ── Category name ────────────────────────────────────────────────────
```

Standard categories (in order): basic functionality, error field & Display
verification, known-value cross-validation, determinism, boundary tests,
integer overflow safety, security edge-case tests.

### Required Test Categories

Every parser module must include tests for:

- **Happy path:** parse well-formed input, verify fields.
- **Error paths:** each `Error` variant the module can return must be tested,
  including verification that structured fields carry correct values.
- **Display messages:** at least one test asserting `Error::Display` output
  contains the key numeric values.
- **Determinism:** parse (or decode) the same input twice, assert equality.
- **Boundary:** test both sides of every limit (exactly at cap succeeds,
  one past cap fails; minimum valid input succeeds, one byte short fails).
- **Overflow safety:** craft inputs with `u32::MAX` or near-max values to
  exercise `saturating_add` / bounds-check paths; assert no panic and
  correct error return.

### Security Testing (V38)

Every parser module must include **adversarial** tests that exercise the V38
safety invariants with crafted malicious inputs.  These tests ensure that
future changes do not regress the security guarantees.

#### Required Adversarial Tests

Each format module must have:

- **All-`0xFF` test (`adversarial_all_ff_no_panic`):** Feed `N` bytes of
  `0xFF` to the parser/decompressor.  All header fields maximise
  (`u16::MAX`, `u32::MAX`), exercising every overflow guard, output-cap
  check, and offset bounds validation simultaneously.  The test asserts
  that parsing does not panic; the return value is ignored (`let _ = …`).

- **All-zero test (`adversarial_all_zero_no_panic`):** Feed `N` bytes of
  `0x00`.  This exercises zero-dimension paths (division-by-zero guards),
  zero-count loops, and degenerate empty-payload handling.

- **Module-specific adversarial tests** targeting the format's unique attack
  surfaces.  Identify the format's riskiest structural features — e.g.
  internal offset tables (self-referencing, overlapping, or header-pointing
  offsets), variable-length count fields (mismatches between declared and
  actual counts), dimension fields used in arithmetic (non-divisible values,
  zero dimensions), and size fields that influence allocation
  (near-`u32::MAX` sizes triggering OOM).  Write at least one adversarial
  test per identified surface.

#### Adversarial Test Pattern

```rust
/// `ModuleName::parse` on N bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer maximises every header field, exercising
/// overflow guards, output caps, and offset bounds checks.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = ModuleName::parse(&data);
}
```

The test does **not** assert a specific error variant — only that the parser
returns without panicking.  This catches integer overflow, out-of-bounds
indexing, division by zero, and unbounded allocation.

#### `read.rs` Foundation Tests

The safe-read helpers in `src/read.rs` are the security-critical foundation
for all parsers.  Every helper function must maintain unit tests covering:

- Valid reads at offset 0 and at the last valid position.
- One-past-end (returns the appropriate error with correct context fields).
- Empty input slice.
- `usize::MAX` offset (exercises `checked_add` overflow path).

When new read helpers are added, they must include the same boundary test
coverage before merging.

#### Integration-Level Adversarial Tests

`tests/integration.rs` must include a combined adversarial test that feeds
all-`0xFF` data to every public parser in a single test function, confirming
the entire public API surface is safe.

#### Error Variant Coverage

Every `Error` enum variant must be tested for:

1. **Structured field correctness** — match on the variant and assert field
   values.
2. **Display output** — assert that `to_string()` contains the key numeric
   values (byte counts, offsets, identifiers).

This includes variants not yet returned by production code — test the Display
output directly to prevent regressions when the variant is eventually used.

### Verification Workflow

After any code change, always run the full verification before considering
the task complete:

```
cargo test
cargo clippy --tests -- -D warnings
cargo fmt --check
```

All three must pass cleanly (zero warnings, zero format diffs).

## Local Rules

- **Language:** Rust (2021 edition)
- **Build:** `cargo build`
- **Test:** `cargo test`
- **Lint:** `cargo clippy --tests -- -D warnings`
- **Format:** `cargo fmt --check`
- **License check:** `cargo deny check licenses`
- **Local CI (PowerShell):** `./ci-local.ps1`
- **Local CI (Bash/WSL):** `bash ci-local.sh`

### Local CI Scripts

`ci-local.ps1` (PowerShell) and `ci-local.sh` (Bash) mirror the GitHub Actions
CI pipeline locally.  Run either script from the repo root before pushing.

Steps performed (in order):

1. UTF-8 encoding validation (all `.rs` files, `Cargo.toml`, `README.md`)
2. Auto-fix formatting and clippy (`cargo fmt`, `cargo clippy --fix`)
3. Format check (`cargo fmt --check`)
4. Clippy lint — all features and no-default-features
5. Tests — all features and no-default-features
6. Documentation build (`cargo doc` with `-D warnings`)
7. License check (`cargo deny check licenses`)
8. Security audit (`cargo audit`)
9. MSRV check (compile, clippy, and test against `rust-version` from
   `Cargo.toml`)

Optional tools (`cargo-deny`, `cargo-audit`) are auto-installed if missing.
MSRV toolchain is auto-installed via `rustup` if missing.

## Current Implementation Target

- Active milestone: `M1`
- Active `G*` steps: `G1` (RA asset parsing)
  - `G1.2` `.mix` extraction: **complete** (basic + extended + Blowfish-encrypted)
  - `G1.3` `.shp/.pal` validation: **complete**
  - `G1.4` `.aud/.vqa` header validation: **complete**
  - `G1.5` `.tmp/.wsa/.fnt` parsing: **complete**
- All D076 binary codec modules are implemented
- Text format modules (`.ini`, MiniYAML) are implemented
- CLI tool (`cnc-formats` binary) is implemented with `validate`, `inspect`,
  `convert`, `list`, and `extract` subcommands
- Goal #8 (`std::io::Read` streaming API) is tracked separately from the
  current slice-based crate surface

## Execution Overlay Mapping

- **Milestone:** `M1` (Resource Fidelity)
- **Priority:** `P-Core`
- **Feature Cluster:** D076 Tier 1
- **Depends on:** none (standalone from inception)
