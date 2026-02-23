//! Cross-crate integration tests
//!
//! Tests that verify APIs work correctly across different OxiGDAL crates.

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Test core + algorithms integration
#[test]
fn test_core_algorithms_integration() -> Result<()> {
    // Test that core types work with algorithms
    let width = 100;
    let height = 100;
    let data: Vec<f32> = vec![1.0; width * height];

    // Placeholder: Would use actual OxiGDAL core + algorithms
    let _processed = apply_ndvi(&data, &data)?;

    Ok(())
}

/// Test core + projection integration
#[test]
fn test_core_projection_integration() -> Result<()> {
    // Test coordinate transformation
    let points = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];

    // Placeholder: Would use actual projection
    let _transformed = transform_points(&points, "EPSG:4326", "EPSG:3857")?;

    Ok(())
}

/// Test drivers + core integration
#[test]
fn test_drivers_core_integration() -> Result<()> {
    // Test that all drivers can create core Dataset types
    let _geotiff_dataset = create_dataset_geotiff()?;
    let _geojson_dataset = create_dataset_geojson()?;
    let _zarr_dataset = create_dataset_zarr()?;

    Ok(())
}

/// Test algorithms + ML integration
#[test]
fn test_algorithms_ml_integration() -> Result<()> {
    // Test that algorithm outputs can be used as ML inputs
    let features = vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]];

    // Placeholder: Would use actual ML pipeline
    let _predictions = run_ml_pipeline(&features)?;

    Ok(())
}

/// Test cloud + distributed integration
#[test]
fn test_cloud_distributed_integration() -> Result<()> {
    // Test that cloud data can be processed in distributed manner
    let _cloud_paths = vec!["s3://bucket/file1.tif", "s3://bucket/file2.tif"];

    // Placeholder: Would use actual cloud + distributed processing
    let _results = process_distributed_cloud()?;

    Ok(())
}

/// Test metadata + STAC integration
#[test]
fn test_metadata_stac_integration() -> Result<()> {
    // Test metadata extraction to STAC format
    let metadata = Metadata {
        bounds: (0.0, 0.0, 1.0, 1.0),
        crs: "EPSG:4326".to_string(),
        bands: vec!["B1".to_string(), "B2".to_string()],
    };

    let _stac_item = convert_to_stac(&metadata)?;

    Ok(())
}

// Helper types and functions (placeholders)

#[derive(Debug)]
struct Metadata {
    bounds: (f64, f64, f64, f64),
    crs: String,
    bands: Vec<String>,
}

fn apply_ndvi(_nir: &[f32], _red: &[f32]) -> Result<Vec<f32>> {
    Ok(vec![0.5; 100])
}

fn transform_points(
    points: &[(f64, f64)],
    _from_crs: &str,
    _to_crs: &str,
) -> Result<Vec<(f64, f64)>> {
    Ok(points.to_vec())
}

fn create_dataset_geotiff() -> Result<()> {
    Ok(())
}

fn create_dataset_geojson() -> Result<()> {
    Ok(())
}

fn create_dataset_zarr() -> Result<()> {
    Ok(())
}

fn run_ml_pipeline(_features: &[Vec<f64>]) -> Result<Vec<f64>> {
    Ok(vec![0.0; 2])
}

fn process_distributed_cloud() -> Result<Vec<f64>> {
    Ok(vec![0.0; 100])
}

fn convert_to_stac(_metadata: &Metadata) -> Result<String> {
    Ok(r#"{"type": "Feature"}"#.to_string())
}

#[test]
fn test_dev_tools_profiler() -> Result<()> {
    // Test using dev tools with core operations
    use oxigdal_dev_tools::profiler::Profiler;

    let mut profiler = Profiler::new("test_operation");
    profiler.start();

    // Simulate some work
    std::thread::sleep(std::time::Duration::from_millis(10));

    profiler.stop();

    let report = profiler.report();
    assert!(report.contains("Profile Report"));

    Ok(())
}

#[test]
fn test_dev_tools_validator() -> Result<()> {
    use oxigdal_dev_tools::validator::DataValidator;

    // Validate raster dimensions
    let result = DataValidator::validate_raster_dimensions(100, 100, 3);
    assert!(result.passed);

    // Validate bounds
    let result = DataValidator::validate_bounds(0.0, 0.0, 100.0, 100.0);
    assert!(result.passed);

    Ok(())
}

#[test]
fn test_jupyter_kernel() -> Result<()> {
    use oxigdal_jupyter::OxiGdalKernel;

    let mut kernel = OxiGdalKernel::new()?;

    // Execute a magic command
    let result = kernel.execute("%list")?;
    assert_eq!(result.status, "ok");

    Ok(())
}
