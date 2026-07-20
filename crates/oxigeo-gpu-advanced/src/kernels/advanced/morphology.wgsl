// Advanced morphological operations for image processing
// Includes dilation, erosion, opening, closing, and morphological gradient

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<storage, read> structuring_element: array<f32>;
@group(0) @binding(3) var<uniform> params: MorphParams;

struct MorphParams {
    width: u32,
    height: u32,
    kernel_width: u32,
    kernel_height: u32,
    threshold: f32,
    pad: array<u32, 3>,
}

// Dilation: expands bright regions
@compute @workgroup_size(16, 16, 1)
fn dilate(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let half_kw = params.kernel_width / 2u;
    let half_kh = params.kernel_height / 2u;

    var max_val: f32 = 0.0;

    for (var ky = 0u; ky < params.kernel_height; ky = ky + 1u) {
        for (var kx = 0u; kx < params.kernel_width; kx = kx + 1u) {
            let offset_x = i32(x) + i32(kx) - i32(half_kw);
            let offset_y = i32(y) + i32(ky) - i32(half_kh);

            if (offset_x >= 0 && offset_x < i32(params.width) &&
                offset_y >= 0 && offset_y < i32(params.height)) {

                let idx = u32(offset_y) * params.width + u32(offset_x);
                let k_idx = ky * params.kernel_width + kx;

                if (structuring_element[k_idx] > 0.0) {
                    max_val = max(max_val, input[idx]);
                }
            }
        }
    }

    let out_idx = y * params.width + x;
    output[out_idx] = max_val;
}

// Erosion: shrinks bright regions
@compute @workgroup_size(16, 16, 1)
fn erode(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let half_kw = params.kernel_width / 2u;
    let half_kh = params.kernel_height / 2u;

    var min_val: f32 = 1e10;

    for (var ky = 0u; ky < params.kernel_height; ky = ky + 1u) {
        for (var kx = 0u; kx < params.kernel_width; kx = kx + 1u) {
            let offset_x = i32(x) + i32(kx) - i32(half_kw);
            let offset_y = i32(y) + i32(ky) - i32(half_kh);

            if (offset_x >= 0 && offset_x < i32(params.width) &&
                offset_y >= 0 && offset_y < i32(params.height)) {

                let idx = u32(offset_y) * params.width + u32(offset_x);
                let k_idx = ky * params.kernel_width + kx;

                if (structuring_element[k_idx] > 0.0) {
                    min_val = min(min_val, input[idx]);
                }
            }
        }
    }

    let out_idx = y * params.width + x;
    output[out_idx] = min_val;
}

// Opening: erosion followed by dilation
// Removes small bright spots
@compute @workgroup_size(16, 16, 1)
fn opening(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    // First pass: erosion (assuming temp buffer exists)
    // Second pass: dilation
    // This is simplified; full implementation requires two passes
    erode(global_id);
}

// Closing: dilation followed by erosion
// Removes small dark spots
@compute @workgroup_size(16, 16, 1)
fn closing(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    // First pass: dilation (assuming temp buffer exists)
    // Second pass: erosion
    // This is simplified; full implementation requires two passes
    dilate(global_id);
}

// Morphological gradient: dilation - erosion
@compute @workgroup_size(16, 16, 1)
fn morphological_gradient(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let half_kw = params.kernel_width / 2u;
    let half_kh = params.kernel_height / 2u;

    var max_val: f32 = 0.0;
    var min_val: f32 = 1e10;

    for (var ky = 0u; ky < params.kernel_height; ky = ky + 1u) {
        for (var kx = 0u; kx < params.kernel_width; kx = kx + 1u) {
            let offset_x = i32(x) + i32(kx) - i32(half_kw);
            let offset_y = i32(y) + i32(ky) - i32(half_kh);

            if (offset_x >= 0 && offset_x < i32(params.width) &&
                offset_y >= 0 && offset_y < i32(params.height)) {

                let idx = u32(offset_y) * params.width + u32(offset_x);
                let k_idx = ky * params.kernel_width + kx;

                if (structuring_element[k_idx] > 0.0) {
                    max_val = max(max_val, input[idx]);
                    min_val = min(min_val, input[idx]);
                }
            }
        }
    }

    let out_idx = y * params.width + x;
    output[out_idx] = max_val - min_val;
}

// Top-hat transform: input - opening(input)
@compute @workgroup_size(16, 16, 1)
fn top_hat(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let original = input[idx];

    // Compute opening (simplified)
    let half_kw = params.kernel_width / 2u;
    let half_kh = params.kernel_height / 2u;

    var min_val: f32 = 1e10;
    for (var ky = 0u; ky < params.kernel_height; ky = ky + 1u) {
        for (var kx = 0u; kx < params.kernel_width; kx = kx + 1u) {
            let offset_x = i32(x) + i32(kx) - i32(half_kw);
            let offset_y = i32(y) + i32(ky) - i32(half_kh);

            if (offset_x >= 0 && offset_x < i32(params.width) &&
                offset_y >= 0 && offset_y < i32(params.height)) {

                let pidx = u32(offset_y) * params.width + u32(offset_x);
                let k_idx = ky * params.kernel_width + kx;

                if (structuring_element[k_idx] > 0.0) {
                    min_val = min(min_val, input[pidx]);
                }
            }
        }
    }

    output[idx] = original - min_val;
}

// Black-hat transform: closing(input) - input
@compute @workgroup_size(16, 16, 1)
fn black_hat(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let original = input[idx];

    // Compute closing (simplified)
    let half_kw = params.kernel_width / 2u;
    let half_kh = params.kernel_height / 2u;

    var max_val: f32 = 0.0;
    for (var ky = 0u; ky < params.kernel_height; ky = ky + 1u) {
        for (var kx = 0u; kx < params.kernel_width; kx = kx + 1u) {
            let offset_x = i32(x) + i32(kx) - i32(half_kw);
            let offset_y = i32(y) + i32(ky) - i32(half_kh);

            if (offset_x >= 0 && offset_x < i32(params.width) &&
                offset_y >= 0 && offset_y < i32(params.height)) {

                let pidx = u32(offset_y) * params.width + u32(offset_x);
                let k_idx = ky * params.kernel_width + kx;

                if (structuring_element[k_idx] > 0.0) {
                    max_val = max(max_val, input[pidx]);
                }
            }
        }
    }

    output[idx] = max_val - original;
}
