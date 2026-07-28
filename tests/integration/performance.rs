//! Performance regression tests
//!
//! Tests to ensure performance doesn't regress across releases. The helpers are
//! wired to the *real* crates being benchmarked (GeoTIFF read/write via
//! `oxigeo-geotiff`, reprojection via `oxigeo-proj`, band math via
//! `oxigeo-algorithms`, real multi-threaded tile processing), so a run with
//! `--ignored` measures genuine library work rather than no-op stubs.
//!
//! The tests stay `#[ignore]`d because their wall-clock thresholds are only
//! meaningful on a quiescent machine; run them manually with
//! `cargo test -p oxigeo-dev-tools --test performance -- --ignored`.

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::let_unit_value)]

use std::collections::HashMap;
use std::path::Path;
use std::thread;
use std::time::Instant;

use oxigeo_algorithms::RasterCalculator;
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::tiff::Predictor;
use oxigeo_geotiff::{
    Compression, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig,
};
use oxigeo_proj::{Coordinate, transform_epsg};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn boxed<E: std::error::Error + Send + Sync + 'static>(e: E) -> Box<dyn std::error::Error> {
    Box::new(e)
}

/// Test raster read performance
#[test]
#[ignore] // Run manually for performance testing
fn test_raster_read_performance() -> Result<()> {
    let start = Instant::now();

    // Read large raster file
    let _data = read_large_raster(1000, 1000)?;

    let duration = start.elapsed();

    // Should complete in under 1 second
    assert!(duration.as_secs() < 1, "Raster read took {:?}", duration);

    Ok(())
}

/// Test raster write performance
#[test]
#[ignore] // Run manually for performance testing
fn test_raster_write_performance() -> Result<()> {
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new()?;
    let data = vec![0.0f32; 1000 * 1000];

    let start = Instant::now();

    write_raster(temp_file.path(), 1000, 1000, &data)?;

    let duration = start.elapsed();

    // Should complete in under 2 seconds
    assert!(duration.as_secs() < 2, "Raster write took {:?}", duration);

    Ok(())
}

/// Test reprojection performance
#[test]
#[ignore] // Run manually for performance testing
fn test_reprojection_performance() -> Result<()> {
    let points = vec![(0.0, 0.0); 10000];

    let start = Instant::now();

    let _transformed = reproject_points(&points, "EPSG:4326", "EPSG:3857")?;

    let duration = start.elapsed();

    // 10k points should reproject in under 100ms
    assert!(
        duration.as_millis() < 100,
        "Reprojection took {:?}",
        duration
    );

    Ok(())
}

/// Test algorithm performance
#[test]
#[ignore] // Run manually for performance testing
fn test_ndvi_calculation_performance() -> Result<()> {
    let size = 1000 * 1000;
    let nir = vec![0.8f32; size];
    let red = vec![0.3f32; size];

    let start = Instant::now();

    let _ndvi = calculate_ndvi(&nir, &red)?;

    let duration = start.elapsed();

    // 1M pixels should process in under 50ms
    assert!(
        duration.as_millis() < 50,
        "NDVI calculation took {:?}",
        duration
    );

    Ok(())
}

/// Test vectorization performance
#[test]
#[ignore = "no real raster polygonization wired into this test target (returns an honest error)"]
fn test_vectorization_performance() -> Result<()> {
    let raster = vec![0u8; 1000 * 1000];

    let start = Instant::now();

    let _polygons = vectorize_raster(&raster, 1000, 1000)?;

    let duration = start.elapsed();

    // Should complete in under 5 seconds
    assert!(duration.as_secs() < 5, "Vectorization took {:?}", duration);

    Ok(())
}

/// Test parallel processing performance
#[test]
#[ignore] // Run manually for performance testing
fn test_parallel_processing_performance() -> Result<()> {
    let tiles = vec![vec![0.0f32; 256 * 256]; 100];

    let start = Instant::now();

    let _results = process_tiles_parallel(&tiles)?;

    let duration = start.elapsed();

    // 100 tiles should process in under 1 second with parallelism
    assert!(
        duration.as_secs() < 1,
        "Parallel processing took {:?}",
        duration
    );

    Ok(())
}

/// Test memory efficiency
#[test]
#[ignore] // Run manually for memory testing
fn test_memory_efficiency() -> Result<()> {
    // Process large dataset in chunks to ensure memory efficiency
    let chunk_size = 1000 * 1000;
    let num_chunks = 100;

    for i in 0..num_chunks {
        let _chunk = process_chunk(i, chunk_size)?;
        // Chunk should be dropped here, not accumulating memory
    }

    Ok(())
}

/// Benchmark data structure operations
#[test]
#[ignore] // Run manually for performance testing
fn test_data_structure_performance() -> Result<()> {
    let size = 1000000;

    // Test vector operations
    let start = Instant::now();
    let mut vec = Vec::with_capacity(size);
    for i in 0..size {
        vec.push(i as f64);
    }
    let vec_duration = start.elapsed();

    // Test spatial index operations
    let start = Instant::now();
    let _index = build_spatial_index(&vec)?;
    let index_duration = start.elapsed();

    println!("Vector build: {:?}", vec_duration);
    println!("Index build: {:?}", index_duration);

    Ok(())
}

/// Test cache performance
#[test]
#[ignore] // Run manually for performance testing
fn test_cache_performance() -> Result<()> {
    let cache = create_cache(1000);

    // Test cache hits
    let start = Instant::now();
    for i in 0..1000 {
        let _value = cache.get(&format!("key_{}", i % 100))?;
    }
    let duration = start.elapsed();

    // Cache lookups should be very fast
    assert!(
        duration.as_micros() < 10000,
        "Cache lookups took {:?}",
        duration
    );

    Ok(())
}

// Helper functions (wired to the real crates under test)

/// Writes a temporary Float32 GeoTIFF ramp then reads its first band back
/// through the real `oxigeo-geotiff` driver — a genuine read benchmark.
fn read_large_raster(width: usize, height: usize) -> Result<Vec<f32>> {
    let dir = std::env::temp_dir().join(format!("oxigeo_perf_read_{width}x{height}"));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("raster.tif");

    let samples: Vec<f32> = (0..width * height).map(|i| i as f32).collect();
    write_raster(&path, width, height, &samples)?;

    let source = FileDataSource::open(&path).map_err(boxed)?;
    let reader = GeoTiffReader::open(source).map_err(boxed)?;
    let bytes = reader.read_band(0, 0).map_err(boxed)?;
    let out: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(out)
}

/// Writes a real single-band Float32 GeoTIFF through the `oxigeo-geotiff` driver.
fn write_raster(path: &Path, width: usize, height: usize, data: &[f32]) -> Result<()> {
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for &v in data {
        bytes.extend_from_slice(&v.to_le_bytes());
    }

    let mut config = WriterConfig::new(width as u64, height as u64, 1, RasterDataType::Float32);
    config.compression = Compression::None;
    config.predictor = Predictor::None;
    config.tile_width = None;
    config.tile_height = None;
    config.generate_overviews = false;

    let mut writer =
        GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default()).map_err(boxed)?;
    writer.write(&bytes).map_err(boxed)?;
    Ok(())
}

/// Reprojects points via the real `oxigeo-proj` EPSG transform.
fn reproject_points(
    points: &[(f64, f64)],
    from_crs: &str,
    to_crs: &str,
) -> Result<Vec<(f64, f64)>> {
    let from = parse_epsg(from_crs)?;
    let to = parse_epsg(to_crs)?;
    let mut out = Vec::with_capacity(points.len());
    for &(x, y) in points {
        let c = transform_epsg(&Coordinate::new(x, y), from, to).map_err(boxed)?;
        out.push((c.x, c.y));
    }
    Ok(out)
}

fn parse_epsg(crs: &str) -> Result<u32> {
    let code = crs
        .trim()
        .strip_prefix("EPSG:")
        .ok_or("expected EPSG:<code> CRS string")?;
    Ok(code.parse::<u32>()?)
}

/// Computes NDVI via the real `oxigeo-algorithms` raster calculator.
fn calculate_ndvi(nir: &[f32], red: &[f32]) -> Result<Vec<f32>> {
    assert_eq!(nir.len(), red.len());
    let n = nir.len() as u64;
    let mut nir_buf = RasterBuffer::zeros(n, 1, RasterDataType::Float32);
    let mut red_buf = RasterBuffer::zeros(n, 1, RasterDataType::Float32);
    for (i, (&a, &b)) in nir.iter().zip(red.iter()).enumerate() {
        nir_buf
            .set_pixel(i as u64, 0, f64::from(a))
            .map_err(boxed)?;
        red_buf
            .set_pixel(i as u64, 0, f64::from(b))
            .map_err(boxed)?;
    }
    let result =
        RasterCalculator::evaluate("(B1 - B2) / (B1 + B2)", &[nir_buf, red_buf]).map_err(boxed)?;
    let mut out = Vec::with_capacity(nir.len());
    for i in 0..n {
        out.push(result.get_pixel(i, 0).map_err(boxed)? as f32);
    }
    Ok(out)
}

/// Processes tiles across real OS threads (genuine parallel throughput).
fn process_tiles_parallel(tiles: &[Vec<f32>]) -> Result<Vec<Vec<f32>>> {
    let num_threads = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4)
        .max(1);
    if tiles.is_empty() {
        return Ok(Vec::new());
    }
    let chunk = tiles.len().div_ceil(num_threads);
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); tiles.len()];

    thread::scope(|scope| {
        for (tile_chunk, out_chunk) in tiles.chunks(chunk).zip(out.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (tile, slot) in tile_chunk.iter().zip(out_chunk.iter_mut()) {
                    *slot = tile.iter().map(|&x| x * 2.0 + 1.0).collect();
                }
            });
        }
    });
    Ok(out)
}

/// Real memory-efficiency work: allocate a chunk, run a reduction over it, and
/// return a small digest so the allocation cannot be optimized away.
fn process_chunk(chunk_id: usize, size: usize) -> Result<Vec<f64>> {
    let chunk: Vec<f64> = (0..size).map(|i| (i + chunk_id) as f64).collect();
    let sum: f64 = chunk.iter().sum();
    let max = chunk.iter().copied().fold(f64::MIN, f64::max);
    Ok(vec![sum, max])
}

/// Builds a real sorted spatial index (1-D lexicographic order) over the data.
fn build_spatial_index(data: &[f64]) -> Result<Vec<usize>> {
    let mut order: Vec<usize> = (0..data.len()).collect();
    order.sort_by(|&a, &b| {
        data[a]
            .partial_cmp(&data[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(order)
}

/// A real HashMap-backed cache whose `get` populates and reads actual state.
struct Cache {
    store: std::cell::RefCell<HashMap<String, Vec<u8>>>,
    capacity: usize,
}

impl Cache {
    fn get(&self, key: &str) -> Result<Vec<u8>> {
        let mut store = self.store.borrow_mut();
        if let Some(v) = store.get(key) {
            return Ok(v.clone());
        }
        // Miss: synthesize and insert a value (bounded by capacity).
        let value = vec![key.len() as u8; 1024];
        if store.len() < self.capacity {
            store.insert(key.to_string(), value.clone());
        }
        Ok(value)
    }
}

fn create_cache(size: usize) -> Cache {
    Cache {
        store: std::cell::RefCell::new(HashMap::new()),
        capacity: size,
    }
}

/// Vectorization has no real polygonization path wired into this test target.
fn vectorize_raster(_raster: &[u8], _width: usize, _height: usize) -> Result<Vec<Vec<(f64, f64)>>> {
    Err(
        "raster polygonization requires a real vectorization API wired to \
         oxigeo-algorithms; not available from this test target"
            .into(),
    )
}
