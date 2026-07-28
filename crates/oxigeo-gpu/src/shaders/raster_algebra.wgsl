// GPU raster algebra evaluation.
// Supported operations (params.operation):
//   0=add, 1=sub, 2=mul, 3=div, 4=min, 5=max, 6=sqrt, 7=abs,
//   8=pow(a, scalar0), 9=clamp(a, scalar0, scalar1),
//   10=normalize(a; src_min=scalar0, src_max=scalar1, dst_min=scalar2, dst_max=scalar3)
// Unary ops (6..=10) ignore band_b. Binary ops (0..=5) require has_b != 0.
// An unrecognized operation code writes NaN as an explicit sentinel rather than
// silently passing the input through unchanged.

struct AlgebraParams {
    width: u32,
    height: u32,
    operation: u32,
    use_nodata: u32,
    // Whether band_b holds real data (binary op) vs. a zeroed placeholder.
    has_b: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
    nodata_a: f32,
    nodata_b: f32,
    output_nodata: f32,
    scalar0: f32,
    scalar1: f32,
    scalar2: f32,
    scalar3: f32,
    _p3: f32,
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

    // Nodata masking (matches the CPU `is_nodata` 1e-6 threshold).
    if params.use_nodata != 0u {
        let a_masked = abs(a - params.nodata_a) < 1e-6;
        let b_masked = params.has_b != 0u && abs(b - params.nodata_b) < 1e-6;
        if a_masked || b_masked {
            output[idx] = params.output_nodata;
            return;
        }
    }

    var result: f32;
    switch params.operation {
        case 0u: { result = a + b; }
        case 1u: { result = a - b; }
        case 2u: { result = a * b; }
        case 3u: { result = select(params.output_nodata, a / b, abs(b) > 1e-10); }
        case 4u: { result = min(a, b); }
        case 5u: { result = max(a, b); }
        case 6u: { result = sqrt(max(0.0, a)); }
        case 7u: { result = abs(a); }
        case 8u: { result = pow(a, params.scalar0); }
        case 9u: { result = clamp(a, params.scalar0, params.scalar1); }
        case 10u: {
            let range = params.scalar1 - params.scalar0;
            result = select(
                params.scalar2,
                (a - params.scalar0) / range * (params.scalar3 - params.scalar2) + params.scalar2,
                abs(range) >= 1e-10
            );
        }
        // Unknown operation code: explicit NaN sentinel (never silent identity).
        default: { result = bitcast<f32>(0x7fc00000u); }
    }

    output[idx] = result;
}
