# TODO: oxigdal-gpu

## High Priority
- [ ] Implement actual WGSL compute shaders for tile compositing (currently placeholder)
- [ ] Add GPU-to-GPU direct memory transfer without CPU roundtrip
- [ ] Implement async readback with callback support instead of blocking `read_blocking()`
- [ ] Add proper error recovery when GPU device is lost mid-pipeline
- [ ] Implement workgroup size auto-tuning based on adapter limits
- [ ] Add f16 (half-precision) buffer support for bandwidth-sensitive operations
- [ ] Implement proper Lanczos kernel with configurable window size in WGSL

## Medium Priority
- [ ] Add FFT-based convolution shader for large kernel sizes
- [ ] Implement texture-based resampling using hardware samplers
- [ ] Add pipeline caching to avoid recompilation of identical shader configurations
- [ ] Implement tiled processing for rasters that exceed VRAM budget
- [ ] Add storage texture support for direct image output
- [ ] Implement band math expression compiler that generates optimized WGSL
- [ ] Add compute pipeline profiling with GPU timestamp queries
- [ ] Implement automatic CPU fallback when GPU operations fail or timeout
- [ ] Add WebGPU browser compatibility testing via wasm32 target

## Low Priority / Future
- [ ] Implement ray-marching shader for volumetric DEM rendering
- [ ] Add support for wgpu push constants when adapter supports them
- [ ] Implement cooperative matrix operations for ML inference on GPU
- [ ] Add indirect dispatch for adaptive workload sizing
- [ ] Add benchmarking suite comparing GPU vs CPU paths for each kernel
- [ ] Implement shader hot-reload file watcher for development mode
- [ ] Add support for multi-format texture compression (BC, ASTC, ETC2) output
