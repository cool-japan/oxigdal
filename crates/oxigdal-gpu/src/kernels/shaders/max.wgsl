// Element-wise maximum shader
// Computes: output[i] = max(input_a[i], input_b[i])

@group(0) @binding(0) var<storage, read> input_a: array<f32>;
@group(0) @binding(1) var<storage, read> input_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn max_op(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {
        return;
    }
    output[idx] = max(input_a[idx], input_b[idx]);
}
