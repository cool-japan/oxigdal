//! Execute-and-compare tests for the backend WGSL shader generators.
//!
//! These tests exercise the real WGSL emitted by the per-backend optimizers
//! (`SubgroupOptimizer`, `WarpPrimitives`, `SimdGroupOperations`,
//! `MetalPerformanceShaders`) by dispatching it on a live wgpu device and
//! comparing the results against a CPU reference.
//!
//! * The **native subgroup** paths (`subgroupAdd`, `subgroupShuffle`, …) require
//!   [`wgpu::Features::SUBGROUP`]; the corresponding tests skip cleanly when the
//!   adapter does not expose it (or when no adapter is present at all).  On an
//!   Apple-Silicon Metal host the feature is available, so they run.
//! * The **emulation** paths (`workgroupBarrier()` shared-memory reductions) and
//!   the Metal MPS generators (Gaussian filter, reduction, ReLU + tiled matmul)
//!   need no special features and run on any adapter.
//!
//! Each test follows the `try_gpu_context` skip pattern used across this crate:
//! when `GpuContext::new()` fails (CI without a GPU) the test returns early.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::float_cmp,
    missing_docs
)]

#[cfg(any(feature = "metal", feature = "vulkan", feature = "cuda"))]
mod harness {
    #![allow(dead_code)]

    use oxigdal_gpu::{GpuContext, GpuContextConfig};

    /// One `@group(0)` storage binding for [`dispatch_collect`].
    pub struct StorageBinding {
        pub binding: u32,
        pub bytes: Vec<u8>,
    }

    /// Try to create a default GPU context without panicking.
    pub fn try_gpu_context() -> Option<GpuContext> {
        use std::panic::AssertUnwindSafe;
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            pollster::block_on(GpuContext::new()).ok()
        }))
        .ok()
        .flatten()
    }

    /// Try to create a GPU context that has `Features::SUBGROUP` enabled.
    ///
    /// Returns `None` when there is no adapter or the adapter cannot honour the
    /// subgroup feature — the caller then skips the native-path assertions.
    pub fn try_gpu_context_subgroup() -> Option<GpuContext> {
        use std::panic::AssertUnwindSafe;
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            pollster::block_on(
                GpuContextConfig::new()
                    .with_features(wgpu::Features::SUBGROUP)
                    .build(),
            )
            .ok()
        }))
        .ok()
        .flatten()
    }

    /// Compile `shader_src`, bind `group0` (storage) and optionally a
    /// `@group(1) @binding(b)` uniform, dispatch `entry`, and read back the
    /// requested buffers.
    ///
    /// `readbacks` lists `(binding, byte_len)` for the `group0` buffers to copy
    /// back; the returned `Vec` holds their bytes in the same order.
    pub fn dispatch_collect(
        ctx: &GpuContext,
        shader_src: &str,
        entry: &str,
        group0: &[StorageBinding],
        uniform: Option<(u32, Vec<u8>)>,
        readbacks: &[(u32, usize)],
        wg: (u32, u32, u32),
    ) -> Vec<Vec<u8>> {
        let device = ctx.device();
        let queue = ctx.queue();

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backend_test_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("backend_test_pipeline"),
            layout: None,
            module: &module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        // Create and upload the group(0) storage buffers.
        let mut buffers: Vec<(u32, wgpu::Buffer)> = Vec::new();
        for b in group0 {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backend_test_storage"),
                size: (b.bytes.len() as u64).max(4),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buf, 0, &b.bytes);
            buffers.push((b.binding, buf));
        }

        let g0_entries: Vec<wgpu::BindGroupEntry> = buffers
            .iter()
            .map(|(binding, buf)| wgpu::BindGroupEntry {
                binding: *binding,
                resource: buf.as_entire_binding(),
            })
            .collect();

        let g0_layout = pipeline.get_bind_group_layout(0);
        let g0_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backend_test_g0"),
            layout: &g0_layout,
            entries: &g0_entries,
        });

        // Optional group(1) uniform.
        let g1_bind = uniform.as_ref().map(|(binding, bytes)| {
            let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backend_test_uniform"),
                size: (bytes.len() as u64).max(16),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&ubuf, 0, bytes);
            let g1_layout = pipeline.get_bind_group_layout(1);
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("backend_test_g1"),
                layout: &g1_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: ubuf.as_entire_binding(),
                }],
            })
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("backend_test_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("backend_test_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &g0_bind, &[]);
            if let Some(g1) = g1_bind.as_ref() {
                pass.set_bind_group(1, g1, &[]);
            }
            pass.dispatch_workgroups(wg.0, wg.1, wg.2);
        }

        // Copy the requested buffers into MAP_READ staging buffers.
        let mut readback_bufs: Vec<wgpu::Buffer> = Vec::new();
        for (binding, len) in readbacks {
            let src = buffers
                .iter()
                .find(|(b, _)| b == binding)
                .map(|(_, buf)| buf)
                .expect("readback binding must exist in group0");
            let rb = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backend_test_readback"),
                size: *len as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(src, 0, &rb, 0, *len as u64);
            readback_bufs.push(rb);
        }

        queue.submit(std::iter::once(encoder.finish()));

        // Map each staging buffer and collect its bytes.
        let mut results = Vec::new();
        for rb in &readback_bufs {
            let slice = rb.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            while let Ok(poll) = device.poll(wgpu::PollType::Poll) {
                if matches!(poll, wgpu::PollStatus::QueueEmpty) {
                    break;
                }
                if rx.try_recv().is_ok() {
                    break;
                }
            }
            let mapped = rx.recv_timeout(std::time::Duration::from_secs(5));
            assert!(mapped.is_ok(), "map_async timed out");
            assert!(mapped.unwrap().is_ok(), "map_async returned an error");
            let view = slice.get_mapped_range().expect("get_mapped_range");
            results.push(view.to_vec());
            drop(view);
            rb.unmap();
        }

        results
    }

    /// Convenience: a zeroed byte buffer of `n` `f32`/`u32` elements.
    pub fn zeros(n: usize) -> Vec<u8> {
        vec![0u8; n * 4]
    }

    /// Decode a little-endian byte slice as `f32` (alignment-independent).
    pub fn as_f32(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// Decode a little-endian byte slice as `u32` (alignment-independent).
    pub fn as_u32(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// `f32` bytes.
    pub fn f32_bytes(v: &[f32]) -> Vec<u8> {
        bytemuck::cast_slice(v).to_vec()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Metal MPS generators — Gaussian filter, reduction, ReLU + tiled matmul.
// These need no special features and run on any adapter.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(feature = "metal")]
mod metal_tests {
    use super::harness::*;
    use oxigdal_gpu::backends::metal::{MPSKernelType, MetalPerformanceShaders};

    fn gaussian_weights(radius: usize, sigma: f32) -> Vec<f32> {
        let mut w = Vec::with_capacity(2 * radius + 1);
        let mut sum = 0.0f32;
        for k in 0..(2 * radius + 1) {
            let x = k as f32 - radius as f32;
            let g = (-(x * x) / (2.0 * sigma * sigma)).exp();
            w.push(g);
            sum += g;
        }
        for v in &mut w {
            *v /= sum;
        }
        w
    }

    fn cpu_separable_gaussian(
        input: &[f32],
        w: usize,
        h: usize,
        weights: &[f32],
        radius: i32,
    ) -> Vec<f32> {
        let mut tmp = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0f32;
                for k in -radius..=radius {
                    let sx = (x as i32 + k).clamp(0, w as i32 - 1) as usize;
                    acc += weights[(k + radius) as usize] * input[y * w + sx];
                }
                tmp[y * w + x] = acc;
            }
        }
        let mut out = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0f32;
                for k in -radius..=radius {
                    let sy = (y as i32 + k).clamp(0, h as i32 - 1) as usize;
                    acc += weights[(k + radius) as usize] * tmp[sy * w + x];
                }
                out[y * w + x] = acc;
            }
        }
        out
    }

    #[test]
    fn test_gaussian_filter_matches_cpu() {
        let Some(ctx) = try_gpu_context() else {
            println!("no adapter — skipping");
            return;
        };
        let mps = MetalPerformanceShaders::new();
        let shader = mps.generate_mps_shader(MPSKernelType::ImageFilter);

        let w = 24usize;
        let h = 20usize;
        let radius = 3usize;
        let weights = gaussian_weights(radius, 1.5);

        // Deterministic pseudo-image.
        let mut input = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                input[y * w + x] = ((x * 7 + y * 13) % 17) as f32;
            }
        }

        let params: [u32; 4] = [w as u32, h as u32, radius as u32, 0];
        let wg = ((w as u32).div_ceil(16), (h as u32).div_ceil(16), 1);

        // Pass 1 (horizontal): src=input, dst=temp.
        let pass1 = dispatch_collect(
            &ctx,
            &shader,
            "gaussian_horizontal",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: f32_bytes(&input),
                },
                StorageBinding {
                    binding: 1,
                    bytes: f32_bytes(&weights),
                },
                StorageBinding {
                    binding: 2,
                    bytes: zeros(w * h),
                },
            ],
            Some((0, bytemuck::cast_slice(&params).to_vec())),
            &[(2, w * h * 4)],
            wg,
        );
        let temp = as_f32(&pass1[0]);

        // Pass 2 (vertical): src=temp, dst=out.
        let pass2 = dispatch_collect(
            &ctx,
            &shader,
            "gaussian_vertical",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: f32_bytes(&temp),
                },
                StorageBinding {
                    binding: 1,
                    bytes: f32_bytes(&weights),
                },
                StorageBinding {
                    binding: 2,
                    bytes: zeros(w * h),
                },
            ],
            Some((0, bytemuck::cast_slice(&params).to_vec())),
            &[(2, w * h * 4)],
            wg,
        );
        let gpu = as_f32(&pass2[0]);

        let cpu = cpu_separable_gaussian(&input, w, h, &weights, radius as i32);
        let mut max_diff = 0.0f32;
        for i in 0..(w * h) {
            max_diff = max_diff.max((gpu[i] - cpu[i]).abs());
        }
        assert!(
            max_diff < 1e-3,
            "gaussian GPU vs CPU max diff {max_diff} exceeds tolerance"
        );
    }

    #[test]
    fn test_reduction_two_pass_matches_cpu() {
        let Some(ctx) = try_gpu_context() else {
            println!("no adapter — skipping");
            return;
        };
        let mps = MetalPerformanceShaders::new();
        let shader = mps.generate_mps_shader(MPSKernelType::Reduction);

        let n = 1000usize;
        let input: Vec<f32> = (0..n).map(|i| ((i % 31) as f32) - 5.0).collect();

        // Pass 1: reduce n elements → ceil(n/256) partials.
        let num_wg = (n as u32).div_ceil(256);
        let params1: [u32; 4] = [n as u32, 0, 0, 0];
        let p1 = dispatch_collect(
            &ctx,
            &shader,
            "reduce_sum",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: f32_bytes(&input),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(num_wg as usize),
                },
            ],
            Some((0, bytemuck::cast_slice(&params1).to_vec())),
            &[(1, num_wg as usize * 4)],
            (num_wg, 1, 1),
        );
        let partials = as_f32(&p1[0]);

        // Pass 2: reduce the partials → single scalar.
        let params2: [u32; 4] = [num_wg, 0, 0, 0];
        let p2 = dispatch_collect(
            &ctx,
            &shader,
            "reduce_sum",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: f32_bytes(&partials),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(1),
                },
            ],
            Some((0, bytemuck::cast_slice(&params2).to_vec())),
            &[(1, 4)],
            (1, 1, 1),
        );
        let gpu_sum = as_f32(&p2[0])[0];
        let cpu_sum: f32 = input.iter().sum();
        assert!(
            (gpu_sum - cpu_sum).abs() < 1e-1,
            "reduce_sum GPU {gpu_sum} vs CPU {cpu_sum}"
        );

        // Single-pass min / max over a 256-chunk.
        let m = 256usize;
        let sub: Vec<f32> = (0..m).map(|i| ((i * 7 + 3) % 91) as f32 - 40.0).collect();
        let params_m: [u32; 4] = [m as u32, 0, 0, 0];
        let pmin = dispatch_collect(
            &ctx,
            &shader,
            "reduce_min",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: f32_bytes(&sub),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(1),
                },
            ],
            Some((0, bytemuck::cast_slice(&params_m).to_vec())),
            &[(1, 4)],
            (1, 1, 1),
        );
        let gpu_min = as_f32(&pmin[0])[0];
        let cpu_min = sub.iter().cloned().fold(f32::INFINITY, f32::min);
        assert_eq!(gpu_min, cpu_min, "reduce_min mismatch");

        let pmax = dispatch_collect(
            &ctx,
            &shader,
            "reduce_max",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: f32_bytes(&sub),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(1),
                },
            ],
            Some((0, bytemuck::cast_slice(&params_m).to_vec())),
            &[(1, 4)],
            (1, 1, 1),
        );
        let gpu_max = as_f32(&pmax[0])[0];
        let cpu_max = sub.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(gpu_max, cpu_max, "reduce_max mismatch");
    }

    #[test]
    fn test_relu_tiled_matmul_matches_cpu() {
        let Some(ctx) = try_gpu_context() else {
            println!("no adapter — skipping");
            return;
        };
        let mps = MetalPerformanceShaders::new();
        let shader = mps.generate_mps_shader(MPSKernelType::NeuralNetwork);

        let m = 20usize;
        let k = 24usize;
        let n = 28usize;

        // Deterministic A (M×K) and B (K×N) with a mix of signs so ReLU bites.
        let a: Vec<f32> = (0..m * k).map(|i| ((i % 13) as f32) - 6.0).collect();
        let b: Vec<f32> = (0..k * n).map(|i| ((i % 11) as f32) - 5.0).collect();

        let dims: [u32; 4] = [m as u32, n as u32, k as u32, 0];
        let wg = ((n as u32).div_ceil(16), (m as u32).div_ceil(16), 1);
        let out = dispatch_collect(
            &ctx,
            &shader,
            "matmul_relu",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: f32_bytes(&a),
                },
                StorageBinding {
                    binding: 1,
                    bytes: f32_bytes(&b),
                },
                StorageBinding {
                    binding: 2,
                    bytes: zeros(m * n),
                },
            ],
            Some((0, bytemuck::cast_slice(&dims).to_vec())),
            &[(2, m * n * 4)],
            wg,
        );
        let gpu = as_f32(&out[0]);

        // CPU reference: relu(A × B).
        let mut cpu = vec![0.0f32; m * n];
        for row in 0..m {
            for col in 0..n {
                let mut sum = 0.0f32;
                for kk in 0..k {
                    sum += a[row * k + kk] * b[kk * n + col];
                }
                cpu[row * n + col] = sum.max(0.0);
            }
        }
        let mut max_diff = 0.0f32;
        for i in 0..(m * n) {
            max_diff = max_diff.max((gpu[i] - cpu[i]).abs());
        }
        assert!(
            max_diff < 1e-2,
            "matmul_relu GPU vs CPU max diff {max_diff} exceeds tolerance"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Vulkan subgroup arithmetic — native (Features::SUBGROUP) + emulation.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(feature = "vulkan")]
mod vulkan_tests {
    use super::harness::*;
    use oxigdal_gpu::backends::vulkan::{
        SubgroupOptimizer, VulkanFeatureDetector, VulkanOptimizationConfig,
    };

    // A shader that sums each invocation's value across the group and writes the
    // (broadcast) result. `{HELPERS}` is replaced by the generated snippet.
    fn reduce_core(wg: u32) -> String {
        format!(
            r#"
@group(0) @binding(0) var<storage, read_write> out_reduce: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_max: array<f32>;
@group(0) @binding(2) var<storage, read_write> out_val: array<f32>;

@compute @workgroup_size({wg})
fn main(@builtin(local_invocation_index) lid: u32) {{
    let n = {wg}u;
    let v = f32(lid) + 1.0;
    out_val[lid] = v;
    out_reduce[lid] = subgroup_add(v, lid, n);
    out_max[lid] = subgroup_max(v, lid, n);
}}
"#
        )
    }

    #[test]
    fn test_subgroup_reduction_native_single_subgroup() {
        let Some(ctx) = try_gpu_context_subgroup() else {
            println!("no subgroup-capable adapter — skipping native test");
            return;
        };
        let info = ctx.adapter_info();
        let sg = info.subgroup_max_size;
        if info.subgroup_min_size != info.subgroup_max_size || sg == 0 || sg > 256 {
            println!("variable/unsupported subgroup size {sg} — skipping strict native test");
            return;
        }

        // A workgroup of exactly `sg` invocations forms a single subgroup, so a
        // native subgroup reduction covers the whole workgroup deterministically.
        let features = VulkanFeatureDetector::new(&ctx).features().clone();
        assert!(
            features.subgroup_arithmetic,
            "subgroup context must report arithmetic support"
        );
        let opt = SubgroupOptimizer::new(features, VulkanOptimizationConfig::default());
        let shader = opt.optimize_shader(&reduce_core(sg));

        let outs = dispatch_collect(
            &ctx,
            &shader,
            "main",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: zeros(sg as usize),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(sg as usize),
                },
                StorageBinding {
                    binding: 2,
                    bytes: zeros(sg as usize),
                },
            ],
            None,
            &[(0, sg as usize * 4), (1, sg as usize * 4)],
            (1, 1, 1),
        );
        let reduce = as_f32(&outs[0]);
        let maxv = as_f32(&outs[1]);

        let total: f32 = (1..=sg).map(|i| i as f32).sum();
        let expected_max = sg as f32;
        for i in 0..sg as usize {
            assert!(
                (reduce[i] - total).abs() < 0.5,
                "native subgroup_add lane {i} = {}, expected {total}",
                reduce[i]
            );
            assert!(
                (maxv[i] - expected_max).abs() < 0.5,
                "native subgroup_max lane {i} = {}, expected {expected_max}",
                maxv[i]
            );
        }
    }

    #[test]
    fn test_subgroup_reduction_and_scan_emulation() {
        // Plain (non-subgroup) context → SubgroupOptimizer detects no subgroup
        // support and emits the workgroup emulation.
        let Some(ctx) = try_gpu_context() else {
            println!("no adapter — skipping");
            return;
        };
        let mut features = VulkanFeatureDetector::new(&ctx).features().clone();
        // Force the emulation path regardless of what the adapter supports so
        // the workgroup fallback itself is exercised.
        features.subgroup_arithmetic = false;
        features.subgroup_ballot = false;
        let opt = SubgroupOptimizer::new(features, VulkanOptimizationConfig::default());

        let wg = 64u32;
        let core = format!(
            r#"
@group(0) @binding(0) var<storage, read_write> out_sum: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_incl: array<f32>;
@group(0) @binding(2) var<storage, read_write> out_excl: array<f32>;

@compute @workgroup_size({wg})
fn main(@builtin(local_invocation_index) lid: u32) {{
    let n = {wg}u;
    let v = f32(lid) + 1.0;
    out_sum[lid] = subgroup_add(v, lid, n);
    out_incl[lid] = subgroup_inclusive_add(v, lid, n);
    out_excl[lid] = subgroup_exclusive_add(v, lid, n);
}}
"#
        );
        let shader = opt.optimize_shader(&core);

        let outs = dispatch_collect(
            &ctx,
            &shader,
            "main",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: zeros(wg as usize),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(wg as usize),
                },
                StorageBinding {
                    binding: 2,
                    bytes: zeros(wg as usize),
                },
            ],
            None,
            &[
                (0, wg as usize * 4),
                (1, wg as usize * 4),
                (2, wg as usize * 4),
            ],
            (1, 1, 1),
        );
        let sum = as_f32(&outs[0]);
        let incl = as_f32(&outs[1]);
        let excl = as_f32(&outs[2]);

        let total: f32 = (1..=wg).map(|i| i as f32).sum();
        for i in 0..wg as usize {
            let inclusive: f32 = (1..=(i as u32 + 1)).map(|x| x as f32).sum();
            let exclusive: f32 = (1..=(i as u32)).map(|x| x as f32).sum();
            assert!(
                (sum[i] - total).abs() < 0.5,
                "emu subgroup_add lane {i} = {}, expected {total}",
                sum[i]
            );
            assert!(
                (incl[i] - inclusive).abs() < 0.5,
                "emu inclusive lane {i} = {}, expected {inclusive}",
                incl[i]
            );
            assert!(
                (excl[i] - exclusive).abs() < 0.5,
                "emu exclusive lane {i} = {}, expected {exclusive}",
                excl[i]
            );
        }
    }

    #[test]
    fn test_subgroup_ballot_emulation_counts() {
        let Some(ctx) = try_gpu_context() else {
            println!("no adapter — skipping");
            return;
        };
        let mut features = VulkanFeatureDetector::new(&ctx).features().clone();
        features.subgroup_arithmetic = false;
        features.subgroup_ballot = false;
        let opt = SubgroupOptimizer::new(features, VulkanOptimizationConfig::default());

        let wg = 64u32;
        // Predicate: lane is even. Count of true lanes over 64 = 32.
        let core = format!(
            r#"
@group(0) @binding(0) var<storage, read_write> out_count: array<u32>;
@group(0) @binding(1) var<storage, read_write> out_all: array<u32>;
@group(0) @binding(2) var<storage, read_write> out_any: array<u32>;

@compute @workgroup_size({wg})
fn main(@builtin(local_invocation_index) lid: u32) {{
    let n = {wg}u;
    let pred = (lid % 2u) == 0u;
    out_count[lid] = subgroup_ballot(pred, lid, n);
    out_all[lid] = select(0u, 1u, subgroup_all(pred, lid, n));
    out_any[lid] = select(0u, 1u, subgroup_any(pred, lid, n));
}}
"#
        );
        let shader = opt.optimize_shader(&core);
        let outs = dispatch_collect(
            &ctx,
            &shader,
            "main",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: zeros(wg as usize),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(wg as usize),
                },
                StorageBinding {
                    binding: 2,
                    bytes: zeros(wg as usize),
                },
            ],
            None,
            &[
                (0, wg as usize * 4),
                (1, wg as usize * 4),
                (2, wg as usize * 4),
            ],
            (1, 1, 1),
        );
        let count = as_u32(&outs[0]);
        let all = as_u32(&outs[1]);
        let any = as_u32(&outs[2]);
        for i in 0..wg as usize {
            assert_eq!(count[i], wg / 2, "ballot count lane {i}");
            assert_eq!(all[i], 0, "not all lanes even, subgroup_all must be 0");
            assert_eq!(any[i], 1, "some lanes even, subgroup_any must be 1");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CUDA warp shuffle — native (Features::SUBGROUP) + emulation.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(feature = "cuda")]
mod cuda_tests {
    use super::harness::*;
    use oxigdal_gpu::backends::cuda::WarpPrimitives;

    #[test]
    fn test_warp_shuffle_native_single_subgroup() {
        let Some(ctx) = try_gpu_context_subgroup() else {
            println!("no subgroup-capable adapter — skipping native warp test");
            return;
        };
        let info = ctx.adapter_info();
        let sg = info.subgroup_max_size;
        if info.subgroup_min_size != info.subgroup_max_size || sg == 0 || sg > 256 {
            println!("variable subgroup size {sg} — skipping native warp test");
            return;
        }
        assert!(WarpPrimitives::native_subgroups(&ctx));

        let helpers = WarpPrimitives::warp_shuffle_shader(true);
        let core = format!(
            r#"
{helpers}
@group(0) @binding(0) var<storage, read_write> out_bcast: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_xor: array<f32>;

@compute @workgroup_size({sg})
fn main(@builtin(local_invocation_index) lid: u32) {{
    let n = {sg}u;
    let v = f32(lid) + 1.0;
    out_bcast[lid] = warp_shuffle(v, 0u, lid, n);
    out_xor[lid] = warp_shuffle_xor(v, 1u, lid, n);
}}
"#
        );
        let outs = dispatch_collect(
            &ctx,
            &core,
            "main",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: zeros(sg as usize),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(sg as usize),
                },
            ],
            None,
            &[(0, sg as usize * 4), (1, sg as usize * 4)],
            (1, 1, 1),
        );
        let bcast = as_f32(&outs[0]);
        let xor = as_f32(&outs[1]);
        for i in 0..sg as usize {
            assert!(
                (bcast[i] - 1.0).abs() < 0.5,
                "warp_shuffle broadcast lane {i} = {}, expected 1.0",
                bcast[i]
            );
            let partner = (i as u32) ^ 1u32;
            let expected = partner as f32 + 1.0;
            assert!(
                (xor[i] - expected).abs() < 0.5,
                "warp_shuffle_xor lane {i} = {}, expected {expected}",
                xor[i]
            );
        }
    }

    #[test]
    fn test_warp_shuffle_emulation() {
        let Some(ctx) = try_gpu_context() else {
            println!("no adapter — skipping");
            return;
        };
        let helpers = WarpPrimitives::warp_shuffle_shader(false);
        let wg = 64u32;
        let core = format!(
            r#"
{helpers}
@group(0) @binding(0) var<storage, read_write> out_from5: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_xor: array<f32>;
@group(0) @binding(2) var<storage, read_write> out_down: array<f32>;

@compute @workgroup_size({wg})
fn main(@builtin(local_invocation_index) lid: u32) {{
    let n = {wg}u;
    let v = f32(lid) + 1.0;
    out_from5[lid] = warp_shuffle(v, 5u, lid, n);
    out_xor[lid] = warp_shuffle_xor(v, 1u, lid, n);
    out_down[lid] = warp_shuffle_down(v, 3u, lid, n);
}}
"#
        );
        let outs = dispatch_collect(
            &ctx,
            &core,
            "main",
            &[
                StorageBinding {
                    binding: 0,
                    bytes: zeros(wg as usize),
                },
                StorageBinding {
                    binding: 1,
                    bytes: zeros(wg as usize),
                },
                StorageBinding {
                    binding: 2,
                    bytes: zeros(wg as usize),
                },
            ],
            None,
            &[
                (0, wg as usize * 4),
                (1, wg as usize * 4),
                (2, wg as usize * 4),
            ],
            (1, 1, 1),
        );
        let from5 = as_f32(&outs[0]);
        let xor = as_f32(&outs[1]);
        let down = as_f32(&outs[2]);
        for i in 0..wg as usize {
            assert!(
                (from5[i] - 6.0).abs() < 0.5,
                "warp_shuffle(_,5) lane {i} = {}, expected 6.0",
                from5[i]
            );
            let partner = (i as u32) ^ 1u32;
            assert!(
                (xor[i] - (partner as f32 + 1.0)).abs() < 0.5,
                "warp_shuffle_xor lane {i}"
            );
            let src = i as u32 + 3;
            let expected = if src < wg {
                src as f32 + 1.0
            } else {
                i as f32 + 1.0
            };
            assert!(
                (down[i] - expected).abs() < 0.5,
                "warp_shuffle_down lane {i} = {}, expected {expected}",
                down[i]
            );
        }
    }

    #[test]
    fn test_warp_reduce_emulation_32() {
        let Some(ctx) = try_gpu_context() else {
            println!("no adapter — skipping");
            return;
        };
        // warp_reduce_* use a 32-lane butterfly; drive a 32-invocation workgroup.
        let shuffle = WarpPrimitives::warp_shuffle_shader(false);
        let reduce = WarpPrimitives::warp_reduce_shader();
        let core = format!(
            r#"
{shuffle}
{reduce}
@group(0) @binding(0) var<storage, read_write> out_sum: array<f32>;

@compute @workgroup_size(32)
fn main(@builtin(local_invocation_index) lid: u32) {{
    let v = f32(lid) + 1.0;
    out_sum[lid] = warp_reduce_sum(v, lid, 32u);
}}
"#
        );
        let outs = dispatch_collect(
            &ctx,
            &core,
            "main",
            &[StorageBinding {
                binding: 0,
                bytes: zeros(32),
            }],
            None,
            &[(0, 32 * 4)],
            (1, 1, 1),
        );
        let sum = as_f32(&outs[0]);
        // Butterfly reduction leaves the full sum on lane 0.
        let total: f32 = (1..=32).map(|i| i as f32).sum();
        assert!(
            (sum[0] - total).abs() < 0.5,
            "warp_reduce_sum lane 0 = {}, expected {total}",
            sum[0]
        );
    }
}
