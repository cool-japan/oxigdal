# TODO: oxigdal-gpu-advanced

## High Priority
- [ ] Implement real multi-GPU data partitioning with overlap regions for convolution
- [ ] Add peer-to-peer GPU memory copy support (PCIe/NVLink detection)
- [ ] Implement GPU memory defragmentation in MemoryCompactor with actual buffer migration
- [ ] Add real GPU timestamp query support in GpuProfiler (currently CPU-timed)
- [ ] Implement shader optimizer passes beyond basic dead-code elimination
- [ ] Connect work-stealing queue to actual wgpu command submission

## Medium Priority
- [ ] Add automatic GPU selection benchmark that measures actual throughput
- [ ] Implement cross-GPU synchronization primitives backed by real fences
- [ ] Add memory pressure notification callback for adaptive allocation
- [ ] Implement pipeline auto-tuning that varies workgroup sizes per-kernel
- [ ] Add GPU thermal throttling detection and adaptive clock awareness
- [ ] Implement terrain analysis kernels (slope, aspect, curvature) as actual WGSL shaders
- [ ] Add ML inference WGSL kernels for matrix multiply and activation functions
- [ ] Implement batch submission with dependency graph for multi-kernel pipelines

## Low Priority / Future
- [ ] Add Vulkan-specific subgroup operations via SPIR-V backend
- [ ] Implement shader compilation cache persistence to disk
- [ ] Add GPU cluster support across network (RDMA/InfiniBand awareness)
- [ ] Implement dynamic load balancing based on real-time GPU utilization metrics
- [ ] Add profiling report export (Chrome Trace Event format, Perfetto)
- [ ] Implement cooperative multi-GPU rendering for large raster visualization
- [ ] Add power-aware GPU scheduling (prefer iGPU for light tasks, dGPU for heavy)
