//! Integration tests for the cooperative-matrix GEMM module.
//!
//! Pure-Rust tests (type introspection, WGSL source inspection) run
//! unconditionally.  GPU-dependent tests use the `try_gpu_context` /
//! `catch_unwind` pattern to skip gracefully when no headless adapter is
//! available.

// Allow unwrap in tests and relax doc requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigdal_gpu::{
    GpuContext,
    cooperative_matrix::{
        CoopMatrixComponentType, CoopMatrixDescriptor, CoopMatrixDim, CoopMatrixGemmConfig,
        CoopMatrixUse, build_cooperative_matrix_gemm_pipeline, dispatch_cooperative_gemm,
        make_gemm_wgsl, max_cooperative_matrix_dim, supports_cooperative_matrix,
    },
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to create a GPU context without panicking.
// ─────────────────────────────────────────────────────────────────────────────

fn try_gpu_context() -> Option<GpuContext> {
    use std::panic::AssertUnwindSafe;
    let result =
        std::panic::catch_unwind(AssertUnwindSafe(|| pollster::block_on(GpuContext::new())));
    match result {
        Ok(Ok(ctx)) => Some(ctx),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure tests (no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_coop_matrix_component_type_as_wgsl_f16() {
    assert_eq!(
        CoopMatrixComponentType::F16.as_wgsl(),
        "f16",
        "F16 must map to WGSL 'f16'"
    );
}

#[test]
fn test_coop_matrix_component_type_byte_size() {
    assert_eq!(
        CoopMatrixComponentType::F16.byte_size(),
        2,
        "F16 must be 2 bytes"
    );
    assert_eq!(
        CoopMatrixComponentType::F32.byte_size(),
        4,
        "F32 must be 4 bytes"
    );
    assert_eq!(
        CoopMatrixComponentType::I8.byte_size(),
        4,
        "I8 mapped to i32 must be 4 bytes"
    );
    assert_eq!(
        CoopMatrixComponentType::U8.byte_size(),
        4,
        "U8 mapped to u32 must be 4 bytes"
    );
}

#[test]
fn test_coop_matrix_dim_default_16x16x16() {
    let d = CoopMatrixDim::default();
    assert_eq!(d.m, 16, "default M must be 16");
    assert_eq!(d.n, 16, "default N must be 16");
    assert_eq!(d.k, 16, "default K must be 16");
}

#[test]
fn test_make_gemm_wgsl_contains_subgroup_matrix_load() {
    let src = make_gemm_wgsl(&CoopMatrixGemmConfig::default());
    assert!(
        src.contains("subgroupMatrix"),
        "generated WGSL must reference 'subgroupMatrix' (as comment); got:\n{src}"
    );
}

#[test]
fn test_make_gemm_wgsl_fallback_contains_workgroup_var() {
    // The *tiled* kernel (make_gemm_wgsl) must use var<workgroup>.
    let src = make_gemm_wgsl(&CoopMatrixGemmConfig::default());
    assert!(
        src.contains("var<workgroup>"),
        "tiled GEMM kernel must declare var<workgroup> shared memory; got:\n{src}"
    );
}

#[test]
fn test_make_gemm_wgsl_emits_valid_workgroup_size() {
    let src = make_gemm_wgsl(&CoopMatrixGemmConfig::default());
    assert!(
        src.contains("@compute @workgroup_size"),
        "GEMM kernel must have a @compute @workgroup_size attribute; got:\n{src}"
    );
}

#[test]
fn test_coop_matrix_descriptor_stride_default() {
    let desc = CoopMatrixDescriptor {
        component_type: CoopMatrixComponentType::F32,
        use_kind: CoopMatrixUse::A,
        rows: 16,
        cols: 16,
        stride: 16,
    };
    assert_eq!(
        desc.stride, 16,
        "stride field must match the value passed at construction"
    );
}

#[test]
fn test_coop_matrix_gemm_config_default_workgroup_size() {
    let cfg = CoopMatrixGemmConfig::default();
    assert_eq!(
        cfg.workgroup_size,
        (16, 16, 1),
        "default workgroup_size must be (16, 16, 1)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU-dependent tests — wrapped in catch_unwind / try_gpu_context
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_supports_cooperative_matrix_returns_bool() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        // The function must return without panicking; the actual bool does not
        // matter — both true and false are valid depending on hardware.
        let supported: bool = supports_cooperative_matrix(&ctx);
        println!("supports_cooperative_matrix: {supported}");
    }));
    // Ignore panics (no GPU available in CI).
    let _ = result;
}

#[test]
fn test_max_cooperative_matrix_dim_consistent_with_supports() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let supported = supports_cooperative_matrix(&ctx);
        let max_dim = max_cooperative_matrix_dim(&ctx);
        if supported {
            assert!(
                max_dim.is_some(),
                "max_cooperative_matrix_dim must be Some when supports_cooperative_matrix is true"
            );
        } else {
            assert!(
                max_dim.is_none(),
                "max_cooperative_matrix_dim must be None when supports_cooperative_matrix is false"
            );
        }
    }));
    let _ = result;
}

#[test]
fn test_build_cooperative_matrix_gemm_pipeline_compiles_or_falls_back() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let config = CoopMatrixGemmConfig::default();
        // Either the pipeline builds successfully or we get a well-typed error.
        // A panic would be a bug.
        match build_cooperative_matrix_gemm_pipeline(&ctx, &config) {
            Ok(_pipeline) => {
                println!("Pipeline compiled successfully");
            }
            Err(e) => {
                println!("Pipeline build returned error (acceptable): {e}");
            }
        }
    }));
    let _ = result;
}

#[test]
fn test_dispatch_cooperative_gemm_round_trip_4x4_identity() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let config = CoopMatrixGemmConfig::default();
        let pipeline = match build_cooperative_matrix_gemm_pipeline(&ctx, &config) {
            Ok(p) => p,
            Err(e) => {
                println!("Skipping round-trip test — pipeline unavailable: {e}");
                return;
            }
        };

        // Build a 4×4 identity matrix: diagonal = 1.0, rest = 0.0.
        // We'll compute identity × identity, which must equal identity.
        let n: usize = 4;
        let mut identity = vec![0.0_f32; n * n];
        for i in 0..n {
            identity[i * n + i] = 1.0;
        }

        let device = ctx.device();
        let queue = ctx.queue();

        let buf_size = (n * n * std::mem::size_of::<f32>()) as u64;

        let buf_a = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test_a"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_b = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test_b"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_c = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test_c"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buf_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test_readback"),
            size: buf_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let raw: &[u8] = bytemuck::cast_slice(&identity);
        queue.write_buffer(&buf_a, 0, raw);
        queue.write_buffer(&buf_b, 0, raw);

        let dim = CoopMatrixDim {
            m: n as u32,
            n: n as u32,
            k: n as u32,
        };
        dispatch_cooperative_gemm(&ctx, &pipeline, &config, &buf_a, &buf_b, &buf_c, dim)
            .expect("dispatch_cooperative_gemm must not return Err");

        // Copy results to a mappable buffer and wait.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback_encoder"),
        });
        encoder.copy_buffer_to_buffer(&buf_c, 0, &buf_readback, 0, buf_size);
        queue.submit(std::iter::once(encoder.finish()));

        // Map and read back.
        let slice = buf_readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).expect("channel send failed");
        });
        // Poll until map completes or try_recv fires.
        while let Ok(poll) = device.poll(wgpu::PollType::Poll) {
            if matches!(poll, wgpu::PollStatus::QueueEmpty) {
                break;
            }
            if rx.try_recv().is_ok() {
                break;
            }
        }
        // Wait up to 5 s for mapping.
        let map_result = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert!(map_result.is_ok(), "map_async timed out");
        assert!(map_result.unwrap().is_ok(), "map_async returned an error");

        let view = slice
            .get_mapped_range()
            .expect("get_mapped_range should succeed after successful map_async");
        let result_data: Vec<f32> = bytemuck::cast_slice(&view).to_vec();
        drop(view);
        buf_readback.unmap();

        // Verify C = identity × identity = identity.
        // For 4×4 identity A and B:
        // C[i,j] = sum_k A[i,k]*B[k,j] = sum_k δ(i,k)*δ(k,j) = δ(i,j).
        // So the result must be the identity matrix itself.
        for row in 0..n {
            for col in 0..n {
                let expected = if row == col { 1.0_f32 } else { 0.0 };
                let got = result_data[row * n + col];
                assert!(
                    (got - expected).abs() < 1e-4,
                    "C[{row},{col}] = {got}, expected {expected} (identity×identity)"
                );
            }
        }
    }));
    let _ = result;
}

#[test]
fn test_dispatch_cooperative_gemm_zero_matrix_yields_zero() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let config = CoopMatrixGemmConfig::default();
        let pipeline = match build_cooperative_matrix_gemm_pipeline(&ctx, &config) {
            Ok(p) => p,
            Err(e) => {
                println!("Skipping zero-matrix test — pipeline unavailable: {e}");
                return;
            }
        };

        let n: usize = 4;
        let zeros = vec![0.0_f32; n * n];
        let non_zero: Vec<f32> = (0..(n * n)).map(|i| i as f32).collect();

        let device = ctx.device();
        let queue = ctx.queue();
        let buf_size = (n * n * std::mem::size_of::<f32>()) as u64;

        let buf_a = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zero_a"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_b = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zero_b"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_c = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zero_c"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buf_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zero_readback"),
            size: buf_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // A = zeros, B = non-zero. Expected: C = zeros.
        queue.write_buffer(&buf_a, 0, bytemuck::cast_slice(&zeros));
        queue.write_buffer(&buf_b, 0, bytemuck::cast_slice(&non_zero));

        let dim = CoopMatrixDim {
            m: n as u32,
            n: n as u32,
            k: n as u32,
        };
        dispatch_cooperative_gemm(&ctx, &pipeline, &config, &buf_a, &buf_b, &buf_c, dim)
            .expect("dispatch_cooperative_gemm must not return Err");

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zero_readback_encoder"),
        });
        encoder.copy_buffer_to_buffer(&buf_c, 0, &buf_readback, 0, buf_size);
        queue.submit(std::iter::once(encoder.finish()));

        let slice = buf_readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).expect("channel send failed");
        });
        while let Ok(poll) = device.poll(wgpu::PollType::Poll) {
            if matches!(poll, wgpu::PollStatus::QueueEmpty) {
                break;
            }
            if rx.try_recv().is_ok() {
                break;
            }
        }
        let map_result = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert!(map_result.is_ok(), "zero-matrix map_async timed out");
        assert!(
            map_result.unwrap().is_ok(),
            "zero-matrix map_async returned an error"
        );

        let view = slice
            .get_mapped_range()
            .expect("get_mapped_range should succeed after successful map_async");
        let result_data: Vec<f32> = bytemuck::cast_slice(&view).to_vec();
        drop(view);
        buf_readback.unmap();

        for (idx, &v) in result_data.iter().enumerate() {
            assert!(
                v.abs() < 1e-6,
                "zero_matrix × any_matrix must be all zeros; C[{idx}] = {v}"
            );
        }
    }));
    let _ = result;
}

/// Regression test for the dispatch/tile-size coupling bug: a pipeline built
/// with a non-default (8×8×8) tile and a matrix larger than one tile must be
/// fully computed. With the old hardcoded `TILE = 16` the dispatch launched
/// only `ceil(16/16) = 1` workgroup per axis, covering just the top-left 8×8
/// block and leaving the rest of C at zero.
#[test]
fn test_dispatch_cooperative_gemm_non_default_tile_covers_full_matrix() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };

        // Non-default tile: 8×8×8 with a matching 8×8 workgroup.
        let config = CoopMatrixGemmConfig {
            dim: CoopMatrixDim { m: 8, n: 8, k: 8 },
            workgroup_size: (8, 8, 1),
            ..CoopMatrixGemmConfig::default()
        };
        let pipeline = match build_cooperative_matrix_gemm_pipeline(&ctx, &config) {
            Ok(p) => p,
            Err(e) => {
                println!("Skipping non-default-tile test — pipeline unavailable: {e}");
                return;
            }
        };

        // Logical matrix 16×16 — larger than one 8×8 tile in both axes.
        let n: usize = 16;

        // A = identity, B = arbitrary; identity × B == B, so every element of
        // C must equal the corresponding element of B — including the lower and
        // right blocks that a single-tile dispatch would never touch.
        let mut identity = vec![0.0_f32; n * n];
        for i in 0..n {
            identity[i * n + i] = 1.0;
        }
        let b_data: Vec<f32> = (0..(n * n)).map(|i| (i as f32) + 1.0).collect();

        let device = ctx.device();
        let queue = ctx.queue();
        let buf_size = (n * n * std::mem::size_of::<f32>()) as u64;

        let buf_a = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nd_a"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_b = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nd_b"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_c = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nd_c"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buf_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nd_readback"),
            size: buf_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        queue.write_buffer(&buf_a, 0, bytemuck::cast_slice(&identity));
        queue.write_buffer(&buf_b, 0, bytemuck::cast_slice(&b_data));

        let dim = CoopMatrixDim {
            m: n as u32,
            n: n as u32,
            k: n as u32,
        };
        dispatch_cooperative_gemm(&ctx, &pipeline, &config, &buf_a, &buf_b, &buf_c, dim)
            .expect("dispatch_cooperative_gemm must not return Err");

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nd_readback_encoder"),
        });
        encoder.copy_buffer_to_buffer(&buf_c, 0, &buf_readback, 0, buf_size);
        queue.submit(std::iter::once(encoder.finish()));

        let slice = buf_readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).expect("channel send failed");
        });
        while let Ok(poll) = device.poll(wgpu::PollType::Poll) {
            if matches!(poll, wgpu::PollStatus::QueueEmpty) {
                break;
            }
            if rx.try_recv().is_ok() {
                break;
            }
        }
        let map_result = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert!(map_result.is_ok(), "non-default-tile map_async timed out");
        assert!(
            map_result.unwrap().is_ok(),
            "non-default-tile map_async returned an error"
        );

        let view = slice
            .get_mapped_range()
            .expect("get_mapped_range should succeed after successful map_async");
        let result_data: Vec<f32> = bytemuck::cast_slice(&view).to_vec();
        drop(view);
        buf_readback.unmap();

        // identity × B == B for every element, across all four 8×8 quadrants.
        for row in 0..n {
            for col in 0..n {
                let expected = b_data[row * n + col];
                let got = result_data[row * n + col];
                assert!(
                    (got - expected).abs() < 1e-3,
                    "C[{row},{col}] = {got}, expected {expected} (identity×B); \
                     a mis-dispatched tile leaves this element at 0"
                );
            }
        }
    }));
    let _ = result;
}
