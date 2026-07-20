//! GeoVault sample scene — "Site K-7" synthetic DEM generator.
//!
//! SYNTHETIC DATA — DEMONSTRATION. The terrain, its nominal location, and
//! every planted feature below are procedurally generated. Nothing in this
//! scene corresponds to a real site.
//!
//! Produces `demo/geovault/data/site-k7.tif`: 512x512, single-band Float32,
//! tiled 256x256, DEFLATE-compressed, EPSG:4326, x2/x4 overviews, GDAL
//! NoData tag -9999 (no NoData pixels are planted; the tag exercises the
//! metadata path).
//!
//! Determinism: all randomness comes from a seeded 64-bit LCG whose state is
//! passed through a PCG-style xorshift-multiply output permutation (a raw
//! LCG is affine in its seed, so neighbouring lattice seeds would produce
//! linearly correlated values). Pure integer arithmetic — identical bytes on
//! every run and platform, no `rand` dependency.
//!
//! Terrain: five octaves of ridged value noise (smoothstep-interpolated
//! lattice noise, ridge = `1 - |2n - 1|`, squared for sharp crests) over a
//! gentle regional tilt. Relief is deliberately subdued (sigma ~7 m) so the
//! planted features are unambiguous outliers for GeoVault's anomaly
//! workbench.
//!
//! Planted anomalies (all detected by z-score >= 3.0, IQR >= 1.5 and
//! modified z-score >= 3.5 — the workbench's default thresholds):
//! - 25x25 px excavation pit, floor exactly 80 m below the local terrain,
//!   pixel rows/cols 144..=168 x 198..=222 (center 156, 210)
//! - 3 px wide linear trench, 48 m deep, from pixel (300, 96) to (420, 216)
//! - 7-pixel spike cluster around (400, 400), +76..+92 m
//!
//! Usage (run from the repository root so the relative output path
//! resolves, like `render_hero.rs`):
//!
//! ```bash
//! cargo run -p oxigdal-geotiff --example geovault_scene              # generate + verify
//! cargo run -p oxigdal-geotiff --example geovault_scene -- --verify  # verify existing file
//! ```
//!
//! Verification re-opens the file with [`GeoTiffReader`], asserts the
//! metadata (dimensions, band count, data type, tiling, compression, CRS,
//! geotransform, NoData, overviews), compares every pixel bit-for-bit
//! against a regenerated scene, and re-runs the three anomaly detectors to
//! prove the planted features are flagged at default thresholds.

use std::error::Error;
use std::fs;
use std::path::Path;

use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_geotiff::tiff::{Compression, Predictor};
use oxigdal_geotiff::writer::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
};

// ---------------------------------------------------------------------------
// Scene constants (the values below ARE the scene definition; change any of
// them and the verification step will fail against previously written files)
// ---------------------------------------------------------------------------

/// Image width in pixels.
const WIDTH: usize = 512;
/// Image height in pixels.
const HEIGHT: usize = 512;
/// Master seed for the whole scene ("K7" scene, revision 1).
const SEED: u64 = 0x4B37_0001;

/// Base elevation of the plateau, meters.
const BASE_ELEVATION_M: f64 = 640.0;
/// Peak-to-trough amplitude of the ridged noise, meters.
const RELIEF_AMPLITUDE_M: f64 = 32.0;
/// Regional tilt across the full width (west-to-east), meters.
const TILT_EAST_M: f64 = 3.0;
/// Regional tilt across the full height (north-to-south), meters.
const TILT_SOUTH_M: f64 = -2.0;

/// Ridged-noise octaves: (lattice cell size in pixels, weight).
const OCTAVES: [(f64, f64); 5] = [
    (160.0, 1.0),
    (80.0, 0.5),
    (40.0, 0.25),
    (20.0, 0.125),
    (10.0, 0.0625),
];

/// Excavation pit: top-left pixel, edge length, and depth below terrain.
const PIT_X0: usize = 144;
/// Pit top row.
const PIT_Y0: usize = 198;
/// Pit edge length in pixels (25x25 per the GeoVault design contract).
const PIT_SIZE: usize = 25;
/// Pit depth below the local terrain surface, meters.
const PIT_DEPTH_M: f32 = 80.0;

/// Trench segment start, pixel coordinates.
const TRENCH_START: (f64, f64) = (300.0, 96.0);
/// Trench segment end, pixel coordinates.
const TRENCH_END: (f64, f64) = (420.0, 216.0);
/// Half-width of the trench in pixels (1.5 -> 3 px wide).
const TRENCH_HALF_WIDTH_PX: f64 = 1.5;
/// Trench depth below the local terrain surface, meters.
const TRENCH_DEPTH_M: f32 = 48.0;

/// Spike cluster: (x, y, height above terrain in meters).
const SPIKES: [(usize, usize, f32); 7] = [
    (400, 400, 92.0),
    (401, 400, 84.0),
    (400, 401, 88.0),
    (399, 400, 79.0),
    (400, 399, 86.0),
    (401, 401, 76.0),
    (399, 399, 90.0),
];

/// Fictional georeference: top-left corner (lon, lat) and pixel size in
/// degrees (~30 m nominal). Kazakh-steppe-like coordinates, entirely made up.
const ORIGIN_LON: f64 = 63.4200;
/// Top-left latitude (north-up image).
const ORIGIN_LAT: f64 = 46.6000;
/// Pixel size in degrees.
const PIXEL_SIZE_DEG: f64 = 0.000_27;
/// NoData sentinel written to the GDAL NoData tag.
const NODATA: f64 = -9999.0;

/// Output location, relative to the repository root.
const OUTPUT_DIR: &str = "demo/geovault/data";
/// Output GeoTIFF path, relative to the repository root.
const OUTPUT_PATH: &str = "demo/geovault/data/site-k7.tif";

/// Default detector thresholds (must match the GeoVault workbench defaults).
const ZSCORE_THRESHOLD: f64 = 3.0;
/// IQR-score default threshold.
const IQR_THRESHOLD: f64 = 1.5;
/// Modified z-score default threshold.
const MODZ_THRESHOLD: f64 = 3.5;

// ---------------------------------------------------------------------------
// Deterministic pseudo-randomness: seeded LCG + PCG-style output permutation
// ---------------------------------------------------------------------------

/// Knuth MMIX LCG multiplier.
const LCG_MUL: u64 = 6_364_136_223_846_793_005;
/// Knuth MMIX LCG increment.
const LCG_INC: u64 = 1_442_695_040_888_963_407;

/// One LCG state transition.
fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_MUL).wrapping_add(LCG_INC)
}

/// PCG-style output permutation (SplitMix64 finalizer). Breaks the affine
/// relationship between LCG states derived from nearby seeds.
fn permute(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministic lattice value in [0, 1) for one noise octave.
fn lattice_value(seed: u64, octave: u64, ix: i64, iy: i64) -> f64 {
    let mixed = seed
        .wrapping_add((ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add((iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
        .wrapping_add(octave.wrapping_mul(0x1656_67B1_9E37_79F9));
    let bits = permute(lcg_step(mixed));
    // 53 high bits -> f64 in [0, 1).
    (bits >> 11) as f64 / (1u64 << 53) as f64
}

/// Smoothstep-interpolated value noise, sampled at lattice coordinates.
fn value_noise(seed: u64, octave: u64, x: f64, y: f64) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let u = fx * fx * (3.0 - 2.0 * fx);
    let v = fy * fy * (3.0 - 2.0 * fy);
    let ix = x0 as i64;
    let iy = y0 as i64;
    let v00 = lattice_value(seed, octave, ix, iy);
    let v10 = lattice_value(seed, octave, ix + 1, iy);
    let v01 = lattice_value(seed, octave, ix, iy + 1);
    let v11 = lattice_value(seed, octave, ix + 1, iy + 1);
    let top = v00 + (v10 - v00) * u;
    let bottom = v01 + (v11 - v01) * u;
    top + (bottom - top) * v
}

/// Multi-octave ridged fractal in roughly [0, 1].
fn ridged_fractal(seed: u64, px: f64, py: f64) -> f64 {
    let mut sum = 0.0;
    let mut weight_total = 0.0;
    for (octave, (cell, weight)) in OCTAVES.iter().enumerate() {
        let n = value_noise(seed, octave as u64, px / cell, py / cell);
        let ridge = 1.0 - (2.0 * n - 1.0).abs();
        sum += weight * ridge * ridge;
        weight_total += weight;
    }
    sum / weight_total
}

/// Undisturbed terrain elevation at a pixel, meters (before anomalies).
fn base_elevation(x: usize, y: usize) -> f32 {
    let px = x as f64;
    let py = y as f64;
    let relief = ridged_fractal(SEED, px, py) * RELIEF_AMPLITUDE_M;
    let tilt = TILT_EAST_M * px / (WIDTH - 1) as f64 + TILT_SOUTH_M * py / (HEIGHT - 1) as f64;
    (BASE_ELEVATION_M + relief + tilt) as f32
}

// ---------------------------------------------------------------------------
// Scene assembly
// ---------------------------------------------------------------------------

/// Pixel indices of each planted feature, for targeted verification.
struct PlantedRegions {
    /// Row-major indices covered by the excavation pit.
    pit: Vec<usize>,
    /// Row-major indices covered by the trench.
    trench: Vec<usize>,
    /// Row-major indices of the spike cluster.
    spikes: Vec<usize>,
}

/// Distance from a point to a line segment, in pixel units.
fn segment_distance(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt()
}

/// Builds the full Site K-7 scene: ridged terrain plus planted anomalies.
fn build_scene() -> (Vec<f32>, PlantedRegions) {
    let mut values = vec![0.0f32; WIDTH * HEIGHT];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            values[y * WIDTH + x] = base_elevation(x, y);
        }
    }

    // Excavation pit: sheer-walled 25x25 square, floor 80 m below terrain.
    let mut pit = Vec::with_capacity(PIT_SIZE * PIT_SIZE);
    for y in PIT_Y0..PIT_Y0 + PIT_SIZE {
        for x in PIT_X0..PIT_X0 + PIT_SIZE {
            let idx = y * WIDTH + x;
            values[idx] = base_elevation(x, y) - PIT_DEPTH_M;
            pit.push(idx);
        }
    }

    // Linear trench: 3 px wide band along a segment, 48 m deep.
    let (ax, ay) = TRENCH_START;
    let (bx, by) = TRENCH_END;
    let x_min = (ax.min(bx) - TRENCH_HALF_WIDTH_PX).floor().max(0.0) as usize;
    let x_max = ((ax.max(bx) + TRENCH_HALF_WIDTH_PX).ceil() as usize).min(WIDTH - 1);
    let y_min = (ay.min(by) - TRENCH_HALF_WIDTH_PX).floor().max(0.0) as usize;
    let y_max = ((ay.max(by) + TRENCH_HALF_WIDTH_PX).ceil() as usize).min(HEIGHT - 1);
    let mut trench = Vec::new();
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let dist = segment_distance(x as f64 + 0.5, y as f64 + 0.5, ax, ay, bx, by);
            if dist <= TRENCH_HALF_WIDTH_PX {
                let idx = y * WIDTH + x;
                values[idx] = base_elevation(x, y) - TRENCH_DEPTH_M;
                trench.push(idx);
            }
        }
    }

    // Spike cluster: isolated positive outliers.
    let mut spikes = Vec::with_capacity(SPIKES.len());
    for &(x, y, height) in &SPIKES {
        let idx = y * WIDTH + x;
        values[idx] = base_elevation(x, y) + height;
        spikes.push(idx);
    }

    (
        values,
        PlantedRegions {
            pit,
            trench,
            spikes,
        },
    )
}

// ---------------------------------------------------------------------------
// Anomaly detectors (faithful ports of oxigdal-analytics timeseries/anomaly.rs
// scoring: population-sigma z-score, |x - median| / IQR, and the MAD-based
// modified z-score with the 1.4826 / 0.6745 normal-consistency constants)
// ---------------------------------------------------------------------------

/// Linear-interpolation percentile of pre-sorted data (closest-ranks method).
fn percentile(sorted: &[f64], pct: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let rank = (pct / 100.0) * (n - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let fraction = rank - lower as f64;
    sorted[lower] + fraction * (sorted[upper] - sorted[lower])
}

/// Per-pixel |z-score| >= threshold flags (population standard deviation).
fn zscore_flags(values: &[f64], threshold: f64) -> Result<Vec<bool>, Box<dyn Error>> {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n;
    let std = variance.sqrt();
    if std < f64::EPSILON {
        return Err("z-score: standard deviation is degenerate".into());
    }
    Ok(values
        .iter()
        .map(|v| ((v - mean) / std).abs() >= threshold)
        .collect())
}

/// Per-pixel IQR-score (|x - median| / IQR) >= threshold flags.
fn iqr_flags(values: &[f64], threshold: f64) -> Result<Vec<bool>, Box<dyn Error>> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let q1 = percentile(&sorted, 25.0);
    let q3 = percentile(&sorted, 75.0);
    let iqr = q3 - q1;
    if iqr < f64::EPSILON {
        return Err("IQR score: interquartile range is degenerate".into());
    }
    let median = percentile(&sorted, 50.0);
    Ok(values
        .iter()
        .map(|v| ((v - median).abs() / iqr) >= threshold)
        .collect())
}

/// Per-pixel |modified z-score| >= threshold flags (MAD-based).
fn modified_zscore_flags(values: &[f64], threshold: f64) -> Result<Vec<bool>, Box<dyn Error>> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = percentile(&sorted, 50.0);
    let mut abs_dev: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
    abs_dev.sort_by(f64::total_cmp);
    let mad = percentile(&abs_dev, 50.0);
    let normalized_mad = 1.4826 * mad;
    if normalized_mad < f64::EPSILON {
        return Err("modified z-score: MAD is degenerate".into());
    }
    Ok(values
        .iter()
        .map(|v| (0.6745 * (v - median) / normalized_mad).abs() >= threshold)
        .collect())
}

/// Fraction of the given region indices that are flagged.
fn coverage(flags: &[bool], region: &[usize]) -> f64 {
    if region.is_empty() {
        return 0.0;
    }
    let hits = region.iter().filter(|&&idx| flags[idx]).count();
    hits as f64 / region.len() as f64
}

// ---------------------------------------------------------------------------
// Write + verify
// ---------------------------------------------------------------------------

/// Writes the scene to `OUTPUT_PATH` as a tiled DEFLATE Float32 GeoTIFF.
fn write_scene(values: &[f32]) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(OUTPUT_DIR)?;

    let geo_transform = GeoTransform {
        origin_x: ORIGIN_LON,
        origin_y: ORIGIN_LAT,
        pixel_width: PIXEL_SIZE_DEG,
        pixel_height: -PIXEL_SIZE_DEG,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let config = WriterConfig::new(WIDTH as u64, HEIGHT as u64, 1, RasterDataType::Float32)
        .with_tile_size(256, 256)
        .with_compression(Compression::Deflate)
        // Predictor 2 is integer-only in TIFF and predictor 3 is not fully
        // implemented; raw DEFLATE keeps every consumer compatible.
        .with_predictor(Predictor::None)
        .with_geo_transform(geo_transform)
        .with_epsg_code(4326)
        .with_nodata(NoDataValue::Float(NODATA))
        .with_overviews(true, OverviewResampling::Average)
        .with_overview_levels(vec![2, 4]);

    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut writer = GeoTiffWriter::create(OUTPUT_PATH, config, GeoTiffWriterOptions::default())?;
    writer.write(&bytes)?;
    Ok(())
}

/// Fails with a labeled error unless `condition` holds.
fn check(condition: bool, label: &str) -> Result<(), Box<dyn Error>> {
    if condition {
        println!("  ok  {label}");
        Ok(())
    } else {
        Err(format!("verification failed: {label}").into())
    }
}

/// Re-opens the written file and verifies metadata, pixels, and detectability.
fn verify(path: &str) -> Result<(), Box<dyn Error>> {
    println!("Verifying {path} ...");
    let reader = GeoTiffReader::open(FileDataSource::open(path)?)?;

    check(reader.width() == WIDTH as u64, "width == 512")?;
    check(reader.height() == HEIGHT as u64, "height == 512")?;
    check(reader.band_count() == 1, "band count == 1")?;
    check(
        reader.data_type() == Some(RasterDataType::Float32),
        "data type == Float32",
    )?;
    check(
        reader.compression() == Compression::Deflate,
        "compression == DEFLATE",
    )?;
    check(reader.tile_size() == Some((256, 256)), "tiles == 256x256")?;
    check(reader.epsg_code() == Some(4326), "EPSG == 4326")?;
    check(reader.overview_count() == 2, "overview count == 2")?;
    check(
        reader.nodata().as_f64() == Some(NODATA),
        "NoData tag == -9999",
    )?;

    let gt = reader
        .geo_transform()
        .ok_or("verification failed: missing geotransform")?;
    check(
        (gt.origin_x - ORIGIN_LON).abs() < 1e-9
            && (gt.origin_y - ORIGIN_LAT).abs() < 1e-9
            && (gt.pixel_width - PIXEL_SIZE_DEG).abs() < 1e-12
            && (gt.pixel_height + PIXEL_SIZE_DEG).abs() < 1e-12,
        "geotransform matches scene definition",
    )?;

    // Full bit-exact pixel round-trip against a regenerated scene.
    let raw = reader.read_band(0, 0)?;
    check(
        raw.len() == WIDTH * HEIGHT * 4,
        "band byte length == 512*512*4",
    )?;
    let file_values: Vec<f32> = raw
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let (expected, regions) = build_scene();
    let mismatches = file_values
        .iter()
        .zip(expected.iter())
        .filter(|(a, b)| a.to_bits() != b.to_bits())
        .count();
    check(
        mismatches == 0,
        "all 262,144 pixels bit-identical to the deterministic scene",
    )?;

    // Planted-feature semantics.
    let pit_cx = PIT_X0 + PIT_SIZE / 2;
    let pit_cy = PIT_Y0 + PIT_SIZE / 2;
    let pit_center = file_values[pit_cy * WIDTH + pit_cx];
    check(
        pit_center.to_bits() == (base_elevation(pit_cx, pit_cy) - PIT_DEPTH_M).to_bits(),
        "pit center is exactly 80 m below the local terrain",
    )?;
    check(
        regions.pit.len() == PIT_SIZE * PIT_SIZE,
        "pit covers exactly 25x25 pixels",
    )?;
    check(!regions.trench.is_empty(), "trench region is non-empty")?;
    check(
        regions.spikes.len() == SPIKES.len(),
        "7 spike pixels planted",
    )?;

    // Detectability at the workbench's default thresholds.
    let as_f64: Vec<f64> = file_values.iter().map(|&v| f64::from(v)).collect();
    let detectors: [(&str, Vec<bool>); 3] = [
        ("z-score >= 3.0", zscore_flags(&as_f64, ZSCORE_THRESHOLD)?),
        ("IQR >= 1.5", iqr_flags(&as_f64, IQR_THRESHOLD)?),
        (
            "modified z >= 3.5",
            modified_zscore_flags(&as_f64, MODZ_THRESHOLD)?,
        ),
    ];
    println!("  detector coverage (fraction of planted pixels flagged):");
    for (name, flags) in &detectors {
        let pit_cov = coverage(flags, &regions.pit);
        let trench_cov = coverage(flags, &regions.trench);
        let spike_cov = coverage(flags, &regions.spikes);
        println!(
            "    {name:<18} pit {:>5.1}%  trench {:>5.1}%  spikes {:>5.1}%",
            pit_cov * 100.0,
            trench_cov * 100.0,
            spike_cov * 100.0
        );
        check(
            pit_cov >= 0.95,
            &format!("{name}: >= 95% of pit pixels flagged"),
        )?;
        check(
            trench_cov >= 0.60,
            &format!("{name}: >= 60% of trench pixels flagged"),
        )?;
        check(
            (spike_cov - 1.0).abs() < f64::EPSILON,
            &format!("{name}: all spike pixels flagged"),
        )?;
    }

    Ok(())
}

/// Prints simple scene statistics for the generation log.
fn print_scene_stats(values: &[f32]) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    let mut sum = 0.0f64;
    for &v in values {
        min = min.min(v);
        max = max.max(v);
        sum += f64::from(v);
    }
    let mean = sum / values.len() as f64;
    let variance = values
        .iter()
        .map(|&v| (f64::from(v) - mean) * (f64::from(v) - mean))
        .sum::<f64>()
        / values.len() as f64;
    println!(
        "  elevation: min {min:.2} m, max {max:.2} m, mean {mean:.2} m, sigma {:.2} m",
        variance.sqrt()
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verify_only = match args.as_slice() {
        [] => false,
        [flag] if flag == "--verify" => true,
        _ => {
            return Err(
                format!("unknown arguments {args:?}; usage: geovault_scene [--verify]").into(),
            );
        }
    };

    println!("GeoVault sample scene — Site K-7 (SYNTHETIC DATA — DEMONSTRATION)");

    if verify_only {
        verify(OUTPUT_PATH)?;
        println!("Verification passed: {OUTPUT_PATH}");
        return Ok(());
    }

    println!("Generating deterministic 512x512 Float32 DEM (seed {SEED:#x}) ...");
    let (values, _regions) = build_scene();
    print_scene_stats(&values);

    println!("Writing {OUTPUT_PATH} (tiled 256x256, DEFLATE, EPSG:4326) ...");
    write_scene(&values)?;
    let file_len = fs::metadata(Path::new(OUTPUT_PATH))?.len();
    println!("  wrote {file_len} bytes");

    verify(OUTPUT_PATH)?;
    println!("Done: {OUTPUT_PATH} generated and verified.");
    Ok(())
}
