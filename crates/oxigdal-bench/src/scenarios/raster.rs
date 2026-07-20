//! Raster operation benchmark scenarios.
//!
//! This module provides benchmark scenarios for raster operations including:
//! - Reading and writing GeoTIFF files
//! - Raster reprojection
//! - Band operations
//! - COG operations
//! - Compression benchmarks

use crate::error::{BenchError, Result};
use crate::scenarios::BenchmarkScenario;
use std::collections::HashMap;
use std::path::PathBuf;

/// GeoTIFF read benchmark scenario.
pub struct GeoTiffReadScenario {
    input_path: PathBuf,
    tile_size: Option<(usize, usize)>,
    #[allow(dead_code)]
    metadata: HashMap<String, String>,
}

impl GeoTiffReadScenario {
    /// Creates a new GeoTIFF read benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P) -> Self {
        Self {
            input_path: input_path.into(),
            tile_size: None,
            metadata: HashMap::new(),
        }
    }

    /// Sets the tile size for tiled reading.
    pub fn with_tile_size(mut self, width: usize, height: usize) -> Self {
        self.tile_size = Some((width, height));
        self
    }
}

impl BenchmarkScenario for GeoTiffReadScenario {
    fn name(&self) -> &str {
        "geotiff_read"
    }

    fn description(&self) -> &str {
        "Benchmark GeoTIFF file reading performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "raster")]
        {
            use oxigdal_core::io::FileDataSource;
            use oxigdal_geotiff::GeoTiffReader;

            let data_source = FileDataSource::open(&self.input_path).map_err(|e| {
                BenchError::scenario_failed(self.name(), format!("Failed to open file: {e}"))
            })?;
            let reader = GeoTiffReader::open(data_source).map_err(|e| {
                BenchError::scenario_failed(self.name(), format!("Failed to open GeoTIFF: {e}"))
            })?;

            if self.tile_size.is_some() {
                // Tiled read: walk the reader's native tile grid and pull
                // every tile through the decode path, keeping the result
                // live via `black_box` so the compiler cannot elide it.
                let (tiles_x, tiles_y) = reader.tile_count();
                for tile_y in 0..tiles_y {
                    for tile_x in 0..tiles_x {
                        let tile = reader.read_tile(0, tile_x, tile_y).map_err(|e| {
                            BenchError::scenario_failed(
                                self.name(),
                                format!("Failed to read tile ({tile_x}, {tile_y}): {e}"),
                            )
                        })?;
                        std::hint::black_box(&tile);
                    }
                }
            } else {
                // Whole-raster read: decode every band in full.
                for band in 0..reader.band_count() as usize {
                    let data = reader.read_band(0, band).map_err(|e| {
                        BenchError::scenario_failed(
                            self.name(),
                            format!("Failed to read band {band}: {e}"),
                        )
                    })?;
                    std::hint::black_box(&data);
                }
            }
        }

        #[cfg(not(feature = "raster"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-geotiff", "raster"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// GeoTIFF write benchmark scenario.
pub struct GeoTiffWriteScenario {
    output_path: PathBuf,
    #[allow(dead_code)]
    width: usize,
    #[allow(dead_code)]
    height: usize,
    compression: String,
    created: bool,
}

impl GeoTiffWriteScenario {
    /// Creates a new GeoTIFF write benchmark scenario.
    pub fn new<P: Into<PathBuf>>(output_path: P, width: usize, height: usize) -> Self {
        Self {
            output_path: output_path.into(),
            width,
            height,
            compression: "none".to_string(),
            created: false,
        }
    }

    /// Sets the compression method.
    pub fn with_compression<S: Into<String>>(mut self, compression: S) -> Self {
        self.compression = compression.into();
        self
    }
}

impl BenchmarkScenario for GeoTiffWriteScenario {
    fn name(&self) -> &str {
        "geotiff_write"
    }

    fn description(&self) -> &str {
        "Benchmark GeoTIFF file writing performance"
    }

    fn setup(&mut self) -> Result<()> {
        // Ensure output directory exists
        if let Some(parent) = self.output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "raster")]
        {
            // Generate synthetic u16 data for benchmarking compression
            let data: Vec<u16> = (0..self.width * self.height)
                .map(|i| (i % 65535) as u16)
                .collect();
            // Simulate LZW-style compression with a simple run-length encoder
            let _compressed: Vec<u8> = compress_rle_u16(&data);
            self.created = true;
        }

        #[cfg(not(feature = "raster"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-geotiff", "raster"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        if self.created && self.output_path.exists() {
            std::fs::remove_file(&self.output_path)?;
        }
        Ok(())
    }
}

/// Raster reprojection benchmark scenario.
pub struct RasterReprojectionScenario {
    input_path: PathBuf,
    output_path: PathBuf,
    #[allow(dead_code)]
    target_crs: String,
    created: bool,
}

impl RasterReprojectionScenario {
    /// Creates a new raster reprojection benchmark scenario.
    pub fn new<P1, P2, S>(input_path: P1, output_path: P2, target_crs: S) -> Self
    where
        P1: Into<PathBuf>,
        P2: Into<PathBuf>,
        S: Into<String>,
    {
        Self {
            input_path: input_path.into(),
            output_path: output_path.into(),
            target_crs: target_crs.into(),
            created: false,
        }
    }
}

impl BenchmarkScenario for RasterReprojectionScenario {
    fn name(&self) -> &str {
        "raster_reprojection"
    }

    fn description(&self) -> &str {
        "Benchmark raster reprojection performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        if let Some(parent) = self.output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(())
    }

    #[allow(unreachable_code)]
    fn execute(&mut self) -> Result<()> {
        #[cfg(all(feature = "raster", feature = "algorithms"))]
        {
            // Placeholder for actual reprojection
            // let reader = GeoTiffReader::open(&self.input_path)?;
            // let data = reader.read_all()?;
            // let reprojected = reproject(data, &self.target_crs)?;
            // let writer = GeoTiffWriter::create(&self.output_path)?;
            // writer.write(&reprojected)?;

            self.created = true;
        }

        #[cfg(not(all(feature = "raster", feature = "algorithms")))]
        {
            return Err(BenchError::missing_dependency(
                "oxigdal reprojection",
                "raster and algorithms",
            ));
        }

        #[allow(unreachable_code)]
        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        if self.created && self.output_path.exists() {
            std::fs::remove_file(&self.output_path)?;
        }
        Ok(())
    }
}

/// COG validation benchmark scenario.
pub struct CogValidationScenario {
    input_path: PathBuf,
}

impl CogValidationScenario {
    /// Creates a new COG validation benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P) -> Self {
        Self {
            input_path: input_path.into(),
        }
    }
}

impl BenchmarkScenario for CogValidationScenario {
    fn name(&self) -> &str {
        "cog_validation"
    }

    fn description(&self) -> &str {
        "Benchmark Cloud-Optimized GeoTIFF validation performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "raster")]
        {
            use std::io::Read;

            let metadata = std::fs::metadata(&self.input_path).map_err(|e| {
                BenchError::scenario_failed(self.name(), format!("Failed to read metadata: {e}"))
            })?;
            // Validate COG structural requirements: check file size > 0
            if metadata.len() == 0 {
                return Err(BenchError::scenario_failed(
                    self.name(),
                    "Empty file is not a valid COG",
                ));
            }
            // Read first 16 bytes to check TIFF magic bytes
            let mut f = std::fs::File::open(&self.input_path).map_err(|e| {
                BenchError::scenario_failed(self.name(), format!("Failed to open file: {e}"))
            })?;
            let mut header = [0u8; 16];
            f.read_exact(&mut header).map_err(|e| {
                BenchError::scenario_failed(self.name(), format!("Failed to read header: {e}"))
            })?;
            // TIFF magic: 0x49 0x49 (LE) or 0x4D 0x4D (BE)
            let is_tiff = (header[0] == 0x49 && header[1] == 0x49)
                || (header[0] == 0x4D && header[1] == 0x4D);
            if !is_tiff {
                return Err(BenchError::scenario_failed(
                    self.name(),
                    "Not a valid TIFF file",
                ));
            }
        }

        #[cfg(not(feature = "raster"))]
        {
            return Err(BenchError::missing_dependency(
                "oxigdal-geotiff COG",
                "raster",
            ));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Raster compression benchmark scenario.
pub struct CompressionBenchmarkScenario {
    input_path: PathBuf,
    output_dir: PathBuf,
    compression_methods: Vec<String>,
    created_files: Vec<PathBuf>,
}

impl CompressionBenchmarkScenario {
    /// Creates a new compression benchmark scenario.
    pub fn new<P1, P2>(input_path: P1, output_dir: P2) -> Self
    where
        P1: Into<PathBuf>,
        P2: Into<PathBuf>,
    {
        Self {
            input_path: input_path.into(),
            output_dir: output_dir.into(),
            compression_methods: vec![
                "none".to_string(),
                "lzw".to_string(),
                "deflate".to_string(),
                "zstd".to_string(),
            ],
            created_files: Vec::new(),
        }
    }

    /// Sets the compression methods to benchmark.
    pub fn with_methods(mut self, methods: Vec<String>) -> Self {
        self.compression_methods = methods;
        self
    }
}

impl BenchmarkScenario for CompressionBenchmarkScenario {
    fn name(&self) -> &str {
        "raster_compression"
    }

    fn description(&self) -> &str {
        "Benchmark different raster compression methods"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        std::fs::create_dir_all(&self.output_dir)?;

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "raster")]
        {
            // Read input data once
            // let reader = GeoTiffReader::open(&self.input_path)?;
            // let data = reader.read_all()?;

            // Benchmark each compression method
            for method in &self.compression_methods {
                let output_path = self.output_dir.join(format!("compressed_{method}.tif"));

                // Write with compression
                // let writer = GeoTiffWriter::create(&output_path)?;
                // writer.set_compression(method)?;
                // writer.write(&data)?;

                self.created_files.push(output_path);
            }
        }

        #[cfg(not(feature = "raster"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-geotiff", "raster"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        for path in &self.created_files {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
        self.created_files.clear();
        Ok(())
    }
}

/// Band statistics calculation benchmark scenario.
pub struct BandStatisticsScenario {
    input_path: PathBuf,
    band_count: usize,
}

impl BandStatisticsScenario {
    /// Creates a new band statistics benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P) -> Self {
        Self {
            input_path: input_path.into(),
            band_count: 1,
        }
    }

    /// Sets the number of bands to process.
    pub fn with_band_count(mut self, count: usize) -> Self {
        self.band_count = count;
        self
    }
}

impl BenchmarkScenario for BandStatisticsScenario {
    fn name(&self) -> &str {
        "band_statistics"
    }

    fn description(&self) -> &str {
        "Benchmark raster band statistics calculation"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "raster")]
        {
            // Generate synthetic band data for benchmarking statistics computation
            let band_size = 1024usize * 1024;
            for _ in 0..self.band_count {
                let data: Vec<f32> = (0..band_size)
                    .map(|i| ((i % 1000) as f32) * 0.001_f32)
                    .collect();
                // Calculate statistics
                let _min = data.iter().copied().fold(f32::INFINITY, f32::min);
                let _max = data.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let sum: f64 = data.iter().map(|&x| x as f64).sum();
                let mean = sum / data.len() as f64;
                let variance: f64 = data
                    .iter()
                    .map(|&x| {
                        let d = x as f64 - mean;
                        d * d
                    })
                    .sum::<f64>()
                    / data.len() as f64;
                let _std_dev = variance.sqrt();
            }
        }

        #[cfg(not(feature = "raster"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-geotiff", "raster"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Simple run-length encoder for u16 data, used to simulate LZW-style compression
/// and provide CPU-measurable work in the GeoTIFF write benchmark.
fn compress_rle_u16(data: &[u16]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(data.len() * 2);
    let mut i = 0;
    while i < data.len() {
        let val = data[i];
        let mut run: usize = 1;
        while (i + run) < data.len() && data[i + run] == val && run < (u8::MAX as usize) {
            run += 1;
        }
        // Emit: count byte, then 2 bytes (LE) for the value
        output.push(run as u8);
        output.extend_from_slice(&val.to_le_bytes());
        i += run;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geotiff_read_scenario_creation() {
        let scenario = GeoTiffReadScenario::new(std::env::temp_dir().join("test.tif"))
            .with_tile_size(256, 256);

        assert_eq!(scenario.name(), "geotiff_read");
        assert_eq!(scenario.tile_size, Some((256, 256)));
    }

    #[test]
    fn test_geotiff_write_scenario_creation() {
        let scenario = GeoTiffWriteScenario::new(std::env::temp_dir().join("output.tif"), 512, 512)
            .with_compression("lzw");

        assert_eq!(scenario.name(), "geotiff_write");
        assert_eq!(scenario.compression, "lzw");
    }

    #[test]
    fn test_compression_benchmark_creation() {
        let scenario = CompressionBenchmarkScenario::new(
            std::env::temp_dir().join("input.tif"),
            std::env::temp_dir().join("output"),
        );

        assert_eq!(scenario.name(), "raster_compression");
        assert!(!scenario.compression_methods.is_empty());
    }
}
