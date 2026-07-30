//! cool-japan/oxigeo#14 — reading a GeoTIFF DEM into an `Array2<f64>`, the
//! idiomatic (and fast) way.
//!
//! The reporter of the issue was porting a GDAL project whose hot path is
//! `RasterBand::read_into_slice(...)`: read **and** convert straight into a
//! buffer the caller already owns. Their first OxiGeo attempt had to go the long
//! way round —
//!
//! ```ignore
//! let band = dataset.bands().next().ok_or_else(|| anyhow!("no bands"))??;
//! let bytes_slice = band.as_bytes();                          // 1. whole band, as bytes
//! let f32_slice: &[f32] = bytemuck::cast_slice(bytes_slice);  // 2. reinterpret
//! let dem_grid = ndarray::ArrayView2::from_shape((height, width), f32_slice)?
//!     .mapv(|v| v as f64);                                    // 3. second full-size buffer
//! ```
//!
//! — which materialises the band twice (once as `f32` bytes, once as the `f64`
//! array) and walks every pixel twice (decode, then `mapv`).
//!
//! `Dataset::read_band_into` collapses all of that into one pass into one
//! buffer: the driver decodes each tile/strip and converts `Float32 → f64`
//! straight into the destination you hand it. The only large allocation is the
//! `Array2` you were going to allocate anyway, and peak extra memory is a single
//! tile regardless of raster size.
//!
//! Run it with:
//!
//! ```text
//! cargo run -p oxigeo-examples --example issue_14_dem_to_array2
//! ```

use std::error::Error;
use std::path::{Path, PathBuf};

use oxigeo::geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
use oxigeo::geotiff::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigeo::{Dataset, GeoTransform, RasterDataType};

// House policy forbids depending on `ndarray` directly; the SciRS2 core
// re-export is the supported path to the very same types.
use scirs2_core::ndarray::{Array1, Array2};

/// Errors bubble up as boxed trait objects so the example stays free of an
/// error-handling dependency (the issue's original code used `anyhow`).
type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Read a DEM into `(grid, lon, lat)`.
///
/// * `grid` — `height × width` elevations as `f64`, row-major, row 0 at the top.
/// * `lon`  — `width` cell-centre X coordinates.
/// * `lat`  — `height` cell-centre Y coordinates.
///
/// This is the shape of the function in the issue, implemented the new way.
fn return_dem_grid(path: &Path) -> Result<(Array2<f64>, Array1<f64>, Array1<f64>)> {
    let path_str = path.to_str().ok_or("DEM path is not valid UTF-8")?;
    let dataset = Dataset::open(path_str)?;

    let geo_transform = *dataset
        .geotransform()
        .ok_or("Dataset missing geotransform")?;
    let width = dataset.width() as usize;
    let height = dataset.height() as usize;

    // The on-disk element type is known from the header, before a single pixel
    // is read — so the destination can be sized and typed up front.
    println!(
        "  on-disk element type: {:?} ({}×{} px, {} band(s))",
        dataset.data_type(),
        width,
        height,
        dataset.band_count()
    );

    // ── The fast path ────────────────────────────────────────────────────────
    // Allocate the array once, then decode + convert directly into its backing
    // slice. No `as_bytes()`, no `cast_slice`, no `mapv`, no second buffer.
    let mut grid = Array2::<f64>::zeros((height, width));
    {
        let destination = grid
            .as_slice_mut()
            .ok_or("array is not in standard (row-major, contiguous) layout")?;
        dataset.read_band_into(0, destination)?;
    }
    // ─────────────────────────────────────────────────────────────────────────

    // Coordinate axes from the geo-transform, at cell centres.
    let lon = Array1::from_iter(
        (0..width).map(|col| geo_transform.pixel_to_world(col as f64 + 0.5, 0.0).0),
    );
    let lat = Array1::from_iter(
        (0..height).map(|row| geo_transform.pixel_to_world(0.0, row as f64 + 0.5).1),
    );

    Ok((grid, lon, lat))
}

/// Elevation of the synthetic DEM at `(col, row)`, in metres.
fn synthetic_elevation(col: usize, row: usize) -> f32 {
    let x = col as f32 / 16.0;
    let y = row as f32 / 16.0;
    1200.0 + 180.0 * (x.sin() + y.cos()) - 0.75 * y * y
}

/// Write a small `Float32` GeoTIFF DEM into the system temp directory so the
/// example is runnable with no data files checked in.
fn write_sample_dem(width: usize, height: usize) -> Result<PathBuf> {
    let path = std::env::temp_dir().join("oxigeo_issue_14_sample_dem.tif");

    let mut pixels = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for col in 0..width {
            pixels.extend_from_slice(&synthetic_elevation(col, row).to_ne_bytes());
        }
    }

    // EPSG:4326-ish framing: 0.001° cells with the origin near Mt. Fuji.
    let geo_transform = GeoTransform::north_up(138.6, 35.5, 0.001, -0.001);
    let mut config = WriterConfig::new(width as u64, height as u64, 1, RasterDataType::Float32)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_photometric(PhotometricInterpretation::BlackIsZero)
        .with_geo_transform(geo_transform);
    config.tile_width = Some(64);
    config.tile_height = Some(64);
    config.generate_overviews = false;

    let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
    writer.write(&pixels)?;
    Ok(path)
}

fn main() -> Result<()> {
    let (width, height) = (512, 384);

    println!("OxiGeo — issue #14: GeoTIFF DEM → Array2<f64>");
    let path = write_sample_dem(width, height)?;
    println!("Sample DEM written to {}", path.display());

    let started = std::time::Instant::now();
    let (grid, lon, lat) = return_dem_grid(&path)?;
    let elapsed = started.elapsed();

    println!("  grid shape:  {:?}", grid.shape());
    println!("  lon axis:    {} values, first {:.4}", lon.len(), lon[0]);
    println!("  lat axis:    {} values, first {:.4}", lat.len(), lat[0]);
    println!("  read in:     {elapsed:?}");

    // Sanity: the array really holds the DEM, exactly (f32 → f64 is lossless).
    let corner = grid[[0, 0]];
    let expected = f64::from(synthetic_elevation(0, 0));
    println!("  grid[0,0]:   {corner:.4} m (expected {expected:.4} m)");
    if (corner - expected).abs() > 1e-9 {
        return Err("decoded DEM does not match the synthetic source".into());
    }

    let min = grid.iter().copied().fold(f64::INFINITY, f64::min);
    let max = grid.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    println!("  elevation:   {min:.2} m … {max:.2} m");

    // Windowed reads use the same "you own the buffer" contract, so a tile walk
    // costs one allocation in total rather than one per tile.
    let dataset = Dataset::open(path.to_str().ok_or("DEM path is not valid UTF-8")?)?;
    let mut tile = vec![0.0f64; 64 * 64];
    let mut tiles_read = 0usize;
    for tile_row in 0..height / 64 {
        for tile_col in 0..width / 64 {
            dataset.read_window_into(
                0,
                (tile_col * 64) as u32,
                (tile_row * 64) as u32,
                64,
                64,
                &mut tile,
            )?;
            tiles_read += 1;
        }
    }
    println!("  tile walk:   {tiles_read} × 64×64 windows into one reused buffer");

    std::fs::remove_file(&path)?;
    Ok(())
}
