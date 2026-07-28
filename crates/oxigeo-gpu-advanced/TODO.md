# TODO: oxigeo-gpu-advanced

> **Purpose:** Multi-GPU orchestration, memory pooling/compaction, shader optimizer + cache, ML/terrain GPU kernels, work-stealing queue, profiler — built on top of `oxigeo-gpu` (wgpu 30).
> **Status (2026-07-28):** 11,662 LoC (as of last count) · 121 tests (all-features) · 0 real stubs in the WGSL FFT kernel — 5 of the 7 High Priority items from the 2026-05-16 audit have since landed (FFT, GPU timestamp profiling, memory compaction, work-stealing submission, shader optimizer passes); multi-GPU partitioning and P2P copy remain open below.
> **Roadmap:** v0.1.7 → v0.2.1 → v1.0.0

## High Priority (verified gaps)
- [x] Real 2-D FFT in `fft.wgsl`
  - **Done:** `src/kernels/advanced/fft.wgsl` no longer has placeholder comments — `fft_2d_rows` and `fft_2d_cols` both call a real `fft_1d_strided(base, stride, n, inverse)` helper (`fft_2d_rows` with `stride=1u` per row, `fft_2d_cols` with `stride=params.n` per column), implementing a genuine separable two-pass GPU FFT rather than a stub.
  - **Files:** `src/kernels/advanced/fft.wgsl`.

- [ ] Multi-GPU data partitioning with overlap regions for convolution
  - **Re-verified 2026-07-28:** Still open — no `src/multi_gpu/partitioner.rs`, no `ConvolutionPartitioner` in source.
  - **Goal:** When a convolution kernel of radius `r` runs across `K` GPUs splitting a `(H, W)` raster row-wise, each GPU's slab must include a `r`-pixel halo at top/bottom from its neighbours. Currently `MultiGpuManager` distributes contiguous slabs without overlap.
  - **Design:** Add `ConvolutionPartitioner { radius: u32 }` that emits `Vec<Partition { device_id, rows: Range<usize>, halo_top: u32, halo_bottom: u32 }>`. Boundaries gathered post-execution via the cross-GPU gather path (see oxigeo-gpu Item 2). Edge devices have one-sided halo only.
  - **Files:** `crates/oxigeo-gpu-advanced/src/multi_gpu/load_balancer.rs` (extend), new `src/multi_gpu/partitioner.rs`.
  - **Tests:** (proposed) `test_partitioner_two_gpus_3x3_kernel_halo_one`, `test_partitioner_edge_device_one_sided_halo`, `test_partitioner_kernel_larger_than_slab_errors`, `test_partitioner_three_gpus_symmetric_halos`.
  - **Risk:** Halo size > slab size on small inputs — explicit error.
  - **Prerequisites:** Cross-GPU gather (oxigeo-gpu).

- [ ] Peer-to-peer GPU memory copy with PCIe/NVLink detection
  - **Re-verified 2026-07-28:** Still open — no `src/multi_gpu/p2p.rs`, no `supports_p2p`/`copy_p2p` in source.
  - **Goal:** Detect topology via adapter info, fall back to host-staged copy when P2P unavailable.
  - **Design:** Probe via `Adapter::get_info().vendor` and presence of `Features::MAPPABLE_PRIMARY_BUFFERS` plus PCIe BDF parsing on Linux (`/sys/bus/pci/devices/*/numa_node`). Expose `MultiGpuManager::supports_p2p(a, b) -> bool` and `copy_p2p(src_device, src_buf, dst_device, dst_buf) -> Future`.
  - **Files:** `crates/oxigeo-gpu-advanced/src/multi_gpu/device_manager.rs`, new `src/multi_gpu/p2p.rs`.
  - **Tests:** (proposed) `test_p2p_detection_returns_false_when_one_device`, `test_p2p_copy_matches_host_staged_result`, `test_p2p_fallback_when_unsupported`.
  - **Risk:** wgpu 29 exposes no direct P2P API — falls back to host staging until upstream adds it; document the limit.
  - **Prerequisites:** None.

- [x] Real GPU timestamp queries in `GpuProfiler`
  - **Done:** `src/profiling.rs` now has a real `GpuTimestampProfiler` backed by `wgpu::QuerySet` (`query_sets: Arc<RwLock<Vec<wgpu::QuerySet>>>`) with `begin_pass`/`end_pass`/`resolve` writing/resolving timestamps and converting through `queue.get_timestamp_period()` for adapter-correct nanosecond scaling, gated behind `Features::TIMESTAMP_QUERY` (returns `None`/CPU-timed fallback when unsupported rather than fabricating GPU timing).
  - **Files:** `src/profiling.rs`.

- [x] Memory defragmentation with actual buffer migration in `MemoryCompactor`
  - **Done:** `src/memory_compaction.rs` implements real migration — `compact`, `compact_by_copy`, `compact_in_place`, and `compact_hybrid` strategies all issue genuine `encoder.copy_buffer_to_buffer(...)` commands, and `compact_offsets` computes and returns the `Vec<BufferMove>` migration plan alongside byte totals, not a stub.
  - **Files:** `src/memory_compaction.rs`.

- [x] Wire work-stealing queue to real wgpu command submission
  - **Done:** `src/multi_gpu/work_queue.rs::submit_work` sends a boxed work closure through an internal channel to a worker holding a real `GpuDevice`, and resolves a `oneshot` receiver with the closure's result — `submit_batch`, `submit_batch_to_devices`, and `submit_batch_work_stealing` build on the same primitive. The actual `queue.submit(...)` call happens inside the GPU operation the caller supplies (e.g. a `ComputePipeline` execution), so the queue's job — real cross-device scheduling and completion signalling — is genuinely wired, not conceptual.
  - **Files:** `src/multi_gpu/work_queue.rs`.

- [x] Shader optimizer passes beyond dead-code elimination
  - **Done:** `src/shader_compiler/optimizer.rs::optimize()` runs each enabled `OptimizationPass` (`DeadCodeElimination`, `ConstantFolding`, `LoopUnrolling`, `CommonSubexpressionElimination`, `InstructionCombining`) against a real `naga::Module`. `fold_constants` walks `naga::Expression::Binary`/`Unary` nodes, resolves literal operands, and rewrites them in place — genuine AST transformation, not a no-op passthrough.
  - **Files:** `src/shader_compiler/optimizer.rs`.

## Medium Priority
- [ ] Automatic GPU selection benchmark that measures real throughput before binding.
  - **Files:** `src/adaptive/mod.rs` (extend `AdaptiveSelector`).
  - **Why deferred:** Existing static selection works; throughput probe is an optimization.
- [ ] Cross-GPU synchronization via real fences (wgpu `Queue::on_submitted_work_done`).
  - **Files:** `src/multi_gpu/sync.rs`.
  - **Why deferred:** Single-GPU is the common case; multi-GPU sync needs P2P first.
- [ ] Memory-pressure notification callback for adaptive allocation.
  - **Files:** `src/memory_pool.rs`.
  - **Why deferred:** Requires OS-level signals on non-Apple platforms; ship after VRAM budget tracking matures.
- [ ] Pipeline auto-tuning with per-kernel workgroup-size sweeps.
  - **Files:** `src/pipeline_builder.rs`.
  - **Why deferred:** Costly first-run autotune; gate behind feature.
- [ ] GPU thermal-throttling detection.
  - **Files:** `src/profiling.rs` (extend bottleneck classifier).
  - **Why deferred:** Adapter-specific telemetry not exposed by wgpu 29.
- [ ] Terrain WGSL kernels (slope, aspect, curvature) — actual shader code, not placeholders.
  - **Files:** `src/gpu_terrain.rs` (43 KB file — audit for stub paths), new `src/kernels/terrain.wgsl`.
  - **Why deferred:** CPU reference exists in `oxigeo-terrain`; port after FFT lands.
- [ ] ML-inference WGSL kernels (matmul + activations).
  - **Files:** `src/gpu_ml/compute.rs`, `src/gpu_ml/neural.rs`.
  - **Why deferred:** Coordinate with oxigeo-ml CUDA/Metal EP roadmap.
- [ ] Batch submission with dependency graph for multi-kernel pipelines.
  - **Files:** `src/pipeline_builder.rs` (extend).
  - **Why deferred:** Simple linear pipelines cover 90% of cases; defer DAG.

## Low Priority / Future (one-liners)
- [ ] Vulkan-specific subgroup operations via SPIR-V backend.
- [ ] On-disk shader-compilation cache (`ShaderCache` currently in-memory).
- [ ] Network-distributed GPU cluster (RDMA/InfiniBand) — coordinate with oxigeo-cluster.
- [ ] Dynamic load balancing from real-time utilization metrics.
- [ ] Profiling report export (Chrome Trace Event, Perfetto).
- [ ] Cooperative multi-GPU rendering for large-raster visualization.
- [ ] Power-aware scheduling (iGPU for light, dGPU for heavy).

## Cross-crate dependencies
- **Blocks:** oxigeo-services (GPU compositing), oxigeo-ml (Vulkan/Metal EP wired to multi-GPU).
- **Blocked by:** oxigeo-gpu (this crate extends it), wgpu 30 timestamp/feature surface, OxiFFT (reference path).

## Recently completed (verbatim)
*(No `[x]` entries on the original 2026-05-16/17 TODO; this 2026-07-28 audit found the real 2-D FFT, GPU timestamp profiling, memory compaction, work-stealing submission, and shader optimizer passes already implemented in source.)*

---
*Last audited: 2026-07-28*
