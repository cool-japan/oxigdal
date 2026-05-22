# TODO: oxigdal-qc

> **Purpose:** Quality control and validation suite for OxiGDAL — comprehensive data integrity checks for geospatial data.
> **Status (2026-05-16):** 6,849 Rust LoC · 89 tests · 0 real stubs
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)

- [ ] Add STAC item/collection schema validation
  - **Verified gap:** `src/lib.rs:51-57` module list — `pub mod error; pub mod fix; pub mod metadata; pub mod raster; pub mod report; pub mod rules; pub mod vector;` — no `stac` module; `src/metadata/completeness.rs` validates ISO 19115 only.
  - **Goal:** New `StacValidator` checking STAC Item / Collection / Catalog conformance against the SpatioTemporal Asset Catalog 1.0.0 spec. Emits `QcIssue` for missing required fields (`stac_version`, `id`, `geometry`, `bbox`, `properties.datetime`, `assets`), invalid JSON schema (assets MIME types, link rel values), and broken cross-references.
  - **Design:** Reuse `oxigdal-stac` (workspace member) for the parsed types — add as a dep. Validation stages: (1) JSON-schema-conformance via `serde_json` errors on `oxigdal_stac::Item::deserialize`; (2) spec-conformance pass: `stac_version` is "1.0.0", `geometry` is GeoJSON `Polygon`/`MultiPolygon`, `bbox` length is 4 or 6 and matches `geometry`, `properties.datetime` parses as RFC 3339 or both `start_datetime` & `end_datetime` set; (3) extension validation: if `eo:cloud_cover` present, must be 0-100; (4) link/asset reachability (opt-in, network).
  - **Files:** `(new) src/stac.rs` (~500 LoC); `src/lib.rs` (declare `pub mod stac;`, re-export `StacValidator` from prelude); `Cargo.toml` (add `oxigdal-stac = { workspace = true }`)
  - **Tests:** `(proposed)` test_stac_item_minimal_valid, test_stac_item_missing_geometry_critical, test_stac_item_invalid_datetime_format_major, test_stac_collection_missing_extent_critical, test_stac_eo_cloud_cover_out_of_range_warns, test_stac_bbox_length_mismatch_geometry_major
  - **Risk:** STAC extension schemas are external JSON; ship a curated subset (`eo`, `proj`, `view`) and accept user-supplied JSON schemas via `StacValidator::with_extension_schema(name, schema)`.
  - **Prerequisites:** `oxigdal-stac` already a workspace member (`crates/oxigdal-stac/`).

- [ ] Add batch QC mode for processing entire directories
  - **Verified gap:** `src/lib.rs:60-80` prelude — no `BatchRunner`/`DirectoryScanner` exports; all checkers operate on a single file/buffer.
  - **Goal:** `BatchRunner::new(rules).run(&Path)` walks a directory tree, dispatches each file to the right validator based on extension (`.tif`/`.tiff` → `CogComplianceChecker` + `CompletenessChecker`; `.shp`/`.geojson`/`.gpkg` → `TopologyChecker` + `AttributionChecker`; `.json` → `StacValidator` if matches STAC fingerprint), aggregates `QcIssue`s into a single `BatchReport`.
  - **Design:** `walkdir` for traversal (already a workspace dep). Configurable parallelism via `rayon` feature: `par_iter` over found files. `BatchReport` aggregates per-file issue counts by `Severity`; render via existing `report::QualityReport`. Streaming output mode flushes each file's report as soon as available (useful for CI).
  - **Files:** `(new) src/batch.rs` (~350 LoC); `src/lib.rs` (declare module, re-export from prelude); `Cargo.toml` (add `walkdir = { workspace = true }`, gate `rayon` behind a `parallel` feature)
  - **Tests:** `(proposed)` test_batch_walks_directory_tree, test_batch_dispatches_by_extension, test_batch_aggregates_severities, test_batch_skips_non_geospatial_files, test_batch_parallel_matches_sequential
  - **Risk:** STAC fingerprint detection (`{"stac_version": ...}` in first 4 KiB) may misclassify random JSON; mirror the umbrella crate's `is_stac_json()` helper from `oxigdal/src/open.rs`.
  - **Prerequisites:** Item 1 (STAC validator) for full coverage; can ship with raster + vector only as v0.1.5 minimum.

- [ ] Add GeoPackage compliance validation (OGC GeoPackage 1.4 spec)
  - **Verified gap:** `src/raster/` lists `cog.rs`, `nodata.rs`, `crs_extent.rs`, `completeness.rs`, `consistency.rs`, `accuracy.rs` — no `gpkg.rs`. `src/lib.rs:65-70` prelude has no `GpkgComplianceChecker`.
  - **Goal:** `GpkgComplianceChecker` opens a `.gpkg` file via `oxigdal-gpkg` and verifies (OGC 12-128r19): (R1) SQLite header magic `SQLite format 3\0`; (R2) `application_id = 0x47504B47` ("GPKG") at offset 68; (R3) `user_version` ≥ 10300 for v1.3; (R4) presence of mandatory tables `gpkg_spatial_ref_sys`, `gpkg_contents`, `gpkg_geometry_columns`, `gpkg_extensions`; (R5) every entry in `gpkg_contents` has matching SRS row; (R6) feature tables declared in `gpkg_geometry_columns` actually exist and have the declared geometry column; (R7) WKB blobs parse and match declared `geometry_type_name`.
  - **Design:** Wrap `oxigdal-gpkg::Database` for SQL access. Each requirement → `QcIssue` with severity per OGC mandate (Section 1.1 lists 161 normative requirements; map "shall" → Critical, "should" → Major, "may" → Warning).
  - **Files:** `(new) src/raster/gpkg.rs` (~600 LoC — despite vector content, GPKG can also be raster tiles; place under raster/ for now or create `(new) src/gpkg.rs` at top level); `src/lib.rs` (declare + re-export); `Cargo.toml` (add `oxigdal-gpkg = { workspace = true }`)
  - **Tests:** `(proposed)` test_gpkg_valid_minimal_passes, test_gpkg_missing_application_id_critical, test_gpkg_missing_gpkg_contents_critical, test_gpkg_orphan_feature_table_major, test_gpkg_wkb_type_mismatch_major, test_gpkg_invalid_srs_reference_critical
  - **Risk:** OGC spec has 161 numbered requirements; phase delivery — R1-R30 (header + core tables) in 0.1.5, R31-R161 (extensions, tiles, geopackage attributes) in 0.2.0.
  - **Prerequisites:** `oxigdal-gpkg` already a workspace member.

- [ ] Implement raster radiometric range validation per sensor type
  - **Verified gap:** `src/raster/nodata.rs` checks NoData consistency but not value plausibility against sensor-specific dynamic range. No `radiometric.rs` exists.
  - **Goal:** `RadiometricValidator` rejects rasters whose pixel statistics violate the sensor's declared dynamic range. E.g. Landsat 8 SR Band 1 (Coastal Aerosol) reflectance must fall in [0, 1] after gain/offset; Sentinel-2 L2A reflectance in [0, 10000] (×10⁴ scale). Reports `OutOfRangeBelowMin`/`OutOfRangeAboveMax` per band with pixel count + percentage.
  - **Design:** `SensorProfile` enum: Landsat8_SR, Landsat9_SR, Sentinel2_L2A, Sentinel2_L1C, MODIS_Surface_Reflectance, ASTER_RadianceAtSensor, custom (user-supplied min/max per band). For each band: sample N pixels (default 100k random with stable seed), compute min/max/mean/p99, compare to profile. Critical if >0.1% pixels out of range; Major if any out-of-range pixel found; Warning if mean drifts >2σ from expected.
  - **Files:** `(new) src/raster/radiometric.rs` (~400 LoC); `src/lib.rs` (declare + re-export); profile registry in `src/raster/radiometric/profiles.rs`
  - **Tests:** `(proposed)` test_radiometric_landsat8_sr_in_range_passes, test_radiometric_sentinel2_l2a_overflow_critical, test_radiometric_custom_profile_user_supplied, test_radiometric_mean_drift_warning, test_radiometric_sampling_deterministic_seed
  - **Risk:** Sensor profile registry must be updated as new missions launch; document as a "best-effort, not authoritative" check.
  - **Prerequisites:** None — `oxigdal-geotiff` already a workspace dep.

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

- **Blocks:** `oxigdal-services` (validation in pipeline), `oxigdal-cli` (qc subcommand)
- **Blocked by:** `oxigdal-algorithms` (sweep-line intersection), `oxigdal-index` (STRtree for R5/R6), `oxigdal-geotiff` (COG/raster open), `oxigdal-proj` (CRS validation), `oxigdal-stac`/`oxigdal-gpkg` (new validators)

## Recently completed (verbatim)

- [x] Replace stub `has_self_intersection` + `check_topology_rules` with real Bentley-Ottmann + STRtree-backed topology rule engine (completed 2026-05-07)
  - **Goal:** Make TopologyChecker actually validate geometry-topology rules. Both functions currently take `_`-prefixed params and return placeholder values; replace with rules R1–R6 implementations that catch real OGC simple-features violations.
  - **Design:**
    1. `has_self_intersection(linestring: &LineString) -> Option<Vec<(usize, usize)>>`: use existing `oxigdal_algorithms::vector::intersect_linestrings_sweep` (Bentley-Ottmann); filter endpoint-shared adjacency (i/i+1 neighbours); return None for clean, Some(pairs) for self-intersecting.
    2. `TopologyViolation` enum in new `vector/violations.rs`: SelfIntersection {feature_id, segments}, RingOrientation {feature_id, ring_index, expected_ccw}, UnclosedRing {feature_id, ring_index}, Gap {feature_a, feature_b, area}, Overlap {feature_a, feature_b, area}, DanglingEndpoint {feature_id, point_index} (opt-in).
    3. `check_topology_rules(features: &FeatureCollection) -> Vec<TopologyViolation>`: R1 (LineString self-intersect), R2 (polygon ring orientation via shoelace), R3 (ring closure), R4 (polygon ring self-intersect), R5 (gaps — opt-in via TopologyOptions::detect_gaps), R6 (overlaps via polygon_intersection + STRtree).
    4. STRtree spatial pre-filter for R5/R6: `STRtree::insert_all` over polygon bboxes from `oxigdal_index`; O(n log n + k) instead of O(n²).
  - **Files:**
    - `crates/oxigdal-qc/src/vector/topology.rs` (replace stubs ~lines 559 and 706)
    - `crates/oxigdal-qc/src/vector/violations.rs` (new — TopologyViolation enum)
    - `crates/oxigdal-qc/src/vector/mod.rs` (re-export TopologyViolation, TopologyOptions)
    - `crates/oxigdal-qc/Cargo.toml` (add oxigdal-index, oxigdal-algorithms if absent)
  - **Tests:** test_self_intersect_simple_x, test_self_intersect_no_intersection, test_self_intersect_endpoint_shared_only_neighbours, test_self_intersect_collinear_overlap, test_check_topology_rules_polygon_orientation_violation, test_check_topology_rules_unclosed_ring, test_check_topology_rules_polygon_self_intersect_ring, test_check_topology_rules_overlap_detection, test_check_topology_rules_gap_detection_optional, test_check_topology_rules_clean_data_returns_empty, test_check_topology_rules_uses_strtree_for_o_n_log_n
  - **Risk:** Gap detection (R5) fragile under FP; gate behind TopologyOptions::detect_gaps (default off), configurable tolerance 1e-9. R7 (dangles) behind detect_dangles flag.
- [x] Add cloud-optimized GeoTIFF (COG) compliance checker (completed 2026-05-08)
  - **Goal:** New `CogComplianceChecker` in `oxigdal-qc` that wraps existing `oxigdal-geotiff::cog::validate_cog_detailed`, adds strict-spec checks (16-byte alignment, ghost-area parsing/strict-mode enforcement, BigTIFF distinction, SubIFD legacy warning, WebP/LERC compression), and translates findings into `oxigdal-qc::error::QcIssue`.
  - **Design:** Reuse `oxigdal-qc::error::Severity` (no new `ComplianceLevel`). Two modes: `StrictMode::Ogc10` (default, OGC COG 1.0 spec minimum) and `StrictMode::GdalCogger` (adds 16-byte alignment + ghost-area requirement). Striped under strict mode → Critical. SubIFD overviews → Warning (legacy GDAL pre-2.5). WebP/LERC compression → `Warning("UnknownCompression: {value}")`.
  - **Files:** New `crates/oxigdal-drivers/geotiff/src/cog/ghost_area.rs` (~250 LoC; ghost-area parser as PREREQUISITE per IMPLEMENT POLICY — `TiffTag::GhostArea = 65535` is recognized at `cog/tags.rs:91` but never parsed); modify `cog/mod.rs` to export it. New `crates/oxigdal-qc/src/raster/cog.rs` (~450 LoC); modify `raster/mod.rs` and `lib.rs` for re-exports; modify `Cargo.toml` to add `oxigdal-geotiff = { workspace = true }`.
  - **Prerequisites (folded in per IMPLEMENT POLICY):** Ghost-area parser in oxigdal-geotiff. Parse the NUL-terminated ASCII KV block between TIFF header and first IFD; lenient parsing with `raw_kv` for unknown keys.
  - **Tests:** test_cog_strict_ogc10_minimal_pass, test_cog_strict_ogc10_rejects_striped, test_cog_strict_gdal_requires_ghost_area, test_cog_alignment_violation_emits_critical, test_cog_overviews_not_factor_of_2_emits_minor, test_cog_ghost_area_parsing_kv_pairs, test_cog_ghost_area_absent_is_info_under_ogc10, test_cog_subifd_overview_legacy_warning. Plus 3 ghost-area parser unit tests in oxigdal-geotiff.
  - **Risk:** Test fixtures — verify `tests/fixtures/` has a real COG; if not, generate via `CogWriter` in `make_test_cog()`. GDAL cogger format drift — parse leniently with `raw_kv`; never fail on unknown keys.
- [x] Implement raster NoData consistency validation across bands (completed 2026-05-08)
  - **Goal:** New `NoDataValidator` opens multi-band raster and reports inconsistencies: bands with different declared NoData values, bands with declared NoData but no actual NoData pixels (and vice versa), bands missing NoData metadata when others have it.
  - **Design:** Per-band scan: count actual NoData pixels (matching declared value within ε for floats), compute coverage statistics (% NoData per band, common NoData footprint via bitwise-AND across band masks). Issues: `BandHasNoDataMetadataButNoNoDataPixels` (Warning), `BandHasNoDataPixelsButNoMetadata` (Major), `BandsHaveDifferentNoDataValues` (Major if values differ; Info if intentional). "Outlier" check: a band substantially under-using the common NoData footprint → Warning (likely fill-value pollution). Float ε: `1e-6` (f32) / `1e-12` (f64), configurable. Open path: `oxigdal-drivers/geotiff` for now; netcdf/hdf5 follow-up.
  - **Files:** New `crates/oxigdal-qc/src/raster/nodata.rs` (~350 LoC); modify `raster/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — `oxigdal-geotiff` will already be a workspace dep from Item 6.
  - **Tests:** test_nodata_consistent_across_bands_passes, test_nodata_metadata_without_pixels_warns, test_nodata_pixels_without_metadata_majors, test_nodata_values_differ_majors, test_nodata_common_footprint_outlier_warns, test_nodata_float_eps_tolerance.
  - **Risk:** Memory — naive scan reads each band fully; for >1GB rasters switch to streaming chunked reads (use `read_window` if available); document chunk size.
- [x] Implement CRS + spatial extent validation (combined module) (completed 2026-05-08)
  - **Goal:** New `CrsAndExtentValidator` emits issues for malformed/unrecognized CRS, axis-order mismatches, datum mismatches, geographic-bounds-out-of-range, projected-bounds-implausible, inverted/empty bounds, bounds-vs-pixel-grid arithmetic mismatches.
  - **Design:** CRS validation wraps `oxigdal-proj` (already a workspace dep). Issues: `CrsUnparseable` (Critical), `CrsAuthorityUnknown` (Major if parse failed; Warning if parsed but authority unknown — EPSG snapshot may be stale), `CrsAxisOrderAmbiguous` (Warning). Geographic bounds: lon ∈ [-180, 180], lat ∈ [-90, 90]; out → Critical. Projected bounds plausibility: per CRS family — UTM x ∈ ~[-167k, 833k] m; ECEF in [-6.7M, 6.7M]; Web Mercator ~[-2e7, 2e7]; out → Major (Warning if borderline). Inverted bounds (xmin > xmax or ymin > ymax) → Critical; zero-area → Major. Bounds-vs-pixel-grid: recompute `xmin + width × pixel_size_x` vs declared `xmax`; mismatch > 0.5 px → Major.
  - **Files:** New `crates/oxigdal-qc/src/raster/crs_extent.rs` (~400 LoC); modify `raster/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — oxigdal-proj already a dep.
  - **Tests:** test_crs_unparseable_critical, test_crs_axis_order_ambiguous_warns, test_geographic_bounds_lat_91_critical, test_projected_bounds_utm_implausible_x_majors, test_bounds_inverted_critical, test_bounds_zero_area_majors, test_bounds_pixel_grid_mismatch_majors, test_bounds_pixel_grid_within_half_px_passes.
  - **Risk:** EPSG database freshness — emit Warning (not Major) when authority unknown but parse succeeded. Axis-order: default OGC convention; document.
- [x] Implement spatial extent validation — see "CRS + spatial extent validation (combined module)" above (completed 2026-05-08)

---
*Last audited: 2026-05-16*
