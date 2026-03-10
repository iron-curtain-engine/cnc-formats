# cnc-formats

Clean-room binary format parsers for Command & Conquer game files.

Parses `.mix` archives, `.shp` sprites, `.pal` palettes, `.aud` audio,
`.vqa` video, and MiniYAML configuration files used by Red Alert,
Tiberian Dawn, and related C&C titles.

## Status

> ⚠️ **Early development** — API is unstable and incomplete.

## Design

This crate is a **clean-room implementation** — no EA-derived code.
All parsing logic is written from publicly available format documentation
and binary analysis. This is what allows the MIT/Apache-2.0 licensing.

For EA GPL-derived parsing (e.g., game-specific rule interpretation),
see the `ra-formats` crate in the [Iron Curtain engine](https://github.com/iron-curtain-engine/iron-curtain).

## Usage

```rust
// Coming soon — API not yet stable.
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
