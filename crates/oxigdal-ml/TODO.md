# TODO: oxigdal-ml

## High Priority
- [ ] Complete migration from ndarray to SciRS2-Core for remaining linear algebra modules
- [x] Implement actual ONNX model loading and inference (completed 2026-04-19)
  - **Done:** ort→oxionnx migration complete. Compiles under all feature flags (default, gpu, coreml). Fixed misleading with_intra_threads comment. 12 new end-to-end inference tests via programmatic graph construction (SessionBuilder::build_from_graph).
  - **Tests added:** Identity/Relu/Add-bias inference, metadata extraction (NCHW/dynamic/3D), two-node pipeline, ndarray tensor roundtrip, session builder with threads, execution provider variants, model not found error, gpu feature compilation
- [x] Add real NMS with configurable IoU threshold computation (completed 2026-04-19)
  - **Done:** SuppressMethod (Hard/Linear/Gaussian), DistanceMetric (IoU/GIoU/DIoU/CIoU), RotatedBoundingBox with Sutherland-Hodgman polygon clipping IoU, non_maximum_suppression_rotated(). All backward compatible — NmsConfig::default() unchanged. R-tree spatial indexing deferred (minor deviation).
  - **Tests added:** test_soft_nms_gaussian, test_soft_nms_linear, test_soft_nms_preserves_hard_behavior, test_giou_non_overlapping, test_giou_identical, test_giou_partial_overlap, test_diou_center_distance, test_ciou_aspect_ratio, test_distance_metric_dispatch, test_rotated_bbox_corners_zero_angle, test_rotated_bbox_corners_90_degrees, test_rotated_bbox_corners_45_degrees, test_rotated_bbox_iou_axis_aligned, test_rotated_bbox_iou_rotated_square, test_rotated_bbox_iou_no_overlap, test_rotated_bbox_iou_identical, test_nms_rotated_suppression, test_nms_rotated_different_classes, test_polygon_area_triangle, test_polygon_area_square, test_sutherland_hodgman_full_inside, test_sutherland_hodgman_no_overlap, test_nms_default_backward_compatible, test_nms_invalid_threshold, test_nms_invalid_gaussian_sigma, test_nms_invalid_linear_score_threshold
- [x] Implement tile-based inference for rasters larger than model input size (completed 2026-04-19)
  - **Done:** Created shared `tiling.rs` module unifying tile layout from preprocessing.rs and superres/model.rs. `TileSpec` pure-geometry struct, `compute_tile_grid()` stride-based layout, `compute_blend_weight()` distance-from-edge linear weight, `BlendStrategy` enum (WeightedAverage/MaxConfidence/None), `merge_tile_detections()` for cross-tile NMS, `TileSource` trait + `TileIterator` for streaming, `InMemoryTileSource` implementation. Refactored preprocessing.rs `tile_raster()`, inference.rs `compute_tile_weight()`, and superres/model.rs `extract_tiles()`/`create_blend_weights()` to delegate to shared module. Exact behavior preserved for all callers.
  - **Tests added:** test_compute_tile_grid_no_overlap, test_compute_tile_grid_with_overlap, test_compute_tile_grid_non_divisible, test_compute_tile_grid_zero_tile_size, test_compute_tile_grid_overlap_too_large, test_compute_tile_grid_zero_image, test_compute_tile_grid_single_tile, test_blend_weight_center_is_one, test_blend_weight_edge_is_low, test_blend_weight_no_overlap, test_blend_weight_linear_ramp, test_blend_strategy_default, test_tile_spec_contains_point, test_tile_spec_area, test_tile_spec_overlaps, test_merge_tile_detections, test_merge_tile_detections_preserves_non_overlapping, test_merge_tile_detections_mismatched_lengths, test_merge_tile_detections_empty, test_merge_tile_detections_offsets_bbox, test_in_memory_tile_source, test_streaming_iterator_yields_all_tiles, test_streaming_iterator_with_overlap, test_tile_iterator_specs, test_grid_equivalence_with_preprocessing
- [ ] Add model quantization (INT8/FP16) in optimization pipeline with accuracy validation
- [ ] Connect GPU backends (CUDA/CoreML/DirectML) to real runtime APIs

## Medium Priority
- [ ] Implement streaming inference for continuous satellite imagery feeds
- [ ] Add model ensemble support (voting, stacking, blending strategies)
- [ ] Implement active learning loop with uncertainty sampling from segmentation outputs
- [x] Add ONNX graph optimization passes (constant folding, operator fusion) (completed 2026-04-19)
  - **Done:** Created `optimization/graph_opt.rs` with `GraphOptConfig`, `OptimizationBenchmark`, `apply_graph_optimization()`, `benchmark_optimization()`. Wired `operator_fusion` field in `OptimizationPipeline` via `effective_graph_opt_config()` and `opt_level()`. `measure_speedup`/`benchmark_model` now accept `OptLevel` parameter.
  - **Tests added (12):** GraphOptConfig default/none/partial, apply_graph_optimization identity/none, operator_fusion changes node count (MatMul+Add fusion), pipeline with fusion flag, benchmark struct fields/no-improvement/zero-nodes, benchmark_optimization identity/matmul_add, config-to-OptLevel roundtrip, effective_graph_opt_config from fusion flag and explicit override
- [ ] Implement real model versioning with registry and rollback support
- [ ] Add geospatial-aware data augmentation (rotation preserving north-up, scale-aware crop)
- [ ] Implement change detection model pipeline (bi-temporal image differencing)
- [ ] Add model explainability (GradCAM, SHAP) for classification outputs
- [ ] Implement real model zoo with HTTP download, checksum verification, and local cache

## Low Priority / Future
- [ ] Add federated learning support for distributed satellite imagery processing
- [ ] Implement knowledge distillation pipeline for edge model compression
- [ ] Add AutoML hyperparameter search for geospatial segmentation tasks
- [ ] Implement ONNX Runtime WebAssembly backend for browser-side inference
- [ ] Add support for temporal model architectures (ConvLSTM, transformer-based)
- [ ] Implement model A/B testing framework with statistical significance reporting
- [ ] Add TFLite model format support for mobile deployment path
