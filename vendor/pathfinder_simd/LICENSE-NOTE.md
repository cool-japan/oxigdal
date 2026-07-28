# License note for the vendored `pathfinder_simd` crate

This directory is a vendored copy of the third-party crate
[`pathfinder_simd`](https://crates.io/crates/pathfinder_simd) version
0.5.6, developed upstream as part of the
[servo/pathfinder](https://github.com/servo/pathfinder) project. See
`README.md` in this directory for why it is vendored and exactly what was
changed relative to upstream (six intrinsic renames in `src/arm/mod.rs`;
everything else is unmodified).

## What was verified, and how

No `LICENSE`/`LICENSE-MIT`/`LICENSE-APACHE` file was vendored alongside
the source (only `Cargo.toml`, `Cargo.lock`, `build.rs`, `README.md` and
`src/` were copied in). The license was therefore verified from the
vendored `Cargo.toml` metadata itself (the file cargo generates from the
package's own manifest on publish), not assumed:

```toml
[package]
name = "pathfinder_simd"
version = "0.5.6"
authors = ["Patrick Walton <pcwalton@mimiga.net>"]
homepage = "https://github.com/servo/pathfinder"
license = "MIT OR Apache-2.0"
repository = "https://github.com/servo/pathfinder"
```

- **Upstream project**: `pathfinder_simd`, part of servo/pathfinder.
- **Upstream repository / homepage**: <https://github.com/servo/pathfinder>
- **Upstream author**: Patrick Walton <pcwalton@mimiga.net>
- **Upstream license (per its own `Cargo.toml`)**: `MIT OR Apache-2.0`
  (dual-licensed, licensee's choice).

## License election for this vendored copy

This project (OxiGeo) is Apache-2.0 licensed. For this vendored copy, we
elect the **Apache License, Version 2.0** option of `pathfinder_simd`'s
dual license. The full Apache-2.0 text applicable to this election is the
repository-root `LICENSE` file (the same text upstream `pathfinder_simd`
itself offers as one of its two license options).

## TODO (open item -- do not treat this note as a substitute)

The upstream repository is expected to carry its own `LICENSE-APACHE` and
`LICENSE-MIT` files (this is the standard Rust/Servo project convention),
but neither was present in the vendored tree copied into this repository
tarball, so their exact upstream copyright-line text could not be
reproduced verbatim here without fetching them from the network. Before
the next release, please pull the verbatim `LICENSE-APACHE` /
`LICENSE-MIT` files from
<https://github.com/servo/pathfinder/tree/master> (or from the
`pathfinder_simd` crate's own repository if split out) and add them
alongside this note.
