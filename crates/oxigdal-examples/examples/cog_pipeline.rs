//! Comprehensive Cloud Optimized GeoTIFF (COG) Generation Pipeline
//!
//! This example demonstrates a production-style COG generation pipeline:
//! - Synthetic input dataset generation (standing in for a real input archive)
//! - Automatic tile size optimization based on dataset size
//! - Compression algorithm comparison
//! - Internal overview generation
//! - Validation against the COG specification (via `CogWriter::write`)
//! - Parallel batch processing with Rayon
//! - JSON processing report
//!
//! Run with:
//! ```bash
//! cargo run --example cog_pipeline --release
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};
use rayon::prelude::*;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, info, warn};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("cog_pipeline=info")
        .init();

    info!("Starting COG Generation Pipeline");

    let output_dir = std::env::temp_dir().join("cog_pipeline_output");
    std::fs::create_dir_all(&output_dir)?;

    let config = PipelineConfig {
        output_dir: output_dir.clone(),
        compression_candidates: vec![
            Compression::Lzw,
            Compression::Deflate,
            Compression::Packbits,
        ],
        overview_levels: vec![2, 4, 8, 16],
        parallel_jobs: rayon::current_num_threads(),
        generate_report: true,
    };

    // Step 1: Generate (or discover) input datasets
    info!("Step 1: Preparing input datasets");

    let input_datasets = generate_input_datasets(6)?;
    info!(
        "  Prepared {} synthetic input datasets",
        input_datasets.len()
    );

    // Step 2: Analyze input datasets
    info!("Step 2: Analyzing input datasets");

    let mut file_info = Vec::new();
    for (idx, dataset) in input_datasets.iter().enumerate() {
        info!(
            "  [{}/{}] {} -> {}x{}, {:?}",
            idx + 1,
            input_datasets.len(),
            dataset.name,
            dataset.buffer.width(),
            dataset.buffer.height(),
            dataset.buffer.data_type()
        );
        file_info.push(dataset.clone());
    }

    // Step 3: Optimize tile size based on dataset characteristics
    info!("Step 3: Optimizing tile sizes");

    let optimized: Vec<(InputDataset, u32)> = file_info
        .into_iter()
        .map(|dataset| {
            let tile_size = optimal_tile_size(dataset.buffer.width(), dataset.buffer.height());
            info!("  {} -> tile size: {}", dataset.name, tile_size);
            (dataset, tile_size)
        })
        .collect();

    // Step 4: Compare compression algorithms on the first dataset
    if config.compression_candidates.len() > 1 && !optimized.is_empty() {
        info!("Step 4: Comparing compression algorithms on sample dataset");

        let (sample, tile_size) = &optimized[0];
        let compression_results = compare_compressions(sample, *tile_size, &config)?;

        for result in &compression_results {
            info!(
                "    {:?}: {:.3} MB, time: {:.3}s",
                result.compression,
                result.output_size_mb,
                result.processing_time.as_secs_f64()
            );
        }

        if let Some(best) = compression_results.iter().min_by(|a, b| {
            a.output_size_mb
                .partial_cmp(&b.output_size_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            info!("  Selected compression: {:?}", best.compression);
        }
    } else {
        info!("Step 4: Skipping compression comparison");
    }

    // Step 5: Process datasets in parallel
    info!(
        "Step 5: Processing {} datasets with {} parallel jobs",
        optimized.len(),
        config.parallel_jobs
    );

    let start_time = Instant::now();

    let processing_results: Vec<ProcessingResult> = optimized
        .par_iter()
        .map(|(dataset, tile_size)| process_single_dataset(dataset, *tile_size, &config))
        .collect();

    let total_time = start_time.elapsed();

    // Step 6: Report validation status (already computed during writing)
    info!("Step 6: COG validation results");

    for result in &processing_results {
        match &result.output_path {
            Ok(path) => {
                if result.cog_valid {
                    info!("  Valid COG: {}", path.display());
                } else {
                    warn!("  Invalid COG: {}", path.display());
                    for issue in &result.validation_messages {
                        warn!("    - {}", issue);
                    }
                }
            }
            Err(e) => error!("  Failed: {}", e),
        }
    }

    // Step 7: Generate processing report
    if config.generate_report {
        info!("Step 7: Generating processing report");

        let report = generate_report(&processing_results, total_time);

        let report_path = config.output_dir.join("processing_report.json");
        let report_json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&report_path, report_json)?;

        info!("  Report saved to: {}", report_path.display());

        info!("");
        info!("=== Processing Summary ===");
        info!("  Total datasets: {}", report.total_files);
        info!("  Successful: {}", report.successful);
        info!("  Failed: {}", report.failed);
        info!("  Total output size: {:.3} MB", report.total_output_size_mb);
        info!("  Total processing time: {:.3}s", report.total_time_secs);
        info!(
            "  Average time per dataset: {:.3}s",
            report.avg_time_per_file_secs
        );
    }

    info!("");
    info!("COG pipeline completed successfully!");

    Ok(())
}

/// Pipeline configuration
#[derive(Debug, Clone)]
struct PipelineConfig {
    output_dir: PathBuf,
    compression_candidates: Vec<Compression>,
    overview_levels: Vec<u32>,
    parallel_jobs: usize,
    generate_report: bool,
}

/// A synthetic input dataset (standing in for a real file discovered on disk / cloud storage)
#[derive(Clone)]
struct InputDataset {
    name: String,
    buffer: RasterBuffer,
    geo_transform: GeoTransform,
}

#[derive(Debug)]
struct ProcessingResult {
    name: String,
    output_path: Result<PathBuf, String>,
    processing_time: std::time::Duration,
    output_size_mb: Option<f64>,
    cog_valid: bool,
    validation_messages: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct ProcessingReport {
    total_files: usize,
    successful: usize,
    failed: usize,
    total_output_size_mb: f64,
    total_time_secs: f64,
    avg_time_per_file_secs: f64,
    files: Vec<FileReport>,
}

#[derive(Debug, serde::Serialize)]
struct FileReport {
    name: String,
    output_file: Option<String>,
    success: bool,
    output_size_mb: Option<f64>,
    cog_valid: bool,
    processing_time_secs: f64,
}

/// Generate a handful of synthetic input rasters at varying resolutions
fn generate_input_datasets(count: u64) -> Result<Vec<InputDataset>, Box<dyn std::error::Error>> {
    (0..count)
        .map(|i| {
            let size = 128 + i * 64; // vary resolution across datasets
            let mut buffer = RasterBuffer::zeros(size, size, RasterDataType::Float32);

            for y in 0..size {
                for x in 0..size {
                    let nx = x as f64 / size as f64;
                    let ny = y as f64 / size as f64;
                    let value =
                        ((nx * 6.0).sin() + (ny * 6.0).cos() + i as f64 * 0.1) * 500.0 + 1000.0;
                    let _ = buffer.set_pixel(x, y, value);
                }
            }

            let bbox = BoundingBox::new(-120.0 + i as f64, 35.0, -120.0 + i as f64 + 1.0, 36.0)?;
            let geo_transform = GeoTransform::from_bounds(&bbox, size, size)?;

            Ok(InputDataset {
                name: format!("scene_{i:02}"),
                buffer,
                geo_transform,
            })
        })
        .collect()
}

/// Choose a tile size based on the dataset's pixel count
fn optimal_tile_size(width: u64, height: u64) -> u32 {
    let pixels = width * height;
    if pixels < 100_000 {
        128
    } else if pixels < 1_000_000 {
        256
    } else {
        512
    }
}

struct CompressionResult {
    compression: Compression,
    output_size_mb: f64,
    processing_time: std::time::Duration,
}

/// Compare different compression algorithms on a sample dataset
fn compare_compressions(
    sample: &InputDataset,
    tile_size: u32,
    config: &PipelineConfig,
) -> Result<Vec<CompressionResult>, Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir().join("cog_compression_test");
    std::fs::create_dir_all(&temp_dir)?;

    let mut results = Vec::new();

    for &compression in &config.compression_candidates {
        let output_path = temp_dir.join(format!("test_{compression:?}.tif"));

        let start_time = Instant::now();

        let writer_config = WriterConfig::new(
            sample.buffer.width(),
            sample.buffer.height(),
            1,
            sample.buffer.data_type(),
        )
        .with_compression(compression)
        .with_tile_size(tile_size, tile_size)
        .with_geo_transform(sample.geo_transform);

        let mut writer =
            CogWriter::create(&output_path, writer_config, CogWriterOptions::default())?;
        writer.write(sample.buffer.as_bytes())?;

        let processing_time = start_time.elapsed();

        let metadata = std::fs::metadata(&output_path)?;
        let output_size_mb = metadata.len() as f64 / 1_000_000.0;

        results.push(CompressionResult {
            compression,
            output_size_mb,
            processing_time,
        });

        std::fs::remove_file(&output_path)?;
    }

    std::fs::remove_dir_all(&temp_dir)?;

    Ok(results)
}

/// Process a single dataset into a validated COG
fn process_single_dataset(
    dataset: &InputDataset,
    tile_size: u32,
    config: &PipelineConfig,
) -> ProcessingResult {
    let start_time = Instant::now();

    let output_path = config.output_dir.join(format!("{}_cog.tif", dataset.name));

    let writer_config = WriterConfig::new(
        dataset.buffer.width(),
        dataset.buffer.height(),
        1,
        dataset.buffer.data_type(),
    )
    .with_compression(Compression::Lzw)
    .with_tile_size(tile_size, tile_size)
    .with_geo_transform(dataset.geo_transform)
    .with_epsg_code(4326)
    .with_overviews(true, OverviewResampling::Average)
    .with_overview_levels(config.overview_levels.clone());

    let result = CogWriter::create(&output_path, writer_config, CogWriterOptions::default())
        .and_then(|mut writer| writer.write(dataset.buffer.as_bytes()));

    let processing_time = start_time.elapsed();

    match result {
        Ok(validation) => {
            let output_size_mb = std::fs::metadata(&output_path)
                .map(|m| m.len() as f64 / 1_000_000.0)
                .ok();

            ProcessingResult {
                name: dataset.name.clone(),
                output_path: Ok(output_path),
                processing_time,
                output_size_mb,
                cog_valid: validation.is_valid,
                validation_messages: validation.messages,
            }
        }
        Err(e) => ProcessingResult {
            name: dataset.name.clone(),
            output_path: Err(e.to_string()),
            processing_time,
            output_size_mb: None,
            cog_valid: false,
            validation_messages: Vec::new(),
        },
    }
}

/// Generate a processing report
fn generate_report(
    results: &[ProcessingResult],
    total_time: std::time::Duration,
) -> ProcessingReport {
    let successful = results.iter().filter(|r| r.output_path.is_ok()).count();
    let failed = results.len() - successful;

    let total_output_size_mb: f64 = results.iter().filter_map(|r| r.output_size_mb).sum();

    let files = results
        .iter()
        .map(|r| FileReport {
            name: r.name.clone(),
            output_file: r.output_path.as_ref().ok().map(|p| p.display().to_string()),
            success: r.output_path.is_ok(),
            output_size_mb: r.output_size_mb,
            cog_valid: r.cog_valid,
            processing_time_secs: r.processing_time.as_secs_f64(),
        })
        .collect();

    ProcessingReport {
        total_files: results.len(),
        successful,
        failed,
        total_output_size_mb,
        total_time_secs: total_time.as_secs_f64(),
        avg_time_per_file_secs: total_time.as_secs_f64() / results.len().max(1) as f64,
        files,
    }
}
