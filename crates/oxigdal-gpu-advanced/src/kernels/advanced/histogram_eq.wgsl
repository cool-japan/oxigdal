// Histogram equalization for image enhancement

@group(0) @binding(0) var<storage, read> input_image: array<f32>;
@group(0) @binding(1) var<storage, read_write> output_image: array<f32>;
@group(0) @binding(2) var<storage, read_write> histogram: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> cdf: array<u32>;
@group(0) @binding(4) var<uniform> params: HistogramParams;

struct HistogramParams {
    width: u32,
    height: u32,
    bins: u32,
    min_val: f32,
    max_val: f32,
    pad: array<u32, 3>,
}

// Step 1: Compute histogram
@compute @workgroup_size(16, 16, 1)
fn compute_histogram(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let value = input_image[idx];

    // Normalize value to bin index
    let normalized = (value - params.min_val) / (params.max_val - params.min_val);
    let bin = u32(clamp(normalized * f32(params.bins - 1u), 0.0, f32(params.bins - 1u)));

    atomicAdd(&histogram[bin], 1u);
}

// Step 2: Compute cumulative distribution function (CDF)
@compute @workgroup_size(256, 1, 1)
fn compute_cdf(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let bin = global_id.x;

    if (bin >= params.bins) {
        return;
    }

    var sum = 0u;
    for (var i = 0u; i <= bin; i = i + 1u) {
        sum = sum + atomicLoad(&histogram[i]);
    }

    cdf[bin] = sum;
}

// Step 3: Equalize using CDF
@compute @workgroup_size(16, 16, 1)
fn histogram_equalize(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let value = input_image[idx];

    // Find bin
    let normalized = (value - params.min_val) / (params.max_val - params.min_val);
    let bin = u32(clamp(normalized * f32(params.bins - 1u), 0.0, f32(params.bins - 1u)));

    // Apply equalization
    let total_pixels = params.width * params.height;
    let cdf_min = cdf[0];
    let cdf_val = cdf[bin];

    let equalized = f32(cdf_val - cdf_min) / f32(total_pixels - cdf_min);
    output_image[idx] = params.min_val + equalized * (params.max_val - params.min_val);
}

// Adaptive histogram equalization (CLAHE - simplified)
@compute @workgroup_size(8, 8, 1)
fn clahe(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    // Define local region (simplified - would use actual tiles in full implementation)
    let region_size = 32u;
    let local_x_start = (x / region_size) * region_size;
    let local_y_start = (y / region_size) * region_size;

    // Compute local histogram
    var local_hist: array<u32, 256>;
    for (var i = 0u; i < 256u; i = i + 1u) {
        local_hist[i] = 0u;
    }

    for (var dy = 0u; dy < region_size; dy = dy + 1u) {
        for (var dx = 0u; dx < region_size; dx = dx + 1u) {
            let px = local_x_start + dx;
            let py = local_y_start + dy;

            if (px < params.width && py < params.height) {
                let pidx = py * params.width + px;
                let pval = input_image[pidx];
                let normalized = (pval - params.min_val) / (params.max_val - params.min_val);
                let bin = u32(clamp(normalized * 255.0, 0.0, 255.0));
                local_hist[bin] = local_hist[bin] + 1u;
            }
        }
    }

    // Apply local equalization
    let idx = y * params.width + x;
    let value = input_image[idx];
    let normalized = (value - params.min_val) / (params.max_val - params.min_val);
    let bin = u32(clamp(normalized * 255.0, 0.0, 255.0));

    // Compute local CDF
    var local_cdf = 0u;
    for (var i = 0u; i <= bin; i = i + 1u) {
        local_cdf = local_cdf + local_hist[i];
    }

    let total = region_size * region_size;
    let equalized = f32(local_cdf) / f32(total);
    output_image[idx] = params.min_val + equalized * (params.max_val - params.min_val);
}
