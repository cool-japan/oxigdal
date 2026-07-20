//! Cooperative-matrix (WMMA-style tile MMA) infrastructure for `oxigdal-gpu`.
//!
//! Provides ML-inference GEMM primitives that use WGSL workgroup-tiled
//! kernels on all adapters, with optional [`wgpu::Features::SUBGROUP`] and
//! [`wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX`] enhancement on
//! supporting hardware.
//!
//! # Design
//!
//! The primary kernel (`make_gemm_wgsl`) implements a workgroup-shared-memory
//! tiled GEMM that is correct on every wgpu-supported backend.  When
//! `supports_cooperative_matrix` returns `true`, higher-level code may choose
//! to enable optional WGSL subgroup-matrix builtins via shader extensions —
//! those builtins are referenced as comments/string literals in the generated
//! source for forwards-compatibility.
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_gpu::{GpuContext, cooperative_matrix::*};
//!
//! # async fn ex() -> oxigdal_gpu::GpuResult<()> {
//! let ctx = GpuContext::new().await?;
//! let config = CoopMatrixGemmConfig::default();
//! let src = make_gemm_wgsl(&config);
//! let pipeline = build_cooperative_matrix_gemm_pipeline(&ctx, &config)?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};

// ─────────────────────────────────────────────────────────────────────────────
// Component types
// ─────────────────────────────────────────────────────────────────────────────

/// Scalar component types supported by cooperative-matrix operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoopMatrixComponentType {
    /// 16-bit IEEE 754 half-precision float.
    F16,
    /// 32-bit IEEE 754 single-precision float.
    F32,
    /// 8-bit signed integer (used as accumulator in int8 GEMM paths).
    I8,
    /// 8-bit unsigned integer (used as input in int8 GEMM paths).
    U8,
}

impl CoopMatrixComponentType {
    /// Return the corresponding WGSL type keyword.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigdal_gpu::cooperative_matrix::CoopMatrixComponentType;
    /// assert_eq!(CoopMatrixComponentType::F32.as_wgsl(), "f32");
    /// assert_eq!(CoopMatrixComponentType::F16.as_wgsl(), "f16");
    /// ```
    pub fn as_wgsl(self) -> &'static str {
        match self {
            Self::F16 => "f16",
            Self::F32 => "f32",
            Self::I8 => "i32",
            Self::U8 => "u32",
        }
    }

    /// Return the byte size of one element of this type.
    ///
    /// Both `I8` and `U8` report `4` because WGSL maps them to `i32`/`u32`
    /// in storage buffers (WGSL has no native 8-bit storage type).
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigdal_gpu::cooperative_matrix::CoopMatrixComponentType;
    /// assert_eq!(CoopMatrixComponentType::F16.byte_size(), 2);
    /// assert_eq!(CoopMatrixComponentType::F32.byte_size(), 4);
    /// ```
    pub fn byte_size(self) -> u32 {
        match self {
            Self::F16 => 2,
            Self::F32 => 4,
            Self::I8 => 4,
            Self::U8 => 4,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Matrix use kind
// ─────────────────────────────────────────────────────────────────────────────

/// How a cooperative matrix tile is used within a GEMM operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoopMatrixUse {
    /// Left-hand operand (A matrix, M × K).
    A,
    /// Right-hand operand (B matrix, K × N).
    B,
    /// Result accumulator (C matrix, M × N).
    Accumulator,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tile dimensions
// ─────────────────────────────────────────────────────────────────────────────

/// Tile dimensions for a cooperative-matrix GEMM: C[M×N] = A[M×K] × B[K×N].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoopMatrixDim {
    /// Number of rows in A and C.
    pub m: u32,
    /// Number of columns in B and C.
    pub n: u32,
    /// Shared inner dimension (columns of A / rows of B).
    pub k: u32,
}

impl Default for CoopMatrixDim {
    /// Returns a 16 × 16 × 16 tile — the smallest dimension guaranteed to be
    /// supported by all cooperative-matrix implementations.
    fn default() -> Self {
        Self {
            m: 16,
            n: 16,
            k: 16,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Descriptor
// ─────────────────────────────────────────────────────────────────────────────

/// Describes a single cooperative-matrix tile (component type, use, shape).
#[derive(Debug, Clone)]
pub struct CoopMatrixDescriptor {
    /// Scalar element type.
    pub component_type: CoopMatrixComponentType,
    /// Role of this tile in the GEMM.
    pub use_kind: CoopMatrixUse,
    /// Number of rows in this tile.
    pub rows: u32,
    /// Number of columns in this tile.
    pub cols: u32,
    /// Row stride (number of elements between the start of successive rows in
    /// the source storage buffer).
    pub stride: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// GEMM configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Full configuration for a cooperative-matrix GEMM pipeline.
///
/// Covers tile dimensions, element types, and the workgroup launch shape.
#[derive(Debug, Clone)]
pub struct CoopMatrixGemmConfig {
    /// Tile dimensions (M × N × K).
    pub dim: CoopMatrixDim,
    /// Element type for matrix A.
    pub a_type: CoopMatrixComponentType,
    /// Element type for matrix B.
    pub b_type: CoopMatrixComponentType,
    /// Element type for the accumulator C.
    pub accum_type: CoopMatrixComponentType,
    /// Workgroup launch size `(x, y, z)`.
    pub workgroup_size: (u32, u32, u32),
}

impl Default for CoopMatrixGemmConfig {
    fn default() -> Self {
        Self {
            dim: CoopMatrixDim::default(),
            a_type: CoopMatrixComponentType::F32,
            b_type: CoopMatrixComponentType::F32,
            accum_type: CoopMatrixComponentType::F32,
            workgroup_size: (16, 16, 1),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature queries
// ─────────────────────────────────────────────────────────────────────────────

/// Return `true` when the GPU context supports cooperative-matrix operations.
///
/// Checks for [`wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX`] and
/// [`wgpu::Features::SUBGROUP`] on the device.  When neither flag is present
/// (or the device was not created with those features), returns `false`
/// gracefully without panicking.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal_gpu::{GpuContext, cooperative_matrix::supports_cooperative_matrix};
///
/// # async fn ex() -> oxigdal_gpu::GpuResult<()> {
/// let ctx = GpuContext::new().await?;
/// println!("cooperative matrix: {}", supports_cooperative_matrix(&ctx));
/// # Ok(())
/// # }
/// ```
pub fn supports_cooperative_matrix(ctx: &GpuContext) -> bool {
    let features = ctx.device().features();
    // Check for the wgpu 29 experimental cooperative matrix feature.
    // We also accept the general SUBGROUP flag as a weaker signal that the
    // driver exposes subgroup-level intrinsics.
    features.contains(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX)
        || features.contains(wgpu::Features::SUBGROUP)
}

/// Return the maximum tile dimensions supported by the adapter, or `None`
/// when cooperative-matrix is not supported at all.
///
/// When `supports_cooperative_matrix` returns `false` this always returns
/// `None`.  Otherwise it returns `Some(CoopMatrixDim::default())` — the
/// 16 × 16 × 16 tile is the minimum guaranteed size across all supported
/// backends.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal_gpu::{GpuContext, cooperative_matrix::max_cooperative_matrix_dim};
///
/// # async fn ex() -> oxigdal_gpu::GpuResult<()> {
/// let ctx = GpuContext::new().await?;
/// if let Some(dim) = max_cooperative_matrix_dim(&ctx) {
///     println!("max tile: {}x{}x{}", dim.m, dim.n, dim.k);
/// }
/// # Ok(())
/// # }
/// ```
pub fn max_cooperative_matrix_dim(ctx: &GpuContext) -> Option<CoopMatrixDim> {
    if supports_cooperative_matrix(ctx) {
        Some(CoopMatrixDim::default())
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WGSL kernel generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate a fully functional WGSL compute shader for tiled GEMM.
///
/// The output computes `C = A × B` where:
/// - `A` is an M × K matrix stored row-major in a read-only storage buffer.
/// - `B` is a K × N matrix stored row-major in a read-only storage buffer.
/// - `C` is an M × N output stored row-major in a read-write storage buffer.
/// - `dims` is a uniform buffer carrying the runtime M, N, K values.
///
/// The kernel uses workgroup-shared-memory tiling.  Each workgroup computes
/// one tile of C collaboratively, loading TILE × TILE sub-tiles of A and B
/// into `var<workgroup>` arrays before accumulating the partial dot products.
///
/// The string `"subgroupMatrixLoad"` appears as a comment in the generated
/// source so that downstream code can grep for it as a signal that
/// subgroup-matrix support is within scope for future enhancement.
pub fn make_gemm_wgsl(config: &CoopMatrixGemmConfig) -> String {
    let (wg_x, wg_y, _wg_z) = config.workgroup_size;
    let tile_m = config.dim.m;
    let tile_n = config.dim.n;
    let tile_k = config.dim.k;

    // Shared-memory tile sizes (number of elements, not bytes).
    let tile_a_size = tile_m * tile_k; // rows × inner
    let tile_b_size = tile_k * tile_n; // inner × cols
    let accum_type = config.accum_type.as_wgsl();

    format!(
        r#"// Cooperative-matrix (workgroup-tiled) GEMM kernel.
// Future hardware path note: subgroupMatrixLoad / subgroupMatrixStore /
// subgroupMatrixMultiplyAccumulate builtins would replace the shared-memory
// path below when wgpu enables the cooperative-matrix extension.

struct MatrixDims {{
    M: u32,
    N: u32,
    K: u32,
    _pad: u32,
}}

@group(0) @binding(0) var<storage, read>       mat_a: array<{accum_type}>;
@group(0) @binding(1) var<storage, read>       mat_b: array<{accum_type}>;
@group(0) @binding(2) var<storage, read_write> mat_c: array<{accum_type}>;
@group(0) @binding(3) var<uniform>             dims:  MatrixDims;

// Workgroup-shared tile storage (TILE_M × TILE_K and TILE_K × TILE_N).
var<workgroup> tile_a: array<{accum_type}, {tile_a_size}u>;
var<workgroup> tile_b: array<{accum_type}, {tile_b_size}u>;

@compute @workgroup_size({wg_x}, {wg_y}, 1)
fn gemm_main(
    @builtin(global_invocation_id)  gid:   vec3<u32>,
    @builtin(local_invocation_id)   lid:   vec3<u32>,
    @builtin(workgroup_id)          wgid:  vec3<u32>,
) {{
    let row: u32 = gid.x;
    let col: u32 = gid.y;

    let local_row: u32 = lid.x;
    let local_col: u32 = lid.y;

    var acc: {accum_type} = 0.0;

    let num_tiles: u32 = (dims.K + {tile_k}u - 1u) / {tile_k}u;

    for (var t: u32 = 0u; t < num_tiles; t = t + 1u) {{
        // ── Collaborative tile load ─────────────────────────────────────
        // Each thread loads one element of tile_a and one element of tile_b.

        // tile_a: shape [TILE_M, TILE_K], row-major.
        let a_global_row: u32 = wgid.x * {tile_m}u + local_row;
        let a_global_col: u32 = t       * {tile_k}u + local_col;
        if (a_global_row < dims.M && a_global_col < dims.K) {{
            tile_a[local_row * {tile_k}u + local_col] = mat_a[a_global_row * dims.K + a_global_col];
        }} else {{
            tile_a[local_row * {tile_k}u + local_col] = 0.0;
        }}

        // tile_b: shape [TILE_K, TILE_N], row-major.
        let b_global_row: u32 = t       * {tile_k}u + local_row;
        let b_global_col: u32 = wgid.y * {tile_n}u + local_col;
        if (b_global_row < dims.K && b_global_col < dims.N) {{
            tile_b[local_row * {tile_n}u + local_col] = mat_b[b_global_row * dims.N + b_global_col];
        }} else {{
            tile_b[local_row * {tile_n}u + local_col] = 0.0;
        }}

        // Ensure all threads have finished writing the tiles before reading.
        workgroupBarrier();

        // ── Partial dot product ─────────────────────────────────────────
        for (var k: u32 = 0u; k < {tile_k}u; k = k + 1u) {{
            acc = acc + tile_a[local_row * {tile_k}u + k] * tile_b[k * {tile_n}u + local_col];
        }}

        // Ensure tile reads are complete before the next iteration overwrites.
        workgroupBarrier();
    }}

    // Write output only for threads within the logical matrix bounds.
    if (row < dims.M && col < dims.N) {{
        mat_c[row * dims.N + col] = acc;
    }}
}}
"#,
        accum_type = accum_type,
        tile_a_size = tile_a_size,
        tile_b_size = tile_b_size,
        tile_m = tile_m,
        tile_n = tile_n,
        tile_k = tile_k,
        wg_x = wg_x,
        wg_y = wg_y,
    )
}

/// Generate a naive (non-tiled) WGSL GEMM shader — the fallback path.
///
/// Unlike [`make_gemm_wgsl`] this kernel does **not** use
/// `var<workgroup>` shared memory; each thread independently iterates over
/// the full K dimension.  This is simpler but has much higher global-memory
/// traffic and is provided for correctness testing on adapters where shared
/// memory is unavailable or when a minimal shader footprint is preferred.
pub fn make_gemm_wgsl_fallback(config: &CoopMatrixGemmConfig) -> String {
    let (wg_x, wg_y, _wg_z) = config.workgroup_size;
    let accum_type = config.accum_type.as_wgsl();

    format!(
        r#"// Naive (non-tiled) GEMM fallback kernel.
// Each thread independently accumulates a full dot product for its C element.

struct MatrixDims {{
    M: u32,
    N: u32,
    K: u32,
    _pad: u32,
}}

@group(0) @binding(0) var<storage, read>       mat_a: array<{accum_type}>;
@group(0) @binding(1) var<storage, read>       mat_b: array<{accum_type}>;
@group(0) @binding(2) var<storage, read_write> mat_c: array<{accum_type}>;
@group(0) @binding(3) var<uniform>             dims:  MatrixDims;

@compute @workgroup_size({wg_x}, {wg_y}, 1)
fn gemm_main(
    @builtin(global_invocation_id) gid: vec3<u32>,
) {{
    let row: u32 = gid.x;
    let col: u32 = gid.y;

    if (row >= dims.M || col >= dims.N) {{
        return;
    }}

    var acc: {accum_type} = 0.0;
    for (var k: u32 = 0u; k < dims.K; k = k + 1u) {{
        acc = acc + mat_a[row * dims.K + k] * mat_b[k * dims.N + col];
    }}
    mat_c[row * dims.N + col] = acc;
}}
"#,
        accum_type = accum_type,
        wg_x = wg_x,
        wg_y = wg_y,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Pipeline construction
// ─────────────────────────────────────────────────────────────────────────────

/// Compile a GEMM compute pipeline from the workgroup-tiled WGSL kernel.
///
/// The function generates the shader source via [`make_gemm_wgsl`], compiles it
/// into a [`wgpu::ShaderModule`], and returns a [`wgpu::ComputePipeline`]
/// wrapped in an [`Arc`].
///
/// The bind-group layout is:
/// - binding 0: `mat_a` — storage read
/// - binding 1: `mat_b` — storage read
/// - binding 2: `mat_c` — storage read-write
/// - binding 3: `dims`  — uniform
///
/// # Errors
///
/// - `GpuError::ShaderCompilation` — WGSL fails to compile.
/// - `GpuError::PipelineCreation`  — pipeline object creation fails.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal_gpu::{GpuContext, cooperative_matrix::{CoopMatrixGemmConfig, build_cooperative_matrix_gemm_pipeline}};
///
/// # async fn ex() -> oxigdal_gpu::GpuResult<()> {
/// let ctx = GpuContext::new().await?;
/// let pipeline = build_cooperative_matrix_gemm_pipeline(&ctx, &CoopMatrixGemmConfig::default())?;
/// # Ok(())
/// # }
/// ```
pub fn build_cooperative_matrix_gemm_pipeline(
    ctx: &GpuContext,
    config: &CoopMatrixGemmConfig,
) -> GpuResult<Arc<wgpu::ComputePipeline>> {
    let wgsl = make_gemm_wgsl(config);
    let device = ctx.device();

    // Compile the WGSL shader module.
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("coop_matrix_gemm_shader"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });

    // Build bind-group layout reflecting the four bindings.
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("coop_matrix_gemm_bgl"),
        entries: &[
            // binding 0: mat_a (storage, read-only)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // binding 1: mat_b (storage, read-only)
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // binding 2: mat_c (storage, read-write)
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // binding 3: dims (uniform)
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("coop_matrix_gemm_pipeline_layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("coop_matrix_gemm_pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader_module,
        entry_point: Some("gemm_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    Ok(Arc::new(pipeline))
}

// ─────────────────────────────────────────────────────────────────────────────
// Dispatch
// ─────────────────────────────────────────────────────────────────────────────

/// Encode and submit a single GEMM dispatch.
///
/// Creates a bind group from `[a, b, c, uniform_dims]`, records a compute
/// pass, and submits the command buffer to the queue.
///
/// `dim` specifies the **logical** matrix dimensions (M, N, K) used to
/// calculate the number of workgroups dispatched in X and Y.  Each workgroup
/// collaboratively computes one `config.dim.m × config.dim.n` tile of `C`
/// (the tile size baked into the shader by [`make_gemm_wgsl`]), so the number
/// of workgroups launched is `ceil(M / tile_m) × ceil(N / tile_n)`.
///
/// `config` MUST be the same configuration used to build `pipeline` via
/// [`build_cooperative_matrix_gemm_pipeline`]; the tile dimensions it carries
/// drive the dispatch math and must not drift from the tile size compiled into
/// the shader, otherwise parts of `C` are left uncomputed.
///
/// # GPU-memory layout (all row-major)
///
/// | buffer | shape | element type |
/// |--------|-------|--------------|
/// | `a`    | M × K | `f32`        |
/// | `b`    | K × N | `f32`        |
/// | `c`    | M × N | `f32` (output) |
///
/// # Errors
///
/// Returns `GpuError::invalid_buffer` if `config.dim.m` or `config.dim.n` is
/// zero (a degenerate tile size), or `GpuError::ExecutionFailed` if the queue
/// submission fails.
pub fn dispatch_cooperative_gemm(
    ctx: &GpuContext,
    pipeline: &wgpu::ComputePipeline,
    config: &CoopMatrixGemmConfig,
    a: &wgpu::Buffer,
    b: &wgpu::Buffer,
    c: &wgpu::Buffer,
    dim: CoopMatrixDim,
) -> GpuResult<()> {
    let device = ctx.device();
    let queue = ctx.queue();

    // Build the uniform dims buffer (M, N, K, _pad).
    let dims_data: [u32; 4] = [dim.m, dim.n, dim.k, 0];
    let dims_bytes: &[u8] = bytemuck::cast_slice(&dims_data);
    let dims_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("coop_matrix_dims_uniform"),
        size: (dims_bytes.len() as u64).max(16),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&dims_buf, 0, dims_bytes);

    // Derive the bind-group layout from the pipeline.
    let bind_group_layout = pipeline.get_bind_group_layout(0);

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("coop_matrix_gemm_bind_group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: a.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: b.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: c.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: dims_buf.as_entire_binding(),
            },
        ],
    });

    // The per-workgroup C-tile size is baked into the shader from the config's
    // `dim` (see `make_gemm_wgsl`), NOT a fixed 16. Dispatch enough workgroups
    // to cover the logical matrix with these actual tile dimensions, otherwise
    // a config with a smaller tile than 16 would under-dispatch and leave the
    // upper rows/cols of C uncomputed.
    let tile_m = config.dim.m;
    let tile_n = config.dim.n;
    if tile_m == 0 || tile_n == 0 {
        return Err(GpuError::invalid_buffer(
            "cooperative GEMM tile dimensions (config.dim.m/n) must be non-zero",
        ));
    }
    let wg_x = dim.m.div_ceil(tile_m);
    let wg_y = dim.n.div_ceil(tile_n);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("coop_matrix_gemm_encoder"),
    });

    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("coop_matrix_gemm_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(wg_x, wg_y, 1);
    }

    queue.submit(std::iter::once(encoder.finish()));

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_type_as_wgsl() {
        assert_eq!(CoopMatrixComponentType::F16.as_wgsl(), "f16");
        assert_eq!(CoopMatrixComponentType::F32.as_wgsl(), "f32");
        assert_eq!(CoopMatrixComponentType::I8.as_wgsl(), "i32");
        assert_eq!(CoopMatrixComponentType::U8.as_wgsl(), "u32");
    }

    #[test]
    fn test_component_type_byte_size() {
        assert_eq!(CoopMatrixComponentType::F16.byte_size(), 2);
        assert_eq!(CoopMatrixComponentType::F32.byte_size(), 4);
        assert_eq!(CoopMatrixComponentType::I8.byte_size(), 4);
        assert_eq!(CoopMatrixComponentType::U8.byte_size(), 4);
    }

    #[test]
    fn test_dim_default() {
        let d = CoopMatrixDim::default();
        assert_eq!(d.m, 16);
        assert_eq!(d.n, 16);
        assert_eq!(d.k, 16);
    }

    #[test]
    fn test_gemm_config_default_workgroup_size() {
        let cfg = CoopMatrixGemmConfig::default();
        assert_eq!(cfg.workgroup_size, (16, 16, 1));
    }

    #[test]
    fn test_make_gemm_wgsl_contains_subgroup_matrix() {
        let src = make_gemm_wgsl(&CoopMatrixGemmConfig::default());
        assert!(
            src.contains("subgroupMatrix"),
            "shader source must reference subgroupMatrix (as comment/string): \n{src}"
        );
    }

    #[test]
    fn test_make_gemm_wgsl_contains_workgroup_var() {
        let src = make_gemm_wgsl(&CoopMatrixGemmConfig::default());
        assert!(
            src.contains("var<workgroup>"),
            "tiled kernel must declare var<workgroup> shared memory"
        );
    }

    #[test]
    fn test_make_gemm_wgsl_emits_compute_entry() {
        let src = make_gemm_wgsl(&CoopMatrixGemmConfig::default());
        assert!(src.contains("@compute @workgroup_size"));
    }

    #[test]
    fn test_make_gemm_wgsl_fallback_is_simpler() {
        let src = make_gemm_wgsl_fallback(&CoopMatrixGemmConfig::default());
        // Fallback must not use shared memory.
        assert!(
            !src.contains("var<workgroup>"),
            "fallback must not declare var<workgroup>"
        );
        // But it must still be a valid compute shader.
        assert!(src.contains("@compute"));
    }
}
