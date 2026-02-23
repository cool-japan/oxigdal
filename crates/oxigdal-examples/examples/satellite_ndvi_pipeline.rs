//! Satellite NDVI Pipeline - End-to-End Sentinel-2 Processing
#![allow(missing_docs)]
#![allow(dead_code)]
//!
//! This example demonstrates a real-world workflow for vegetation analysis:
//! 1. Search STAC catalog for Sentinel-2 imagery
//! 2. Download NIR and Red bands as COG
//! 3. Calculate NDVI (Normalized Difference Vegetation Index)
//! 4. Classify NDVI into vegetation categories
//! 5. Save results as Cloud-Optimized GeoTIFF
//!
//! NDVI Formula: NDVI = (NIR - Red) / (NIR + Red)
//! Values range from -1.0 to 1.0, where higher values indicate healthier vegetation.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example satellite_ndvi_pipeline
//! ```
//!
//! # Workflow
//!
//! STAC Search → Download COG → Calculate NDVI → Classify → Write COG
//!
//! # Dependencies
//!
//! This example requires network access to query STAC APIs and download satellite imagery.

// Note: Some raster algorithm modules (calculator, classify, statistics) are not yet public.
// This example demonstrates the intended workflow but uses local implementations.
use oxigdal_core::{buffer::RasterBuffer, types::RasterDataType};
use std::collections::HashMap;
use std::time::Instant;
use tempfile::TempDir;
use thiserror::Error;

/// Local metadata structure for the example
#[derive(Debug, Default, Clone)]
pub struct Metadata {
    properties: HashMap<String, String>,
}

impl Metadata {
    /// Create a new empty metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a metadata property
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.properties.insert(key.into(), value.into());
    }

    /// Get a metadata property
    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(|s| s.as_str())
    }
}

/// Custom error types for NDVI pipeline
#[derive(Debug, Error)]
pub enum NdviError {
    /// STAC search errors
    #[error("STAC search failed: {0}")]
    StacSearch(String),

    /// Band extraction errors
    #[error("Failed to extract band: {0}")]
    BandExtraction(String),

    /// Calculation errors
    #[error("NDVI calculation failed: {0}")]
    Calculation(String),

    /// Classification errors
    #[error("Classification failed: {0}")]
    Classification(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Algorithm errors
    #[error("Algorithm error: {0}")]
    Algorithm(String),

    /// No imagery found
    #[error("No imagery found for specified criteria")]
    NoImagery,
}

type Result<T> = std::result::Result<T, NdviError>;

/// NDVI vegetation classification categories
#[derive(Debug, Clone, Copy)]
pub enum VegetationClass {
    /// Water or no vegetation (NDVI < 0.0)
    Water,
    /// Bare soil or very sparse vegetation (0.0 <= NDVI < 0.2)
    BareSoil,
    /// Low vegetation density (0.2 <= NDVI < 0.4)
    LowVegetation,
    /// Moderate vegetation (0.4 <= NDVI < 0.6)
    ModerateVegetation,
    /// Dense vegetation (0.6 <= NDVI < 0.8)
    DenseVegetation,
    /// Very dense/healthy vegetation (NDVI >= 0.8)
    VeryDenseVegetation,
}

impl VegetationClass {
    /// Get the class value (0-5)
    pub fn value(&self) -> u8 {
        match self {
            Self::Water => 0,
            Self::BareSoil => 1,
            Self::LowVegetation => 2,
            Self::ModerateVegetation => 3,
            Self::DenseVegetation => 4,
            Self::VeryDenseVegetation => 5,
        }
    }

    /// Get class name
    pub fn name(&self) -> &str {
        match self {
            Self::Water => "Water/No Vegetation",
            Self::BareSoil => "Bare Soil/Very Sparse",
            Self::LowVegetation => "Low Vegetation",
            Self::ModerateVegetation => "Moderate Vegetation",
            Self::DenseVegetation => "Dense Vegetation",
            Self::VeryDenseVegetation => "Very Dense Vegetation",
        }
    }

    /// Get NDVI range for this class
    pub fn range(&self) -> (f32, f32) {
        match self {
            Self::Water => (-1.0, 0.0),
            Self::BareSoil => (0.0, 0.2),
            Self::LowVegetation => (0.2, 0.4),
            Self::ModerateVegetation => (0.4, 0.6),
            Self::DenseVegetation => (0.6, 0.8),
            Self::VeryDenseVegetation => (0.8, 1.0),
        }
    }
}

/// Sentinel-2 band information
#[derive(Debug, Clone)]
pub struct Sentinel2Bands {
    /// Red band (B04) - 665 nm
    pub red: RasterBuffer,
    /// Near-infrared band (B08) - 842 nm
    pub nir: RasterBuffer,
    /// Metadata from source imagery
    pub metadata: Metadata,
}

/// NDVI calculation result
#[derive(Debug)]
pub struct NdviResult {
    /// NDVI values (-1.0 to 1.0)
    pub ndvi: RasterBuffer,
    /// Classified vegetation map
    pub classified: RasterBuffer,
    /// Statistics about NDVI values
    pub statistics: NdviStatistics,
}

/// Statistics about calculated NDVI
#[derive(Debug)]
pub struct NdviStatistics {
    /// Minimum NDVI value
    pub min: f32,
    /// Maximum NDVI value
    pub max: f32,
    /// Mean NDVI value
    pub mean: f32,
    /// Standard deviation
    pub std_dev: f32,
    /// Pixel counts per vegetation class
    pub class_counts: Vec<(VegetationClass, usize)>,
}

/// NDVI pipeline configuration
pub struct NdviPipeline {
    /// STAC API endpoint
    stac_endpoint: String,
    /// Target bounding box [west, south, east, north]
    bbox: [f64; 4],
    /// Maximum cloud cover percentage (0-100)
    max_cloud_cover: f32,
    /// Output directory for results
    output_dir: TempDir,
}

impl NdviPipeline {
    /// Create a new NDVI pipeline
    ///
    /// # Arguments
    ///
    /// * `stac_endpoint` - STAC API endpoint URL
    /// * `bbox` - Bounding box [west, south, east, north]
    /// * `max_cloud_cover` - Maximum cloud cover percentage (0-100)
    pub fn new(
        stac_endpoint: impl Into<String>,
        bbox: [f64; 4],
        max_cloud_cover: f32,
    ) -> Result<Self> {
        let output_dir = TempDir::new()?;
        Ok(Self {
            stac_endpoint: stac_endpoint.into(),
            bbox,
            max_cloud_cover,
            output_dir,
        })
    }

    /// Search for Sentinel-2 imagery
    ///
    /// This method queries the STAC API to find suitable Sentinel-2 scenes
    /// based on the configured bounding box and cloud cover threshold.
    async fn search_imagery(&self) -> Result<Vec<String>> {
        println!("Searching for Sentinel-2 imagery...");
        println!("  Bounding box: {:?}", self.bbox);
        println!("  Max cloud cover: {}%", self.max_cloud_cover);

        // Note: In a real implementation, this would use the STAC client
        // For this example, we'll simulate the search
        println!("  [Simulated] Found 3 scenes matching criteria");

        // Simulated scene IDs
        Ok(vec![
            "S2A_MSIL2A_20240115T101234_N0509_R065_T32TQM_20240115T123456".to_string(),
            "S2B_MSIL2A_20240110T101234_N0509_R065_T32TQM_20240110T123456".to_string(),
            "S2A_MSIL2A_20240105T101234_N0509_R065_T32TQM_20240105T123456".to_string(),
        ])
    }

    /// Generate synthetic Sentinel-2 bands for demonstration
    ///
    /// In a production implementation, this would download actual COG data
    /// from cloud storage (e.g., AWS S3, Google Cloud Storage).
    fn generate_synthetic_bands(&self, width: usize, height: usize) -> Result<Sentinel2Bands> {
        println!(
            "Generating synthetic Sentinel-2 bands ({} x {})...",
            width, height
        );

        // Create synthetic Red band (B04)
        // Typical values: 0-10000 (scaled reflectance)
        let mut red_data = vec![0u16; width * height];
        for (i, pixel) in red_data.iter_mut().enumerate() {
            let x = i % width;
            let y = i / width;
            // Create a pattern: lower values (darker) in center, higher at edges
            let dx = (x as f32 - width as f32 / 2.0).abs();
            let dy = (y as f32 - height as f32 / 2.0).abs();
            let dist = (dx * dx + dy * dy).sqrt();
            *pixel = ((dist / 10.0) + 1000.0).min(3000.0) as u16;
        }

        // Create synthetic NIR band (B08)
        // Vegetation has high NIR reflectance
        let mut nir_data = vec![0u16; width * height];
        for (i, pixel) in nir_data.iter_mut().enumerate() {
            let x = i % width;
            let y = i / width;
            // Create a pattern: higher values (brighter) in center (vegetation)
            let dx = (x as f32 - width as f32 / 2.0).abs();
            let dy = (y as f32 - height as f32 / 2.0).abs();
            let dist = (dx * dx + dy * dy).sqrt();
            *pixel = (8000.0 - (dist / 5.0)).max(2000.0) as u16;
        }

        let red = RasterBuffer::from_typed_vec(width, height, red_data, RasterDataType::UInt16)
            .map_err(|e: oxigdal_core::error::OxiGdalError| {
                NdviError::BandExtraction(e.to_string())
            })?;

        let nir = RasterBuffer::from_typed_vec(width, height, nir_data, RasterDataType::UInt16)
            .map_err(|e: oxigdal_core::error::OxiGdalError| {
                NdviError::BandExtraction(e.to_string())
            })?;

        let mut metadata = Metadata::new();
        metadata.set("sensor", "Sentinel-2");
        metadata.set("processing_level", "L2A");
        metadata.set("red_band", "B04");
        metadata.set("nir_band", "B08");

        Ok(Sentinel2Bands { red, nir, metadata })
    }

    /// Calculate NDVI from Red and NIR bands
    ///
    /// NDVI = (NIR - Red) / (NIR + Red)
    ///
    /// The formula is applied pixel-by-pixel. Values range from -1.0 to 1.0.
    fn calculate_ndvi(&self, bands: &Sentinel2Bands) -> Result<RasterBuffer> {
        println!("Calculating NDVI...");

        let width = bands.red.width() as usize;
        let height = bands.red.height() as usize;
        let mut ndvi_data = vec![0.0f32; width * height];

        // Convert bands to f32 for calculation
        let red_data = bands
            .red
            .as_slice::<u16>()
            .map_err(|e| NdviError::Calculation(e.to_string()))?;
        let nir_data = bands
            .nir
            .as_slice::<u16>()
            .map_err(|e| NdviError::Calculation(e.to_string()))?;

        // Calculate NDVI pixel by pixel
        for i in 0..ndvi_data.len() {
            let red = red_data[i] as f32;
            let nir = nir_data[i] as f32;

            // NDVI formula
            let denominator = nir + red;
            if denominator > 0.0 {
                ndvi_data[i] = (nir - red) / denominator;
            } else {
                ndvi_data[i] = -1.0; // No data or water
            }
        }

        RasterBuffer::from_typed_vec(width, height, ndvi_data, RasterDataType::Float32)
            .map_err(|e: oxigdal_core::error::OxiGdalError| NdviError::Calculation(e.to_string()))
    }

    /// Classify NDVI into vegetation categories
    fn classify_ndvi(&self, ndvi: &RasterBuffer) -> Result<RasterBuffer> {
        println!("Classifying NDVI into vegetation categories...");

        let width = ndvi.width() as usize;
        let height = ndvi.height() as usize;
        let mut classified_data = vec![0u8; width * height];

        let ndvi_data = ndvi
            .as_slice::<f32>()
            .map_err(|e| NdviError::Classification(e.to_string()))?;

        // Classify each pixel
        for (i, ndvi_val) in ndvi_data.iter().enumerate() {
            let class = if *ndvi_val < 0.0 {
                VegetationClass::Water
            } else if *ndvi_val < 0.2 {
                VegetationClass::BareSoil
            } else if *ndvi_val < 0.4 {
                VegetationClass::LowVegetation
            } else if *ndvi_val < 0.6 {
                VegetationClass::ModerateVegetation
            } else if *ndvi_val < 0.8 {
                VegetationClass::DenseVegetation
            } else {
                VegetationClass::VeryDenseVegetation
            };

            classified_data[i] = class.value();
        }

        RasterBuffer::from_typed_vec(width, height, classified_data, RasterDataType::UInt8).map_err(
            |e: oxigdal_core::error::OxiGdalError| NdviError::Classification(e.to_string()),
        )
    }

    /// Calculate statistics for NDVI
    fn calculate_statistics(
        &self,
        ndvi: &RasterBuffer,
        classified: &RasterBuffer,
    ) -> Result<NdviStatistics> {
        println!("Calculating NDVI statistics...");

        let ndvi_data = ndvi
            .as_slice::<f32>()
            .map_err(|e| NdviError::Algorithm(e.to_string()))?;
        let class_data = classified
            .as_slice::<u8>()
            .map_err(|e| NdviError::Algorithm(e.to_string()))?;

        // Calculate basic statistics
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let mut sum = 0.0;
        let mut count = 0;

        for &val in ndvi_data {
            if val.is_finite() {
                min = min.min(val);
                max = max.max(val);
                sum += val;
                count += 1;
            }
        }

        let mean = if count > 0 { sum / count as f32 } else { 0.0 };

        // Calculate standard deviation
        let mut sum_sq = 0.0;
        for &val in ndvi_data {
            if val.is_finite() {
                let diff = val - mean;
                sum_sq += diff * diff;
            }
        }
        let std_dev = if count > 0 {
            (sum_sq / count as f32).sqrt()
        } else {
            0.0
        };

        // Count pixels per vegetation class
        let mut class_counts = [0usize; 6];
        for &class in class_data {
            if (class as usize) < class_counts.len() {
                class_counts[class as usize] += 1;
            }
        }

        let class_counts_vec = vec![
            (VegetationClass::Water, class_counts[0]),
            (VegetationClass::BareSoil, class_counts[1]),
            (VegetationClass::LowVegetation, class_counts[2]),
            (VegetationClass::ModerateVegetation, class_counts[3]),
            (VegetationClass::DenseVegetation, class_counts[4]),
            (VegetationClass::VeryDenseVegetation, class_counts[5]),
        ];

        Ok(NdviStatistics {
            min,
            max,
            mean,
            std_dev,
            class_counts: class_counts_vec,
        })
    }

    /// Save NDVI as Cloud-Optimized GeoTIFF
    fn save_as_cog(&self, ndvi: &RasterBuffer, _metadata: &Metadata) -> Result<std::path::PathBuf> {
        println!("Saving NDVI as Cloud-Optimized GeoTIFF...");

        let output_path = self.output_dir.path().join("ndvi_output.tif");

        // Note: In a real implementation, this would use GeoTiffWriter
        // For this example, we simulate the write
        println!("  Output path: {}", output_path.display());
        println!("  Dimensions: {} x {}", ndvi.width(), ndvi.height());
        println!("  Data type: Float32");
        println!("  Compression: Deflate");
        println!("  Tiling: 512x512");

        // Simulated write
        std::fs::write(&output_path, b"GeoTIFF placeholder")?;

        Ok(output_path)
    }

    /// Run the complete NDVI pipeline
    pub async fn run(&self) -> Result<NdviResult> {
        let start = Instant::now();
        println!("=== Satellite NDVI Pipeline ===\n");

        // Step 1: Search for imagery
        let _scenes = self.search_imagery().await?;

        // Step 2: Generate/download bands
        // In production: download actual bands from COG URLs
        let bands = self.generate_synthetic_bands(1024, 1024)?;
        println!("  Red band range: 0-10000 (scaled reflectance)");
        println!("  NIR band range: 0-10000 (scaled reflectance)\n");

        // Step 3: Calculate NDVI
        let ndvi = self.calculate_ndvi(&bands)?;
        println!("  NDVI calculated\n");

        // Step 4: Classify vegetation
        let classified = self.classify_ndvi(&ndvi)?;
        println!("  Classification complete\n");

        // Step 5: Calculate statistics
        let statistics = self.calculate_statistics(&ndvi, &classified)?;

        // Step 6: Save as COG
        let output_path = self.save_as_cog(&ndvi, &bands.metadata)?;
        println!("  Saved to: {}\n", output_path.display());

        // Print statistics
        println!("=== NDVI Statistics ===");
        println!("  Min:  {:.3}", statistics.min);
        println!("  Max:  {:.3}", statistics.max);
        println!("  Mean: {:.3}", statistics.mean);
        println!("  StdDev: {:.3}", statistics.std_dev);
        println!();

        println!("=== Vegetation Classification ===");
        let total_pixels: usize = statistics.class_counts.iter().map(|(_, count)| count).sum();
        for (class, count) in &statistics.class_counts {
            let percentage = (*count as f64 / total_pixels as f64) * 100.0;
            println!(
                "  {:<25} {:>8} pixels ({:>5.1}%)",
                class.name(),
                count,
                percentage
            );
        }
        println!();

        let elapsed = start.elapsed();
        println!("=== Pipeline Complete ===");
        println!("Total time: {:.2}s", elapsed.as_secs_f64());

        Ok(NdviResult {
            ndvi,
            classified,
            statistics,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("Satellite NDVI Pipeline - Sentinel-2 Processing Example\n");

    // Configure pipeline
    // Example: San Francisco Bay Area
    let bbox = [-122.5, 37.5, -122.0, 38.0];
    let pipeline = NdviPipeline::new(
        "https://earth-search.aws.element84.com/v1",
        bbox,
        20.0, // Max 20% cloud cover
    )?;

    // Run the pipeline
    let _result = pipeline.run().await?;

    println!("\nExample completed successfully!");
    println!("This demonstrates the complete workflow for satellite-based vegetation analysis.");

    Ok(())
}
