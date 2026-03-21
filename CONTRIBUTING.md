# Contributing to cnc-formats

Thank you for your interest in contributing!

## Developer Certificate of Origin (DCO)

All contributions require a
[Developer Certificate of Origin](https://developercertificate.org/) sign-off.
Add `Signed-off-by` to your commit messages:

```
git commit -s -m "your commit message"
```

## License

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed under MIT OR Apache-2.0, without any additional terms or
conditions.

## Clean-Room Requirement — No GPL-Derived Code

This crate is licensed under MIT OR Apache-2.0. It must **never** contain code
derived from Electronic Arts' GPL-licensed C&C source code releases.

**Do not:**

- Copy or adapt code from the EA GPL source releases (including struct
  definitions, lookup tables, and compression algorithms)
- Copy code from the `ic-cnc-content` crate in the Iron Curtain engine (which is
  GPL v3)
- Copy code from any GPL-licensed C&C community tool without verifying the
  license is compatible

**Do:**

- Implement parsing logic from publicly available format documentation
- Use binary analysis of game files
- Reference clean-room reverse engineering and community specifications
- Check the [binary-codecs.md](https://github.com/iron-curtain-engine/iron-curtain-design-docs/blob/main/src/formats/binary-codecs.md)
  design document for format specifications

If you are unsure whether a piece of code is GPL-derived, ask in the pull
request before submitting.

## Code Style

Read [AGENTS.md](AGENTS.md) for the full coding standards. Key rules:

- No direct indexing in production code — use `.get()` or safe-read helpers
- No `.unwrap()` in production code — use `?`, `.ok_or()`, or `.unwrap_or()`
- Use `saturating_add` / `checked_add` for arithmetic on untrusted input
- Every parser module needs unit tests, adversarial tests, and a fuzz target

## Running Tests

Both feature modes must pass — `--all-features` and `--no-default-features`:

```
cargo test --all-features
cargo test --no-default-features
cargo clippy --all-features --tests -- -D warnings
cargo clippy --no-default-features --tests -- -D warnings
cargo fmt --check
```

Or run the full local CI:

```powershell
./ci-local.ps1      # PowerShell
bash ci-local.sh     # Bash / WSL
```
