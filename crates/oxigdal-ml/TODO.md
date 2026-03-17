# TODO: oxigdal-ml

## High Priority
- [ ] Complete migration from ndarray to SciRS2-Core for remaining linear algebra modules
- [ ] Implement actual ONNX model loading and inference (currently struct-based placeholders)
- [ ] Add real NMS (Non-Maximum Suppression) with configurable IoU threshold computation
- [ ] Implement tile-based inference for rasters larger than model input size
- [ ] Add model quantization (INT8/FP16) in optimization pipeline with accuracy validation
- [ ] Connect GPU backends (CUDA/CoreML/DirectML) to real runtime APIs

## Medium Priority
- [ ] Implement streaming inference for continuous satellite imagery feeds
- [ ] Add model ensemble support (voting, stacking, blending strategies)
- [ ] Implement active learning loop with uncertainty sampling from segmentation outputs
- [ ] Add ONNX graph optimization passes (constant folding, operator fusion)
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
