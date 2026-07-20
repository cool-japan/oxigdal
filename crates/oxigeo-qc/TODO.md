# TODO: oxigeo-qc

> **Purpose:** Quality control and validation suite for OxiGeo — comprehensive data integrity checks for geospatial data.
> **Status (2026-05-16):** 6,849 Rust LoC · 89 tests · 0 real stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)

- [x] Add STAC item/collection schema validation (planned 2026-06-06)
  - **Goal:** `StacValidator` struct in `src/stac.rs` implementing `check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<StacValidationResult>`. Result holds `issues: Vec<QcIssue>` + `is_valid()`.
  - **Design:** Read file bytes → sniff JSON `"type"` field to determine Item/Collection/Catalog → `serde_json::from_slice::<oxigeo_stac::Item>` (or Collection) → call `.validate()` → translate errors to QcIssue (Critical: missing required fields, wrong type, unsupported version; Major: datetime/bbox issues). Extra checks: bbox length 4 or 6, bbox↔geometry consistency, RFC3339 datetime format, `eo:cloud_cover ∈ [0,100]` from additional_fields. Add dep `oxigeo-stac` to Cargo.toml. Map foreign errors manually (no `#[from]` for stac errors in QcError). Register `pub mod stac;` in `src/lib.rs`.
  - **Files:** new `src/stac.rs`; update `src/lib.rs` (add pub mod + pub use); update `Cargo.toml` (add oxigeo-stac dep).
  - **Tests:** minimal valid item → no issues; item missing geometry → Critical; item with bad datetime → Major; collection missing extent → Critical; eo:cloud_cover out of range → Warning; bbox/geometry length mismatch → Major. (~6 tests)
  - **Risk:** Low — serde_json deserialization is robust; STAC schema is well-defined.

- [x] Add batch QC mode for processing entire directories (planned 2026-06-06)
  - **Goal:** `BatchRunner` struct in `src/batch.rs` with `new(cfg: BatchConfig) -> Self` and `run<P: AsRef<Path>>(&self, dir: P) -> QcResult<BatchReport>`. BatchReport: `per_file: Vec<FileQcResult>`, `severity_counts: SeverityCounts`, `total_files`, `total_issues`.
  - **Design:** Recursive directory walk using `std::fs::read_dir` (no walkdir dep); dispatch by file extension: .tif/.tiff → CogValidator + CompletenessChecker + RadiometricValidator; .shp/.geojson → TopologyChecker + AttributionChecker; .gpkg → GpkgValidator + TopologyChecker; .json → check via local `is_stac_json` fingerprint (read 4 KiB, look for `"stac_version"` or `"stac_extensions"`, use `unwrap_or`) → StacValidator if STAC. Aggregate all QcIssues into BatchReport with SeverityCounts per severity level.
  - **Files:** new `src/batch.rs`; update `src/lib.rs` (add pub mod + pub use).
  - **Prerequisites:** StacValidator (#8) and GpkgValidator (#9) and RadiometricValidator (#10) must be implemented first.
  - **Tests:** walk a temp directory tree; dispatch by extension routes correctly; severity counts aggregate correctly; non-geospatial files skipped. (~5 tests)
  - **Risk:** Low — pure file dispatch logic; no new deps needed.

- [x] Add GeoPackage compliance validation (OGC GeoPackage 1.4 spec) (planned 2026-06-06)
  - **Goal:** `GpkgValidator` struct in `src/gpkg.rs` implementing `check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<GpkgValidationResult>`. Result holds `issues: Vec<QcIssue>` + `is_valid()`.
  - **Design:** `std::fs::read(path)` → `GeoPackage::from_bytes(bytes)` → `load_contents()` → wrap `check_integrity()` (translate `IntegrityIssue` → `QcIssue` manually). Additional 1.4 checks: SQLite magic bytes check, `application_id == 0x47504B47` (Critical if wrong), `user_version >= 10400` (Major if < 10400, targeting GeoPackage 1.4.0), `gpkg_contents` table existence via `scan_table_by_name` (Critical if missing), geometry columns table presence. Use pure-Rust default (no rusqlite feature). Add dep `oxigeo-gpkg` to Cargo.toml. Register `pub mod gpkg;` in `src/lib.rs`.
  - **Files:** new `src/gpkg.rs`; update `src/lib.rs`; update `Cargo.toml` (add oxigeo-gpkg dep).
  - **Tests:** valid minimal GPKG bytes → passes; wrong application_id → Critical; missing gpkg_contents → Critical; user_version < 10400 → Major; orphan feature table → Major. Construct minimal SQLite byte sequences for testing. (~6 tests)
  - **Risk:** Medium — need to construct valid minimal SQLite byte sequences for tests; IntegrityIssue translation requires reading oxigeo-gpkg API.

- [x] Implement raster radiometric range validation per sensor type (planned 2026-06-06)
  - **Goal:** `RadiometricValidator` struct in `src/raster/radiometric.rs` with `SensorProfile` enum and `check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<RadiometricValidationResult>`. Result: `per_band: Vec<BandRadiometricResult>`, `issues: Vec<QcIssue>`, `is_valid()`.
  - **Design:** SensorProfile enum: Landsat8_SR, Landsat9_SR, Sentinel2_L2A, Sentinel2_L1C, MODIS_SR, Custom { per_band_ranges: Vec<(f64, f64)> }. Open raster via `oxigeo_geotiff::cog::CogReader` (mirror pattern from `src/raster/nodata.rs`). Deterministic stride sampling — every Nth pixel based on raster size (no `rand`, no SciRS2 random). Per-band: compute min/max/mean/p99 from samples; compare against profile expected ranges → Critical if >0.1% of samples out of range, Major if any out of range, Warning if mean drifts >2σ from expected center.
  - **Files:** new `src/raster/radiometric.rs`; register in `src/raster/mod.rs`; update `src/lib.rs`.
  - **Tests:** in-range values → no issues; overflow values → Critical; Custom profile with specific ranges; mean-drift → Warning; verify stride sampling is deterministic (same result on two calls). (~5 tests)
  - **Risk:** Low — mirrors existing nodata.rs pattern; sensor ranges are hard-coded constants.

## Medium Priority (planned — design sketched)

- [ ] Implement raster accuracy assessment (confusion matrix, kappa coefficient)
  - **Goal:** `AccuracyChecker::confusion_matrix(reference, classified)` returning row/column totals, overall accuracy, producer/user accuracy per class, Cohen's kappa κ.
  - **Files:** `src/raster/accuracy.rs` (file exists — extend with kappa)
  - **Why deferred:** Existing skeleton has basic stats only; full kappa with variance estimator is a follow-up.

- [ ] Add vector attribution completeness checker with schema enforcement
  - **Goal:** Verify every feature has declared schema fields; flag NULLs in NOT-NULL columns; type-check values.
  - **Files:** `src/vector/attribution.rs` (extend; currently bare-bones)
  - **Why deferred:** Schema format not yet standardised (TOML vs JSON Schema vs OGR FieldDefn).

- [ ] Add duplicate feature detection in vector datasets
  - **Goal:** Detect identical geometry + attribute tuples.
  - **Files:** `src/vector/topology.rs:575-602` already has `find_duplicates` infrastructure (DuplicateGroup, hash_geometry); extract to dedicated module.
  - **Why deferred:** Integrate after STAC + GPKG validators land.

- [ ] Implement HTML report generation with embedded charts
  - **Goal:** `QualityReport::to_html()` produces standalone HTML with severity-coloured tables and per-section drill-downs.
  - **Files:** `src/report.rs` (extend; `html` feature already gated on `quick-xml` per `Cargo.toml:20`)
  - **Why deferred:** JSON output sufficient for CI integration; HTML wanted for human reports.

- [ ] Add TOML-based rule configuration file loading
  - **Goal:** `RulesEngine::from_toml(path)` loads custom rules without recompilation.
  - **Files:** `src/rules.rs` (extend; `toml` workspace dep already present `Cargo.toml:37`)
  - **Why deferred:** Programmatic rule construction works today.

- [ ] Implement fix preview mode (show proposed changes before applying)
  - **Goal:** `TopologyFixer::preview(features)` returns `Vec<FixProposal>` without mutating; `apply(proposals)` commits.
  - **Files:** `src/fix.rs` (refactor — existing `FixStrategy` applies directly)
  - **Why deferred:** Two-phase API is a breaking change; defer to v0.2.0.

## Low Priority / Future (speculative — one-liners only)

- [ ] Add cross-dataset consistency validation (overlapping tiles, seamlines).
- [ ] Implement temporal consistency checking for time series datasets.
- [ ] Add point cloud (LAS/COPC) quality validation.
- [ ] Implement metadata completeness scoring per standard (ISO, FGDC, INSPIRE).
- [ ] Add CI/CD integration (GitHub Actions, GitLab CI output formats).
- [ ] Implement custom rule scripting via embedded expression language.

## Cross-crate dependencies

- **Blocks:** `oxigeo-services` (validation in pipeline), `oxigeo-cli` (qc subcommand)
- **Blocked by:** `oxigeo-algorithms` (sweep-line intersection), `oxigeo-index` (STRtree for R5/R6), `oxigeo-geotiff` (COG/raster open), `oxigeo-proj` (CRS validation), `oxigeo-stac`/`oxigeo-gpkg` (new validators)

## Recently completed (verbatim)

- [x] Replace stub `has_self_intersection` + `check_topology_rules` with real Bentley-Ottmann + STRtree-backed topology rule engine (completed 2026-05-07)
  - **Goal:** Make TopologyChecker actually validate geometry-topology rules. Both functions currently take `_`-prefixed params and return placeholder values; replace with rules R1–R6 implementations that catch real OGC simple-features violations.
  - **Design:**
    1. `has_self_intersection(linestring: &LineString) -> Option<Vec<(usize, usize)>>`: use existing `oxigeo_algorithms::vector::intersect_linestrings_sweep` (Bentley-Ottmann); filter endpoint-shared adjacency (i/i+1 neighbours); return None for clean, Some(pairs) for self-intersecting.
    2. `TopologyViolation` enum in new `vector/violations.rs`: SelfIntersection {feature_id, segments}, RingOrientation {feature_id, ring_index, expected_ccw}, UnclosedRing {feature_id, ring_index}, Gap {feature_a, feature_b, area}, Overlap {feature_a, feature_b, area}, DanglingEndpoint {feature_id, point_index} (opt-in).
    3. `check_topology_rules(features: &FeatureCollection) -> Vec<TopologyViolation>`: R1 (LineString self-intersect), R2 (polygon ring orientation via shoelace), R3 (ring closure), R4 (polygon ring self-intersect), R5 (gaps — opt-in via TopologyOptions::detect_gaps), R6 (overlaps via polygon_intersection + STRtree).
    4. STRtree spatial pre-filter for R5/R6: `STRtree::insert_all` over polygon bboxes from `oxigeo_index`; O(n log n + k) instead of O(n²).
  - **Files:**
    - `crates/oxigeo-qc/src/vector/topology.rs` (replace stubs ~lines 559 and 706)
    - `crates/oxigeo-qc/src/vector/violations.rs` (new — TopologyViolation enum)
    - `crates/oxigeo-qc/src/vector/mod.rs` (re-export TopologyViolation, TopologyOptions)
    - `crates/oxigeo-qc/Cargo.toml` (add oxigeo-index, oxigeo-algorithms if absent)
  - **Tests:** test_self_intersect_simple_x, test_self_intersect_no_intersection, test_self_intersect_endpoint_shared_only_neighbours, test_self_intersect_collinear_overlap, test_check_topology_rules_polygon_orientation_violation, test_check_topology_rules_unclosed_ring, test_check_topology_rules_polygon_self_intersect_ring, test_check_topology_rules_overlap_detection, test_check_topology_rules_gap_detection_optional, test_check_topology_rules_clean_data_returns_empty, test_check_topology_rules_uses_strtree_for_o_n_log_n
  - **Risk:** Gap detection (R5) fragile under FP; gate behind TopologyOptions::detect_gaps (default off), configurable tolerance 1e-9. R7 (dangles) behind detect_dangles flag.
- [x] Add cloud-optimized GeoTIFF (COG) compliance checker (completed 2026-05-08)
  - **Goal:** New `CogComplianceChecker` in `oxigeo-qc` that wraps existing `oxigeo-geotiff::cog::validate_cog_detailed`, adds strict-spec checks (16-byte alignment, ghost-area parsing/strict-mode enforcement, BigTIFF distinction, SubIFD legacy warning, WebP/LERC compression), and translates findings into `oxigeo-qc::error::QcIssue`.
  - **Design:** Reuse `oxigeo-qc::error::Severity` (no new `ComplianceLevel`). Two modes: `StrictMode::Ogc10` (default, OGC COG 1.0 spec minimum) and `StrictMode::GdalCogger` (adds 16-byte alignment + ghost-area requirement). Striped under strict mode → Critical. SubIFD overviews → Warning (legacy GDAL pre-2.5). WebP/LERC compression → `Warning("UnknownCompression: {value}")`.
  - **Files:** New `crates/oxigeo-drivers/geotiff/src/cog/ghost_area.rs` (~250 LoC; ghost-area parser as PREREQUISITE per IMPLEMENT POLICY — `TiffTag::GhostArea = 65535` is recognized at `cog/tags.rs:91` but never parsed); modify `cog/mod.rs` to export it. New `crates/oxigeo-qc/src/raster/cog.rs` (~450 LoC); modify `raster/mod.rs` and `lib.rs` for re-exports; modify `Cargo.toml` to add `oxigeo-geotiff = { workspace = true }`.
  - **Prerequisites (folded in per IMPLEMENT POLICY):** Ghost-area parser in oxigeo-geotiff. Parse the NUL-terminated ASCII KV block between TIFF header and first IFD; lenient parsing with `raw_kv` for unknown keys.
  - **Tests:** test_cog_strict_ogc10_minimal_pass, test_cog_strict_ogc10_rejects_striped, test_cog_strict_gdal_requires_ghost_area, test_cog_alignment_violation_emits_critical, test_cog_overviews_not_factor_of_2_emits_minor, test_cog_ghost_area_parsing_kv_pairs, test_cog_ghost_area_absent_is_info_under_ogc10, test_cog_subifd_overview_legacy_warning. Plus 3 ghost-area parser unit tests in oxigeo-geotiff.
  - **Risk:** Test fixtures — verify `tests/fixtures/` has a real COG; if not, generate via `CogWriter` in `make_test_cog()`. GDAL cogger format drift — parse leniently with `raw_kv`; never fail on unknown keys.
- [x] Implement raster NoData consistency validation across bands (completed 2026-05-08)
  - **Goal:** New `NoDataValidator` opens multi-band raster and reports inconsistencies: bands with different declared NoData values, bands with declared NoData but no actual NoData pixels (and vice versa), bands missing NoData metadata when others have it.
  - **Design:** Per-band scan: count actual NoData pixels (matching declared value within ε for floats), compute coverage statistics (% NoData per band, common NoData footprint via bitwise-AND across band masks). Issues: `BandHasNoDataMetadataButNoNoDataPixels` (Warning), `BandHasNoDataPixelsButNoMetadata` (Major), `BandsHaveDifferentNoDataValues` (Major if values differ; Info if intentional). "Outlier" check: a band substantially under-using the common NoData footprint → Warning (likely fill-value pollution). Float ε: `1e-6` (f32) / `1e-12` (f64), configurable. Open path: `oxigeo-drivers/geotiff` for now; netcdf/hdf5 follow-up.
  - **Files:** New `crates/oxigeo-qc/src/raster/nodata.rs` (~350 LoC); modify `raster/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — `oxigeo-geotiff` will already be a workspace dep from Item 6.
  - **Tests:** test_nodata_consistent_across_bands_passes, test_nodata_metadata_without_pixels_warns, test_nodata_pixels_without_metadata_majors, test_nodata_values_differ_majors, test_nodata_common_footprint_outlier_warns, test_nodata_float_eps_tolerance.
  - **Risk:** Memory — naive scan reads each band fully; for >1GB rasters switch to streaming chunked reads (use `read_window` if available); document chunk size.
- [x] Implement CRS + spatial extent validation (combined module) (completed 2026-05-08)
  - **Goal:** New `CrsAndExtentValidator` emits issues for malformed/unrecognized CRS, axis-order mismatches, datum mismatches, geographic-bounds-out-of-range, projected-bounds-implausible, inverted/empty bounds, bounds-vs-pixel-grid arithmetic mismatches.
  - **Design:** CRS validation wraps `oxigeo-proj` (already a workspace dep). Issues: `CrsUnparseable` (Critical), `CrsAuthorityUnknown` (Major if parse failed; Warning if parsed but authority unknown — EPSG snapshot may be stale), `CrsAxisOrderAmbiguous` (Warning). Geographic bounds: lon ∈ [-180, 180], lat ∈ [-90, 90]; out → Critical. Projected bounds plausibility: per CRS family — UTM x ∈ ~[-167k, 833k] m; ECEF in [-6.7M, 6.7M]; Web Mercator ~[-2e7, 2e7]; out → Major (Warning if borderline). Inverted bounds (xmin > xmax or ymin > ymax) → Critical; zero-area → Major. Bounds-vs-pixel-grid: recompute `xmin + width × pixel_size_x` vs declared `xmax`; mismatch > 0.5 px → Major.
  - **Files:** New `crates/oxigeo-qc/src/raster/crs_extent.rs` (~400 LoC); modify `raster/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — oxigeo-proj already a dep.
  - **Tests:** test_crs_unparseable_critical, test_crs_axis_order_ambiguous_warns, test_geographic_bounds_lat_91_critical, test_projected_bounds_utm_implausible_x_majors, test_bounds_inverted_critical, test_bounds_zero_area_majors, test_bounds_pixel_grid_mismatch_majors, test_bounds_pixel_grid_within_half_px_passes.
  - **Risk:** EPSG database freshness — emit Warning (not Major) when authority unknown but parse succeeded. Axis-order: default OGC convention; document.
- [x] Implement spatial extent validation — see "CRS + spatial extent validation (combined module)" above (completed 2026-05-08)

---
*Last audited: 2026-05-16*
