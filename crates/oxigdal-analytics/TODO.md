# TODO: oxigdal-analytics

> **Purpose:** Pure-Rust geospatial analytics — time series (Mann-Kendall, anomaly), clustering (K-means, DBSCAN), hotspot (Getis-Ord Gi*, Moran's I, LISA), change detection (CVA, PCA, Otsu), interpolation (IDW, ordinary/universal kriging), advanced zonal statistics.
> **Status (2026-05-16):** 4,007 LoC · 57 tests · 0 in-source stubs (clean tree; only one test-data comment marking `0.0` as a placeholder for "missing" in `timeseries/mod.rs:265`).
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Universal Kriging with external drift variables
  - **Verified gap:** `src/lib.rs:107` doc-example references `KrigingType` enum; existing `interpolation` module covers IDW and ordinary kriging only (per the doc-example signature and previous TODO entry). External-drift variant is not yet exposed.
  - **Goal:** `KrigingType::Universal { drift_basis: Vec<DriftBasis> }` where `DriftBasis::{Constant, Linear, Quadratic, External(Vec<f64>)}`; predict `Z(x) = Σ μ_k·f_k(x) + ε(x)` with `f_k` the drift functions and `ε(x)` a zero-mean stationary residual modelled by a variogram (Cressie 1993 §3.4.2).
  - **Design:** Solve the augmented kriging system `[Γ F; Fᵀ 0] [λ; μ] = [γ_0; f_0]` where `Γ` is the variogram covariance matrix, `F` the design matrix of drift basis evaluations at known points, `γ_0` covariances to the target, and `f_0` the drift basis at the target. Cholesky via `scirs2-core::linalg`. Per-target `O(n³)` once; reuse `Γ⁻¹` across targets.
  - **Files:** `crates/oxigdal-analytics/src/interpolation/kriging.rs` (extend; new module if currently flat).
  - **Tests:** (proposed) `test_universal_kriging_constant_drift_equals_ordinary`, `test_universal_kriging_linear_drift_recovers_trend`, `test_universal_kriging_external_elevation_drift`, `test_universal_kriging_singular_design_matrix_errors`, `test_universal_kriging_matches_pykrige_reference_within_1e_6`.
  - **Risk:** Drift basis design-matrix conditioning — guard with reciprocal condition number check; document `pykrige` as the validation oracle.
  - **Prerequisites:** None (ordinary kriging path already exists per lib.rs example).
  - **Done:** 2026-05-22 (Slice 26). `src/interpolation/kriging.rs` extended by +468 lines (appended before `#[cfg(test)] mod tests`): `DriftBasis { Constant, Linear, Quadratic, External(Vec<f64>) }`, `UniversalKrigingOptions { drift_bases, variogram, regularization=1e-10 }`, `UniversalKrigingResult { predicted, variance, drift_coefficients }`, `universal_kriging_fit(coords, values, options, query_points, query_external_drift)`. Algorithm (Wackernagel 2003 §16): build Γ matrix (n×n variogram + Tikhonov `regularization * I` on diagonal), F matrix (n×p drift evaluations: Constant=1col, Linear=3col, Quadratic=6col, External=1col), augmented `[[Γ F]; [Fᵀ 0]]` system. Solved via private `gauss_jordan_invert_uked` (mirrors `OrdinaryKriging` solver style); decomposition reused across queries (predicted=λᵀy, variance=λᵀγ_0+μᵀf_0). Rank-deficient → `AnalyticsError::matrix_error("UKED design matrix singular (rank-deficient solve)")` — never panics. `interpolation/mod.rs` +3 lines re-exports.
  - **Tests:** 10 in `crates/oxigdal-analytics/tests/universal_kriging_test.rs` (constant-drift equals ordinary within 1e-8; linear drift recovers 2x+3y trend; quadratic drift recovers x²+y²; external elevation drift; singular design errors; at-sample-point returns observation within eps; variance nonnegative; drift_coefficients length matches p; mismatched external-drift length errors; default options = `vec![DriftBasis::Constant]`).

- [ ] Co-kriging for multivariate spatial interpolation
  - **Goal:** Joint estimation of two correlated variables (e.g., elevation + slope) using cross-variograms — predict primary variable using secondary samples at locations where primary is unsampled.
  - **Design:** Linear model of coregionalization (LMC) per Goovaerts 1997 §4.3.4. Two variograms γ₁, γ₂ plus a cross-variogram γ₁₂; solve a `(n₁+n₂)×(n₁+n₂)` block system. Validate semi-definiteness of the LMC sill matrix via eigenvalue check.
  - **Files:** `crates/oxigdal-analytics/src/interpolation/cokriging.rs` (new ~400 LoC).
  - **Tests:** (proposed) `test_cokriging_no_secondary_falls_back_to_ordinary`, `test_cokriging_strong_correlation_reduces_variance_vs_ordinary`, `test_cokriging_lmc_sill_matrix_psd`, `test_cokriging_collocated_secondary_special_case`.
  - **Risk:** LMC parameter fitting under-determined for sparse data — require explicit pre-fit sill matrix in API.
  - **Prerequisites:** Item 1 (universal kriging) for shared kriging-system solver.

- [x] OPTICS clustering as DBSCAN alternative
  - **Goal:** Ordering Points To Identify the Clustering Structure (Ankerst et al. 1999, SIGMOD'99) — outputs a reachability plot from which clusters at varying density can be extracted, addressing DBSCAN's single-`ε` limitation.
  - **Design:** Two passes: (1) build the ordered-list reachability plot via priority-queue traversal in O(n log n) given a spatial index; (2) extract clusters using `ξ`-extraction (Ankerst 1999 §4) or fixed-ε cutoff. Reuse `rstar` for neighbour queries (already a workspace dep elsewhere; add to this crate via workspace inheritance).
  - **Files:** `crates/oxigdal-analytics/src/clustering/optics.rs` (new ~500 LoC); update `clustering/mod.rs` re-exports.
  - **Tests:** (proposed) `test_optics_two_density_levels_single_pass`, `test_optics_reachability_plot_monotonic_within_cluster`, `test_optics_xi_extraction_matches_eps_cluster`, `test_optics_no_neighbours_marks_unreachable`.
  - **Risk:** ξ-extraction parameter selection — provide a sensible default (`ξ=0.05`) and document tuning.
  - **Prerequisites:** Add `rstar = { workspace = true }` to `Cargo.toml`.
  - **Done:** 2026-05-20 (Slice 24). New `src/clustering/optics.rs` (~600 LoC) implementing the full algorithm: `rstar::RTree::locate_within_distance` (bounded `max_eps`) + `nearest_neighbor_iter` (unbounded), `BinaryHeap<HeapEntry>` priority queue with `f64::total_cmp` reverse-ordering for min-heap semantics, reachability + core-distance emitted in visit-order. ξ-extraction per Ankerst 1999 §4 — steep-down `reach[i+1] ≤ reach[i]·(1-ξ)` + steep-up `reach[i] ≤ reach[i+1]·(1-ξ)`, earliest compatible up-run pairing, span ≥ min_samples validation. DBSCAN-compat extraction walks ordering for maximal contiguous slices with `reachability ≤ eps`. `rstar = { workspace = true }` added.
  - **Tests:** 16 in `crates/oxigdal-analytics/tests/optics_test.rs` (constant-density single block; two density levels single pass; reachability monotonic within cluster; isolated point → UNDEFINED; max_eps cap; min_samples filter; ordering starts from unprocessed; ξ steep-down/up pair; no steep areas; ξ=0.05 default recovers two levels; DBSCAN-compat eps=0.5 matches DbscanClusterer; eps=0 returns empty; empty input; single point; default options assertion).

- [x] Geographically Weighted Regression (GWR)
  - **Goal:** Locally fit a regression per location using nearby observations weighted by a spatial kernel (gaussian/bisquare/exponential), per Brunsdon, Fotheringham & Charlton 1996.
  - **Design:** At each prediction location `s`, solve a weighted least-squares `β(s) = (XᵀW(s)X)⁻¹ XᵀW(s)y` where `W(s)` is diagonal of distance-decay weights. Bandwidth: golden-section optimization on AICc. Output: per-location coefficient surfaces.
  - **Files:** `crates/oxigdal-analytics/src/regression/gwr.rs` (new ~600 LoC); new `regression/mod.rs`.
  - **Tests:** (proposed) `test_gwr_constant_data_returns_global_intercept`, `test_gwr_bandwidth_aicc_optimization`, `test_gwr_bisquare_kernel_zero_beyond_bandwidth`, `test_gwr_matches_spgwr_R_reference`.
  - **Risk:** Heavy O(n²) compute — use rayon parallel over prediction points; bound test sizes.
  - **Prerequisites:** None.
  - **Done:** 2026-05-22 (Slice 25). New `src/regression/{mod,gwr}.rs` (gwr.rs ~855 LoC): `GwrKernel { Gaussian, Bisquare, Exponential }`, `GwrBandwidth { Fixed(f64), AdaptiveKnn(usize) }`, `GwrOptions`, `GwrResult { coefficients, predicted, residuals, local_r2, bandwidth, aicc }`, `gwr_fit`. Per-location weighted-least-squares via the same Gauss-Jordan inversion path `kriging.rs` uses (no new deps); golden-section AICc bandwidth optimization driven by trace-of-hat-matrix; rank-deficiency → `AnalyticsError` (never panics). Per-location loop parallelized behind the existing `parallel` feature.
  - **Tests:** 10 in `crates/oxigdal-analytics/tests/gwr_test.rs` (constant-data global intercept; OLS recovery with huge bandwidth; bisquare zero beyond bandwidth; gaussian weights decrease with distance; local coefficients track spatial trend; adaptive-knn bandwidth; AICc optimization selects reasonable bandwidth; rank-deficient error; predicted+residual=y; single-predictor slope recovery).

- [ ] Local Moran's I scatterplot classification (HH / HL / LH / LL)
  - **Goal:** Extend the existing local Moran's I implementation to classify each observation into `{HighHigh, HighLow, LowHigh, LowLow, NotSignificant}` based on its standardized value and the spatially lagged mean of neighbours, then return classes alongside the local I statistic.
  - **Design:** For each location `i`: `z_i = (x_i - μ)/σ`; `lag_i = Σ_j w_ij·z_j`; classify by quadrant of `(z_i, lag_i)` with significance gate from existing permutation p-value (already in `hotspot`).
  - **Files:** `crates/oxigdal-analytics/src/hotspot/local_moran.rs` (extend).
  - **Tests:** (proposed) `test_local_moran_hh_quadrant_when_high_value_surrounded_by_high`, `test_local_moran_ll_quadrant_when_low_value_surrounded_by_low`, `test_local_moran_outlier_hl_classification`, `test_local_moran_pvalue_threshold_marks_nonsignificant`.
  - **Risk:** Quadrant boundary at exactly zero — assign deterministically to `NotSignificant`.
  - **Prerequisites:** None.

- [ ] Parallel zonal statistics for large rasters
  - **Goal:** Rayon-parallel zonal aggregator that streams the raster in row-blocks, accumulates per-zone running statistics (count, sum, sum-of-squares, min, max), and merges thread-local accumulators at the end.
  - **Design:** `ZonalCalculator::calculate_parallel(values, zones) -> ZonalResult`; block size = `ceil(rows / num_threads)`; per-thread `HashMap<ZoneId, RunningStats>` then a deterministic merge using `Welford's online algorithm` for numerically stable variance (Welford 1962).
  - **Files:** `crates/oxigdal-analytics/src/zonal/mod.rs` (extend).
  - **Tests:** (proposed) `test_zonal_parallel_matches_serial`, `test_zonal_parallel_welford_numerical_stability_large_values`, `test_zonal_parallel_empty_zones_dropped`, `test_zonal_parallel_thread_count_independent_result`.
  - **Risk:** Determinism across threads — Welford accumulation is associative; document tolerance.
  - **Prerequisites:** None; `parallel` feature already gated on `rayon`.

## Medium Priority
- [ ] KDE for point patterns (Gaussian/Epanechnikov kernels, automatic bandwidth via Silverman's rule).
  - **Files:** `src/spatial/kde.rs` (new), new `spatial/mod.rs`.
  - **Why deferred:** Demand from raster heatmap users; not blocking core analytics.
- [ ] Ripley's K and L functions for spatial point-pattern analysis.
  - **Files:** `src/spatial/ripley.rs` (new).
  - **Why deferred:** Niche; defer until KDE lands.
- [ ] Semivariogram cloud + model fitting (spherical, exponential, Gaussian).
  - **Files:** `src/interpolation/variogram_fit.rs` (new).
  - **Why deferred:** Manual sill/range setting works for now; fit is QoL.
- [ ] Spatial regression models (SAR, SEM, SDM).
  - **Files:** `src/regression/spatial_models.rs` (new).
  - **Why deferred:** After GWR lands (Item 4).
- [ ] Random-forest spatial classification.
  - **Files:** `src/classification/random_forest.rs` (new).
  - **Why deferred:** ML belongs primarily in oxigdal-ml; this would be a thin wrapper.
- [ ] Cross-validation framework for interpolation methods (leave-one-out, k-fold).
  - **Files:** `src/interpolation/cv.rs` (new).
  - **Why deferred:** Useful but not blocking.
- [ ] Empirical Bayesian Kriging (Krivoruchko 2012).
  - **Files:** `src/interpolation/ebk.rs` (new).
  - **Why deferred:** After universal kriging (Item 1).
- [ ] Multi-Resolution Index of Valley-Bottom Flatness (MRVBF, Gallant & Dowling 2003).
  - **Files:** `src/morphometry/mrvbf.rs` (new).
  - **Why deferred:** Overlap with oxigdal-terrain.
- [ ] ISODATA unsupervised classification.
  - **Files:** `src/clustering/isodata.rs` (new).
  - **Why deferred:** K-means covers most cases.

## Low Priority / Future (one-liners)
- [ ] Space-time kriging for spatiotemporal interpolation.
- [ ] Agent-based spatial simulation framework.
- [ ] Network-constrained spatial analysis (shortest path, service area).
- [ ] Fuzzy overlay analysis.
- [ ] Spatial sampling strategies (random, stratified, systematic, Latin hypercube).
- [ ] MCDA / AHP (multi-criteria decision analysis).

## Cross-crate dependencies
- **Blocks:** oxigdal-services (analytics endpoints), oxigdal-jupyter (notebook recipes).
- **Blocked by:** scirs2-core 0.4.4 linalg surface (already pinned).

## Recently completed (verbatim)
*(No `[x]` entries on previous TODO.)*

---
*Last audited: 2026-05-17*
