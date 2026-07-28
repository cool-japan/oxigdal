// GPU-accelerated raster reprojection.
//
// Each destination pixel is mapped back to source pixel coordinates using the
// destination inverse geo-transform (dst pixel -> geo) composed with the
// analytic inverse of the source geo-transform (geo -> src pixel).  This is the
// exact same math as `GpuReprojector::reproject_cpu`, so GPU and CPU outputs
// agree bit-for-bit up to floating-point rounding.
//
// The source raster is bound as a flat `array<f32>` storage buffer (row-major,
// width * height) rather than a texture so that nearest / bilinear sampling and
// nodata handling match the CPU reference precisely (hardware texture samplers
// would diverge at edges and on nodata).
//
// Uniform layout note: WGSL requires uniform-address-space array elements to
// have a 16-byte stride, so the six affine coefficients are packed into
// `vec4<f32>` slots (a,b,c,d) + (e,f,_,_) instead of a naive `array<f32,6>`.

struct ReprojParams {
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
    // 0 = nearest neighbour, 1 = bilinear
    resample_method: u32,
    use_nodata: u32,
    _pad0: u32,
    _pad1: u32,
    // Source geo-transform [a, b, c, d, e, f]:
    //   x_geo = c + col * a + row * b
    //   y_geo = f + col * d + row * e
    src_gt0: vec4<f32>, // (a, b, c, d)
    src_gt1: vec4<f32>, // (e, f, _, _)
    // Destination inverse geo-transform [a, b, c, d, e, f]:
    //   x_geo = a + col * b + row * c
    //   y_geo = d + col * e + row * f
    dst_inv_gt0: vec4<f32>, // (a, b, c, d)
    dst_inv_gt1: vec4<f32>, // (e, f, _, _)
    nodata: vec4<f32>,      // (nodata, _, _, _)
};

@group(0) @binding(0) var<uniform> params: ReprojParams;
@group(0) @binding(1) var<storage, read> src_buffer: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst_buffer: array<f32>;

fn sample_src(col: i32, row: i32, nodata_fill: f32) -> f32 {
    if col < 0 || row < 0 || col >= i32(params.src_width) || row >= i32(params.src_height) {
        return nodata_fill;
    }
    let idx = u32(row) * params.src_width + u32(col);
    return src_buffer[idx];
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dst_col = gid.x;
    let dst_row = gid.y;

    if dst_col >= params.dst_width || dst_row >= params.dst_height {
        return;
    }

    let dst_idx = dst_row * params.dst_width + dst_col;

    let nodata_fill = select(0.0, params.nodata.x, params.use_nodata != 0u);

    // Source geo-transform coefficients.
    let a = params.src_gt0.x;
    let b = params.src_gt0.y;
    let c = params.src_gt0.z;
    let d = params.src_gt0.w;
    let e = params.src_gt1.x;
    let f = params.src_gt1.y;

    // Destination inverse geo-transform coefficients.
    let ia = params.dst_inv_gt0.x;
    let ib = params.dst_inv_gt0.y;
    let ic = params.dst_inv_gt0.z;
    let id = params.dst_inv_gt0.w;
    let ie = params.dst_inv_gt1.x;
    let if_ = params.dst_inv_gt1.y;

    // Pixel centre in destination pixel space.
    let dst_x = f32(dst_col) + 0.5;
    let dst_y = f32(dst_row) + 0.5;

    // Destination pixel -> destination geo coordinates.
    let geo_x = ia + dst_x * ib + dst_y * ic;
    let geo_y = id + dst_x * ie + dst_y * if_;

    // Destination geo -> source pixel coordinates via analytic inverse.
    let det = a * e - b * d;

    var src_col_f: f32;
    var src_row_f: f32;
    if abs(det) > 1.1920929e-7 { // f32::EPSILON
        let dx = geo_x - c;
        let dy = geo_y - f;
        src_col_f = (e * dx - b * dy) / det;
        src_row_f = (a * dy - d * dx) / det;
    } else {
        // Degenerate source transform: fall back to naive ratio scaling.
        src_col_f = f32(dst_col) * f32(params.src_width) / f32(params.dst_width);
        src_row_f = f32(dst_row) * f32(params.src_height) / f32(params.dst_height);
    }

    var result: f32 = nodata_fill;

    if params.resample_method == 0u {
        // Nearest neighbour: truncate toward zero (matches Rust `as i32`).
        let sc = i32(src_col_f);
        let sr = i32(src_row_f);
        if sc >= 0 && sr >= 0 && sc < i32(params.src_width) && sr < i32(params.src_height) {
            let idx = u32(sr) * params.src_width + u32(sc);
            result = src_buffer[idx];
        }
    } else {
        // Bilinear interpolation with nodata fill on out-of-bounds taps.
        let fx = floor(src_col_f);
        let fy = floor(src_row_f);
        let x0 = i32(fx);
        let y0 = i32(fy);
        let x1 = x0 + 1;
        let y1 = y0 + 1;
        let tx = src_col_f - fx;
        let ty = src_row_f - fy;

        let v00 = sample_src(x0, y0, nodata_fill);
        let v10 = sample_src(x1, y0, nodata_fill);
        let v01 = sample_src(x0, y1, nodata_fill);
        let v11 = sample_src(x1, y1, nodata_fill);

        let v0 = v00 + (v10 - v00) * tx;
        let v1 = v01 + (v11 - v01) * tx;
        result = v0 + (v1 - v0) * ty;
    }

    dst_buffer[dst_idx] = result;
}
