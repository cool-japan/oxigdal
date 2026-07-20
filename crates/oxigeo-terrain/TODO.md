# TODO: oxigeo-terrain

> **Purpose:** Advanced terrain analysis and DEM processing for OxiGeo — derivatives, hydrology, viewshed, geomorphometry
> **Status (2026-05-16):** 4,847 Rust LoC · 103 unit tests (+ workspace integration) · 0 real-code stubs (clean tree after v0.1.5 hydrology/morphometry slice)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority
- [x] D-infinity (Tarboton 1997) flow direction/accumulation + Wang & Liu (2006) priority-flood sink fill (completed 2026-05-07)
  - **Goal:** Add scientifically-correct D-infinity flow direction/accumulation alongside existing D8, and replace iterative fill with O(n log n) Wang & Liu priority-flood. Match Whitebox/SAGA/GRASS reference outputs to within ε; process 4096×4096 DEM in <2s.
  - **Design:**
    1. D-infinity flow direction (Tarboton 1997): for each pixel examine 8 triangular facets; compute steepest down-slope vector (s1 cardinal, s2 diagonal); clamp to facet angular range; output angle in [0, 2π) CCW from east, or NaN for pits/flats.
    2. D-infinity flow accumulation: decompose flow into 1-2 downstream neighbours with weights w1=(β-θ)/(β-α), w2=(θ-α)/(β-α); topological sort by elevation descending; single-pass O(n) accumulation.
    3. Wang & Liu (2006) priority-flood: init min-heap with boundary pixels; pop lowest, set unvisited neighbours to max(elev, current+ε), push with new elevation; slope-preserving ε default 1e-9. Keep iterative fill as `fill_sinks_iterative` for parity.
    4. No-alloc-friendly: one BinaryHeap + one Vec<f64> buffer, no per-cell allocation.
  - **Files:**
    - `crates/oxigeo-terrain/src/hydrology/flow_direction.rs` (add flow_direction_dinf)
    - `crates/oxigeo-terrain/src/hydrology/flow_accumulation.rs` (add flow_accumulation_dinf)
    - `crates/oxigeo-terrain/src/hydrology/sink_fill.rs` (add fill_sinks_priority_flood; rename old to fill_sinks_iterative)
    - `crates/oxigeo-terrain/src/hydrology/mod.rs` (re-export new API)
  - **Tests:** test_dinf_uniform_slope_45deg, test_dinf_pit_returns_nan, test_dinf_tarboton_paper_example, test_dinf_accumulation_uniform_slope, test_dinf_accumulation_splits_proportional, test_priority_flood_simple_pit, test_priority_flood_complex_basin, test_priority_flood_preserves_non_sink_pixels, test_priority_flood_no_sinks_no_change, bench_priority_flood_4k_random
  - **Risk:** D-inf orientation convention varies in literature — document CCW-from-east in rustdoc; cross-check Tarboton 1997 Fig. 2. Wang & Liu ε in f64 default 1e-9; f32 callers must scale.
- [x] Implement Strahler stream ordering (replace existing stub) (planned 2026-05-08)
  - **Goal:** A channel grid where each channel cell carries its Strahler order σ ∈ {1, 2, …}; off-channel cells stay 0. Disconnected components ordered independently from their own heads.
  - **Design:** Stub-replacement at `crates/oxigeo-terrain/src/hydrology/stream_network.rs:34-57` (signature `Array2<u8>` preserved — max σ < 256 in real DEMs). Strahler always computed on D8 graph regardless of which algorithm produced the channel-defining accumulation grid (Tarboton 1991; D-inf cannot build a graph due to fractional flow split). Pipeline: `flow_direction_d8` → `flow_accumulation` → `extract_streams(threshold)` → topological sort by Kahn's algorithm → assign σ. Junction rule: σ_self = max(σ_children) + 1 iff ≥2 children share that max; otherwise max(σ_children); heads = 1. Add `strahler_order_from_d8(channel_mask, flow_dir_d8) -> Result<Array2<u8>>` for callers that already have both grids. Use `Vec`-based FIFO (no `HashMap` — hash iteration order leaks).
  - **Files:** `crates/oxigeo-terrain/src/hydrology/stream_network.rs` (replace body, add lower-level entry); `crates/oxigeo-terrain/src/hydrology/mod.rs` (re-export); `crates/oxigeo-terrain/src/lib.rs` (re-export).
  - **Prerequisites:** None — flow_direction_d8 + flow_accumulation + extract_streams already exist.
  - **Tests:** test_strahler_simple_y_junction, test_strahler_three_way_tied_max, test_strahler_one_dominant_tributary, test_strahler_disconnected_components, test_strahler_channel_head_only_non_channel_upstream, test_strahler_off_grid_outlet, test_strahler_unfilled_sink_returns_diagnostic, test_strahler_dinf_accumulation_threshold_d8_graph.
  - **Risk:** Cycle from epsilon underflow on f32 DEM post-fill — Kahn's leaves cells unprocessed; count and fail with diagnostic. Avoid O(n²) memory by never materializing adjacency map.
- [x] Add catchment/sub-watershed delineation from multiple pour points (planned 2026-05-08)
  - **Goal:** Given DEM, sink-filled flow-direction grid, and list of pour-point coordinates, emit `Array2<u32>` labelled grid (catchment IDs 1..N; 0 = outside) plus `Vec<CatchmentInfo>` summary `{id, pour_row, pour_col, area_cells, area_m2}`.
  - **Design:** Inverse traversal — for each cell, its parent set is the up-to-8 neighbours that flow into it (built on the fly from D8 flow direction). Per pour point, BFS upslope through the inverse graph; mark visited cells with pour-point ID. Overlap: earlier pour point in input list wins (deterministic). Snap modes: `SnapPolicy::ToHighestAccum(radius_cells)` (default radius=3, snap to highest-accumulation cell within radius), `SnapPolicy::Exact` (no snap; error if pour point not on a flow cell).
  - **Files:** New `crates/oxigeo-terrain/src/hydrology/catchment.rs` (~350 LoC); modify `hydrology/mod.rs` and `lib.rs` for re-exports.
  - **Prerequisites:** None — flow_direction_d8 already exists.
  - **Tests:** test_catchment_single_pour_point_simple_basin, test_catchment_two_disjoint_basins, test_catchment_overlapping_pour_points_first_wins, test_catchment_snap_to_max_accum, test_catchment_exact_no_snap, test_catchment_pour_point_outside_dem_errors.
  - **Risk:** Pour-point coords in geographic CRS while DEM is projected → silent area miscount; document that coords must match DEM CRS.
- [x] Implement profile and plan curvature (completed 2026-05-08)
  - **Goal:** Two raster outputs — profile curvature (1/m, positive = concave, negative = convex) and plan curvature (1/m, positive = divergent, negative = convergent flow).
  - **Design:** Use Zevenbergen & Thorne (1987) finite-difference formulation (superior numerical stability over Horn for second-derivative-based curvature; Horn's method is the slope/aspect family). 3×3 kernel. Profile: `Kpr = -(p²·r + 2·p·q·s + q²·t) / ((p² + q²) · (1 + p² + q²)^1.5)`. Plan: `Kpl = -(q²·r - 2·p·q·s + p²·t) / ((p² + q²)^1.5)`. NaN-safe: if `p² + q² < ε` return 0. Boundary cells (outermost row/col): emit `f64::NAN`. Wrap in `compute_curvature(dem, cell_size, nodata) -> Result<(Array2<f64>, Array2<f64>)>`.
  - **Files:** New `crates/oxigeo-terrain/src/morphometry/curvature.rs` (~300 LoC); new `crates/oxigeo-terrain/src/morphometry/mod.rs` shell; modify `lib.rs` to add `pub mod morphometry` and re-exports.
  - **Prerequisites:** None.
  - **Tests:** test_curvature_flat_dem_returns_zero, test_curvature_concave_bowl_positive_profile, test_curvature_convex_dome_negative_profile, test_curvature_planar_slope_zero_curvature, test_curvature_boundary_is_nan, test_curvature_units_per_meter.
  - **Risk:** NaN propagation on flat regions if division by `p²+q²` not guarded. Cell-size assumed isotropic; document.
- [ ] Add parallel tile-based processing for large DEMs

## Medium Priority
- [x] Implement topographic wetness index (TWI) (completed 2026-05-08)
  - **Goal:** Raster output `TWI = ln(a / tan(slope))` where `a` is specific catchment area. High = wet/saturated, low = ridge/dry.
  - **Design:** Reuse `flow_accumulation_dinf` (preferred for TWI; fractional split avoids overconcentrated flow lines). Specific catchment area `a = (A_total × pixel_area) / contour_width` where contour width ≈ `cell_size` (orthogonal flow) or `cell_size × √2` (diagonal). Slope: reuse existing slope helper if found; else compute Horn-method slope inline. Numerical floor: `tan(slope)` clamped at `1e-4` to keep TWI finite on flat areas. Output: `Array2<f64>`; nodata → NaN.
  - **Files:** New `crates/oxigeo-terrain/src/morphometry/twi.rs` (~200 LoC); modify `morphometry/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — flow_accumulation_dinf already exists.
  - **Tests:** test_twi_uniform_slope_constant, test_twi_higher_in_valley_than_ridge, test_twi_flat_cell_clamped_finite, test_twi_nodata_propagates_to_nan, test_twi_d8_vs_dinf_consistency_smoke.
  - **Risk:** Slope dependency — if no shared helper, this duplicates ~80 LoC; consolidate later.
- [x] Add solar radiation modeling (hillshade with sun position over time)
  - **Done:** 2026-05-22 (Slice 25). New `src/radiation/{mod,solar}.rs` gated `#[cfg(feature = "derivatives")]`: solar geometry (Cooper 1969 declination, eccentricity correction, hour angle, zenith/altitude, NOAA `atan2` azimuth clockwise-from-north), inline Horn 1981 slope/aspect, `hillshade_at` cos-incidence shaded relief, `solar_radiation` Beer-Lambert direct beam (transmittance + air-mass clamp) + isotropic-sky diffuse + cast-shadow azimuth ray-march + per-cell sunlit duration, `SolarOptions`/`SolarPosition`/`SolarRadiationResult`. NoData→NaN throughout. `lib.rs` adds 2 additive blocks.
  - **Tests:** 12 in `crates/oxigeo-terrain/tests/solar_test.rs` (equinox-noon equatorial near-overhead; sunrise altitude≈0; summer-solstice δ positive; flat DEM hillshade matches sin altitude; south-facing slope > north-facing (NH); insolation non-negative; cast shadow blocks low sun behind ridge; insolation 0 when sun below horizon; integrated insolation positive over day; NoData propagates NaN; diffuse nonzero when enabled; deterministic cast-shadow geometry).
- [x] Implement terrain ruggedness index (Riley et al.) (planned 2026-06-06)
  - **Goal:** Fix `tri_riley` in `src/derivatives/tri.rs` from `Σ|zᵢ−z_c|` to the Riley 1999 formula `sqrt(Σ(zᵢ−z_c)²)`; update tests.
  - **Design:** Edit the accumulation loop in `tri_riley` to sum squared diffs then take sqrt. Update all test expected values for `tri_riley` to match the corrected formula.
  - **Files:** `src/derivatives/tri.rs`
  - **Tests:** Update existing TRI-Riley tests; add one explicit check: `tri_riley` on 3×3 with constant off-center offset matches `sqrt(8 * offset^2)`.
  - **Risk:** Low — isolated formula fix; tests catch regressions.
- [x] Add multi-scale TPI (Topographic Position Index) for landform classification (planned 2026-06-06)
  - **Goal:** Add `tpi_annulus`, `tpi_standardized`, and `landform_classification_tpi` (Weiss 2001 10-class) to `src/derivatives/tpi.rs`.
  - **Design:** Annulus = box-window mean excluding Chebyshev distance < inner_r (mirror existing `tpi` box-mean + `is_nodata`/`validate_inputs`). Standardize via neighbourhood z-score. Weiss decision table: standardized TPI (small scale + large scale) + slope → 10 landform classes.
  - **Files:** extend `src/derivatives/tpi.rs`; update `src/derivatives/mod.rs` re-exports.
  - **Tests:** annulus-mean correctness; hilltop→positive TPI; valley→negative TPI; Weiss classification on synthetic DEM. (~6 tests)
  - **Risk:** Low — additive extension of existing tpi.rs.
- [x] Implement Fresnel zone analysis for viewshed (planned 2026-06-06)
  - **Goal:** `fresnel_zone_radius(freq_hz, d1, d2, n) -> f64` and `fresnel_clearance<T>(dem, tx, rx, freq_hz, cell_size, zone, nodata) -> Result<FresnelResult>` in `src/visibility/fresnel.rs`.
  - **Design:** `r_n = sqrt(n * lambda * d1 * d2 / (d1 + d2))`; walk DEM samples along tx→rx LOS; per-sample clearance = (LOS elevation − terrain elevation) − Fresnel radius; optional Earth-curvature bulge correction. FresnelResult: Vec<ClearanceSample>, worst_clearance_ratio, is_blocked.
  - **Files:** new `src/visibility/fresnel.rs`; register in `src/visibility/mod.rs`. Gated by `visibility` feature.
  - **Tests:** clear path → ratio ≥ 1; obstructed terrain → is_blocked; radius numeric check for known frequency and distances. (~5 tests)
  - **Risk:** Low — new file, only visibility/mod.rs needs adding.
- [x] Add cumulative viewshed (observer frequency surface) (planned 2026-06-06)
  - **Goal:** Verify correctness of existing `viewshed_cumulative`; add `viewshed_cumulative_parallel` under `#[cfg(feature="parallel")]`; mark done.
  - **Design:** Read `src/visibility/viewshed.rs`; confirm the observer-frequency accumulation is correct; add parallel variant using `rayon::par_iter` over observers then fold cumulative sum atomically.
  - **Files:** `src/visibility/viewshed.rs`; `src/visibility/mod.rs` re-exports.
  - **Tests:** verify existing tests pass; add parallel variant test that matches sequential output exactly.
  - **Risk:** Low — parallel variant is additive; correctness is already tested.
- [x] Implement valley depth and ridge height extraction (planned 2026-06-06)
  - **Goal:** `valley_depth<T>(dem, accumulation_threshold, cell_size, nodata) -> Result<Array2<f64>>` and `ridge_height<T>(...)` in `src/morphometry/valley_ridge.rs`.
  - **Design:** (1) sink-fill + flow-accumulation to identify channel cells (threshold on accumulation); (2) iterative Jacobi/Laplace relaxation to build smooth base-level surface with Dirichlet BC at channel cells (≤500 iterations, tolerance 1e-6); (3) `valley_depth = base_level − dem` (≥0 in valleys). `ridge_height` applies same logic on inverted DEM.
  - **Files:** new `src/morphometry/valley_ridge.rs`; register in `src/morphometry/mod.rs`. Gated by `derivatives` + `hydrology` features.
  - **Tests:** V-shaped valley → depth increases toward thalweg; planar → ≈0; synthetic ridge. (~5 tests)
  - **Risk:** Medium — Jacobi convergence; cap iterations + tolerance guards against infinite loops.
- [x] Add cost-distance/cost-path analysis on terrain surfaces (planned 2026-06-06)
  - **Goal:** `cost_distance<T>` and `least_cost_path<T>` in `src/hydrology/cost.rs`; add `TerrainError::NoPath { message }` to `src/error.rs`.
  - **Design:** Dijkstra over 8-connectivity using `BinaryHeap<(Reverse<NotNan<f64>>, idx)>` (ordered-float already a dep). Edge cost = mean of two cell friction values × distance (cell_size ortho, cell_size·√2 diagonal). nodata = impassable. Backlink array for path reconstruction. Optional `cost_allocation` (nearest source labeling per cell).
  - **Files:** new `src/hydrology/cost.rs`; register in `src/hydrology/mod.rs`; add `NoPath { message: String }` variant to `src/error.rs`. Gated by `hydrology` feature.
  - **Tests:** uniform cost surface → expected circular distance field; barrier wall → route goes around; unreachable destination → NoPath error; hand-computed small grid. (~7 tests)
  - **Risk:** Low — standard Dijkstra pattern; ordered-float already a dependency.
- [x] Implement channel network extraction with adaptive threshold (planned 2026-05-08)
  - **Goal:** From a DEM, produce binary channel mask `Array2<u8>` plus `Vec<ChannelSegment>` topological graph (head → confluence → outlet). Threshold modes: fixed, quantile, or auto-calibrated by Tarboton's slope-area method.
  - **Design:** `ThresholdMode::Fixed(u32)` / `Quantile(f64)` (e.g., 0.95 = top 5%) / `AreaSlope(c, θ)` (cells where `A · S^θ > c`). Mask: cell-wise comparison of accumulation grid against threshold. Segment extraction: walk channel mask under D8; channel head = no upstream channel-cell neighbours; junction = ≥2 incoming channel neighbours; segment = path between two such breakpoints. `ChannelSegment { head_idx, outlet_idx, cells: Vec<(usize, usize)>, strahler_order: Option<u8> }`. Optional `with_strahler == true` flag stamps each segment with its order (uses Item 1).
  - **Files:** New `crates/oxigeo-terrain/src/hydrology/channel_network.rs` (~400 LoC); modify `hydrology/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — flow_accumulation, extract_streams, flow_direction_d8 already exist; Strahler comes from Item 1 (sibling).
  - **Tests:** test_channel_fixed_threshold, test_channel_quantile_threshold, test_channel_area_slope_method, test_channel_segments_y_junction_breakpoints, test_channel_segments_with_strahler_stamps, test_channel_no_channels_returns_empty_segments.
  - **Risk:** Quantile mode pre-computes accumulation histogram — O(n) memory; acceptable for 10k×10k DEMs (~800 MB).

## Low Priority / Future
- [x] Add geomorphon classification (Jasiewicz & Stepinski) (planned 2026-06-06)
  - **Goal:** `geomorphons<T>(dem, cell_size, search_radius, skip_radius, flatness_deg, nodata) -> Result<Array2<u8>>` (10 landform classes: 1=flat,2=peak,3=ridge,4=shoulder,5=spur,6=slope,7=hollow,8=footslope,9=valley,10=pit) in `src/geomorphometry/geomorphon.rs`.
  - **Design:** For each cell cast 8 rays (N, NE, E, SE, S, SW, W, NW) using normalized DDA stepper (reuse pattern from `src/visibility/los.rs`); along [skip_radius, search_radius] track max zenith and nadir LOS angles; classify each direction as {+1, 0, -1} using flatness threshold derived from flatness_deg; map (n_plus, n_minus) integer tuple to 10 geomorphon classes via the canonical Jasiewicz & Stepinski 2013 lookup table (build as `const` array of 11×11).
  - **Files:** new `src/geomorphometry/geomorphon.rs`; register in `src/geomorphometry/mod.rs`. Gated by `geomorphometry` feature.
  - **Tests:** synthetic peak center → class 2 (peak); synthetic pit center → class 10 (pit); planar surface → class 1 (flat); simple ridge shape; simple valley shape; pattern-table spot check for known (n_plus, n_minus) pair. (~8 tests)
  - **Risk:** Medium — correct LOS angle tracking and flatness threshold logic; verify the canonical pattern lookup table carefully.
- [x] Implement terrain texture metrics (entropy, homogeneity) (planned 2026-06-06)
  - **Goal:** `glcm_texture<T>(dem, window_radius, levels, offset, nodata) -> Result<GlcmTextures>` with 6 Haralick feature rasters in `src/derivatives/texture.rs`. GlcmTextures struct: entropy, homogeneity (IDM), contrast, energy (ASM), correlation, dissimilarity — each an `Array2<f64>`.
  - **Design:** Linear-quantize DEM values to `[0, levels-1]`; per window extract all pixel pairs at (dy, dx) offset, build normalized GLCM matrix; compute the 6 Haralick features from the GLCM. Option to average over 4 directions (0°/45°/90°/135°).
  - **Files:** new `src/derivatives/texture.rs`; register in `src/derivatives/mod.rs`. Gated by `derivatives` feature.
  - **Tests:** uniform surface → entropy=0, energy=1, homogeneity=1; checkerboard pattern → high contrast; linear gradient surface. (~6 tests)
  - **Risk:** Low — self-contained new file; GLCM math is well-defined.
- [x] Add 3D terrain mesh generation (TIN from DEM) (planned 2026-06-06)
  - **Goal:** `tin_from_dem<T>(dem, origin, cell_size, max_error, max_points, nodata) -> Result<TerrainTin>` in `src/mesh/tin.rs`. TerrainTin: `vertices: Vec<[f64;3]>`, `triangles: Vec<[usize;3]>`, `interpolate_elevation` method (barycentric).
  - **Design:** Greedy VIP: seed 4 DEM corners → triangulate via `oxigeo_index::triangulate` → repeatedly find DEM cell with maximum vertical error vs TIN barycentric interpolation → insert it → stop when error ≤ max_error or points ≥ max_points. Lightweight terrain-local mesh type (no oxigeo-3d dependency). Add `mesh` feature = `["dep:oxigeo-index"]` to Cargo.toml.
  - **Files:** new `src/mesh/mod.rs` + `src/mesh/tin.rs`; add `mesh` feature to `Cargo.toml`; `pub mod mesh` in `src/lib.rs`. Gated by new `mesh` feature.
  - **Prerequisites:** oxigeo-index Delaunay triangulation (#12) must be implemented first.
  - **Tests:** planar DEM → only corner points needed; error decreases monotonically as more points added; bilinear interpolation accuracy on known DEM values. (~6 tests)
  - **Risk:** Medium — depends on oxigeo-index Delaunay being available; max_error stopping criterion; degenerate triangle handling.
- [ ] Implement glacial landform detection (cirques, moraines)
- [x] Add real-time terrain profile extraction along arbitrary polylines (planned 2026-06-06)
  - **Goal:** `extract_profile<T>(dem, polyline, origin, cell_size, step, nodata) -> Result<TerrainProfile>` in `src/morphometry/profile.rs`. TerrainProfile: `Vec<ProfilePoint { distance, x, y, elevation }>` + helpers (length(), min_elevation(), max_elevation(), total_gain(), total_loss()).
  - **Design:** Densify polyline vertices by `step` (in map units) to produce dense sample points; bilinear-interpolate DEM elevation at each sample (convert map coords → pixel coords via origin + cell_size); accumulate cumulative planar distance along the densified polyline.
  - **Files:** new `src/morphometry/profile.rs`; register in `src/morphometry/mod.rs`. Gated by `derivatives` feature.
  - **Tests:** straight horizontal line over planar gradient → linear profile; diagonal line; small known grid with hand-computed expected bilinear interpolated values. (~5 tests)
  - **Risk:** Low — new file, standard bilinear interpolation math.
- [ ] Implement flood simulation (simple 2D shallow water)
- [ ] Add integration with oxigeo-copc for LiDAR-derived DEMs

## Cross-crate dependencies
- **Blocks:** `oxigeo` (re-exported via `terrain` feature), `oxigeo-cli` (uses `dem`, `contour`, `fillnodata` subcommands)
- **Blocked by:** `oxigeo-core` (RasterBuffer, GeoTransform, Result/Error types)

---
*Last audited: 2026-05-16*
