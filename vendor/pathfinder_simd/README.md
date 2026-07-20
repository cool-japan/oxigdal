# pathfinder_simd 0.5.6 (vendored, interim patch)

This is a vendored copy of [`pathfinder_simd` 0.5.6](https://crates.io/crates/pathfinder_simd) from crates.io.

## Local modifications

The ONLY change relative to upstream is in `src/arm/mod.rs`: 6 calls were renamed
from the older nightly-only intrinsics to their current names.

- `simd_minimum_number_nsz` -> `simd_fmin` (3 call sites)
- `simd_maximum_number_nsz` -> `simd_fmax` (3 call sites)

Recent nightly Rust removed/renamed the `*_number_nsz` variants, which was
breaking the workspace build because `pathfinder_simd` is transitively pulled in
by `criterion` (HTML reports) and by `oxigeo-jupyter` via `plotters -> font-kit`.

## Interim — please remove

This patch is temporary. Delete this directory and the matching
`[patch.crates-io]` block in the workspace `Cargo.toml` once the COOLJAPAN font
crate replaces `plotters` in `oxigeo-jupyter` (and `criterion`'s `html_reports`
path no longer matters).

Upstream license is preserved as-is in `Cargo.toml` (`MIT OR Apache-2.0`).
