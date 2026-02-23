// Texture analysis and feature extraction using GLCM (Gray Level Co-occurrence Matrix)

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> glcm: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> features: array<f32>;
@group(0) @binding(3) var<uniform> params: TextureParams;

struct TextureParams {
    width: u32,
    height: u32,
    levels: u32,        // Number of gray levels
    distance: u32,      // Pixel distance for GLCM
    direction: u32,     // 0=horizontal, 1=diagonal, 2=vertical, 3=anti-diagonal
    min_val: f32,
    max_val: f32,
    pad: u32,
}

// Compute GLCM (Gray Level Co-occurrence Matrix)
@compute @workgroup_size(16, 16, 1)
fn compute_glcm(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    // Quantize pixel value to gray level
    let idx = y * params.width + x;
    let value = input[idx];
    let normalized = (value - params.min_val) / (params.max_val - params.min_val);
    let level = u32(clamp(normalized * f32(params.levels - 1u), 0.0, f32(params.levels - 1u)));

    // Determine neighbor position based on direction
    var neighbor_x: i32 = i32(x);
    var neighbor_y: i32 = y;

    if (params.direction == 0u) {
        // Horizontal
        neighbor_x = neighbor_x + i32(params.distance);
    } else if (params.direction == 1u) {
        // Diagonal (45 degrees)
        neighbor_x = neighbor_x + i32(params.distance);
        neighbor_y = neighbor_y - i32(params.distance);
    } else if (params.direction == 2u) {
        // Vertical
        neighbor_y = neighbor_y + i32(params.distance);
    } else {
        // Anti-diagonal (135 degrees)
        neighbor_x = neighbor_x - i32(params.distance);
        neighbor_y = neighbor_y + i32(params.distance);
    }

    // Check bounds
    if (neighbor_x >= 0 && neighbor_x < i32(params.width) &&
        neighbor_y >= 0 && neighbor_y < i32(params.height)) {

        let neighbor_idx = u32(neighbor_y) * params.width + u32(neighbor_x);
        let neighbor_value = input[neighbor_idx];
        let neighbor_normalized = (neighbor_value - params.min_val) / (params.max_val - params.min_val);
        let neighbor_level = u32(clamp(neighbor_normalized * f32(params.levels - 1u), 0.0, f32(params.levels - 1u)));

        // Update GLCM
        let glcm_idx = level * params.levels + neighbor_level;
        atomicAdd(&glcm[glcm_idx], 1u);
    }
}

// Normalize GLCM
@compute @workgroup_size(256, 1, 1)
fn normalize_glcm(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let idx = global_id.x;
    let total = params.levels * params.levels;

    if (idx >= total) {
        return;
    }

    // Compute sum
    var sum = 0u;
    for (var i = 0u; i < total; i = i + 1u) {
        sum = sum + atomicLoad(&glcm[i]);
    }

    // Normalize (stored as u32, convert when computing features)
    let value = atomicLoad(&glcm[idx]);
    // Normalization done in feature computation
}

// Compute GLCM features
@compute @workgroup_size(1, 1, 1)
fn compute_glcm_features(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let levels = params.levels;
    let total = levels * levels;

    // Compute sum for normalization
    var sum = 0u;
    for (var i = 0u; i < total; i = i + 1u) {
        sum = sum + atomicLoad(&glcm[i]);
    }

    let norm_factor = f32(sum);

    // Feature 0: Contrast (variance)
    var contrast: f32 = 0.0;
    for (var i = 0u; i < levels; i = i + 1u) {
        for (var j = 0u; j < levels; j = j + 1u) {
            let idx = i * levels + j;
            let p = f32(atomicLoad(&glcm[idx])) / norm_factor;
            let diff = f32(i) - f32(j);
            contrast = contrast + diff * diff * p;
        }
    }
    features[0] = contrast;

    // Feature 1: Correlation
    var mean_i: f32 = 0.0;
    var mean_j: f32 = 0.0;
    for (var i = 0u; i < levels; i = i + 1u) {
        for (var j = 0u; j < levels; j = j + 1u) {
            let idx = i * levels + j;
            let p = f32(atomicLoad(&glcm[idx])) / norm_factor;
            mean_i = mean_i + f32(i) * p;
            mean_j = mean_j + f32(j) * p;
        }
    }

    var std_i: f32 = 0.0;
    var std_j: f32 = 0.0;
    for (var i = 0u; i < levels; i = i + 1u) {
        for (var j = 0u; j < levels; j = j + 1u) {
            let idx = i * levels + j;
            let p = f32(atomicLoad(&glcm[idx])) / norm_factor;
            std_i = std_i + (f32(i) - mean_i) * (f32(i) - mean_i) * p;
            std_j = std_j + (f32(j) - mean_j) * (f32(j) - mean_j) * p;
        }
    }
    std_i = sqrt(std_i);
    std_j = sqrt(std_j);

    var correlation: f32 = 0.0;
    for (var i = 0u; i < levels; i = i + 1u) {
        for (var j = 0u; j < levels; j = j + 1u) {
            let idx = i * levels + j;
            let p = f32(atomicLoad(&glcm[idx])) / norm_factor;
            correlation = correlation + (f32(i) - mean_i) * (f32(j) - mean_j) * p;
        }
    }
    if (std_i > 0.0 && std_j > 0.0) {
        correlation = correlation / (std_i * std_j);
    }
    features[1] = correlation;

    // Feature 2: Energy (uniformity)
    var energy: f32 = 0.0;
    for (var i = 0u; i < total; i = i + 1u) {
        let p = f32(atomicLoad(&glcm[i])) / norm_factor;
        energy = energy + p * p;
    }
    features[2] = energy;

    // Feature 3: Homogeneity (inverse difference moment)
    var homogeneity: f32 = 0.0;
    for (var i = 0u; i < levels; i = i + 1u) {
        for (var j = 0u; j < levels; j = j + 1u) {
            let idx = i * levels + j;
            let p = f32(atomicLoad(&glcm[idx])) / norm_factor;
            let diff = f32(i) - f32(j);
            homogeneity = homogeneity + p / (1.0 + diff * diff);
        }
    }
    features[3] = homogeneity;

    // Feature 4: Entropy
    var entropy: f32 = 0.0;
    for (var i = 0u; i < total; i = i + 1u) {
        let p = f32(atomicLoad(&glcm[i])) / norm_factor;
        if (p > 0.0) {
            entropy = entropy - p * log2(p);
        }
    }
    features[4] = entropy;

    // Feature 5: Dissimilarity
    var dissimilarity: f32 = 0.0;
    for (var i = 0u; i < levels; i = i + 1u) {
        for (var j = 0u; j < levels; j = j + 1u) {
            let idx = i * levels + j;
            let p = f32(atomicLoad(&glcm[idx])) / norm_factor;
            dissimilarity = dissimilarity + abs(f32(i) - f32(j)) * p;
        }
    }
    features[5] = dissimilarity;
}

// Local Binary Pattern (LBP) for texture classification
@compute @workgroup_size(16, 16, 1)
fn local_binary_pattern(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x < 1u || x >= params.width - 1u || y < 1u || y >= params.height - 1u) {
        if (x < params.width && y < params.height) {
            let idx = y * params.width + x;
            features[idx] = 0.0;
        }
        return;
    }

    let idx_center = y * params.width + x;
    let center_value = input[idx_center];

    // Compare 8 neighbors
    var lbp_code: u32 = 0u;

    // Top-left
    if (input[(y - 1u) * params.width + (x - 1u)] >= center_value) {
        lbp_code = lbp_code | (1u << 0u);
    }
    // Top
    if (input[(y - 1u) * params.width + x] >= center_value) {
        lbp_code = lbp_code | (1u << 1u);
    }
    // Top-right
    if (input[(y - 1u) * params.width + (x + 1u)] >= center_value) {
        lbp_code = lbp_code | (1u << 2u);
    }
    // Right
    if (input[y * params.width + (x + 1u)] >= center_value) {
        lbp_code = lbp_code | (1u << 3u);
    }
    // Bottom-right
    if (input[(y + 1u) * params.width + (x + 1u)] >= center_value) {
        lbp_code = lbp_code | (1u << 4u);
    }
    // Bottom
    if (input[(y + 1u) * params.width + x] >= center_value) {
        lbp_code = lbp_code | (1u << 5u);
    }
    // Bottom-left
    if (input[(y + 1u) * params.width + (x - 1u)] >= center_value) {
        lbp_code = lbp_code | (1u << 6u);
    }
    // Left
    if (input[y * params.width + (x - 1u)] >= center_value) {
        lbp_code = lbp_code | (1u << 7u);
    }

    features[idx_center] = f32(lbp_code);
}

// Gabor filter for texture analysis
@compute @workgroup_size(16, 16, 1)
fn gabor_filter(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    // Gabor filter parameters (could be passed as uniforms)
    let wavelength: f32 = 10.0;
    let orientation: f32 = 0.0; // radians
    let phase: f32 = 0.0;
    let aspect_ratio: f32 = 0.5;
    let bandwidth: f32 = 1.0;

    let sigma = wavelength / 3.14159265359 * sqrt(log(2.0) / 2.0) *
                (pow(2.0, bandwidth) + 1.0) / (pow(2.0, bandwidth) - 1.0);

    let idx = y * params.width + x;
    let cx = f32(params.width) / 2.0;
    let cy = f32(params.height) / 2.0;

    let xp = (f32(x) - cx) * cos(orientation) + (f32(y) - cy) * sin(orientation);
    let yp = -(f32(x) - cx) * sin(orientation) + (f32(y) - cy) * cos(orientation);

    let gaussian = exp(-0.5 * (xp * xp + aspect_ratio * aspect_ratio * yp * yp) / (sigma * sigma));
    let sinusoid = cos(2.0 * 3.14159265359 * xp / wavelength + phase);

    features[idx] = input[idx] * gaussian * sinusoid;
}
