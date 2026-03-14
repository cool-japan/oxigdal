// GPU raster algebra evaluation
// Supports: +, -, *, /, min, max, abs, sqrt, pow, clamp, nodata masking

struct AlgebraParams {
    width: u32,
    height: u32,
    operation: u32,   // 0=add, 1=sub, 2=mul, 3=div, 4=min, 5=max, 6=sqrt, 7=abs
    scalar: f32,      // scalar operand for binary ops
    nodata_a: f32,
    nodata_b: f32,
    use_nodata: u32,
    output_nodata: f32,
};

@group(0) @binding(0) var<uniform> params: AlgebraParams;
@group(0) @binding(1) var<storage, read> band_a: array<f32>;
@group(0) @binding(2) var<storage, read> band_b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let total = params.width * params.height;
    if idx >= total { return; }

    let a = band_a[idx];
    let b = band_b[idx];

    // Nodata masking
    if params.use_nodata != 0u {
        if abs(a - params.nodata_a) < 0.001 || abs(b - params.nodata_b) < 0.001 {
            output[idx] = params.output_nodata;
            return;
        }
    }

    var result: f32;
    switch params.operation {
        case 0u: { result = a + b; }
        case 1u: { result = a - b; }
        case 2u: { result = a * b; }
        case 3u: { result = select(params.output_nodata, a / b, abs(b) > 0.0001); }
        case 4u: { result = min(a, b); }
        case 5u: { result = max(a, b); }
        case 6u: { result = sqrt(max(0.0, a)); }
        case 7u: { result = abs(a); }
        default: { result = a; }
    }

    output[idx] = result;
}
