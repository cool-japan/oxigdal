// GPU terrain hillshade computation

struct HillshadeParams {
    width: u32,
    height: u32,
    cell_size_x: f32,
    cell_size_y: f32,
    azimuth_rad: f32,   // sun azimuth in radians
    altitude_rad: f32,  // sun altitude in radians
    z_factor: f32,      // vertical exaggeration
    nodata: f32,
    use_nodata: u32,
};

@group(0) @binding(0) var<uniform> params: HillshadeParams;
@group(0) @binding(1) var<storage, read> dem: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

fn get_elev(col: i32, row: i32) -> f32 {
    let c = clamp(col, 0, i32(params.width) - 1);
    let r = clamp(row, 0, i32(params.height) - 1);
    return dem[u32(r) * params.width + u32(c)];
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let col = i32(gid.x);
    let row = i32(gid.y);
    if gid.x >= params.width || gid.y >= params.height { return; }

    let idx = gid.y * params.width + gid.x;
    let center = dem[idx];

    if params.use_nodata != 0u && abs(center - params.nodata) < 0.001 {
        output[idx] = params.nodata;
        return;
    }

    // 3x3 neighborhood for Zevenbergen & Thorne (1987)
    let a = get_elev(col-1, row-1); let b = get_elev(col, row-1); let c = get_elev(col+1, row-1);
    let d = get_elev(col-1, row  );                                let f = get_elev(col+1, row  );
    let g = get_elev(col-1, row+1); let h = get_elev(col, row+1); let i = get_elev(col+1, row+1);

    let dzdx = ((c + 2.0*f + i) - (a + 2.0*d + g)) / (8.0 * params.cell_size_x) * params.z_factor;
    let dzdy = ((g + 2.0*h + i) - (a + 2.0*b + c)) / (8.0 * params.cell_size_y) * params.z_factor;

    let slope = atan(sqrt(dzdx*dzdx + dzdy*dzdy));
    let aspect = atan2(dzdy, -dzdx);

    let cos_z = cos(1.5707963 - params.altitude_rad);
    let sin_z = sin(1.5707963 - params.altitude_rad);

    let hs = 255.0 * (cos_z * cos(slope) + sin_z * sin(slope) * cos(params.azimuth_rad - aspect));
    output[idx] = clamp(hs, 0.0, 255.0);
}
