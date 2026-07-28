# TODO: oxigeo-temporal

> **Purpose:** Multi-temporal raster analysis — time-indexed collections, temporal compositing, change detection, trend, phenology.
> **Status (2026-07-28):** 12,640 LoC · 182 tests · 5 doc-comment placeholder submodules remain (`compositing::{max_ndvi,mean,median}`, `gap_filling::{harmonic,interpolation}`); the BFAST, LandTrendr, STL, and Zarr-export-store stubs from the prior audit are now implemented (see Recently completed).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Implement real BFAST (Breaks For Additive Season and Trend) change detection
  - **Verified gap:** `src/change/detection.rs:377-378` — `// BFAST is complex - use CUSUM as approximation for now / Self::cusum_change(ts, config)`.
  - **Goal:** Detect abrupt and gradual changes in NDVI/EVI time series. Output per-pixel: break-time (Array3<i64>), magnitude (Array3<f64>), direction (Array3<i8>), confidence (Array3<f64>) — already supported by `ChangeDetectionResult`.
  - **Design:** BFAST decomposes Yt = Tt + St + et (trend + season + residual), then detects breaks in Tt and St separately using OLS-MOSUM (moving sum of recursive residuals) per Verbesselt et al. (2010, RSE 114:106-115). Steps per pixel: (1) harmonic seasonal model: Yt = β0 + β1·t + Σ(α_i·sin(2πit/T) + γ_i·cos(2πit/T)); fit via least squares. (2) Compute MOSUM statistic over window. (3) Identify break-times where MOSUM exceeds critical value (5% sig level: ~1.85 for h=0.15). (4) Refit segments. Use existing `scirs2_core::linalg` for least squares.
  - **Files:** `src/change/detection.rs:373-380` (replace `bfast_change` body), add `src/change/bfast.rs` (new ~400 LoC) for harmonic regression + MOSUM helpers.
  - **Tests:** *(proposed)* `test_bfast_no_break_no_change`, `test_bfast_detects_single_abrupt_break`, `test_bfast_detects_seasonal_shift`, `test_bfast_break_time_within_one_period`, `test_bfast_handles_short_series_gracefully`, `test_bfast_critical_value_5pct`.
  - **Risk:** OLS-MOSUM critical values from Chu et al. (1995); document numeric source. Per-pixel cost is O(n²) — must enable `parallel` feature for production-size DEMs.
  - **Prerequisites:** None — `scirs2_core::linalg` already a dep.
  - **Done:** 2026-05-22 (Slice 25). Stub `// BFAST is complex - use CUSUM as approximation for now / Self::cusum_change(ts, config)` at `src/change/detection.rs:377-378` swapped to `crate::change::bfast::bfast_detect(ts, config)`; signature byte-for-byte unchanged. New `src/change/bfast.rs` (~652 LoC): harmonic season+trend OLS fit via `scirs2_core::linalg::lstsq_ndarray`, OLS-MOSUM (h=0.15) with documented Chu-Hornik-Kuan 1995 critical-value table (5% ≈ 1.85), break localization at argmax|MOSUM|, magnitude = trend-mean shift, direction = sign, confidence bounded; relative-σ̂ floor guards degenerate (perfectly-modelled) series. `change/mod.rs` +1 line `pub mod bfast;`.
  - **Tests:** 10 in `crates/oxigeo-temporal/tests/bfast_test.rs` (no-break flat; no-break pure seasonal; single abrupt break; break time within one period; trend shift; short-series graceful; 5% critical value; magnitude sign matches direction; harmonic fit recovers coeffs; period/order inference).

- [x] Implement real LandTrendr (Landsat-based detection of Trends in Disturbance and Recovery)
  - **Verified gap:** `src/change/detection.rs:382-388` — `/// LandTrendr change detection (simplified approximation)` body just computes OLS slope per pixel.
  - **Goal:** Segment a yearly NDVI time series into ≤N piecewise-linear segments and emit segment vertices as breakpoints. Per Kennedy et al. (2010, RSE 114:2897-2910).
  - **Design:** Per-pixel iterative algorithm: (1) initial fit with N-1 segments at year-spaced vertices. (2) Compute MSE; iteratively remove vertex contributing least to fit. (3) After each removal, refit using point-to-point regression with continuity constraint. (4) Stop when next removal would exceed `p_of_f` threshold (F-stat). Emit final vertices as breakpoints with magnitude = slope change. Inputs: per-pixel annual NDVI vector; outputs: vertices `Vec<(year_idx, value)>` per pixel + magnitude/direction grids.
  - **Files:** `src/change/detection.rs:382-432` (replace `landtrendr_change` body), `src/change/landtrendr.rs` (new ~500 LoC) for segmentation kernel.
  - **Tests:** *(proposed)* `test_landtrendr_constant_no_vertices`, `test_landtrendr_single_disturbance_two_segments`, `test_landtrendr_recovery_after_disturbance`, `test_landtrendr_respects_max_segments`, `test_landtrendr_continuity_at_vertices`.
  - **Risk:** Reference implementation is in IDL (Google Earth Engine port available); cite Kennedy 2010 §2.2 for segmentation rules. Float precision near vertices critical — use f64.
  - **Prerequisites:** None.
  - **Done:** 2026-05-22 (Slice 26). New `src/change/landtrendr.rs` (~795 LoC): `LandTrendrOptions { max_segments=6, spike_threshold=0.9, vertex_count_overshoot=3, prevent_one_year_recovery=true, recovery_threshold=0.25, pval_threshold=0.05, best_model_proportion=0.75, min_observations=6 }`, `LandTrendrSegment`, `LandTrendrVertex`, `LandTrendrResult { vertices, segments, mse, p_of_f }`, `landtrendr_segment(values, options)`. Algorithm follows Kennedy 2010 §2.2: spike-pre-process (single-pass triplet damping), **iterative-bisection vertex seeding** (deviation from spec — bisection placement is the original Kennedy strategy; equal-spaced seeding misses step breakpoints), iterative least-MSE vertex removal, **LSQ-fitted anchor values** via `scirs2_core::linalg::lstsq_ndarray` (deviation from spec point-to-point — LSQ is what Kennedy 2010 actually does), F-statistic best-model selection against the full model (deviation from local pairwise walk — vs-full is the Kennedy criterion), prevent-one-year-recovery merge, recovery-threshold post-pass. `detection.rs::landtrendr_change` body swapped to per-pixel call (signature byte-for-byte unchanged); BFAST swap from Slice 25 preserved. `change/mod.rs` +4 lines (`pub mod landtrendr;` + re-exports). Approximate F-crit = 2.0 documented in source.
  - **Tests:** 12 in `crates/oxigeo-temporal/tests/landtrendr_test.rs` (constant→2-vertex; pure linear; single disturbance→3-vertex; recovery→4-vertex; max_segments cap; continuity at vertices; spike-dampening; under-min-observations error; default-options match Kennedy 2010; p_of_f threshold; prevent-one-year-recovery; end-to-end via `ChangeDetector::detect`). Full crate suite 116/116 (BFAST + STL + LandTrendr coexist).

- [x] Implement real STL (Seasonal-Trend decomposition using Loess)
  - **Verified gap:** `src/analysis/seasonality.rs:198` — `// For now, use additive decomposition as a placeholder / Self::additive_decomposition(ts, period)`.
  - **Goal:** Decompose time series into trend, seasonal, and residual components via STL (Cleveland et al. 1990, J. Off. Stat. 6:3-73) with Loess (LOcally Estimated Scatterplot Smoothing) for both seasonal and trend smoothing.
  - **Design:** Inner loop (n_i iterations): (1) detrend Yt - Tt^(k); (2) cycle-subseries smoothing via Loess of degree=1, q=`n_s`; (3) low-pass filter (length p, q=`n_l`); (4) detrending: St^(k+1) = cycle_smoothed - lowpass; (5) deseasonalize: Yt - St^(k+1); (6) Loess of deseasonalized → Tt^(k+1). Outer loop (n_o iterations) reweights with robustness weights. Default params: n_p=period, n_i=2, n_o=0 (non-robust), n_s=7, n_l=next_odd(period), n_t=next_odd(1.5·n_p / (1 - 1.5/n_s)).
  - **Files:** `src/analysis/seasonality.rs:196-202` (replace `stl_decomposition`), `src/analysis/loess.rs` (new ~250 LoC) for Loess primitive.
  - **Tests:** *(proposed)* `test_stl_pure_sine_extracts_seasonal`, `test_stl_pure_trend_extracts_trend`, `test_stl_decomposition_sums_to_original`, `test_stl_period_24_monthly_data`, `test_stl_robustness_iterations_dampen_outlier`, `test_loess_local_linear_recovers_line`.
  - **Risk:** Loess bandwidth q at series boundaries needs careful handling — use one-sided neighbors per Cleveland §3.5. Compute cost O(n·q) per pass; document for long series.
  - **Prerequisites:** None.
  - **Done:** 2026-05-20 (Slice 24). New `src/analysis/loess.rs` (~360 LoC) — tricube-weighted local polynomial fit (Cholesky via `scirs2_core::linalg::solve_ndarray`; weighted-mean fallback on rank-deficient design). New `src/analysis/stl.rs` (~360 LoC) — Cleveland 1990 inner+outer loop; one-sided weights at boundaries per §3.5; `with_robust()` engages 5 outer iterations with bisquare reweighting; `next_odd(x) = ceil(x).max(1) + (0 if odd else 1)`. `seasonality.rs::stl_decomposition` body replaced with a per-pixel call to `stl_decompose` (signature unchanged).
  - **Tests:** 17 in `crates/oxigeo-temporal/tests/stl_test.rs` (Loess: constant/linear identity, smooth-quadratic recovery, boundary one-sided, bandwidth-zero edge, rank-deficient fallback; STL: pure sine, pure trend, sum-invariant within 1e-10, period-24 monthly, robust outlier dampening, seasonal zero-mean-per-cycle, residual low-autocorrelation, short-series 2, default n_trend recipe, robust flag engages outer loop, raster integration).

- [x] Implement zarr store export return in DataCube::to_zarr_memory
  - **Verified gap:** `src/timeseries/datacube.rs:894-899` — `// Since ZarrV3Writer doesn't expose the store, we create a new one from the same Arc / This is a limitation - we need to return the data somehow / For now, return a new empty store as a placeholder / In practice, the store should be returned from the writer / Ok(MemoryStore::new())`.
  - **Goal:** `to_zarr_memory()` returns a `MemoryStore` containing the actual Zarr v3 artifacts (`.zarray` metadata + chunk blobs).
  - **Done:** 2026-07-21 (option (a), fully local). `MemoryStore` is a cheap handle over a shared `Arc<RwLock<HashMap>>`, so `to_zarr_memory()` now clones the store (`let result_store = store.clone();`) before handing one handle to `ZarrV3Writer::new`, writes all chunks + metadata, finalizes, and returns `result_store` — which observes exactly the data the writer wrote. No oxigeo-zarr API change needed. New regression test `test_zarr_memory_roundtrip` asserts the returned store is non-empty and that a full `to_zarr_memory → from_zarr_memory` round-trip reproduces every value + dimensions + metadata.

- [x] Implement actual lazy loading for `TimeSeriesRaster` entries
  - **Goal:** When entry created via `TemporalRasterEntry::new_lazy(metadata, source_path)`, deferred read realised via a configurable loader trait.
  - **Done:** 2026-07-21. Added the pluggable `RasterLoader` trait (`fn load(&self, source_path: &str) -> Result<Array3<f64>>`) in `src/timeseries/mod.rs` with a blanket impl for any `Fn(&str) -> Result<Array3<f64>>` closure, keeping the temporal crate format-agnostic (callers wire in oxigeo-geotiff / oxigeo-zarr without this crate depending on them). `TemporalRasterEntry` gained `ensure_loaded(&mut self, &dyn RasterLoader)`, `load_data(...)`, and `unload()`; `TimeSeriesRaster` gained `load_all(&dyn RasterLoader)` (idempotent, shape-validated). The `extract_pixel_timeseries` error message now names the real remediation. `new_lazy` doc clarified. Tests: `test_lazy_entry_loads_via_loader`, `test_load_all_idempotent_and_shape_validated`, `test_ensure_loaded_and_unload_roundtrip`, `test_load_data_without_source_path_errors`.
  - **Deferred:** an LRU eviction policy and a bundled default GeoTIFF/NetCDF `FileSystemLoader` (would pull oxigeo-geotiff/oxigeo-zarr into this crate) remain out of scope; the pluggable trait already makes lazy loading fully functional.

## Medium Priority
- [ ] Promote empty submodule placeholders to documented re-exports or remove them
  - **Verified gap:** `src/compositing/{max_ndvi,mean,median}.rs` (each 7 lines) — `//! This module serves as a placeholder for potential future specialized {NDVI,mean,median} algorithms.`
  - **Verified gap:** `src/gap_filling/{harmonic,interpolation}.rs` (each 7 lines) — `//! This module serves as a placeholder for potential future specialized {harmonic,interpolation} algorithms.`
  - **Goal:** Either implement specialised algorithms or delete and consolidate into parent module to reduce surface confusion.
  - **Files:** as above (5 files, ~35 LoC total).
  - **Why deferred:** Existing functionality lives in parent `mod.rs` (compositing/mod.rs is 503 LoC, gap_filling/mod.rs is 474 LoC); skeleton is harmless but noisy.

- [x] Whittaker smoother for noisy NDVI time series
  - Done: 2026-05-31 (Slice 29). Tests: 28 new (smoothing_test + inline in whittaker.rs + savgol.rs) + 116 existing = 144 total.
  - Eilers 2003 `(W + λ·Dᵀ·D)·z = W·y` via `solve_ndarray` (dense, O(n³), fine for NDVI-length n≤100). Weights 0 at NaN. New `src/gap_filling/whittaker.rs`, `GapFillMethod::Whittaker`, `GapFillParams::{whittaker_lambda=100.0, whittaker_order=2}`.

- [x] Savitzky-Golay filter for vegetation index time series
  - Done: 2026-05-31 (Slice 29). Tests: included in smoothing_test above.
  - Vandermonde normal-equation kernel (`(AᵀA)⁻¹ e_{center}` via `solve_ndarray`), asymmetric edge windows, NaN pre-interpolation. New `src/gap_filling/savgol.rs`, `GapFillMethod::SavitzkyGolay`, `GapFillParams::{savgol_window=7, savgol_poly_order=2}`.

- [ ] Harmonic regression for phenology extraction
  - **Goal:** Fourier model Yt = β0 + Σ(α_i·sin + γ_i·cos) → green-up/peak/senescence dates.
  - **Files:** `src/phenology.rs` (existing, 261 LoC).
  - **Why deferred:** Threshold-based phenology metrics already in `phenology.rs`; harmonic version is enhancement.

- [ ] Holt-Winters triple exponential smoothing forecasting
  - **Goal:** Additive/multiplicative seasonal forecasting (level + trend + season).
  - **Files:** `src/analysis/forecast.rs` (existing, 398 LoC).
  - **Why deferred:** Simple ARIMA/linear baselines may suffice; HW adds parameter-tuning complexity.

- [ ] Real CUSUM critical-value tables for change detection
  - **Goal:** Tighten `CUSUM` (currently arbitrary `threshold` param at `src/change/detection.rs:337`) to use Page (1954) or Hawkins critical values for given α/n.
  - **Files:** `src/change/detection.rs:320-370`.
  - **Why deferred:** Functional today; statistical rigor enhancement.

- [ ] Cloud-backed time series (stream rasters from S3/GCS via oxigeo-rs3gw)
  - **Goal:** `RasterLoader` impl for object-store URLs; reuse `oxigeo-rs3gw` byte-range reader.
  - **Files:** `src/timeseries/loader.rs` (new — see lazy-loading task above).
  - **Why deferred:** Blocked by lazy-loading trait (High Priority).

## Low Priority / Future (one-liners)
- [ ] BFAST Monitor variant for near-real-time disturbance detection
- [ ] Temporal data cube slicing with xarray-like coordinate syntax
- [ ] Time series animation / GIF export
- [ ] Temporal resampling between different observation frequencies (e.g., Landsat 16-day → MODIS 8-day)
- [ ] STAC catalog integration for temporal queries (oxigeo-stac integration)
- [ ] Multi-sensor fusion (Landsat + Sentinel-2 BRDF harmonization)
- [ ] NetCDF time dimension read/write support
- [ ] Cross-correlation between two time series rasters (lag detection)

## Cross-crate dependencies
- **Blocks:** None
- **Blocked by:** oxigeo-zarr (for `to_zarr_memory` return-store fix; option (a) local workaround exists), oxigeo-geotiff (for lazy-loading default loader)

## Recently completed (verbatim)
- [x] CUSUM change detection (real implementation) — `src/change/detection.rs:320-370`
- [x] Simple difference / absolute change / relative change / Z-score / threshold change detection — `src/change/detection.rs`
- [x] Additive and multiplicative seasonal decomposition (moving-average based) — `src/analysis/seasonality.rs:108-194`
- [x] Period detection via autocorrelation — `src/analysis/seasonality.rs:265-...`
- [x] Mann-Kendall trend test + Sen's slope + linear regression — `src/analysis/trend.rs` (435 LoC)
- [x] Anomaly detection (Z-score, IQR, MAD) — `src/analysis/anomaly.rs` (331 LoC)
- [x] Forecast methods (simple exponential, Holt linear trend, ARIMA basic, naive) — `src/analysis/forecast.rs` (398 LoC)
- [x] Temporal compositing (median, mean, max-NDVI, quality-weighted) — `src/compositing/mod.rs` (503 LoC)
- [x] Gap filling (linear, nearest, spline) — `src/gap_filling/mod.rs` (474 LoC)
- [x] Temporal aggregation (daily/weekly/monthly/yearly/rolling windows) — `src/aggregation.rs` (765 LoC)
- [x] Phenology metric extraction (start/peak/end of season) — `src/phenology.rs` (261 LoC)
- [x] DataCube (4D time × y × x × bands) with Zarr v3 reader integration — `src/timeseries/datacube.rs` (1537 LoC, near <2000 LoC policy limit — refactor candidate)
- [x] TimeSeriesRaster with BTreeMap timestamp index, gap detection, expected shape validation — `src/timeseries/mod.rs` (728 LoC)
- [x] RasterStack abstraction with stack metadata — `src/stack.rs` (746 LoC)
- [x] Breakpoint detection (Pettitt test, sliding-window) — `src/change/breakpoint.rs` (404 LoC)

---
*Last audited: 2026-07-28*
