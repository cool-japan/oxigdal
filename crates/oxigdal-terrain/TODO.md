# TODO: oxigdal-terrain

> **Purpose:** Advanced terrain analysis and DEM processing for OxiGDAL — derivatives, hydrology, viewshed, geomorphometry
> **Status (2026-05-16):** 4,847 Rust LoC · 103 unit tests (+ workspace integration) · 0 real-code stubs (clean tree after v0.1.5 hydrology/morphometry slice)
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority
- [x] D-infinity (Tarboton 1997) flow direction/accumulation + Wang & Liu (2006) priority-flood sink fill (completed 2026-05-07)
  - **Goal:** Add scientifically-correct D-infinity flow direction/accumulation alongside existing D8, and replace iterative fill with O(n log n) Wang & Liu priority-flood. Match Whitebox/SAGA/GRASS reference outputs to within ε; process 4096×4096 DEM in <2s.
  - **Design:**
    1. D-infinity flow direction (Tarboton 1997): for each pixel examine 8 triangular facets; compute steepest down-slope vector (s1 cardinal, s2 diagonal); clamp to facet angular range; output angle in [0, 2π) CCW from east, or NaN for pits/flats.
    2. D-infinity flow accumulation: decompose flow into 1-2 downstream neighbours with weights w1=(β-θ)/(β-α), w2=(θ-α)/(β-α); topological sort by elevation descending; single-pass O(n) accumulation.
    3. Wang & Liu (2006) priority-flood: init min-heap with boundary pixels; pop lowest, set unvisited neighbours to max(elev, current+ε), push with new elevation; slope-preserving ε default 1e-9. Keep iterative fill as `fill_sinks_iterative` for parity.
    4. No-alloc-friendly: one BinaryHeap + one Vec<f64> buffer, no per-cell allocation.
  - **Files:**
    - `crates/oxigdal-terrain/src/hydrology/flow_direction.rs` (add flow_direction_dinf)
    - `crates/oxigdal-terrain/src/hydrology/flow_accumulation.rs` (add flow_accumulation_dinf)
    - `crates/oxigdal-terrain/src/hydrology/sink_fill.rs` (add fill_sinks_priority_flood; rename old to fill_sinks_iterative)
    - `crates/oxigdal-terrain/src/hydrology/mod.rs` (re-export new API)
  - **Tests:** test_dinf_uniform_slope_45deg, test_dinf_pit_returns_nan, test_dinf_tarboton_paper_example, test_dinf_accumulation_uniform_slope, test_dinf_accumulation_splits_proportional, test_priority_flood_simple_pit, test_priority_flood_complex_basin, test_priority_flood_preserves_non_sink_pixels, test_priority_flood_no_sinks_no_change, bench_priority_flood_4k_random
  - **Risk:** D-inf orientation convention varies in literature — document CCW-from-east in rustdoc; cross-check Tarboton 1997 Fig. 2. Wang & Liu ε in f64 default 1e-9; f32 callers must scale.
- [x] Implement Strahler stream ordering (replace existing stub) (planned 2026-05-08)
  - **Goal:** A channel grid where each channel cell carries its Strahler order σ ∈ {1, 2, …}; off-channel cells stay 0. Disconnected components ordered independently from their own heads.
  - **Design:** Stub-replacement at `crates/oxigdal-terrain/src/hydrology/stream_network.rs:34-57` (signature `Array2<u8>` preserved — max σ < 256 in real DEMs). Strahler always computed on D8 graph regardless of which algorithm produced the channel-defining accumulation grid (Tarboton 1991; D-inf cannot build a graph due to fractional flow split). Pipeline: `flow_direction_d8` → `flow_accumulation` → `extract_streams(threshold)` → topological sort by Kahn's algorithm → assign σ. Junction rule: σ_self = max(σ_children) + 1 iff ≥2 children share that max; otherwise max(σ_children); heads = 1. Add `strahler_order_from_d8(channel_mask, flow_dir_d8) -> Result<Array2<u8>>` for callers that already have both grids. Use `Vec`-based FIFO (no `HashMap` — hash iteration order leaks).
  - **Files:** `crates/oxigdal-terrain/src/hydrology/stream_network.rs` (replace body, add lower-level entry); `crates/oxigdal-terrain/src/hydrology/mod.rs` (re-export); `crates/oxigdal-terrain/src/lib.rs` (re-export).
  - **Prerequisites:** None — flow_direction_d8 + flow_accumulation + extract_streams already exist.
  - **Tests:** test_strahler_simple_y_junction, test_strahler_three_way_tied_max, test_strahler_one_dominant_tributary, test_strahler_disconnected_components, test_strahler_channel_head_only_non_channel_upstream, test_strahler_off_grid_outlet, test_strahler_unfilled_sink_returns_diagnostic, test_strahler_dinf_accumulation_threshold_d8_graph.
  - **Risk:** Cycle from epsilon underflow on f32 DEM post-fill — Kahn's leaves cells unprocessed; count and fail with diagnostic. Avoid O(n²) memory by never materializing adjacency map.
- [x] Add catchment/sub-watershed delineation from multiple pour points (planned 2026-05-08)
  - **Goal:** Given DEM, sink-filled flow-direction grid, and list of pour-point coordinates, emit `Array2<u32>` labelled grid (catchment IDs 1..N; 0 = outside) plus `Vec<CatchmentInfo>` summary `{id, pour_row, pour_col, area_cells, area_m2}`.
  - **Design:** Inverse traversal — for each cell, its parent set is the up-to-8 neighbours that flow into it (built on the fly from D8 flow direction). Per pour point, BFS upslope through the inverse graph; mark visited cells with pour-point ID. Overlap: earlier pour point in input list wins (deterministic). Snap modes: `SnapPolicy::ToHighestAccum(radius_cells)` (default radius=3, snap to highest-accumulation cell within radius), `SnapPolicy::Exact` (no snap; error if pour point not on a flow cell).
  - **Files:** New `crates/oxigdal-terrain/src/hydrology/catchment.rs` (~350 LoC); modify `hydrology/mod.rs` and `lib.rs` for re-exports.
  - **Prerequisites:** None — flow_direction_d8 already exists.
  - **Tests:** test_catchment_single_pour_point_simple_basin, test_catchment_two_disjoint_basins, test_catchment_overlapping_pour_points_first_wins, test_catchment_snap_to_max_accum, test_catchment_exact_no_snap, test_catchment_pour_point_outside_dem_errors.
  - **Risk:** Pour-point coords in geographic CRS while DEM is projected → silent area miscount; document that coords must match DEM CRS.
- [x] Implement profile and plan curvature (completed 2026-05-08)
  - **Goal:** Two raster outputs — profile curvature (1/m, positive = concave, negative = convex) and plan curvature (1/m, positive = divergent, negative = convergent flow).
  - **Design:** Use Zevenbergen & Thorne (1987) finite-difference formulation (superior numerical stability over Horn for second-derivative-based curvature; Horn's method is the slope/aspect family). 3×3 kernel. Profile: `Kpr = -(p²·r + 2·p·q·s + q²·t) / ((p² + q²) · (1 + p² + q²)^1.5)`. Plan: `Kpl = -(q²·r - 2·p·q·s + p²·t) / ((p² + q²)^1.5)`. NaN-safe: if `p² + q² < ε` return 0. Boundary cells (outermost row/col): emit `f64::NAN`. Wrap in `compute_curvature(dem, cell_size, nodata) -> Result<(Array2<f64>, Array2<f64>)>`.
  - **Files:** New `crates/oxigdal-terrain/src/morphometry/curvature.rs` (~300 LoC); new `crates/oxigdal-terrain/src/morphometry/mod.rs` shell; modify `lib.rs` to add `pub mod morphometry` and re-exports.
  - **Prerequisites:** None.
  - **Tests:** test_curvature_flat_dem_returns_zero, test_curvature_concave_bowl_positive_profile, test_curvature_convex_dome_negative_profile, test_curvature_planar_slope_zero_curvature, test_curvature_boundary_is_nan, test_curvature_units_per_meter.
  - **Risk:** NaN propagation on flat regions if division by `p²+q²` not guarded. Cell-size assumed isotropic; document.
- [ ] Add parallel tile-based processing for large DEMs

## Medium Priority
- [x] Implement topographic wetness index (TWI) (completed 2026-05-08)
  - **Goal:** Raster output `TWI = ln(a / tan(slope))` where `a` is specific catchment area. High = wet/saturated, low = ridge/dry.
  - **Design:** Reuse `flow_accumulation_dinf` (preferred for TWI; fractional split avoids overconcentrated flow lines). Specific catchment area `a = (A_total × pixel_area) / contour_width` where contour width ≈ `cell_size` (orthogonal flow) or `cell_size × √2` (diagonal). Slope: reuse existing slope helper if found; else compute Horn-method slope inline. Numerical floor: `tan(slope)` clamped at `1e-4` to keep TWI finite on flat areas. Output: `Array2<f64>`; nodata → NaN.
  - **Files:** New `crates/oxigdal-terrain/src/morphometry/twi.rs` (~200 LoC); modify `morphometry/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — flow_accumulation_dinf already exists.
  - **Tests:** test_twi_uniform_slope_constant, test_twi_higher_in_valley_than_ridge, test_twi_flat_cell_clamped_finite, test_twi_nodata_propagates_to_nan, test_twi_d8_vs_dinf_consistency_smoke.
  - **Risk:** Slope dependency — if no shared helper, this duplicates ~80 LoC; consolidate later.
- [x] Add solar radiation modeling (hillshade with sun position over time)
  - **Done:** 2026-05-22 (Slice 25). New `src/radiation/{mod,solar}.rs` gated `#[cfg(feature = "derivatives")]`: solar geometry (Cooper 1969 declination, eccentricity correction, hour angle, zenith/altitude, NOAA `atan2` azimuth clockwise-from-north), inline Horn 1981 slope/aspect, `hillshade_at` cos-incidence shaded relief, `solar_radiation` Beer-Lambert direct beam (transmittance + air-mass clamp) + isotropic-sky diffuse + cast-shadow azimuth ray-march + per-cell sunlit duration, `SolarOptions`/`SolarPosition`/`SolarRadiationResult`. NoData→NaN throughout. `lib.rs` adds 2 additive blocks.
  - **Tests:** 12 in `crates/oxigdal-terrain/tests/solar_test.rs` (equinox-noon equatorial near-overhead; sunrise altitude≈0; summer-solstice δ positive; flat DEM hillshade matches sin altitude; south-facing slope > north-facing (NH); insolation non-negative; cast shadow blocks low sun behind ridge; insolation 0 when sun below horizon; integrated insolation positive over day; NoData propagates NaN; diffuse nonzero when enabled; deterministic cast-shadow geometry).
- [ ] Implement terrain ruggedness index (Riley et al.)
- [ ] Add multi-scale TPI (Topographic Position Index) for landform classification
- [ ] Implement Fresnel zone analysis for viewshed
- [ ] Add cumulative viewshed (observer frequency surface)
- [ ] Implement valley depth and ridge height extraction
- [ ] Add cost-distance/cost-path analysis on terrain surfaces
- [x] Implement channel network extraction with adaptive threshold (planned 2026-05-08)
  - **Goal:** From a DEM, produce binary channel mask `Array2<u8>` plus `Vec<ChannelSegment>` topological graph (head → confluence → outlet). Threshold modes: fixed, quantile, or auto-calibrated by Tarboton's slope-area method.
  - **Design:** `ThresholdMode::Fixed(u32)` / `Quantile(f64)` (e.g., 0.95 = top 5%) / `AreaSlope(c, θ)` (cells where `A · S^θ > c`). Mask: cell-wise comparison of accumulation grid against threshold. Segment extraction: walk channel mask under D8; channel head = no upstream channel-cell neighbours; junction = ≥2 incoming channel neighbours; segment = path between two such breakpoints. `ChannelSegment { head_idx, outlet_idx, cells: Vec<(usize, usize)>, strahler_order: Option<u8> }`. Optional `with_strahler == true` flag stamps each segment with its order (uses Item 1).
  - **Files:** New `crates/oxigdal-terrain/src/hydrology/channel_network.rs` (~400 LoC); modify `hydrology/mod.rs` and `lib.rs`.
  - **Prerequisites:** None — flow_accumulation, extract_streams, flow_direction_d8 already exist; Strahler comes from Item 1 (sibling).
  - **Tests:** test_channel_fixed_threshold, test_channel_quantile_threshold, test_channel_area_slope_method, test_channel_segments_y_junction_breakpoints, test_channel_segments_with_strahler_stamps, test_channel_no_channels_returns_empty_segments.
  - **Risk:** Quantile mode pre-computes accumulation histogram — O(n) memory; acceptable for 10k×10k DEMs (~800 MB).

## Low Priority / Future
- [ ] Add geomorphon classification (Jasiewicz & Stepinski)
- [ ] Implement terrain texture metrics (entropy, homogeneity)
- [ ] Add 3D terrain mesh generation (TIN from DEM)
- [ ] Implement glacial landform detection (cirques, moraines)
- [ ] Add real-time terrain profile extraction along arbitrary polylines
- [ ] Implement flood simulation (simple 2D shallow water)
- [ ] Add integration with oxigdal-copc for LiDAR-derived DEMs

## Cross-crate dependencies
- **Blocks:** `oxigdal` (re-exported via `terrain` feature), `oxigdal-cli` (uses `dem`, `contour`, `fillnodata` subcommands)
- **Blocked by:** `oxigdal-core` (RasterBuffer, GeoTransform, Result/Error types)

---
*Last audited: 2026-05-16*
