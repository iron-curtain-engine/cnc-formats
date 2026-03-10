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
- `src/16-CODING-STANDARDS.md` + `src/coding-standards/quality-review.md` — code style and review checklist

## Non-Negotiable Rule: No Silent Design Divergence

If implementation reveals a missing detail, contradiction, or infeasible design path:

- do **not** silently invent a new canonical behavior
- open a design-gap/design-change request in the design-doc repo
- mark local work as `implementation placeholder` or `blocked on Pxxx`

### Design Change Escalation Workflow

When a design change is needed:

1. Open an issue in the design-doc repo; include affected `Dxxx`, why the current
   design is insufficient, and proposed options.
2. Document the divergence locally: a comment at the code site referencing the
   issue number and rationale.
3. Keep the local workaround narrow in scope until the design is resolved.

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
