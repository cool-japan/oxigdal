# TODO: oxigeo-gpu-advanced

> **Purpose:** Multi-GPU orchestration, memory pooling/compaction, shader optimizer + cache, ML/terrain GPU kernels, work-stealing queue, profiler — built on top of `oxigeo-gpu` (wgpu 29).
> **Status (2026-05-16):** 11,662 LoC · 67 tests · 2 real stubs in WGSL FFT kernel (`kernels/advanced/fft.wgsl:188, :207`).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Real 2-D FFT in `fft.wgsl`
  - **Verified gap:** `src/kernels/advanced/fft.wgsl:184-190` — `// Process each row independently / // This would need to be called multiple times for complete 2D FFT / let idx = row * params.n + col; / // Placeholder for row FFT processing / // In practice, this would call the 1D FFT algorithm`; `:204-208` — `// Placeholder for column FFT processing`.
  - **Goal:** Working `fft_2d_rows` + `fft_2d_cols` entry points executing radix-2 Cooley-Tukey decimation-in-time per dimension. Match `OxiFFT` CPU output to within 1e-5 relative error on 256×256 random complex input. (Per COOLJAPAN policy: use OxiFFT for the CPU reference path, never `rustfft`.)
  - **Design:** Two-pass GPU FFT (rows → transpose-free column FFT via stride). Each WGSL workgroup `(64, 1, 1)` processes one row; bit-reversal swap kernel runs first; then `log2(N)` butterfly passes via a `for` loop with `workgroupBarrier()` between passes. Twiddle factors `W_N^k = exp(-2πik/N)` precomputed in a uniform buffer of size `N/2` complex values. Stockham auto-sort variant avoids the transpose pass. Reference: Cooley & Tukey 1965; van Loan 1992 §1.3.
  - **Files:** `crates/oxigeo-gpu-advanced/src/kernels/advanced/fft.wgsl` (rewrite `fft_2d_rows`, `fft_2d_cols`), `src/kernels/mod.rs::FftKernel` host-side dispatch.
  - **Tests:** (proposed) `test_fft_2d_dirac_returns_constant_magnitude`, `test_fft_2d_roundtrip_matches_input_within_1e_5`, `test_fft_2d_separable_gaussian_matches_oxifft`, `test_fft_2d_non_power_of_two_returns_error`, `test_fft_2d_size_2048_perf_under_50ms_native_metal`.
  - **Risk:** WGSL lacks complex types — pack as `vec2<f32>`; numerical accumulation in single-precision may lose mantissa for N > 4096 (document f32 limit).
  - **Prerequisites:** None. Workspace already pins `OxiFFT` (COOLJAPAN policy).

- [ ] Multi-GPU data partitioning with overlap regions for convolution
  - **Goal:** When a convolution kernel of radius `r` runs across `K` GPUs splitting a `(H, W)` raster row-wise, each GPU's slab must include a `r`-pixel halo at top/bottom from its neighbours. Currently `MultiGpuManager` distributes contiguous slabs without overlap.
  - **Design:** Add `ConvolutionPartitioner { radius: u32 }` that emits `Vec<Partition { device_id, rows: Range<usize>, halo_top: u32, halo_bottom: u32 }>`. Boundaries gathered post-execution via the cross-GPU gather path (see oxigeo-gpu Item 2). Edge devices have one-sided halo only.
  - **Files:** `crates/oxigeo-gpu-advanced/src/multi_gpu/load_balancer.rs` (extend), new `src/multi_gpu/partitioner.rs`.
  - **Tests:** (proposed) `test_partitioner_two_gpus_3x3_kernel_halo_one`, `test_partitioner_edge_device_one_sided_halo`, `test_partitioner_kernel_larger_than_slab_errors`, `test_partitioner_three_gpus_symmetric_halos`.
  - **Risk:** Halo size > slab size on small inputs — explicit error.
  - **Prerequisites:** Cross-GPU gather (oxigeo-gpu).

- [ ] Peer-to-peer GPU memory copy with PCIe/NVLink detection
  - **Goal:** Detect topology via adapter info, fall back to host-staged copy when P2P unavailable.
  - **Design:** Probe via `Adapter::get_info().vendor` and presence of `Features::MAPPABLE_PRIMARY_BUFFERS` plus PCIe BDF parsing on Linux (`/sys/bus/pci/devices/*/numa_node`). Expose `MultiGpuManager::supports_p2p(a, b) -> bool` and `copy_p2p(src_device, src_buf, dst_device, dst_buf) -> Future`.
  - **Files:** `crates/oxigeo-gpu-advanced/src/multi_gpu/device_manager.rs`, new `src/multi_gpu/p2p.rs`.
  - **Tests:** (proposed) `test_p2p_detection_returns_false_when_one_device`, `test_p2p_copy_matches_host_staged_result`, `test_p2p_fallback_when_unsupported`.
  - **Risk:** wgpu 29 exposes no direct P2P API — falls back to host staging until upstream adds it; document the limit.
  - **Prerequisites:** None.

- [ ] Real GPU timestamp queries in `GpuProfiler`
  - **Verified gap:** Previous TODO calls out "currently CPU-timed". `src/profiling.rs` is 20.6 KB — verify which paths are CPU-only.
  - **Goal:** Issue `wgpu::QuerySetDescriptor { ty: Timestamp, count: 2*N }` before/after each command-encoder pass; resolve via `resolve_query_set`; map staging buffer; convert via `queue.get_timestamp_period()` (nanoseconds/tick).
  - **Design:** Adapter must advertise `Features::TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_PASSES` (wgpu 29). When absent, fall back to current CPU-side `Instant::now()`. Add per-kernel `KernelStats::gpu_time_ns: Option<u64>` distinct from existing CPU timing.
  - **Files:** `crates/oxigeo-gpu-advanced/src/profiling.rs` (extend `GpuProfiler::profile_pass`).
  - **Tests:** (proposed) `test_profiler_falls_back_when_timestamp_unavailable`, `test_profiler_gpu_time_under_cpu_time_for_kernel`, `test_profiler_inside_pass_query_works_on_supported`.
  - **Risk:** Timestamp period varies per adapter (~1ns on NVIDIA, ~52ns on some AMD); always normalize through `get_timestamp_period()`.
  - **Prerequisites:** None.

- [ ] Memory defragmentation with actual buffer migration in `MemoryCompactor`
  - **Goal:** When fragmentation > threshold, allocate a fresh contiguous buffer, `copy_buffer_to_buffer` each live allocation in order, atomically swap pointers in the parent `MemoryPool`. Track migration count in `CompactionStats`.
  - **Design:** `MemoryCompactor::compact()` builds a migration plan `Vec<(src_offset, dst_offset, size)>`; submits one command encoder with all copies; waits for completion; updates pool's free list. Use `CompactionStrategy::{Aggressive, Conservative}` to gate when to run.
  - **Files:** `crates/oxigeo-gpu-advanced/src/memory_compaction.rs` (replace any TODO/stub paths), `src/memory_pool.rs` (expose live-allocation iterator).
  - **Tests:** (proposed) `test_compactor_aggregates_holes_into_contiguous_free`, `test_compactor_preserves_live_data_after_migration`, `test_compactor_no_op_when_fragmentation_below_threshold`, `test_compactor_concurrent_alloc_during_compaction_blocks`.
  - **Risk:** Concurrent allocations during compaction — gate with `Mutex<PoolState>` or copy-on-write swap; document blocking behaviour.
  - **Prerequisites:** None.

- [ ] Wire work-stealing queue to real wgpu command submission
  - **Goal:** `WorkStealingQueue::submit(kernel)` actually dispatches on the chosen GPU's `Queue`, returning a future resolved when the GPU completes the submission. Currently the queue surface exists but execution is conceptual.
  - **Design:** Each `WorkQueue` holds an `Arc<Queue>`; `submit` encodes commands and pushes to internal mpsc; worker thread drains and calls `queue.submit(...)` then registers a callback via `queue.on_submitted_work_done` to fulfil the returned `oneshot::Receiver`.
  - **Files:** `crates/oxigeo-gpu-advanced/src/multi_gpu/work_queue.rs`.
  - **Tests:** (proposed) `test_queue_submits_in_order_when_single_worker`, `test_queue_steal_from_idle_neighbour`, `test_queue_drains_on_drop`, `test_queue_propagates_kernel_error`.
  - **Risk:** wgpu 29 `on_submitted_work_done` is platform-dependent timing — fall back to `poll(Maintain::Wait)` task if callback path fires synchronously.
  - **Prerequisites:** None.

- [ ] Shader optimizer passes beyond dead-code elimination
  - **Goal:** Constant folding, common-subexpression elimination, and loop-invariant code motion at the WGSL AST level via `naga::Module` transformation.
  - **Design:** Implement on `ShaderOptimizer { level: O0..O3 }`. Use `naga::valid::Validator` to confirm semantic equivalence; record `OptimizationMetrics { instructions_before, instructions_after, ratio }`.
  - **Files:** `crates/oxigeo-gpu-advanced/src/shader_compiler/optimizer.rs`.
  - **Tests:** (proposed) `test_cse_removes_duplicate_dot_products`, `test_constant_folding_evaluates_literals`, `test_licm_hoists_invariant_load`, `test_optimizer_preserves_semantics_via_smoke_compute`.
  - **Risk:** naga IR mutation rules tight — start with read-only analysis + suggest-only mode in O0; gate aggressive rewrites behind O2+.
  - **Prerequisites:** None.

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
- **Blocked by:** oxigeo-gpu (this crate extends it), wgpu 29 timestamp/feature surface, OxiFFT (reference path).

## Recently completed (verbatim)
*(No `[x]` entries on previous TODO.)*

---
*Last audited: 2026-05-17*
