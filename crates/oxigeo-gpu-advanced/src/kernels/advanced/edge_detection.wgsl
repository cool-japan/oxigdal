// Advanced edge detection algorithms: Sobel, Prewitt, Canny, Laplacian

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<storage, read_write> gradient_x: array<f32>;
@group(0) @binding(3) var<storage, read_write> gradient_y: array<f32>;
@group(0) @binding(4) var<uniform> params: EdgeParams;

struct EdgeParams {
    width: u32,
    height: u32,
    low_threshold: f32,
    high_threshold: f32,
    pad: array<u32, 4>,
}

// Sobel edge detection
@compute @workgroup_size(16, 16, 1)
fn sobel(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x < 1u || x >= params.width - 1u || y < 1u || y >= params.height - 1u) {
        if (x < params.width && y < params.height) {
            let idx = y * params.width + x;
            output[idx] = 0.0;
        }
        return;
    }

    // Sobel kernels
    // Gx = [[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]]
    // Gy = [[-1, -2, -1], [0, 0, 0], [1, 2, 1]]

    var gx: f32 = 0.0;
    var gy: f32 = 0.0;

    // Top row
    let idx_tl = (y - 1u) * params.width + (x - 1u);
    let idx_tc = (y - 1u) * params.width + x;
    let idx_tr = (y - 1u) * params.width + (x + 1u);

    // Middle row
    let idx_ml = y * params.width + (x - 1u);
    let idx_mr = y * params.width + (x + 1u);

    // Bottom row
    let idx_bl = (y + 1u) * params.width + (x - 1u);
    let idx_bc = (y + 1u) * params.width + x;
    let idx_br = (y + 1u) * params.width + (x + 1u);

    // Compute Gx
    gx = -input[idx_tl] + input[idx_tr]
         - 2.0 * input[idx_ml] + 2.0 * input[idx_mr]
         - input[idx_bl] + input[idx_br];

    // Compute Gy
    gy = -input[idx_tl] - 2.0 * input[idx_tc] - input[idx_tr]
         + input[idx_bl] + 2.0 * input[idx_bc] + input[idx_br];

    // Compute magnitude
    let magnitude = sqrt(gx * gx + gy * gy);

    let idx = y * params.width + x;
    output[idx] = magnitude;
    gradient_x[idx] = gx;
    gradient_y[idx] = gy;
}

// Prewitt edge detection
@compute @workgroup_size(16, 16, 1)
fn prewitt(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x < 1u || x >= params.width - 1u || y < 1u || y >= params.height - 1u) {
        if (x < params.width && y < params.height) {
            let idx = y * params.width + x;
            output[idx] = 0.0;
        }
        return;
    }

    // Prewitt kernels
    // Gx = [[-1, 0, 1], [-1, 0, 1], [-1, 0, 1]]
    // Gy = [[-1, -1, -1], [0, 0, 0], [1, 1, 1]]

    var gx: f32 = 0.0;
    var gy: f32 = 0.0;

    let idx_tl = (y - 1u) * params.width + (x - 1u);
    let idx_tc = (y - 1u) * params.width + x;
    let idx_tr = (y - 1u) * params.width + (x + 1u);
    let idx_ml = y * params.width + (x - 1u);
    let idx_mr = y * params.width + (x + 1u);
    let idx_bl = (y + 1u) * params.width + (x - 1u);
    let idx_bc = (y + 1u) * params.width + x;
    let idx_br = (y + 1u) * params.width + (x + 1u);

    gx = -input[idx_tl] + input[idx_tr]
         - input[idx_ml] + input[idx_mr]
         - input[idx_bl] + input[idx_br];

    gy = -input[idx_tl] - input[idx_tc] - input[idx_tr]
         + input[idx_bl] + input[idx_bc] + input[idx_br];

    let magnitude = sqrt(gx * gx + gy * gy);

    let idx = y * params.width + x;
    output[idx] = magnitude;
}

// Laplacian edge detection
@compute @workgroup_size(16, 16, 1)
fn laplacian(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x < 1u || x >= params.width - 1u || y < 1u || y >= params.height - 1u) {
        if (x < params.width && y < params.height) {
            let idx = y * params.width + x;
            output[idx] = 0.0;
        }
        return;
    }

    // Laplacian kernel
    // [[0, 1, 0], [1, -4, 1], [0, 1, 0]]

    let idx_center = y * params.width + x;
    let idx_top = (y - 1u) * params.width + x;
    let idx_bottom = (y + 1u) * params.width + x;
    let idx_left = y * params.width + (x - 1u);
    let idx_right = y * params.width + (x + 1u);

    let laplacian_val = input[idx_top] + input[idx_bottom]
                      + input[idx_left] + input[idx_right]
                      - 4.0 * input[idx_center];

    output[idx_center] = abs(laplacian_val);
}

// Canny edge detection - Step 1: Compute gradients
@compute @workgroup_size(16, 16, 1)
fn canny_gradient(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    sobel(global_id);
}

// Canny edge detection - Step 2: Non-maximum suppression
@compute @workgroup_size(16, 16, 1)
fn canny_nms(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x < 1u || x >= params.width - 1u || y < 1u || y >= params.height - 1u) {
        if (x < params.width && y < params.height) {
            let idx = y * params.width + x;
            output[idx] = 0.0;
        }
        return;
    }

    let idx = y * params.width + x;
    let gx = gradient_x[idx];
    let gy = gradient_y[idx];
    let magnitude = sqrt(gx * gx + gy * gy);

    // Compute gradient direction
    let angle = atan2(gy, gx);

    // Quantize angle to 4 directions (0, 45, 90, 135 degrees)
    var direction: i32 = 0;
    let pi = 3.14159265359;
    let angle_deg = angle * 180.0 / pi;

    if (angle_deg >= -22.5 && angle_deg < 22.5) || (angle_deg >= 157.5 || angle_deg < -157.5) {
        direction = 0; // Horizontal
    } else if (angle_deg >= 22.5 && angle_deg < 67.5) || (angle_deg >= -157.5 && angle_deg < -112.5) {
        direction = 1; // Diagonal /
    } else if (angle_deg >= 67.5 && angle_deg < 112.5) || (angle_deg >= -112.5 && angle_deg < -67.5) {
        direction = 2; // Vertical
    } else {
        direction = 3; // Diagonal \
    }

    // Compare with neighbors in gradient direction
    var neighbor1: f32;
    var neighbor2: f32;

    if (direction == 0) {
        neighbor1 = sqrt(gradient_x[idx - 1u] * gradient_x[idx - 1u] + gradient_y[idx - 1u] * gradient_y[idx - 1u]);
        neighbor2 = sqrt(gradient_x[idx + 1u] * gradient_x[idx + 1u] + gradient_y[idx + 1u] * gradient_y[idx + 1u]);
    } else if (direction == 1) {
        let idx_ne = (y - 1u) * params.width + (x + 1u);
        let idx_sw = (y + 1u) * params.width + (x - 1u);
        neighbor1 = sqrt(gradient_x[idx_ne] * gradient_x[idx_ne] + gradient_y[idx_ne] * gradient_y[idx_ne]);
        neighbor2 = sqrt(gradient_x[idx_sw] * gradient_x[idx_sw] + gradient_y[idx_sw] * gradient_y[idx_sw]);
    } else if (direction == 2) {
        let idx_n = (y - 1u) * params.width + x;
        let idx_s = (y + 1u) * params.width + x;
        neighbor1 = sqrt(gradient_x[idx_n] * gradient_x[idx_n] + gradient_y[idx_n] * gradient_y[idx_n]);
        neighbor2 = sqrt(gradient_x[idx_s] * gradient_x[idx_s] + gradient_y[idx_s] * gradient_y[idx_s]);
    } else {
        let idx_nw = (y - 1u) * params.width + (x - 1u);
        let idx_se = (y + 1u) * params.width + (x + 1u);
        neighbor1 = sqrt(gradient_x[idx_nw] * gradient_x[idx_nw] + gradient_y[idx_nw] * gradient_y[idx_nw]);
        neighbor2 = sqrt(gradient_x[idx_se] * gradient_x[idx_se] + gradient_y[idx_se] * gradient_y[idx_se]);
    }

    // Suppress non-maximum
    if (magnitude >= neighbor1 && magnitude >= neighbor2) {
        output[idx] = magnitude;
    } else {
        output[idx] = 0.0;
    }
}

// Canny edge detection - Step 3: Double thresholding
@compute @workgroup_size(16, 16, 1)
fn canny_threshold(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let magnitude = input[idx];

    if (magnitude >= params.high_threshold) {
        output[idx] = 1.0; // Strong edge
    } else if (magnitude >= params.low_threshold) {
        output[idx] = 0.5; // Weak edge
    } else {
        output[idx] = 0.0; // Not an edge
    }
}
