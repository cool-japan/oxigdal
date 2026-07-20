# Contributing to OxiGeo

Thank you for considering a contribution to OxiGeo. This document covers development setup,
the policies every change must satisfy, and what to expect from a pull request.

By participating in this project you agree to abide by the [Code of Conduct](CODE_OF_CONDUCT.md).

## Development Setup

### Prerequisites

- **Rust** — install/manage via [rustup](https://rustup.rs/). OxiGeo targets edition 2024
  and requires Rust **1.89+** (see `rust-version` in the workspace `Cargo.toml`).
- **cargo-nextest** — the test runner used across the workspace:

  ```bash
  cargo install cargo-nextest --locked
  ```

- **tokei** (optional) — used to report SLoC when documenting changes:

  ```bash
  cargo install tokei
  ```

### Clone and build

```bash
git clone https://github.com/cool-japan/oxigeo.git
cd oxigeo
cargo build --all-features
```

### Test

```bash
cargo nextest run --all-features
cargo test --doc --all-features   # doc tests (nextest does not run these)
```

Run a single crate while iterating:

```bash
cargo nextest run -p oxigeo-geotiff --all-features
```

### Lint and format

```bash
cargo clippy --all-features -- -D warnings
cargo fmt --all -- --check
```

Format only the files you touched during day-to-day edits (`rustfmt <file>`); use
`cargo fmt --all` before opening a PR to catch anything missed.

## COOLJAPAN Policies

OxiGeo is part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem and every
contribution — including from automated agents — must respect these rules:

| Policy | Requirement |
|--------|-------------|
| **No `unwrap()` / `expect()`** | Production code must not panic. `clippy::unwrap_used` and `clippy::panic` are denied workspace-wide. Return `Result<T, OxiGeoError>` instead. `#[cfg(test)]` code is exempt. |
| **Zero warnings** | `cargo clippy --all-features -- -D warnings` must be clean before merge. |
| **Pure Rust** | No new C/C++/Fortran dependencies in default features. If a native dependency is unavoidable, gate it behind an explicit, non-default Cargo feature. Compression goes through `oxiarc-*`; no `zip`/`flate2`/`zstd`/`bzip2`/`lz4`/`tar`/`snap`/`brotli`/`miniz_oxide`. |
| **Workspace-inherited dependencies** | Member crates declare dependencies as `{ workspace = true }` (with feature additions as needed) and pin versions in the root `Cargo.toml` only, not per-crate. |
| **File size** | Every source file stays under 2,000 lines. Split oversized files (the [`splitrs`](https://github.com/cool-japan/splitrs) tool automates this) rather than letting a module grow unbounded. |
| **Formatting** | `cargo fmt` output is required; CI enforces `--check`. |
| **Naming** | Standard Rust conventions — `snake_case` functions/variables, `CamelCase` types, `SCREAMING_SNAKE_CASE` constants. |

The full rationale and additional patterns (error handling, testing, security, deployment)
live in [docs/BEST_PRACTICES.md](docs/BEST_PRACTICES.md). Please read it before making
non-trivial changes.

If you are reporting or think you have found a security issue, do **not** open a public
issue — follow the process in [SECURITY.md](SECURITY.md) instead.

## Pull Request Expectations

1. **Tests are required.** New behavior needs a covering test (unit, integration, or doc
   test as appropriate); bug fixes need a regression test that fails without the fix.
   `cargo nextest run --all-features` must pass locally before you open the PR.
2. **`CHANGELOG.md` entry.** Add a line under the `[Unreleased]` (or current in-development
   version) section, in the `Added` / `Changed` / `Fixed` category that applies, following
   the existing [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) style already used
   in the file.
3. **No unrelated churn.** Keep diffs scoped to the change described in the PR; avoid
   incidental reformatting of files you didn't otherwise touch.
4. **Describe the change.** Explain *why* the change is needed, not just what it does, and
   link any related issue.
5. **CI must be green.** Build, clippy (`-D warnings`), tests, and formatting checks all run
   in CI; a red pipeline blocks merge.

## Getting Help

- General questions and design discussion: open a
  [GitHub issue](https://github.com/cool-japan/oxigeo/issues) or discussion thread.
- Security reports: see [SECURITY.md](SECURITY.md) — please do not use public issues.

We appreciate every contribution, from typo fixes to new format drivers. Thank you for
helping build a Pure Rust geospatial stack.
