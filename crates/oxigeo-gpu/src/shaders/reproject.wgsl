// GPU-accelerated raster reprojection
// Transforms each output pixel to source coordinates using the inverse of the target CRS transform

struct ReprojParams {
    // Source raster dimensions
    src_width: u32,
    src_height: u32,
    // Output raster dimensions
    dst_width: u32,
    dst_height: u32,
    // Source geo-transform (affine): [a, b, c, d, e, f]
    // x_geo = c + col * a + row * b
    // y_geo = f + col * d + row * e
    src_gt: array<f32, 6>,
    // Destination inverse geo-transform
    dst_inv_gt: array<f32, 6>,
    // Resampling method: 0=nearest, 1=bilinear
    resample_method: u32,
    // Nodata value
    nodata: f32,
    use_nodata: u32,
};

@group(0) @binding(0) var<uniform> params: ReprojParams;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;
@group(0) @binding(3) var<storage, read_write> dst_buffer: array<f32>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dst_col = gid.x;
    let dst_row = gid.y;

    if dst_col >= params.dst_width || dst_row >= params.dst_height {
        return;
    }

    let dst_idx = dst_row * params.dst_width + dst_col;

    // For now: simple identity passthrough (real reprojection requires CRS math)
    let src_col = f32(dst_col) * f32(params.src_width) / f32(params.dst_width);
    let src_row = f32(dst_row) * f32(params.src_height) / f32(params.dst_height);

    let uv = vec2<f32>(src_col / f32(params.src_width), src_row / f32(params.src_height));
    let value = textureSampleLevel(src_texture, src_sampler, uv, 0.0).r;

    dst_buffer[dst_idx] = value;
}
