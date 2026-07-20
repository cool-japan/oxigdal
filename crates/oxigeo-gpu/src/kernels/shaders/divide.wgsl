// Element-wise division shader with safe division (avoid divide by zero)
// Computes: output[i] = input_a[i] / input_b[i], or 0.0 if input_b[i] is near zero

@group(0) @binding(0) var<storage, read> input_a: array<f32>;
@group(0) @binding(1) var<storage, read> input_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

fn safe_div(num: f32, denom: f32) -> f32 {
    if (abs(denom) < 1e-10) {
        return 0.0;
    }
    return num / denom;
}

@compute @workgroup_size(256)
fn divide(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {
        return;
    }
    output[idx] = safe_div(input_a[idx], input_b[idx]);
}
