# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-07-28

### Fixed

#### `no_std` build
- The `no_std` (`--no-default-features`) build was broken: the crate declared
  `#![cfg_attr(not(feature = "std"), no_std)]` but did not actually compile without
  `std`, failing with 63 errors and 4 warnings
- Root cause: `extern crate alloc` was itself gated on `not(feature = "std")`, so every
  module had to repeat a `#[cfg(not(feature = "std"))] use alloc::…;` line — modules
  whose author omitted one broke only in the `no_std` configuration
- `extern crate alloc` is now **unconditional** (the `alloc` crate is always available,
  and `alloc::string::String` *is* `std::string::String`), and the 18 scattered
  cfg-gated alloc imports were replaced with plain `use alloc::…;` declarations
- Added the missing alloc-prelude imports across the crate: `String`, `Vec`, `Box`,
  the `ToString` trait (45 `.to_string()` call sites), and the `format!` macro
- Replaced `std::f64::consts::…` with `core::f64::consts::…` in `ups_projection`,
  `pipeline`, `projections::equirectangular` and `projections::oblique_mercator`
- `geodesic::validate_lat_lon` no longer discards its `label` argument in `no_std`
  builds: the std/no_std message split is gone and both configurations now emit the
  same labelled, value-bearing diagnostics
- The build-generated EPSG snapshot (`register_generated_epsg`, ~5 additional CRS
  registrations) is no longer `std`-only — it needs nothing beyond `alloc`, so
  `no_std` builds now get the complete embedded EPSG registry
- Integration tests that exercise std-only API (`Transformer`, `Pipeline`,
  `crs_registry`, …) are declared `required-features = ["std"]`; the remaining six
  test targets run in both configurations

### Removed

#### Optional C Bindings
- Removed the `proj-sys` feature and the `proj` C-bindings dependency (C bindings to
  the system PROJ library), per the COOLJAPAN Pure Rust Policy
- The feature was vestigial: all coordinate transformation goes through the pure-Rust
  `oxiproj` engine, and `proj-sys` contributed only an unused `ProjSysError` variant
  and its `From<proj::ProjError>` conversion — no transformation path ever called the
  C library
- Its only practical effect was that `--all-features` builds required `cmake` and a
  system libproj (the `proj` crate builds PROJ from source)
- For higher-fidelity CRS coverage, use the pure-Rust `proj-db` feature instead
  (oxisql PROJ.db reader, ~7,500 EPSG codes)

## [0.1.0] - 2025-01-25

### Added

#### CRS Support
- EPSG code database (10,000+ definitions)
- WKT v1 parsing and generation
- CRS type detection (Geographic, Projected, Geocentric)
- Authority and code extraction
- Datum information retrieval

#### Transformation Support
- Geographic to Projected transformations
- Datum shifts (WGS84, NAD83, ETRS89, etc.)
- 7-parameter Helmert transformations
- UTM zone transformations
- Web Mercator (EPSG:3857) support
- Batch coordinate transformation

#### Pure Rust Implementation
- Default implementation via proj4rs
- No external dependencies (C libraries)
- Embedded EPSG database
- Zero configuration required

#### Optional C Bindings
- Optional PROJ.4/PROJ library bindings (feature: `proj-sys`)
- Fallback for maximum compatibility
- Feature-gated to maintain Pure Rust default

### Implementation Details

#### Design Decisions
- **Pure Rust first**: Default implementation is 100% Rust
- **Embedded database**: No external EPSG files required
- **Cached lookups**: Fast CRS retrieval
- **Accuracy**: Comprehensive transformation accuracy tests

#### Known Limitations
- WKT v2 parsing limited (v1 fully supported)
- Some exotic datums not supported
- Grid-based transformations limited
- Geoid models not included

### Performance

- Transformation: ~100ns per coordinate pair
- Batch transformation: ~50ns per coordinate (SIMD optimization)
- CRS lookup: <1μs (cached)
- WKT parsing: ~10μs

### Future Roadmap

#### 0.2.0 (Planned)
- WKT v2 full support
- Grid-based transformations (NTv2, NADCON)
- Geoid models (EGM96, EGM2008)
- PROJ string parsing
- Inverse transformations

#### 0.3.0 (Planned)
- Compound CRS support
- Time-dependent transformations
- Custom CRS definitions
- Transformation pipelines

### Dependencies

- `proj4rs` 0.1.x - Pure Rust projection engine
- `num-traits` 0.2.x - Numeric traits
- `serde` 1.x - Serialization support
- `once_cell` 1.x - Lazy static initialization
- `proj` 0.31.x (optional) - C bindings to PROJ

### Testing

- 150+ unit tests
- Transformation accuracy tests (sub-meter precision)
- Round-trip transformation tests
- EPSG database validation
- Real-world coordinate transformation tests

### License

Apache-2.0

Copyright © 2025 COOLJAPAN OU (Team Kitasan)
