# TODO: oxigdal-sensors

> **Purpose:** Remote sensing / satellite-sensor data processing for OxiGDAL — sensor definitions (Landsat / Sentinel / MODIS / ASTER), radiometric correction, spectral indices, pan-sharpening, and image classification. (**NOT** an IoT sensor ingestion crate — that role belongs to oxigdal-mqtt + oxigdal-streaming.)
> **Status (2026-05-16):** 3,623 LoC · 128 tests · 4 real-code stubs
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Replace stub `MaximumLikelihood::classify` with a real Gaussian maximum-likelihood classifier
  - Done: 2026-05-31 (Slice 28). Tests: 9 new (mlc_test) + 70 existing = 79 total.
  - **Verified gap:** `src/classification/supervised.rs:16-24` — literal:
    `pub fn classify(&self, data: &ArrayView2<f64>, _training_data: &ArrayView2<f64>, _training_labels: &ArrayView1<usize>) -> Result<Array1<usize>> { // Simplified: assign all to class 0  Ok(Array1::zeros(data.nrows())) }`
  - **Goal:** Per-pixel class assignment using the standard Gaussian Maximum-Likelihood Classifier (GMLC, Richards 1999 "Remote Sensing Digital Image Analysis" §8.3): for each class compute mean vector μ_c and covariance Σ_c from training data, classify each pixel x to `argmax_c [-0.5·ln|Σ_c| - 0.5·(x-μ_c)ᵀ Σ_c⁻¹ (x-μ_c) + ln(P(c))]`.
  - **Design:** Use `scirs2-core` linalg (already a workspace dep at `Cargo.toml:43` with `linalg` feature on) for matrix inversion and log-determinant. Pre-compute per-class `(μ_c, Σ_c⁻¹, log_det_Σ_c)` once in `fit(...)` (new method) instead of recomputing per pixel. Default class prior = uniform; allow `with_priors(&[f64])` override. Reject singular covariance with `SensorError::SingularCovariance(class_id)`.
  - **Files:** `src/classification/supervised.rs:7-31` (replace stub); new internal `src/classification/gaussian.rs` for Σ⁻¹ / log|Σ| helpers.
  - **Tests:** (proposed) `test_mlc_two_class_well_separated_gaussians`, `test_mlc_prior_skews_decision_boundary`, `test_mlc_singular_covariance_returns_error`, `test_mlc_against_grass_imagery_iclass_reference_pixel`.
  - **Risk:** Numerical instability for low-band-count training sets — apply Tikhonov regularisation `Σ + λI` with λ=1e-6 default; document.
  - **Prerequisites:** None.

- [ ] Replace `BroveyTransform::sharpen` formula `(p·m) / (m + ε)` with the true per-band Brovey ratio
  - **Verified gap:** `src/pan_sharpening/brovey.rs:26-32` — literal:
    `*out = if m.abs() > 1e-10 { (p * m) / (m + 1e-10) } else { 0.0 };`
  - **Goal:** Correct Brovey Transform per Pohl & van Genderen (1998) "Multisensor image fusion": `MS_sharp_i = MS_i · (Pan / I)` where `I = (MS_R + MS_G + MS_B) / 3`. Operating on a single band against the panchromatic without the sum-of-bands normaliser produces incorrect chromatic balance.
  - **Design:** Change `PanSharpening` trait to accept multi-band MS input: `fn sharpen(&self, ms_bands: &[ArrayView2<f64>], pan: &ArrayView2<f64>) -> Result<Vec<Array2<f64>>>`. Brovey computes intensity `I` over all input bands once, then per band emits `ms_i · (pan / I)`. Maintain backwards-compat helper `sharpen_single(ms, pan)` that warns and forwards to the existing path.
  - **Files:** `src/pan_sharpening/mod.rs` (trait signature), `src/pan_sharpening/brovey.rs:12-36` (real formula).
  - **Tests:** (proposed) `test_brovey_three_band_preserves_chromatic_ratios`, `test_brovey_dimension_mismatch_errors`, `test_brovey_zero_intensity_pixel_handled_without_nan`.
  - **Risk:** Trait change is breaking; coordinate with any callers via deprecated alias for one release.
  - **Prerequisites:** None.

- [ ] Replace `IHSPanSharpening::sharpen` lerp-stub with real Intensity-Hue-Saturation substitution
  - **Verified gap:** `src/pan_sharpening/ihs.rs:19-27` — literal:
    `// Simplified IHS: Replace intensity with panchromatic  let mut sharpened = Array2::zeros(ms.dim());  Zip::from(&mut sharpened).and(ms).and(pan).for_each(|out, &m, &p| { *out = m + (p - m) * 0.5; });`
  - **Goal:** True IHS pan-sharpening per Carper, Lillesand, Kiefer (1990): convert (R, G, B) → (I, H, S); replace I with histogram-matched Pan; convert back to (R, G, B).
  - **Design:** Forward IHS transform (cylindrical-coordinate variant): `I = (R+G+B)/3`, `H = atan2(√3·(G-B), 2R-G-B)`, `S = 1 - 3·min(R,G,B)/(R+G+B)`. Histogram-match Pan to I via empirical CDF mapping. Inverse transform: `R = I·(1 + 2S·cos(H))`, `G = I·(1 - S·(cos(H) - √3·sin(H)))`, `B = I·(1 - S·(cos(H) + √3·sin(H)))`. Operate on 3-band stacks; extend trait per the Brovey item above.
  - **Files:** `src/pan_sharpening/ihs.rs:7-31`; share intensity helper with Brovey.
  - **Tests:** (proposed) `test_ihs_three_band_inverse_recovers_after_no_op`, `test_ihs_preserves_hue_after_intensity_replacement`, `test_ihs_histogram_matching_brings_pan_into_i_range`.
  - **Risk:** Histogram matching is non-trivial — guard against zero-bin denominators in CDF inverse.
  - **Prerequisites:** Trait signature update from the Brovey item.

- [ ] Replace `PCAPanSharpening::sharpen` 0.7/0.3 stub with real PCA-based sharpening
  - **Verified gap:** `src/pan_sharpening/pca.rs:19-27` — literal:
    `// Simplified PCA: Weight by variance  let mut sharpened = Array2::zeros(ms.dim());  Zip::from(&mut sharpened).and(ms).and(pan).for_each(|out, &m, &p| { *out = m * 0.7 + p * 0.3; });`
  - **Goal:** PCA pan-sharpening per Chavez & Kwarteng (1989): project multi-band MS into PC space, replace PC1 with histogram-matched Pan, inverse-project.
  - **Design:** Use `scirs2-core` linalg eigendecomp on the MS band-covariance matrix to obtain eigenvectors V. Compute scores `S = V·MS`. Histogram-match Pan to S[0] (PC1, the highest-variance component, typically the brightness axis). Replace S[0] with matched Pan; reconstruct `MS_sharp = V⁻¹·S`. Multi-band trait per Brovey item.
  - **Files:** `src/pan_sharpening/pca.rs:7-31`; reuse the histogram-match helper from the IHS item.
  - **Tests:** (proposed) `test_pca_eigenvectors_preserve_total_variance`, `test_pca_inverse_round_trips_when_pan_equals_pc1`, `test_pca_three_band_sharpening_against_synthetic_uniform_input`.
  - **Risk:** Eigen-decomp on rank-deficient covariance — fall back to SVD with rank check.
  - **Prerequisites:** Trait signature update from the Brovey item; histogram-match helper from IHS item.

## Medium Priority
- [ ] Full Ross-Thick / Li-Sparse BRDF (replace the documented placeholder)
  - **Goal:** Production-grade kernel-driven BRDF model per Wanner et al. 1995 ("On the derivation of kernels for kernel-driven models of bidirectional reflectance"). Current implementation has a documented placeholder.
  - **Files:** `src/radiometry/brdf.rs:185-197` — literal note: `// Note: Simplified BRDF implementation provided as placeholder. // For production use, consider full MODIS BRDF/Albedo implementation.`
  - **Why deferred:** MODIS MOD43A1 product (BRDF/Albedo) is a heavy spec; current placeholder produces reasonable values for non-extreme angles per test commentary.

- [ ] Sentinel-1 SAR radiometric calibration (sigma0 / gamma0 / beta0)
  - **Goal:** Convert SAR DN → backscatter coefficient (dB) per the Sentinel-1 IPF (Instrument Processing Facility) calibration ATBD: `sigma0_dB = 10·log10(DN² / A_σ²) ` with `A_σ` from the calibration XML annotation.
  - **Files:** new `src/radiometry/sar.rs`.
  - **Why deferred:** `src/sensors/sentinel.rs:110-141` defines the Sentinel-1 sensor metadata but no SAR-specific calibration path exists; needs XML annotation parser.

- [ ] 6S radiative transfer model for surface-reflectance retrieval
  - **Goal:** Atmospheric correction beyond DOS / cosine using vector 6S (Vermote 1997). Used in the LEDAPS/LaSRC Landsat surface reflectance products and Sen2Cor for Sentinel-2.
  - **Files:** new `src/radiometry/six_s.rs`.
  - **Why deferred:** Full 6S has heavy aerosol-model tables; consider integration with a Pure-Rust port of `Py6S` lookup tables.

- [ ] MODIS MOD35 cloud-mask integration (or compute from spectral tests)
  - **Goal:** Per-pixel cloud / cloud-shadow / clear mask per the MOD35 ATBD (Strabala 2005).
  - **Files:** new `src/sensors/modis_cloud.rs`.
  - **Why deferred:** Real MOD35 has 250+ test sub-results; even a Pass-1 thermal+visible decision tree is sizable.

- [ ] Sentinel-2 Scene Classification Layer (SCL) ingest + use as mask
  - **Goal:** Read SCL band from L2A product (Sen2Cor §scene_classification) and expose as a per-pixel `SceneClass` enum (Saturated, Defective, Shadow, CloudShadow, Vegetation, NotVegetated, Water, Cloud, ThinCirrus, Snow).
  - **Files:** new `src/sensors/sentinel2_scl.rs`.
  - **Why deferred:** Requires L2A file-format reader; depends on oxigdal-jpeg2000 / oxigdal-geotiff.

- [ ] Thermal-band brightness-temperature + LST computation
  - **Goal:** Brightness temperature already partially present (`src/radiometry/calibration.rs:149-177`); add Land Surface Temperature (LST) using mono-window (Qin et al. 2001) or split-window (Wan & Dozier 1996) algorithms.
  - **Files:** `src/radiometry/calibration.rs`, new `src/radiometry/lst.rs`.
  - **Why deferred:** Needs surface emissivity lookup (commonly from NDVI threshold method, Sobrino 2008).

- [ ] SAR speckle filters (Lee, Frost, Gamma-MAP)
  - **Goal:** Multiplicative-noise reduction on SAR backscatter — Lee (1980), Frost (1982), Gamma-MAP (Lopes 1990).
  - **Files:** new `src/radiometry/speckle.rs`.
  - **Why deferred:** Pair with SAR calibration above.

- [ ] Topographic correction (Minnaert, C-correction, SCS+C)
  - **Goal:** Reduce illumination differences on sloped terrain using DEM + sun-position. Cosine correction already at `src/radiometry/atmospheric.rs:111-167`.
  - **Files:** `src/radiometry/atmospheric.rs`.
  - **Why deferred:** Requires DEM dependency on oxigdal-terrain.

## Low Priority / Future (one-liners)
- [ ] WorldView-2/3 sensor definitions + spectral response functions
- [ ] VIIRS sensor support (DNB, I-bands, M-bands)
- [ ] Hyperspectral indices (PRI, RENDVI, REP)
- [ ] Sentinel-2 super-resolution (20m / 60m → 10m via DSen2 or geostatistical downscaling)
- [ ] Radiometric cross-calibration between sensors
- [ ] PlanetScope / SkySat sensor definitions
- [ ] Radar polarimetric decomposition (Freeman-Durden, Cloude-Pottier)
- [ ] InSAR coherence computation
- [ ] Multi-temporal SAR change detection
- [ ] Spectral unmixing (endmember extraction, abundance estimation)
- [ ] UAV / drone sensor calibration support
- [ ] SIF (Solar-Induced Fluorescence) retrieval

## Cross-crate dependencies
- **Blocks:** oxigdal-services (analytic raster API)
- **Blocked by:** oxigdal-terrain (DEM for topographic correction), oxigdal-jpeg2000 / oxigdal-geotiff (L2A scene loading)

## Recently completed (verbatim)
- *(none in this slice)*

---
*Last audited: 2026-05-16*
