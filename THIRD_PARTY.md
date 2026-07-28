# Third-Party Attribution

OxiGeo is Apache-2.0 licensed, copyright © 2026 COOLJAPAN OU (Team
Kitasan). This file summarizes the third-party open-source software this
workspace depends on, plus the non-crates.io components it bundles or
vendors. It is a truthful summary, not an exhaustive per-crate manifest —
see "How to regenerate / go deeper" below for the exact tooling to produce
a complete SBOM on demand.

## Snapshot

- **Workspace**: 76 crates under `crates/` (74 published to crates.io; 2
  are internal/non-publishing — `oxigeo-python` and `oxigeo-examples`).
- **Dependency graph**: `cargo metadata` reports **954 distinct external
  crates** (by name+version) across the full resolved graph, including
  every optional feature and every target (native, `wasm32-unknown-unknown`,
  embedded `no_std` targets, etc.) — i.e. the union of everything any
  workspace crate could pull in with some feature combination, not what any
  single default build actually links.
- **License mix** (bucketed from each crate's own `Cargo.toml` `license`
  field; a crate offered under several licenses is bucketed by the most
  permissive option it lists, since that is the option a licensee is free
  to choose):

  | Bucket | Count | Notes |
  |---|---:|---|
  | MIT **or** Apache-2.0 (dual/multi-licensed) | 600 | The Rust ecosystem default; either license may be elected. |
  | MIT only | 186 | |
  | Apache-2.0 only | 113 | |
  | BSD (2- or 3-Clause) | 18 | |
  | Unicode-3.0 / Unicode-DFS-2016 | 18 | Unicode Character Database derivative data (e.g. `unicode-ident`, `unicode-width`). |
  | ISC | 7 | |
  | Zlib (or Zlib among dual/multi options) | 4 | |
  | **MPL-2.0** (weak, file-level copyleft) | 4 | `cbindgen`, `colored`, `ascii_utils`, `fast_chemail` — see "Copyleft" below. |
  | **CDDL-1.0** (weak, file-level copyleft) | 1 | `inferno`, transitive of the optional `pprof` profiling path — see below. |
  | Other permissive (CC0-1.0, CDLA-Permissive-2.0) | 3 | `tiny-keccak` (CC0-1.0); `webpki-roots`/`webpki-root-certs` (CDLA-Permissive-2.0, Mozilla's CA-bundle data license). |

  Every one of the 954 external crates carries an explicit SPDX `license`
  field (no crate relies on a bare `license-file` with no SPDX identifier),
  so this table accounts for the full set.

## Copyleft — what's actually there, and why it's not a problem

COOLJAPAN policy calls out copyleft explicitly, so to be precise:

- **No strong/viral copyleft** (GPL, LGPL, AGPL) is a *required* license
  for anything in the graph. The only near-hit is `r-efi` (two versions,
  `5.3.0`/`6.0.0`), which is licensed `MIT OR Apache-2.0 OR
  LGPL-2.1-or-later` — a licensee-choice dual/triple license, so this
  project consumes it under MIT or Apache-2.0 and never triggers any
  LGPL obligation. `r-efi` is a UEFI protocol-binding crate reachable only
  on `no_std`/embedded targets (`oxigeo-embedded`/`oxigeo-noalloc`
  dependency closures on some targets), not part of any default native
  build.
- **MPL-2.0** (`cbindgen`, `colored`, `ascii_utils`, `fast_chemail`) is a
  *file-level* weak copyleft: it requires that modifications to the
  MPL-licensed files themselves stay available in source form, but does
  not extend to the rest of the program that links against it. `cbindgen`
  is a build-time-only tool (FFI header generation, e.g. for
  `oxigeo-mobile-enhanced`'s C headers); `colored` is a small terminal
  color-formatting dependency of `oxigeo-dev-tools`. Neither is modified
  by this project, so no MPL source-availability obligation is even
  triggered beyond "don't fork it privately."
- **CDDL-1.0** (`inferno`) is likewise file-level weak copyleft (same
  family lineage as MPL). It arrives transitively via `pprof`, which is
  itself gated behind the non-default `profiling` feature of
  `oxigeo-bench`/`oxigeo-dev-tools` (kept out of default builds precisely
  because of this kind of policy sensitivity plus its `miniz_oxide`
  transitive — see `deny.toml`).

None of the above requires this project's own Apache-2.0-licensed source
to be relicensed; they are consumed as unmodified upstream binaries/crates
under their own terms.

## Bundled / vendored components (not fetched from crates.io)

### EPSG Geodetic Parameter Dataset (embedded in `oxigeo-proj`)

`crates/oxigeo-proj/src/epsg/` embeds CRS, datum, ellipsoid and coordinate
operation definitions derived from the **EPSG Geodetic Parameter Dataset**,
maintained by the International Association of Oil & Gas Producers
(IOGP) Geomatics Committee. Per the EPSG dataset's terms of use:

> Uses the EPSG Geodetic Parameter Dataset, ©International Association of
> Oil & Gas Producers (IOGP), reproduced/derived with permission under the
> terms of use published by IOGP at <https://epsg.org/>.

No EPSG software or copyrighted database files are redistributed — only
derived numeric/textual facts (codes, names, parameters) from the public
registry. See `NOTICE` for the full attribution statement.

### `vendor/pathfinder_simd` (vendored crate patch)

`vendor/pathfinder_simd/` vendors `pathfinder_simd` 0.5.6 from
[servo/pathfinder](https://github.com/servo/pathfinder) (upstream author
Patrick Walton), licensed `MIT OR Apache-2.0`; this project elects the
Apache-2.0 option. It carries one interim patch (6 renamed nightly
intrinsics in `src/arm/mod.rs`) needed to build on current nightly Rust; it
is pulled in transitively via `criterion`'s HTML report feature and via
`plotters`/`font-kit` from `oxigeo-jupyter`. See
`vendor/pathfinder_simd/README.md` and
`vendor/pathfinder_simd/LICENSE-NOTE.md` for the full detail and an open
TODO to add verbatim upstream license files.

## COOLJAPAN Pure-Rust substitutions (why some usual-suspect crates are absent)

By policy this workspace does not depend on `zip`, `flate2` (as a direct
dependency — see the accepted transitive note below), `zstd`, `bzip2`,
`lz4`, `tar`, `snap`, `brotli`, or `miniz_oxide` (as a direct dependency)
for compression; those are replaced by the pure-Rust `oxiarc-*` family
(`oxiarc-archive`, `oxiarc-deflate`, `oxiarc-lz4`, `oxiarc-lzw`,
`oxiarc-zstd`, `oxiarc-snappy`, `oxiarc-brotli`). Likewise `rusqlite`/
`libsqlite3-sys` are replaced by `oxisql-core`/`oxisql-sqlite-compat`,
`rustfft` by OxiFFT, `bincode` by oxicode, and `openblas`-family BLAS
bindings by oxiblas. See `deny.toml` for the enforced ban list and the
accepted transitive exceptions (e.g. `png -> miniz_oxide`, which is a
transitive of the `image`/`png` crates and not a direct dependency this
project chose).

## Security advisories

Known, reviewed, and explicitly accepted RUSTSEC advisories (all
transitive dependencies with no available upstream fix reachable within
current semver constraints) are tracked in `.cargo/audit.toml`, currently
15 ignore entries. `deny.toml`'s `[advisories]` section mirrors that same
list; keep the two in sync when either changes. Note that `deny.toml`
deliberately checks each crate's *default*-feature dependency graph (see
the `[graph]` comment there), so a handful of the mirrored ignore entries
(e.g. the AWS-SDK/`azure_core`/`rumqttc` rustls-webpki and http-types
advisories) only actually match when a crate's non-default features
(`oxigeo-cloud`'s `s3`/`azure-blob`, `oxigeo-mqtt`'s broker features, ...)
are enabled — `cargo deny check advisories` reports those as harmless
"not encountered" warnings rather than errors on the default graph.

## How to regenerate / go deeper

This summary was produced from:

```bash
# Full resolved dependency graph (packages + license fields):
cargo metadata --format-version 1 > /tmp/oxigeo-metadata.json

# Or, for a flat distinct crate-name listing:
cargo tree --workspace -e normal --prefix none 2>/dev/null | sort -u
```

For a complete, per-crate SBOM (exact license text pulled per-package,
not just the summary above), use `cargo-license` or `cargo-deny list`:

```bash
cargo install cargo-license
cargo license --workspace --avoid-dev-deps --json > third-party-full.json

# Or, to enforce (not just list) the policy encoded in deny.toml:
cargo install cargo-deny
cargo deny check licenses
cargo deny check bans
cargo deny check advisories
```

This file should be refreshed whenever the dependency graph changes
materially (a new optional backend, a new driver crate, a major version
bump that changes a license) — it is a snapshot, not a build artifact, so
there is no CI gate enforcing it stays byte-for-byte current.
