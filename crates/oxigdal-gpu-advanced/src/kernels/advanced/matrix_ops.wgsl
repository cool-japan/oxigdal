// Advanced matrix operations (GEMM - General Matrix Multiply) on GPU
// Implements tiled matrix multiplication with shared memory optimization

@group(0) @binding(0) var<storage, read> matrix_a: array<f32>;
@group(0) @binding(1) var<storage, read> matrix_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> matrix_c: array<f32>;

struct Dimensions {
    m: u32,  // rows of A and C
    n: u32,  // cols of B and C
    k: u32,  // cols of A and rows of B
    pad: u32,
}

@group(0) @binding(3) var<uniform> dims: Dimensions;

const TILE_SIZE: u32 = 16u;

var<workgroup> tile_a: array<f32, 256>; // 16x16 tile
var<workgroup> tile_b: array<f32, 256>; // 16x16 tile

@compute @workgroup_size(16, 16, 1)
fn matrix_multiply_tiled(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
) {
    let row = global_id.y;
    let col = global_id.x;

    if (row >= dims.m || col >= dims.n) {
        return;
    }

    var sum: f32 = 0.0;
    let num_tiles = (dims.k + TILE_SIZE - 1u) / TILE_SIZE;

    // Iterate over tiles
    for (var t = 0u; t < num_tiles; t = t + 1u) {
        // Load tile of A into shared memory
        let tile_col = local_id.x;
        let tile_row = local_id.y;
        let a_col = t * TILE_SIZE + tile_col;
        let a_row = workgroup_id.y * TILE_SIZE + tile_row;

        if (a_row < dims.m && a_col < dims.k) {
            let a_idx = a_row * dims.k + a_col;
            tile_a[tile_row * TILE_SIZE + tile_col] = matrix_a[a_idx];
        } else {
            tile_a[tile_row * TILE_SIZE + tile_col] = 0.0;
        }

        // Load tile of B into shared memory
        let b_row = t * TILE_SIZE + tile_row;
        let b_col = workgroup_id.x * TILE_SIZE + tile_col;

        if (b_row < dims.k && b_col < dims.n) {
            let b_idx = b_row * dims.n + b_col;
            tile_b[tile_row * TILE_SIZE + tile_col] = matrix_b[b_idx];
        } else {
            tile_b[tile_row * TILE_SIZE + tile_col] = 0.0;
        }

        workgroupBarrier();

        // Compute partial dot product
        for (var i = 0u; i < TILE_SIZE; i = i + 1u) {
            sum = sum + tile_a[local_id.y * TILE_SIZE + i] *
                       tile_b[i * TILE_SIZE + local_id.x];
        }

        workgroupBarrier();
    }

    // Write result
    let c_idx = row * dims.n + col;
    matrix_c[c_idx] = sum;
}

// Naive matrix multiplication for small matrices
@compute @workgroup_size(8, 8, 1)
fn matrix_multiply_naive(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let row = global_id.y;
    let col = global_id.x;

    if (row >= dims.m || col >= dims.n) {
        return;
    }

    var sum: f32 = 0.0;
    for (var i = 0u; i < dims.k; i = i + 1u) {
        let a_idx = row * dims.k + i;
        let b_idx = i * dims.n + col;
        sum = sum + matrix_a[a_idx] * matrix_b[b_idx];
    }

    let c_idx = row * dims.n + col;
    matrix_c[c_idx] = sum;
}

// Matrix transpose
@compute @workgroup_size(16, 16, 1)
fn matrix_transpose(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let row = global_id.y;
    let col = global_id.x;

    if (row >= dims.m || col >= dims.n) {
        return;
    }

    let in_idx = row * dims.n + col;
    let out_idx = col * dims.m + row;

    matrix_c[out_idx] = matrix_a[in_idx];
}

// Matrix-vector multiplication
@compute @workgroup_size(256, 1, 1)
fn matrix_vector_multiply(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let row = global_id.x;

    if (row >= dims.m) {
        return;
    }

    var sum: f32 = 0.0;
    for (var i = 0u; i < dims.n; i = i + 1u) {
        let a_idx = row * dims.n + i;
        sum = sum + matrix_a[a_idx] * matrix_b[i];
    }

    matrix_c[row] = sum;
}
