//! NetCDF Sample Generator for Scientific Raster Data
//!
//! This example creates NetCDF-3 files with CF Conventions metadata
//! for two scientifically significant regions:
//! - Golden Triangle (Southeast Asia)
//! - Basque Country (Northern Spain)
//!
//! Each file includes:
//! - Proper CF Conventions 1.8 metadata
//! - Coordinate variables (lat, lon, time)
//! - Multiple data variables with scientific units
//! - Global attributes (title, institution, history, etc.)
//!
//! # COOLJAPAN Policies Compliance
//!
//! - ✓ Pure Rust: Uses netcdf3 feature only (no C dependencies)
//! - ✓ No unwrap(): All errors properly handled with Result
//! - ✓ Latest crates: Uses workspace dependencies
//! - ✓ Comprehensive documentation
//!
//! # Usage
//!
//! ```bash
//! cargo run --example create_test_netcdf_samples --features netcdf3
//! ```
//!
//! Output files will be created in:
//! - demo/cog-viewer/golden-triangle.nc
//! - demo/cog-viewer/iron-belt.nc

use oxigdal_netcdf::{
    Attribute, AttributeValue, DataType, Dimension, NetCdfVersion, NetCdfWriter, Result, Variable,
};
use std::f32::consts::PI;
use std::path::PathBuf;

/// Geographic extent for Golden Triangle region
struct GoldenTriangle {
    lat_min: f32,
    lat_max: f32,
    lon_min: f32,
    lon_max: f32,
    #[allow(dead_code)]
    name: &'static str,
}

impl GoldenTriangle {
    fn new() -> Self {
        Self {
            lat_min: 15.0,
            lat_max: 25.0,
            lon_min: 95.0,
            lon_max: 105.0,
            name: "Golden Triangle",
        }
    }
}

/// Geographic extent for Basque Country region
struct IronBelt {
    lat_min: f32,
    lat_max: f32,
    lon_min: f32,
    lon_max: f32,
    #[allow(dead_code)]
    name: &'static str,
}

impl IronBelt {
    fn new() -> Self {
        Self {
            lat_min: 41.5,
            lat_max: 44.5,
            lon_min: -5.0,
            lon_max: 0.0,
            name: "Basque Country",
        }
    }
}

/// Generate synthetic elevation data based on lat/lon
fn generate_elevation(lat: f32, lon: f32, base_elevation: f32, variation: f32) -> f32 {
    let noise1 = ((lat * 0.5).sin() * (lon * 0.3).cos() * variation).abs();
    let noise2 = ((lat * 0.8).cos() * (lon * 0.6).sin() * variation * 0.5).abs();
    base_elevation + noise1 + noise2
}

/// Generate synthetic temperature data
fn generate_temperature(lat: f32, _lon: f32, time_idx: usize) -> f32 {
    // Base temperature decreases with latitude
    let base_temp = 35.0 - lat.abs() * 0.5;
    // Seasonal variation
    let seasonal = ((time_idx as f32 / 12.0) * 2.0 * PI).sin() * 5.0;
    // Daily variation
    let daily = ((time_idx as f32 / 365.0) * 2.0 * PI).cos() * 2.0;
    base_temp + seasonal + daily
}

/// Generate synthetic precipitation data
fn generate_precipitation(lat: f32, lon: f32, time_idx: usize) -> f32 {
    let base = ((lat * 0.1).sin() + (lon * 0.1).cos()).abs() * 100.0;
    let seasonal = ((time_idx as f32 / 12.0) * 2.0 * PI).sin() * 50.0;
    (base + seasonal).max(0.0)
}

/// Generate synthetic vegetation index (NDVI-like)
fn generate_vegetation_index(elevation: f32, precipitation: f32) -> f32 {
    // Vegetation index based on elevation and precipitation
    let optimal_elev = 1000.0;
    let elev_factor = 1.0 - ((elevation - optimal_elev) / 2000.0).abs().min(1.0);
    let precip_factor = (precipitation / 200.0).min(1.0);
    (elev_factor * precip_factor * 0.8).clamp(0.0, 1.0)
}

/// Generate synthetic iron concentration data
fn generate_iron_concentration(lat: f32, lon: f32) -> f32 {
    // Hotspots based on Basque Country geology
    let hotspot1 = ((43.26 - lat).powi(2) + (-2.93 - lon).powi(2)).sqrt();
    let hotspot2 = ((42.60 - lat).powi(2) + (-1.61 - lon).powi(2)).sqrt();

    let concentration = (1.0 / (hotspot1 + 0.5)) * 500.0 + (1.0 / (hotspot2 + 0.5)) * 300.0;

    concentration.min(1000.0)
}

/// Generate synthetic geology type (categorical)
fn generate_geology_type(lat: f32, lon: f32) -> i32 {
    // Simple geological zones
    // 1 = sedimentary, 2 = metamorphic, 3 = igneous, 4 = iron-rich
    ((lat + lon) * 10.0) as i32 % 4 + 1
}

/// Generate synthetic land use classification
fn generate_land_use(elevation: f32, geology: i32) -> i32 {
    // 1 = urban, 2 = agricultural, 3 = forest, 4 = mining, 5 = water
    if geology == 4 && elevation < 1500.0 {
        4 // mining in iron-rich areas
    } else if elevation < 800.0 {
        2 // agricultural in lowlands
    } else if elevation < 1200.0 {
        3 // forest in mid-elevation
    } else {
        1 // urban/sparse in highlands
    }
}

/// Create Golden Triangle NetCDF file
fn create_golden_triangle_netcdf(output_path: PathBuf) -> Result<()> {
    println!("Creating Golden Triangle NetCDF file...");

    let region = GoldenTriangle::new();
    let nlat = 50;
    let nlon = 50;
    let ntime = 12; // 12 months

    // Create writer
    let mut writer = NetCdfWriter::create(&output_path, NetCdfVersion::Classic)?;

    // Add dimensions
    writer.add_dimension(Dimension::new("lat", nlat)?)?;
    writer.add_dimension(Dimension::new("lon", nlon)?)?;
    writer.add_dimension(Dimension::new_unlimited("time", ntime)?)?;

    // Add coordinate variables
    let mut lat_var = Variable::new("lat", DataType::F32, vec!["lat".to_string()])?;
    lat_var.set_coordinate(true);
    writer.add_variable(lat_var)?;

    let mut lon_var = Variable::new("lon", DataType::F32, vec!["lon".to_string()])?;
    lon_var.set_coordinate(true);
    writer.add_variable(lon_var)?;

    let mut time_var = Variable::new("time", DataType::F64, vec!["time".to_string()])?;
    time_var.set_coordinate(true);
    writer.add_variable(time_var)?;

    // Add data variables
    writer.add_variable(Variable::new(
        "elevation",
        DataType::F32,
        vec!["lat".to_string(), "lon".to_string()],
    )?)?;

    writer.add_variable(Variable::new(
        "temperature",
        DataType::F32,
        vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
    )?)?;

    writer.add_variable(Variable::new(
        "precipitation",
        DataType::F32,
        vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
    )?)?;

    writer.add_variable(Variable::new(
        "vegetation_index",
        DataType::F32,
        vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
    )?)?;

    // Add coordinate variable attributes
    writer.add_variable_attribute(
        "lat",
        Attribute::new("standard_name", AttributeValue::text("latitude"))?,
    )?;
    writer.add_variable_attribute(
        "lat",
        Attribute::new("long_name", AttributeValue::text("Latitude"))?,
    )?;
    writer.add_variable_attribute(
        "lat",
        Attribute::new("units", AttributeValue::text("degrees_north"))?,
    )?;
    writer.add_variable_attribute("lat", Attribute::new("axis", AttributeValue::text("Y"))?)?;

    writer.add_variable_attribute(
        "lon",
        Attribute::new("standard_name", AttributeValue::text("longitude"))?,
    )?;
    writer.add_variable_attribute(
        "lon",
        Attribute::new("long_name", AttributeValue::text("Longitude"))?,
    )?;
    writer.add_variable_attribute(
        "lon",
        Attribute::new("units", AttributeValue::text("degrees_east"))?,
    )?;
    writer.add_variable_attribute("lon", Attribute::new("axis", AttributeValue::text("X"))?)?;

    writer.add_variable_attribute(
        "time",
        Attribute::new("standard_name", AttributeValue::text("time"))?,
    )?;
    writer.add_variable_attribute(
        "time",
        Attribute::new("long_name", AttributeValue::text("Time"))?,
    )?;
    writer.add_variable_attribute(
        "time",
        Attribute::new(
            "units",
            AttributeValue::text("days since 2024-01-01 00:00:00"),
        )?,
    )?;
    writer.add_variable_attribute(
        "time",
        Attribute::new("calendar", AttributeValue::text("gregorian"))?,
    )?;
    writer.add_variable_attribute("time", Attribute::new("axis", AttributeValue::text("T"))?)?;

    // Add elevation variable attributes
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("standard_name", AttributeValue::text("surface_altitude"))?,
    )?;
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("long_name", AttributeValue::text("Surface Elevation"))?,
    )?;
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("units", AttributeValue::text("m"))?,
    )?;
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("_FillValue", AttributeValue::f32(-9999.0))?,
    )?;

    // Add temperature variable attributes
    writer.add_variable_attribute(
        "temperature",
        Attribute::new("standard_name", AttributeValue::text("air_temperature"))?,
    )?;
    writer.add_variable_attribute(
        "temperature",
        Attribute::new(
            "long_name",
            AttributeValue::text("Near-Surface Air Temperature"),
        )?,
    )?;
    writer.add_variable_attribute(
        "temperature",
        Attribute::new("units", AttributeValue::text("degC"))?,
    )?;
    writer.add_variable_attribute(
        "temperature",
        Attribute::new("_FillValue", AttributeValue::f32(-9999.0))?,
    )?;

    // Add precipitation variable attributes
    writer.add_variable_attribute(
        "precipitation",
        Attribute::new(
            "standard_name",
            AttributeValue::text("precipitation_amount"),
        )?,
    )?;
    writer.add_variable_attribute(
        "precipitation",
        Attribute::new("long_name", AttributeValue::text("Monthly Precipitation"))?,
    )?;
    writer.add_variable_attribute(
        "precipitation",
        Attribute::new("units", AttributeValue::text("mm"))?,
    )?;
    writer.add_variable_attribute(
        "precipitation",
        Attribute::new("_FillValue", AttributeValue::f32(-9999.0))?,
    )?;

    // Add vegetation index variable attributes
    writer.add_variable_attribute(
        "vegetation_index",
        Attribute::new(
            "standard_name",
            AttributeValue::text("normalized_difference_vegetation_index"),
        )?,
    )?;
    writer.add_variable_attribute(
        "vegetation_index",
        Attribute::new(
            "long_name",
            AttributeValue::text("Vegetation Index (NDVI-like)"),
        )?,
    )?;
    writer.add_variable_attribute(
        "vegetation_index",
        Attribute::new("units", AttributeValue::text("1"))?,
    )?;
    writer.add_variable_attribute(
        "vegetation_index",
        Attribute::new("valid_range", AttributeValue::f32_array(vec![0.0, 1.0]))?,
    )?;
    writer.add_variable_attribute(
        "vegetation_index",
        Attribute::new("_FillValue", AttributeValue::f32(-9999.0))?,
    )?;

    // Add global attributes (CF Conventions)
    writer.add_global_attribute(Attribute::new(
        "Conventions",
        AttributeValue::text("CF-1.8"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "title",
        AttributeValue::text("Golden Triangle Environmental Data"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "institution",
        AttributeValue::text("COOLJAPAN OU (Team Kitasan)"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "source",
        AttributeValue::text("OxiGDAL NetCDF Sample Generator"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "history",
        AttributeValue::text("2024-01-01: Created with OxiGDAL"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "references",
        AttributeValue::text("https://github.com/cool-japan/oxigdal"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "comment",
        AttributeValue::text(
            "Synthetic scientific data for the Golden Triangle region (Southeast Asia)",
        ),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lat_min",
        AttributeValue::f32(region.lat_min),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lat_max",
        AttributeValue::f32(region.lat_max),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lon_min",
        AttributeValue::f32(region.lon_min),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lon_max",
        AttributeValue::f32(region.lon_max),
    )?)?;

    // End define mode
    writer.end_define_mode()?;

    // Generate and write coordinate data
    let lat_data: Vec<f32> = (0..nlat)
        .map(|i| {
            region.lat_min + (i as f32 / (nlat - 1) as f32) * (region.lat_max - region.lat_min)
        })
        .collect();
    writer.write_f32("lat", &lat_data)?;

    let lon_data: Vec<f32> = (0..nlon)
        .map(|i| {
            region.lon_min + (i as f32 / (nlon - 1) as f32) * (region.lon_max - region.lon_min)
        })
        .collect();
    writer.write_f32("lon", &lon_data)?;

    let time_data: Vec<f64> = (0..ntime).map(|i| i as f64 * 30.0).collect();
    writer.write_f64("time", &time_data)?;

    // Generate and write elevation data (time-independent)
    let mut elevation_data = Vec::with_capacity(nlat * nlon);
    for lat in &lat_data {
        for lon in &lon_data {
            let elev = generate_elevation(*lat, *lon, 800.0, 1200.0);
            elevation_data.push(elev);
        }
    }
    writer.write_f32("elevation", &elevation_data)?;

    // Generate and write time-dependent data
    let mut temperature_data = Vec::with_capacity(ntime * nlat * nlon);
    let mut precipitation_data = Vec::with_capacity(ntime * nlat * nlon);
    let mut vegetation_data = Vec::with_capacity(ntime * nlat * nlon);

    for t in 0..ntime {
        for (i, lat) in lat_data.iter().enumerate() {
            for (j, lon) in lon_data.iter().enumerate() {
                let temp = generate_temperature(*lat, *lon, t);
                let precip = generate_precipitation(*lat, *lon, t);
                let elev = elevation_data[i * nlon + j];
                let veg_idx = generate_vegetation_index(elev, precip);

                temperature_data.push(temp);
                precipitation_data.push(precip);
                vegetation_data.push(veg_idx);
            }
        }
    }

    writer.write_f32("temperature", &temperature_data)?;
    writer.write_f32("precipitation", &precipitation_data)?;
    writer.write_f32("vegetation_index", &vegetation_data)?;

    // Close and finalize
    writer.close()?;

    println!("✓ Created: {}", output_path.display());
    Ok(())
}

/// Create Basque Country NetCDF file
fn create_iron_belt_netcdf(output_path: PathBuf) -> Result<()> {
    println!("Creating Basque Country NetCDF file...");

    let region = IronBelt::new();
    let nlat = 40;
    let nlon = 30;
    let ntime = 1; // Single time snapshot for geological data

    // Create writer
    let mut writer = NetCdfWriter::create(&output_path, NetCdfVersion::Classic)?;

    // Add dimensions
    writer.add_dimension(Dimension::new("lat", nlat)?)?;
    writer.add_dimension(Dimension::new("lon", nlon)?)?;
    writer.add_dimension(Dimension::new_unlimited("time", ntime)?)?;

    // Add coordinate variables
    let mut lat_var = Variable::new("lat", DataType::F32, vec!["lat".to_string()])?;
    lat_var.set_coordinate(true);
    writer.add_variable(lat_var)?;

    let mut lon_var = Variable::new("lon", DataType::F32, vec!["lon".to_string()])?;
    lon_var.set_coordinate(true);
    writer.add_variable(lon_var)?;

    let mut time_var = Variable::new("time", DataType::F64, vec!["time".to_string()])?;
    time_var.set_coordinate(true);
    writer.add_variable(time_var)?;

    // Add data variables
    writer.add_variable(Variable::new(
        "elevation",
        DataType::F32,
        vec!["lat".to_string(), "lon".to_string()],
    )?)?;

    writer.add_variable(Variable::new(
        "iron_concentration",
        DataType::F32,
        vec!["lat".to_string(), "lon".to_string()],
    )?)?;

    writer.add_variable(Variable::new(
        "geology_type",
        DataType::I32,
        vec!["lat".to_string(), "lon".to_string()],
    )?)?;

    writer.add_variable(Variable::new(
        "land_use",
        DataType::I32,
        vec!["lat".to_string(), "lon".to_string()],
    )?)?;

    // Add coordinate variable attributes
    writer.add_variable_attribute(
        "lat",
        Attribute::new("standard_name", AttributeValue::text("latitude"))?,
    )?;
    writer.add_variable_attribute(
        "lat",
        Attribute::new("long_name", AttributeValue::text("Latitude"))?,
    )?;
    writer.add_variable_attribute(
        "lat",
        Attribute::new("units", AttributeValue::text("degrees_north"))?,
    )?;
    writer.add_variable_attribute("lat", Attribute::new("axis", AttributeValue::text("Y"))?)?;

    writer.add_variable_attribute(
        "lon",
        Attribute::new("standard_name", AttributeValue::text("longitude"))?,
    )?;
    writer.add_variable_attribute(
        "lon",
        Attribute::new("long_name", AttributeValue::text("Longitude"))?,
    )?;
    writer.add_variable_attribute(
        "lon",
        Attribute::new("units", AttributeValue::text("degrees_east"))?,
    )?;
    writer.add_variable_attribute("lon", Attribute::new("axis", AttributeValue::text("X"))?)?;

    writer.add_variable_attribute(
        "time",
        Attribute::new("standard_name", AttributeValue::text("time"))?,
    )?;
    writer.add_variable_attribute(
        "time",
        Attribute::new("long_name", AttributeValue::text("Time"))?,
    )?;
    writer.add_variable_attribute(
        "time",
        Attribute::new(
            "units",
            AttributeValue::text("days since 2024-01-01 00:00:00"),
        )?,
    )?;
    writer.add_variable_attribute(
        "time",
        Attribute::new("calendar", AttributeValue::text("gregorian"))?,
    )?;
    writer.add_variable_attribute("time", Attribute::new("axis", AttributeValue::text("T"))?)?;

    // Add elevation variable attributes
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("standard_name", AttributeValue::text("surface_altitude"))?,
    )?;
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("long_name", AttributeValue::text("Surface Elevation"))?,
    )?;
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("units", AttributeValue::text("m"))?,
    )?;
    writer.add_variable_attribute(
        "elevation",
        Attribute::new("_FillValue", AttributeValue::f32(-9999.0))?,
    )?;

    // Add iron concentration variable attributes
    writer.add_variable_attribute(
        "iron_concentration",
        Attribute::new("long_name", AttributeValue::text("Iron Ore Concentration"))?,
    )?;
    writer.add_variable_attribute(
        "iron_concentration",
        Attribute::new("units", AttributeValue::text("ppm"))?,
    )?;
    writer.add_variable_attribute(
        "iron_concentration",
        Attribute::new(
            "comment",
            AttributeValue::text("Parts per million of iron in geological samples"),
        )?,
    )?;
    writer.add_variable_attribute(
        "iron_concentration",
        Attribute::new("_FillValue", AttributeValue::f32(-9999.0))?,
    )?;

    // Add geology type variable attributes
    writer.add_variable_attribute(
        "geology_type",
        Attribute::new(
            "long_name",
            AttributeValue::text("Geological Formation Type"),
        )?,
    )?;
    writer.add_variable_attribute(
        "geology_type",
        Attribute::new("flag_values", AttributeValue::i32_array(vec![1, 2, 3, 4]))?,
    )?;
    writer.add_variable_attribute(
        "geology_type",
        Attribute::new(
            "flag_meanings",
            AttributeValue::text("sedimentary metamorphic igneous iron_rich"),
        )?,
    )?;
    writer.add_variable_attribute(
        "geology_type",
        Attribute::new("_FillValue", AttributeValue::i32(-9999))?,
    )?;

    // Add land use variable attributes
    writer.add_variable_attribute(
        "land_use",
        Attribute::new("long_name", AttributeValue::text("Land Use Classification"))?,
    )?;
    writer.add_variable_attribute(
        "land_use",
        Attribute::new(
            "flag_values",
            AttributeValue::i32_array(vec![1, 2, 3, 4, 5]),
        )?,
    )?;
    writer.add_variable_attribute(
        "land_use",
        Attribute::new(
            "flag_meanings",
            AttributeValue::text("urban agricultural forest mining water"),
        )?,
    )?;
    writer.add_variable_attribute(
        "land_use",
        Attribute::new("_FillValue", AttributeValue::i32(-9999))?,
    )?;

    // Add global attributes (CF Conventions)
    writer.add_global_attribute(Attribute::new(
        "Conventions",
        AttributeValue::text("CF-1.8"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "title",
        AttributeValue::text("Basque Country Geological and Environmental Data"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "institution",
        AttributeValue::text("COOLJAPAN OU (Team Kitasan)"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "source",
        AttributeValue::text("OxiGDAL NetCDF Sample Generator"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "history",
        AttributeValue::text("2024-01-01: Created with OxiGDAL"),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "references",
        AttributeValue::text("https://github.com/cool-japan/oxigdal"),
    )?)?;
    writer.add_global_attribute(
        Attribute::new("comment", AttributeValue::text("Synthetic geological and environmental data for the Basque Country region (Northern Spain)"))?,
    )?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lat_min",
        AttributeValue::f32(region.lat_min),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lat_max",
        AttributeValue::f32(region.lat_max),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lon_min",
        AttributeValue::f32(region.lon_min),
    )?)?;
    writer.add_global_attribute(Attribute::new(
        "geospatial_lon_max",
        AttributeValue::f32(region.lon_max),
    )?)?;

    // End define mode
    writer.end_define_mode()?;

    // Generate and write coordinate data
    let lat_data: Vec<f32> = (0..nlat)
        .map(|i| {
            region.lat_min + (i as f32 / (nlat - 1) as f32) * (region.lat_max - region.lat_min)
        })
        .collect();
    writer.write_f32("lat", &lat_data)?;

    let lon_data: Vec<f32> = (0..nlon)
        .map(|i| {
            region.lon_min + (i as f32 / (nlon - 1) as f32) * (region.lon_max - region.lon_min)
        })
        .collect();
    writer.write_f32("lon", &lon_data)?;

    let time_data: Vec<f64> = vec![0.0];
    writer.write_f64("time", &time_data)?;

    // Generate and write spatial data
    let mut elevation_data = Vec::with_capacity(nlat * nlon);
    let mut iron_data = Vec::with_capacity(nlat * nlon);
    let mut geology_data = Vec::with_capacity(nlat * nlon);
    let mut landuse_data = Vec::with_capacity(nlat * nlon);

    for lat in &lat_data {
        for lon in &lon_data {
            let elev = generate_elevation(*lat, *lon, 400.0, 300.0);
            let iron = generate_iron_concentration(*lat, *lon);
            let geology = generate_geology_type(*lat, *lon);
            let landuse = generate_land_use(elev, geology);

            elevation_data.push(elev);
            iron_data.push(iron);
            geology_data.push(geology);
            landuse_data.push(landuse);
        }
    }

    writer.write_f32("elevation", &elevation_data)?;
    writer.write_f32("iron_concentration", &iron_data)?;
    writer.write_i32("geology_type", &geology_data)?;
    writer.write_i32("land_use", &landuse_data)?;

    // Close and finalize
    writer.close()?;

    println!("✓ Created: {}", output_path.display());
    Ok(())
}

fn main() -> Result<()> {
    println!("=== OxiGDAL NetCDF Sample Generator ===\n");
    println!("Generating NetCDF-3 files with CF Conventions metadata");
    println!("Pure Rust implementation (no C dependencies)\n");

    // Determine output directory
    let workspace_root = std::env::current_dir().map_err(|e| {
        oxigdal_netcdf::NetCdfError::Other(format!("Failed to get current directory: {}", e))
    })?;

    let output_dir = workspace_root.join("demo").join("cog-viewer");

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(&output_dir).map_err(|e| {
        oxigdal_netcdf::NetCdfError::Other(format!("Failed to create output directory: {}", e))
    })?;

    println!("Output directory: {}\n", output_dir.display());

    // Generate Golden Triangle NetCDF
    let golden_triangle_path = output_dir.join("golden-triangle.nc");
    create_golden_triangle_netcdf(golden_triangle_path)?;

    println!();

    // Generate Basque Country NetCDF
    let iron_belt_path = output_dir.join("iron-belt.nc");
    create_iron_belt_netcdf(iron_belt_path)?;

    println!("\n=== Generation Complete ===");
    println!("\nGenerated files:");
    println!("  - demo/cog-viewer/golden-triangle.nc");
    println!("  - demo/cog-viewer/iron-belt.nc");
    println!("\nBoth files are CF-1.8 compliant and ready for use!");
    println!("\nYou can inspect them with:");
    println!("  ncdump -h demo/cog-viewer/golden-triangle.nc");
    println!("  ncdump -h demo/cog-viewer/iron-belt.nc");

    Ok(())
}
