# TODO: oxigeo-ml

> **Purpose:** Geospatial machine-learning runtime — ONNX inference via Pure-Rust `oxionnx`, segmentation/classification/detection, batch & tile inference, model zoo, monitoring.
> **Status (2026-05-16):** 19,728 LoC · 359 tests (1 skipped per workspace memory) · 3 real stubs surfaced in src tree (WebGPU sync enumeration, TFLite quantization stamp, temporarily-disabled `coreml`/`tflite`/`temporal` features).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Re-enable `coreml` feature against `objc2` 0.6
  - **Verified gap:** `Cargo.toml:29-31` — `# TEMPORARY: Commented out due to objc2 API compatibility issues / CoreML code needs updates for objc2 0.6 breaking changes (alloc, NSArray::from_slice, etc.)`
  - **Goal:** Pure-Rust CoreML execution provider on macOS/iOS, gated behind `coreml` feature, dispatching to `oxionnx` `ExecutionProvider::CoreML`.
  - **Design:** Migrate to `objc2 = "0.6"` + `objc2-core-ml = "0.3"` + `objc2-foundation = "0.3"`; replace `alloc()` with `mtl_init()`/`new()` per objc2 0.6 migration guide; switch `NSArray::from_slice` call sites to `NSArray::from_retained_slice` (new signature); use `MLModelConfiguration::new()` then `setComputeUnits:` instead of struct-field assignment.
  - **Files:** `crates/oxigeo-ml/src/gpu.rs` (CoreML adapter section), `Cargo.toml` (uncomment `coreml` feature + deps).
  - **Tests:** (proposed) `test_coreml_provider_available_on_macos`, `test_coreml_inference_identity_model`, `test_coreml_feature_compile_only_when_macos`.
  - **Risk:** objc2 0.6 transitive ABI churn — pin versions; ensure CI matrix excludes `coreml` on non-Apple targets.
  - **Prerequisites:** None — `oxionnx` already supports the `coreml` execution provider name.

- [ ] Async WebGPU device enumeration
  - **Verified gap:** `src/gpu.rs:1344-1346` — `// WebGPU device enumeration requires async operations which cannot be / done in a synchronous context. Instead, we return a placeholder device / if WebGPU is available.`
  - **Goal:** Replace the placeholder-device synchronous return with a real async query under `cfg(all(feature = "webgpu", target_arch = "wasm32"))`, while keeping a synchronous `Vec::new()` fallback path for downstream callers that cannot `.await`.
  - **Design:** Introduce `async fn enumerate_webgpu_devices_async() -> GpuResult<Vec<GpuDevice>>` using `wasm-bindgen-futures::JsFuture` over `navigator.gpu.requestAdapter(GpuPowerPreference::HighPerformance|LowPower)`; query `adapter.info` (chromium 121+ exposes `vendor`, `architecture`, `device`, `description`) and `adapter.limits` (`maxBufferSize`, `maxStorageBufferBindingSize`) per WebGPU spec WD 2024-09-12. Keep the current sync wrapper but mark it `#[deprecated(note = "Use enumerate_webgpu_devices_async on wasm32")]`.
  - **Files:** `crates/oxigeo-ml/src/gpu.rs` (extend `enumerate_webgpu_devices` block).
  - **Tests:** (proposed) `test_webgpu_async_enumeration_returns_at_least_one_adapter` (wasm32 only, behind `wasm-bindgen-test`), `test_webgpu_sync_fallback_returns_empty_native`.
  - **Risk:** wasm-bindgen-futures pulls additional WASM bloat; gate strictly behind `webgpu` + `target_arch = "wasm32"`.
  - **Prerequisites:** None.

- [ ] Real TFLite quantization scale/zero-point extraction
  - **Verified gap:** `src/models/tflite.rs:427` — `// Mark tensors as quantized (placeholder) … scale: 0.003921569, // 1/255 … zero_point: 0,`
  - **Goal:** Pull `QuantizationParameters{ scale[], zero_point[] }` from the TFLite FlatBuffer schema (`Tensor` field 5) rather than stamping a fixed `1/255` constant.
  - **Design:** Use existing `tflitec` interpreter handle (or, once Pure-Rust TFLite is wired through TenfloweRS, the equivalent reader) to read each input/output tensor's `quantization` substructure; map first scalar to `QuantizationParams { scale, zero_point }`; if the model has per-axis quantization, expose `Vec<QuantizationParams>` via a new `tensor.quantization_per_axis` field.
  - **Files:** `crates/oxigeo-ml/src/models/tflite.rs` (lines 420-436 plus `TensorInfo` struct), upstream blocker noted in `Cargo.toml:32-35`.
  - **Tests:** (proposed) `test_tflite_quantization_per_tensor_scale`, `test_tflite_quantization_zero_point_signed_int8`, `test_tflite_legacy_fixed_1_255_fallback`.
  - **Risk:** `tflitec` feature is currently disabled (Bazel toolchain conflict). Either resolve the build issue or stage this behind a new `tenflowers-tflite` Pure-Rust path (preferred per Pure Rust Policy).
  - **Prerequisites:** Re-enabling `tflite` feature in `Cargo.toml`.

- [ ] Re-enable `temporal` feature behind clean workspace deps
  - **Verified gap:** `Cargo.toml:44-45` — `# TEMPORARILY DISABLED: Optional dependency resolution issues with workspace features / # temporal = ["dep:scirs2-series", "dep:oxigeo-ml-foundation", ...]`
  - **Goal:** Time-series ML for change detection / forecasting (`TemporalForecaster`, `ConvLSTM`) without breaking workspace resolution.
  - **Design:** Move `oxigeo-ml-foundation` and `oxigeo-temporal` optional-deps into `[target.'cfg(any())'.dependencies]`-style virtual gate or split a `oxigeo-ml-temporal` adapter crate that depends on both unconditionally; expose only the re-exported `TemporalForecaster` via `#[cfg(feature = "temporal")] pub mod temporal;` (already wired at `src/lib.rs:317-334`).
  - **Files:** `Cargo.toml`, `crates/oxigeo-ml/src/temporal/` (existing).
  - **Tests:** (proposed) `test_temporal_forecaster_compiles_under_feature`, `test_temporal_convlstm_one_step_ahead`.
  - **Risk:** `oxigeo-ml-foundation?/ml` cycle — needs upstream foundation crate's `ml` feature surface stabilized first.
  - **Prerequisites:** oxigeo-ml-foundation backend stabilization (sibling crate, see its TODO).

- [ ] Model quantization (INT8/FP16) with accuracy regression suite
  - **Goal:** End-to-end INT8 PTQ (post-training quantization) plus FP16 weight conversion in the `optimization` pipeline, with KL-divergence calibration and a regression suite that rejects >1% mAP/mIoU drop on a small public test set.
  - **Design:** Extend `OptimizationPipeline` with `QuantizationConfig { precision: Int8 | Fp16, calibration_dataset, kl_divergence_bins: 2048, symmetric_per_channel: bool }`; emit ONNX `QuantizeLinear`/`DequantizeLinear` (ONNX opset 13+ per onnx.ai/operators) around weight tensors; record `accuracy_delta` in `OptimizationStats`.
  - **Files:** `crates/oxigeo-ml/src/optimization/quantization.rs` (new), wire into `OptimizationPipeline`.
  - **Tests:** (proposed) `test_int8_quantize_identity_model`, `test_int8_kl_calibration_2048_bins`, `test_fp16_weight_conversion_roundtrip`, `test_accuracy_delta_recorded`.
  - **Risk:** Calibration dataset bundling adds CI weight; ship as separate test fixture crate or skip in default test run.
  - **Prerequisites:** `quantization` feature flag already exists (`Cargo.toml:38`).

- [ ] Connect remaining GPU backends to real runtime APIs
  - **Goal:** Replace stubbed Vulkan/ROCm/OpenCL backends with `oxionnx` execution provider names that map to real device init via `libloading`/`ash`/`opencl3` already declared in deps.
  - **Design:** For each backend, on construction probe library presence (`libcuda.so.1`, `librocm_smi64.so`, OpenCL ICD), enumerate devices, and pass `ExecutionProviderDevice` to `oxionnx::SessionBuilder`. Match upstream `oxionnx` provider naming exactly to avoid silent CPU fallback.
  - **Files:** `crates/oxigeo-ml/src/gpu.rs`.
  - **Tests:** (proposed) `test_cuda_backend_falls_back_when_lib_absent`, `test_opencl_enumerate_devices`, `test_rocm_dynamic_load_safe`.
  - **Risk:** Cross-platform CI; gate hardware-dependent tests behind `#[ignore]` + env-var opt-in.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Streaming inference for continuous satellite-imagery feeds (S3/HTTP range-based tile fetch + circular tile buffer + back-pressure).
  - **Files:** `crates/oxigeo-ml/src/inference/streaming.rs` (new).
  - **Why deferred:** Needs cloud-native transport layer (cross-crate); coordinates with `oxigeo-streaming` v0.2.
- [ ] Model ensemble support (voting, stacking, blending).
  - **Files:** `crates/oxigeo-ml/src/inference/ensemble.rs` (new).
  - **Why deferred:** Lower priority than calibrated single-model quality.
- [ ] Active-learning loop with uncertainty sampling from segmentation outputs.
  - **Files:** `crates/oxigeo-ml/src/inference/active_learning.rs` (new).
  - **Why deferred:** Requires interactive labeling UX (out of scope for headless library).
- [ ] Real model versioning with registry, semver, and rollback.
  - **Files:** `crates/oxigeo-ml/src/model_versioning.rs` (extend existing).
  - **Why deferred:** Existing module covers basic versioning; rollback semantics need design.
- [ ] Geospatial-aware augmentations (north-up rotation, scale-aware crop, spectral band dropout) wired to `scirs2-core::random`.
  - **Files:** `crates/oxigeo-ml/src/augmentation/` (extend).
  - **Why deferred:** Generic augmentations exist; geospatial-specific is an enhancement.
- [ ] Change-detection pipeline (bi-temporal differencing + threshold optimization).
  - **Files:** `crates/oxigeo-ml/src/inference/change_detection.rs` (new).
  - **Why deferred:** Overlaps with `oxigeo-analytics::change`; coordinate with analytics crate.
- [ ] Model explainability (GradCAM, SHAP) for classification heads.
  - **Files:** `crates/oxigeo-ml/src/inference/explainability.rs` (new).
  - **Why deferred:** Needs `oxionnx` gradient hooks (upstream feature gap).
- [ ] Real model zoo with HTTP download + SHA-256 verification + local cache.
  - **Files:** `crates/oxigeo-ml/src/zoo.rs` (extend; deps already include `reqwest` + `sha2`).
  - **Why deferred:** Existing zoo is stub-shaped; needs hosted catalog (GitHub Pages JSON manifest).

## Low Priority / Future (one-liners)
- [ ] Federated-learning support for distributed satellite imagery processing.
- [ ] Knowledge distillation pipeline for edge-model compression.
- [ ] AutoML hyperparameter search for geospatial segmentation tasks.
- [ ] ONNX-Runtime WebAssembly backend for browser-side inference.
- [ ] Temporal architectures (ConvLSTM, transformer) — needs `temporal` feature unlocked first.
- [ ] Model A/B-testing framework with statistical-significance reporting.
- [ ] TFLite path via TenfloweRS (Pure-Rust replacement) instead of `tflitec`.

## Cross-crate dependencies
- **Blocks:** oxigeo (umbrella), oxigeo-services (ML inference endpoints), oxigeo-jupyter (ML notebooks).
- **Blocked by:** oxigeo-ml-foundation (training backend), oxigeo-temporal (`temporal` feature), oxionnx (CoreML/Vulkan/OpenCL EP coverage).

## Recently completed (verbatim)
- [x] Implement actual ONNX model loading and inference (completed 2026-04-19)
  - **Done:** ort→oxionnx migration complete. Compiles under all feature flags (default, gpu, coreml). Fixed misleading with_intra_threads comment. 12 new end-to-end inference tests via programmatic graph construction (SessionBuilder::build_from_graph).
  - **Tests added:** Identity/Relu/Add-bias inference, metadata extraction (NCHW/dynamic/3D), two-node pipeline, ndarray tensor roundtrip, session builder with threads, execution provider variants, model not found error, gpu feature compilation
- [x] Add real NMS with configurable IoU threshold computation (completed 2026-04-19)
  - **Done:** SuppressMethod (Hard/Linear/Gaussian), DistanceMetric (IoU/GIoU/DIoU/CIoU), RotatedBoundingBox with Sutherland-Hodgman polygon clipping IoU, non_maximum_suppression_rotated(). All backward compatible — NmsConfig::default() unchanged. R-tree spatial indexing deferred (minor deviation).
  - **Tests added:** test_soft_nms_gaussian, test_soft_nms_linear, test_soft_nms_preserves_hard_behavior, test_giou_non_overlapping, test_giou_identical, test_giou_partial_overlap, test_diou_center_distance, test_ciou_aspect_ratio, test_distance_metric_dispatch, test_rotated_bbox_corners_zero_angle, test_rotated_bbox_corners_90_degrees, test_rotated_bbox_corners_45_degrees, test_rotated_bbox_iou_axis_aligned, test_rotated_bbox_iou_rotated_square, test_rotated_bbox_iou_no_overlap, test_rotated_bbox_iou_identical, test_nms_rotated_suppression, test_nms_rotated_different_classes, test_polygon_area_triangle, test_polygon_area_square, test_sutherland_hodgman_full_inside, test_sutherland_hodgman_no_overlap, test_nms_default_backward_compatible, test_nms_invalid_threshold, test_nms_invalid_gaussian_sigma, test_nms_invalid_linear_score_threshold
- [x] Implement tile-based inference for rasters larger than model input size (completed 2026-04-19)
  - **Done:** Created shared `tiling.rs` module unifying tile layout from preprocessing.rs and superres/model.rs. `TileSpec` pure-geometry struct, `compute_tile_grid()` stride-based layout, `compute_blend_weight()` distance-from-edge linear weight, `BlendStrategy` enum (WeightedAverage/MaxConfidence/None), `merge_tile_detections()` for cross-tile NMS, `TileSource` trait + `TileIterator` for streaming, `InMemoryTileSource` implementation. Refactored preprocessing.rs `tile_raster()`, inference.rs `compute_tile_weight()`, and superres/model.rs `extract_tiles()`/`create_blend_weights()` to delegate to shared module. Exact behavior preserved for all callers.
  - **Tests added:** test_compute_tile_grid_no_overlap, test_compute_tile_grid_with_overlap, test_compute_tile_grid_non_divisible, test_compute_tile_grid_zero_tile_size, test_compute_tile_grid_overlap_too_large, test_compute_tile_grid_zero_image, test_compute_tile_grid_single_tile, test_blend_weight_center_is_one, test_blend_weight_edge_is_low, test_blend_weight_no_overlap, test_blend_weight_linear_ramp, test_blend_strategy_default, test_tile_spec_contains_point, test_tile_spec_area, test_tile_spec_overlaps, test_merge_tile_detections, test_merge_tile_detections_preserves_non_overlapping, test_merge_tile_detections_mismatched_lengths, test_merge_tile_detections_empty, test_merge_tile_detections_offsets_bbox, test_in_memory_tile_source, test_streaming_iterator_yields_all_tiles, test_streaming_iterator_with_overlap, test_tile_iterator_specs, test_grid_equivalence_with_preprocessing
- [x] Add ONNX graph optimization passes (constant folding, operator fusion) (completed 2026-04-19)
  - **Done:** Created `optimization/graph_opt.rs` with `GraphOptConfig`, `OptimizationBenchmark`, `apply_graph_optimization()`, `benchmark_optimization()`. Wired `operator_fusion` field in `OptimizationPipeline` via `effective_graph_opt_config()` and `opt_level()`. `measure_speedup`/`benchmark_model` now accept `OptLevel` parameter.
  - **Tests added (12):** GraphOptConfig default/none/partial, apply_graph_optimization identity/none, operator_fusion changes node count (MatMul+Add fusion), pipeline with fusion flag, benchmark struct fields/no-improvement/zero-nodes, benchmark_optimization identity/matmul_add, config-to-OptLevel roundtrip, effective_graph_opt_config from fusion flag and explicit override

---
*Last audited: 2026-05-17*
