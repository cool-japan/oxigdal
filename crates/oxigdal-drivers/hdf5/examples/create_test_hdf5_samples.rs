//! HDF5 Sample Generator for Hierarchical Raster Data
//!
//! This example generates HDF5 files for Golden Triangle and Basque Country regions
//! with hierarchical structure, chunking, and compression.
//!
//! # Features
//!
//! - Hierarchical groups and datasets
//! - Chunked storage with GZIP compression
//! - Multi-band raster data (temperature, elevation, NDVI, geology)
//! - Metadata attributes
//! - Pure Rust implementation (no unwrap)
//!
//! # Output Files
//!
//! - `demo/cog-viewer/golden-triangle.h5`
//! - `demo/cog-viewer/iron-belt.h5`
//!
//! # Usage
//!
//! ```bash
//! cargo run --example create_test_hdf5_samples
//! ```

use oxigdal_hdf5::{
    Attribute, AttributeValue, DatasetProperties, Datatype, Hdf5Version, Hdf5Writer, Result,
};
use std::f64::consts::PI;
use std::path::Path;

/// Region configuration for generating sample data
#[derive(Debug, Clone)]
struct RegionConfig {
    name: String,
    center_lon: f64,
    center_lat: f64,
    description: String,
    width: usize,
    height: usize,
    resolution: f64,
}

impl RegionConfig {
    /// Create configuration for Golden Triangle region
    fn golden_triangle() -> Self {
        Self {
            name: "Golden Triangle".to_string(),
            center_lon: 100.08466749884738,
            center_lat: 20.35223590060906,
            description: "Thailand/Myanmar/Laos border region".to_string(),
            width: 512,
            height: 512,
            resolution: 0.0001, // ~11 meters at this latitude
        }
    }

    /// Create configuration for Basque Country region
    fn iron_belt() -> Self {
        Self {
            name: "Basque Country".to_string(),
            center_lon: -2.9253,
            center_lat: 43.2630,
            description: "Basque Country iron and steel mining region".to_string(),
            width: 512,
            height: 512,
            resolution: 0.0001, // ~11 meters at this latitude
        }
    }

    /// Get west boundary
    fn west(&self) -> f64 {
        self.center_lon - (self.width as f64 * self.resolution / 2.0)
    }

    /// Get east boundary
    fn east(&self) -> f64 {
        self.center_lon + (self.width as f64 * self.resolution / 2.0)
    }

    /// Get north boundary
    fn north(&self) -> f64 {
        self.center_lat + (self.height as f64 * self.resolution / 2.0)
    }

    /// Get south boundary
    fn south(&self) -> f64 {
        self.center_lat - (self.height as f64 * self.resolution / 2.0)
    }
}

/// Generate synthetic temperature data
fn generate_temperature(config: &RegionConfig) -> Vec<f32> {
    let mut data = Vec::with_capacity(config.width * config.height);

    // Base temperature varies by latitude
    let base_temp = 25.0 - (config.center_lat.abs() * 0.5);

    for y in 0..config.height {
        for x in 0..config.width {
            // Add spatial variation using sine waves
            let dx = (x as f64 - config.width as f64 / 2.0) / config.width as f64;
            let dy = (y as f64 - config.height as f64 / 2.0) / config.height as f64;

            let variation = (dx * PI * 2.0).sin() * (dy * PI * 2.0).cos() * 5.0;
            let temp = base_temp as f32 + variation as f32;

            data.push(temp);
        }
    }

    data
}

/// Generate synthetic elevation data
fn generate_elevation(config: &RegionConfig) -> Vec<f32> {
    let mut data = Vec::with_capacity(config.width * config.height);

    // Base elevation varies by region
    let base_elevation = if config.name.contains("Golden") {
        800.0 // Golden Triangle is mountainous
    } else {
        400.0 // Basque Country is hilly terrain
    };

    for y in 0..config.height {
        for x in 0..config.width {
            let dx = (x as f64 - config.width as f64 / 2.0) / config.width as f64;
            let dy = (y as f64 - config.height as f64 / 2.0) / config.height as f64;

            // Create realistic terrain with multiple frequencies
            let r = (dx * dx + dy * dy).sqrt();
            let terrain = (r * PI * 4.0).sin() * 100.0
                + (dx * PI * 8.0).cos() * 50.0
                + (dy * PI * 16.0).sin() * 25.0;

            let elevation = base_elevation as f32 + terrain as f32;
            data.push(elevation.max(0.0));
        }
    }

    data
}

/// Generate synthetic NDVI data (Normalized Difference Vegetation Index)
fn generate_ndvi(config: &RegionConfig) -> Vec<f32> {
    let mut data = Vec::with_capacity(config.width * config.height);

    for y in 0..config.height {
        for x in 0..config.width {
            let dx = (x as f64 - config.width as f64 / 2.0) / config.width as f64;
            let dy = (y as f64 - config.height as f64 / 2.0) / config.height as f64;

            // NDVI ranges from -1 to 1
            // Create patches of vegetation
            let r = (dx * dx + dy * dy).sqrt();
            let vegetation = if r < 0.3 {
                0.7 + (dx * PI * 10.0).sin() * 0.2 // Dense vegetation
            } else if r < 0.6 {
                0.4 + (dy * PI * 5.0).cos() * 0.2 // Moderate vegetation
            } else {
                0.2 + (r * PI * 3.0).sin() * 0.1 // Sparse vegetation
            };

            let ndvi = vegetation.clamp(-1.0, 1.0) as f32;
            data.push(ndvi);
        }
    }

    data
}

/// Generate synthetic geology data (iron concentration for Basque Country)
fn generate_geology(config: &RegionConfig) -> Vec<f32> {
    let mut data = Vec::with_capacity(config.width * config.height);

    let is_basque_region = config.name.contains("Basque");

    for y in 0..config.height {
        for x in 0..config.width {
            let dx = (x as f64 - config.width as f64 / 2.0) / config.width as f64;
            let dy = (y as f64 - config.height as f64 / 2.0) / config.height as f64;

            let value = if is_basque_region {
                // Iron concentration (ppm)
                let r = (dx * dx + dy * dy).sqrt();
                let concentration = if r < 0.2 {
                    5000.0 + (dx * PI * 8.0).sin() * 2000.0 // High concentration zone
                } else if r < 0.5 {
                    2000.0 + (dy * PI * 4.0).cos() * 1000.0 // Medium concentration
                } else {
                    500.0 + (r * PI * 2.0).sin() * 300.0 // Low background
                };
                concentration.max(0.0) as f32
            } else {
                // Soil pH for Golden Triangle
                let ph = 6.5 + (dx * PI * 3.0).cos() * 0.8 + (dy * PI * 3.0).sin() * 0.5;
                ph.clamp(4.0, 9.0) as f32
            };

            data.push(value);
        }
    }

    data
}

/// Create HDF5 file for a region
fn create_region_hdf5(config: &RegionConfig, output_path: &Path) -> Result<()> {
    println!("Creating HDF5 file for {}...", config.name);
    println!("  Output: {}", output_path.display());
    println!("  Dimensions: {}x{}", config.width, config.height);
    println!("  Resolution: {} degrees", config.resolution);
    println!(
        "  Bounds: [{:.4}, {:.4}] to [{:.4}, {:.4}]",
        config.west(),
        config.south(),
        config.east(),
        config.north()
    );

    // Create HDF5 file
    let mut writer = Hdf5Writer::create(output_path, Hdf5Version::V10)?;

    // Create root metadata group
    writer.create_group("/metadata")?;

    // Add metadata attributes
    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "region_name".to_string(),
            AttributeValue::String(config.name.clone()),
        ),
    )?;

    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "description".to_string(),
            AttributeValue::String(config.description.clone()),
        ),
    )?;

    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "center_longitude".to_string(),
            AttributeValue::Float64(config.center_lon),
        ),
    )?;

    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "center_latitude".to_string(),
            AttributeValue::Float64(config.center_lat),
        ),
    )?;

    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "resolution".to_string(),
            AttributeValue::Float64(config.resolution),
        ),
    )?;

    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "width".to_string(),
            AttributeValue::Int32(config.width as i32),
        ),
    )?;

    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "height".to_string(),
            AttributeValue::Int32(config.height as i32),
        ),
    )?;

    writer.add_group_attribute(
        "/metadata",
        Attribute::new(
            "crs".to_string(),
            AttributeValue::String("EPSG:4326".to_string()),
        ),
    )?;

    // Create raster data group
    writer.create_group("/raster_data")?;

    // Dataset properties with chunking and compression
    let chunk_size = 64; // 64x64 chunks
    let properties = DatasetProperties::new()
        .with_chunks(vec![chunk_size, chunk_size])
        .with_gzip(6); // Compression level 6

    // Generate and write temperature data
    println!("  Generating temperature data...");
    let temperature = generate_temperature(config);
    writer.create_dataset(
        "/raster_data/temperature",
        Datatype::Float32,
        vec![config.height, config.width],
        properties.clone(),
    )?;
    writer.write_f32("/raster_data/temperature", &temperature)?;
    writer.add_dataset_attribute(
        "/raster_data/temperature",
        Attribute::new(
            "units".to_string(),
            AttributeValue::String("celsius".to_string()),
        ),
    )?;
    writer.add_dataset_attribute(
        "/raster_data/temperature",
        Attribute::new(
            "long_name".to_string(),
            AttributeValue::String("Surface Temperature".to_string()),
        ),
    )?;

    // Generate and write elevation data
    println!("  Generating elevation data...");
    let elevation = generate_elevation(config);
    writer.create_dataset(
        "/raster_data/elevation",
        Datatype::Float32,
        vec![config.height, config.width],
        properties.clone(),
    )?;
    writer.write_f32("/raster_data/elevation", &elevation)?;
    writer.add_dataset_attribute(
        "/raster_data/elevation",
        Attribute::new(
            "units".to_string(),
            AttributeValue::String("meters".to_string()),
        ),
    )?;
    writer.add_dataset_attribute(
        "/raster_data/elevation",
        Attribute::new(
            "long_name".to_string(),
            AttributeValue::String("Elevation above sea level".to_string()),
        ),
    )?;

    // Generate and write NDVI data
    println!("  Generating NDVI data...");
    let ndvi = generate_ndvi(config);
    writer.create_dataset(
        "/raster_data/ndvi",
        Datatype::Float32,
        vec![config.height, config.width],
        properties.clone(),
    )?;
    writer.write_f32("/raster_data/ndvi", &ndvi)?;
    writer.add_dataset_attribute(
        "/raster_data/ndvi",
        Attribute::new(
            "units".to_string(),
            AttributeValue::String("dimensionless".to_string()),
        ),
    )?;
    writer.add_dataset_attribute(
        "/raster_data/ndvi",
        Attribute::new(
            "long_name".to_string(),
            AttributeValue::String("Normalized Difference Vegetation Index".to_string()),
        ),
    )?;
    writer.add_dataset_attribute(
        "/raster_data/ndvi",
        Attribute::new(
            "valid_range".to_string(),
            AttributeValue::String("-1.0 to 1.0".to_string()),
        ),
    )?;

    // Generate and write geology data
    println!("  Generating geology data...");
    let geology = generate_geology(config);
    let geology_name = if config.name.contains("Iron") {
        "iron_concentration"
    } else {
        "soil_ph"
    };
    let geology_units = if config.name.contains("Iron") {
        "ppm"
    } else {
        "pH"
    };
    let geology_long_name = if config.name.contains("Iron") {
        "Iron concentration in parts per million"
    } else {
        "Soil pH value"
    };

    writer.create_dataset(
        &format!("/raster_data/{}", geology_name),
        Datatype::Float32,
        vec![config.height, config.width],
        properties,
    )?;
    writer.write_f32(&format!("/raster_data/{}", geology_name), &geology)?;
    writer.add_dataset_attribute(
        &format!("/raster_data/{}", geology_name),
        Attribute::new(
            "units".to_string(),
            AttributeValue::String(geology_units.to_string()),
        ),
    )?;
    writer.add_dataset_attribute(
        &format!("/raster_data/{}", geology_name),
        Attribute::new(
            "long_name".to_string(),
            AttributeValue::String(geology_long_name.to_string()),
        ),
    )?;

    // Create coordinate arrays group
    writer.create_group("/coordinates")?;

    // Generate longitude array
    let longitude: Vec<f64> = (0..config.width)
        .map(|x| config.west() + (x as f64 * config.resolution))
        .collect();
    writer.create_dataset(
        "/coordinates/longitude",
        Datatype::Float64,
        vec![config.width],
        DatasetProperties::new(),
    )?;
    writer.write_f64("/coordinates/longitude", &longitude)?;
    writer.add_dataset_attribute(
        "/coordinates/longitude",
        Attribute::new(
            "units".to_string(),
            AttributeValue::String("degrees_east".to_string()),
        ),
    )?;

    // Generate latitude array
    let latitude: Vec<f64> = (0..config.height)
        .map(|y| config.north() - (y as f64 * config.resolution))
        .collect();
    writer.create_dataset(
        "/coordinates/latitude",
        Datatype::Float64,
        vec![config.height],
        DatasetProperties::new(),
    )?;
    writer.write_f64("/coordinates/latitude", &latitude)?;
    writer.add_dataset_attribute(
        "/coordinates/latitude",
        Attribute::new(
            "units".to_string(),
            AttributeValue::String("degrees_north".to_string()),
        ),
    )?;

    // Finalize file
    writer.finalize()?;

    println!("  ✓ Successfully created {}", output_path.display());
    Ok(())
}

fn main() -> Result<()> {
    println!("=== HDF5 Sample Generator ===\n");
    println!("Generating hierarchical raster data for test regions...\n");

    // Determine output directory
    let project_root = std::env::current_dir()?;
    let demo_dir = project_root.join("demo").join("cog-viewer");

    // Create demo directory if it doesn't exist
    std::fs::create_dir_all(&demo_dir)?;

    // Generate Golden Triangle HDF5
    let golden_config = RegionConfig::golden_triangle();
    let golden_path = demo_dir.join("golden-triangle.h5");
    create_region_hdf5(&golden_config, &golden_path)?;

    println!();

    // Generate Basque Country HDF5
    let iron_config = RegionConfig::iron_belt();
    let iron_path = demo_dir.join("iron-belt.h5");
    create_region_hdf5(&iron_config, &iron_path)?;

    println!("\n=== Generation Complete ===\n");
    println!("Output files:");
    println!("  - {}", golden_path.display());
    println!("  - {}", iron_path.display());
    println!("\nHierarchical structure:");
    println!("  /metadata (attributes: region_name, description, coordinates, etc.)");
    println!("  /raster_data");
    println!("    /temperature (Float32, chunked, GZIP compressed)");
    println!("    /elevation (Float32, chunked, GZIP compressed)");
    println!("    /ndvi (Float32, chunked, GZIP compressed)");
    println!("    /iron_concentration or /soil_ph (Float32, chunked, GZIP compressed)");
    println!("  /coordinates");
    println!("    /longitude (Float64)");
    println!("    /latitude (Float64)");
    println!("\nChunk size: 64x64");
    println!("Compression: GZIP level 6");
    println!("\nYou can inspect these files with h5dump or HDFView.");

    Ok(())
}
