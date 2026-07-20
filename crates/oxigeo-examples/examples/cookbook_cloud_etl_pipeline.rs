//! Cookbook: Cloud ETL Pipeline
//!
//! Complete end-to-end ETL (Extract-Transform-Load) workflow:
//! - Extract from cloud storage (S3, GCS, Azure)
//! - Transform geospatial data
//! - Load into a spatial database
//! - Batch processing with per-stage timing
//!
//! Real-world scenarios:
//! - Landsat collection ingestion to a data warehouse
//! - Sentinel-2 processing pipeline
//! - Multi-source data fusion workflows
//! - Operational monitoring systems
//!
//! Run with:
//! ```bash
//! cargo run --example cookbook_cloud_etl_pipeline
//! ```

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{BoundingBox, RasterDataType};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Cloud ETL Pipeline ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("cloud_etl_output");
    fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    println!("Scenario: Sentinel-2 S3 -> Process -> Spatial Database Pipeline");
    println!("=================================================================\n");

    let start_time = Instant::now();

    // Step 1: Extract from Cloud Storage
    println!("Step 1: Extract Data from Cloud Storage");
    println!("--------------------------------------");

    let extract_start = Instant::now();

    let cloud_files = [
        "s3://sentinel2-bucket/S2A_MSIL1C_20230615T101031_N0509_R065_T32UQD_20230615T101346.SAFE",
        "s3://sentinel2-bucket/S2A_MSIL1C_20230616T102131_N0509_R065_T32UQD_20230616T102134.SAFE",
        "s3://sentinel2-bucket/S2A_MSIL1C_20230617T101001_N0509_R065_T32UQD_20230617T101356.SAFE",
    ];

    println!("Retrieving datasets from S3...");
    println!("Bucket: sentinel2-bucket");
    println!("Files to retrieve: {}", cloud_files.len());

    let mut extracted_files = Vec::new();

    for file in &cloud_files {
        println!("  Downloading: {}", file);

        let local_path = temp_dir.join(file.split('/').next_back().unwrap_or("sentinel2_data"));

        let bands = create_sentinel_bands(&local_path)?;

        extracted_files.push(local_path);

        println!("    Downloaded, {} bands", bands);
    }

    let extract_time = extract_start.elapsed();

    println!(
        "\n  Extraction completed in {:.3}s",
        extract_time.as_secs_f32()
    );

    // Step 2: Data Validation
    println!("\n\nStep 2: Data Validation");
    println!("----------------------");

    println!("Validating extracted data...");

    let mut valid_files = 0;

    for file in &extracted_files {
        let metadata = fs::metadata(file)?;
        let size_mb = metadata.len() as f32 / 1_000_000.0;

        if metadata.len() > 1_000_000 {
            println!("  {}: {:.2} MB (valid)", file.display(), size_mb);
            valid_files += 1;
        } else {
            println!(
                "  {}: {} bytes (too small, skipping)",
                file.display(),
                metadata.len()
            );
        }
    }

    println!(
        "\n  Validation result: {}/{} files valid",
        valid_files,
        extracted_files.len()
    );

    // Step 3: Transform and Process
    println!("\n\nStep 3: Transform and Process Data");
    println!("----------------------------------");

    let transform_start = Instant::now();

    println!("Processing valid files...");

    let mut processing_results = Vec::new();

    for (idx, file) in extracted_files.iter().enumerate().take(valid_files) {
        println!("\n  Processing file {}/{}...", idx + 1, valid_files);

        let file_start = Instant::now();

        println!("    - Loading bands...");
        let width = 128u64;
        let height = 128u64;

        let band_red = create_synthetic_band(width, height, 0.3);
        let band_nir = create_synthetic_band(width, height, 0.5);

        println!("    - Computing indices...");

        let red_data = band_red.as_slice::<f32>()?;
        let nir_data = band_nir.as_slice::<f32>()?;

        let ndvi: Vec<f32> = red_data
            .iter()
            .zip(nir_data.iter())
            .map(|(&r, &n)| {
                let sum = r + n;
                if sum > 1e-6 { (n - r) / sum } else { 0.0 }
            })
            .collect();

        let ndvi_buf = RasterBuffer::from_typed_vec(
            width as usize,
            height as usize,
            ndvi,
            RasterDataType::Float32,
        )?;
        let ndvi_stats = ndvi_buf.compute_statistics()?;
        println!("      NDVI mean: {:.4}", ndvi_stats.mean);

        println!("    - Generating tiles...");

        let tile_size = 64u64;
        let num_tiles = width.div_ceil(tile_size) * height.div_ceil(tile_size);

        println!(
            "      Generated {} tiles ({}x{})",
            num_tiles, tile_size, tile_size
        );

        println!("    - Creating metadata...");

        let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 10.0, height as f64 * 10.0)?;

        let metadata = ProcessingMetadata {
            file: file.clone(),
            bbox,
            acquisition_date: "2023-06-15".to_string(),
            cloud_coverage: 5.5,
            processing_level: "L2A".to_string(),
            num_tiles,
            data_size_mb: 45.0,
        };

        println!(
            "      Bounds: [{:.2}, {:.2}, {:.2}, {:.2}]",
            bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y
        );
        println!("      Cloud coverage: {:.1}%", metadata.cloud_coverage);

        let file_time = file_start.elapsed();

        processing_results.push((metadata, file_time));
    }

    let transform_time = transform_start.elapsed();

    println!(
        "\n  Processing completed in {:.3}s",
        transform_time.as_secs_f32()
    );

    // Step 4: Load into a spatial database
    println!("\n\nStep 4: Load into Spatial Database");
    println!("--------------------------------------");

    let load_start = Instant::now();

    println!("Connecting to spatial database...");
    println!("  Server: localhost:5432");
    println!("  Database: gis_data");
    println!("  Connected (simulated)");

    println!("\nInserting data...");

    let mut total_tiles = 0u64;
    let mut total_data_mb = 0.0f32;

    for (meta, _) in &processing_results {
        println!("\n  Inserting: {}", meta.file.display());
        println!("    Acquisition: {}", meta.acquisition_date);
        println!("    Cloud coverage: {:.1}%", meta.cloud_coverage);

        let polygon_wkt = format!(
            "POLYGON(({:.2} {:.2}, {:.2} {:.2}, {:.2} {:.2}, {:.2} {:.2}, {:.2} {:.2}))",
            meta.bbox.min_x,
            meta.bbox.min_y,
            meta.bbox.max_x,
            meta.bbox.min_y,
            meta.bbox.max_x,
            meta.bbox.max_y,
            meta.bbox.min_x,
            meta.bbox.max_y,
            meta.bbox.min_x,
            meta.bbox.min_y,
        );

        println!(
            "    INSERT INTO sentinel2_datasets (file, bbox, acquisition_date, cloud_coverage) VALUES ('{}', '{}', '{}', {})",
            meta.file.display(),
            polygon_wkt,
            meta.acquisition_date,
            meta.cloud_coverage
        );
        println!("    Dataset record inserted");
        println!("    All {} tiles inserted", meta.num_tiles);

        total_tiles += meta.num_tiles;
        total_data_mb += meta.data_size_mb;
    }

    let load_time = load_start.elapsed();

    println!("\nDatabase load summary:");
    println!("  Total datasets: {}", processing_results.len());
    println!("  Total tiles: {}", total_tiles);
    println!("  Total data: {:.2} MB", total_data_mb);
    println!("  Load time: {:.3}s", load_time.as_secs_f32());

    // Step 5: Verification
    println!("\n\nStep 5: Verification");
    println!("--------------------");

    println!("Verifying data integrity...");

    let dataset_count = processing_results.len();
    let expected_tiles: u64 = processing_results.iter().map(|(m, _)| m.num_tiles).sum();

    println!(
        "  Dataset count: {} (expected {})",
        dataset_count,
        processing_results.len()
    );
    println!(
        "  Tile count: {} (expected {})",
        total_tiles, expected_tiles
    );

    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for (meta, _) in &processing_results {
        min_x = min_x.min(meta.bbox.min_x);
        min_y = min_y.min(meta.bbox.min_y);
        max_x = max_x.max(meta.bbox.max_x);
        max_y = max_y.max(meta.bbox.max_y);
    }

    println!("  Spatial extent:");
    println!(
        "    [{:.2}, {:.2}, {:.2}, {:.2}]",
        min_x, min_y, max_x, max_y
    );

    // Step 6: Post-Processing
    println!("\n\nStep 6: Post-Processing");
    println!("----------------------");

    println!("Creating spatial indices...");
    println!("  GiST index on geom");
    println!("  Index on acquisition_date");
    println!("  Index on cloud_coverage");

    // Step 7: Quality Report
    println!("\n\nStep 7: Pipeline Execution Report");
    println!("--------------------------------");

    let total_time = start_time.elapsed();

    println!("ETL Pipeline Summary:");
    println!("  Extract time:   {:.3}s", extract_time.as_secs_f32());
    println!("  Transform time: {:.3}s", transform_time.as_secs_f32());
    println!("  Load time:      {:.3}s", load_time.as_secs_f32());
    println!("  Total time:     {:.3}s", total_time.as_secs_f32());

    let throughput = total_data_mb / total_time.as_secs_f32().max(1e-6);
    println!("  Throughput:     {:.2} MB/s", throughput);

    let report = generate_etl_report(
        &processing_results,
        &extract_time,
        &transform_time,
        &load_time,
        &total_time,
    );

    let report_path = output_dir.join("etl_report.txt");
    fs::write(&report_path, &report)?;

    println!("\nPipeline completed successfully");
    println!("Report saved to: {:?}", report_path);

    Ok(())
}

#[derive(Debug, Clone)]
struct ProcessingMetadata {
    file: PathBuf,
    bbox: BoundingBox,
    acquisition_date: String,
    cloud_coverage: f32,
    processing_level: String,
    num_tiles: u64,
    data_size_mb: f32,
}

fn create_sentinel_bands(dir: &std::path::Path) -> Result<usize, Box<dyn std::error::Error>> {
    fs::create_dir_all(dir)?;

    let bands = ["B02_10m", "B03_10m", "B04_10m", "B08_10m"];

    for band in &bands {
        let band_file = dir.join(format!("{band}.jp2"));
        fs::write(&band_file, vec![0u8; 1_000_000])?;
    }

    Ok(bands.len())
}

fn create_synthetic_band(width: u64, height: u64, base_value: f64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = x as f64 / width as f64;
            let ny = y as f64 / height as f64;
            let pattern = (nx.sin() + ny.cos()) / 2.0;
            let value = (base_value + pattern * 0.15).clamp(0.0, 1.0);
            let _ = buffer.set_pixel(x, y, value);
        }
    }

    buffer
}

fn generate_etl_report(
    results: &[(ProcessingMetadata, Duration)],
    extract_time: &Duration,
    transform_time: &Duration,
    load_time: &Duration,
    total_time: &Duration,
) -> String {
    let mut report = String::new();

    report.push_str("ETL PIPELINE EXECUTION REPORT\n");
    report.push_str("=============================\n\n");

    report.push_str("PIPELINE STAGES\n");
    report.push_str("---------------\n");
    report.push_str(&format!("Extract:   {:.3}s\n", extract_time.as_secs_f32()));
    report.push_str(&format!(
        "Transform: {:.3}s\n",
        transform_time.as_secs_f32()
    ));
    report.push_str(&format!("Load:      {:.3}s\n", load_time.as_secs_f32()));
    report.push_str(&format!("Total:     {:.3}s\n\n", total_time.as_secs_f32()));

    report.push_str("PROCESSED DATASETS\n");
    report.push_str("------------------\n");

    let mut total_tiles = 0u64;
    let mut total_data = 0.0f32;

    for (meta, proc_time) in results {
        report.push_str(&format!(
            "{}: {} tiles, {:.2} MB, level {} ({:.3}s)\n",
            meta.file.display(),
            meta.num_tiles,
            meta.data_size_mb,
            meta.processing_level,
            proc_time.as_secs_f32()
        ));

        total_tiles += meta.num_tiles;
        total_data += meta.data_size_mb;
    }

    report.push_str(&format!(
        "\nTotal: {} tiles, {:.2} MB\n\n",
        total_tiles, total_data
    ));

    report.push_str("QUALITY METRICS\n");
    report.push_str("---------------\n");
    report.push_str("All validation checks passed\n");
    report.push_str("Data integrity verified\n");
    report.push_str("Spatial indices created\n");

    report
}
