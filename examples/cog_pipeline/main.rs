//! Comprehensive Cloud Optimized GeoTIFF (COG) Generation Pipeline
//!
//! This example demonstrates a production-ready COG generation pipeline including:
//! - Multi-format input support (GeoTIFF, JPEG2000, HDF5, NetCDF)
//! - Automatic tile size optimization
//! - Multiple compression algorithms comparison
//! - Internal/external overview generation
//! - Validation against COG specification
//! - Direct cloud storage upload (S3, Azure, GCS)
//! - Batch processing with parallel execution
//! - COG validation and statistics

use oxigdal_core::{Dataset, DataType};
use oxigdal_geotiff::{GeoTiffDriver, CogOptions, CompressionType};
use oxigdal_cloud::{S3Backend, AzureBackend, GcsBackend, CloudBackend};
use oxigdal_algorithms::Statistics;
use oxigdal_qc::CogValidator;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, warn, error};
use rayon::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("cog_pipeline=info")
        .init();

    info!("Starting COG Generation Pipeline");

    // Pipeline configuration
    let config = PipelineConfig {
        input_pattern: "data/input/**/*.{tif,tiff,jp2,h5,nc}",
        output_dir: PathBuf::from("output/cogs"),
        tile_size: TileSizeStrategy::Auto,
        compression: vec![
            CompressionConfig::Deflate { level: 6 },
            CompressionConfig::Zstd { level: 9 },
            CompressionConfig::Webp { quality: 90 },
        ],
        overview_strategy: OverviewStrategy::Internal,
        overview_levels: vec![2, 4, 8, 16, 32],
        resampling_method: "CUBIC".to_string(),
        validate_output: true,
        cloud_upload: CloudUploadConfig {
            enabled: false,
            provider: CloudProvider::S3,
            bucket: "my-cog-bucket".to_string(),
            prefix: "cogs/".to_string(),
        },
        parallel_jobs: num_cpus::get(),
        generate_report: true,
    };

    // Step 1: Discover input files
    info!("Step 1: Discovering input files matching pattern: {}", config.input_pattern);

    let input_files = discover_input_files(&config.input_pattern)?;
    info!("  Found {} files to process", input_files.len());

    if input_files.is_empty() {
        warn!("No input files found. Exiting.");
        return Ok(());
    }

    // Step 2: Analyze input files
    info!("Step 2: Analyzing input datasets");

    let mut file_info = Vec::new();
    for (idx, path) in input_files.iter().enumerate() {
        info!("  [{}/{}] Analyzing: {}", idx + 1, input_files.len(), path.display());

        match analyze_dataset(path).await {
            Ok(info) => {
                info!("    Size: {}x{}, Bands: {}, Type: {:?}, Size: {:.2} MB",
                      info.width, info.height, info.bands, info.data_type,
                      info.file_size_mb);
                file_info.push((path.clone(), info));
            }
            Err(e) => {
                error!("    Failed to analyze: {}", e);
            }
        }
    }

    // Step 3: Optimize tile size based on dataset characteristics
    info!("Step 3: Optimizing tile sizes");

    let optimized_configs = optimize_tile_sizes(&file_info, &config)?;

    for (path, tile_size, _) in &optimized_configs {
        info!("  {} -> tile size: {}", path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown"), tile_size);
    }

    // Step 4: Compare compression algorithms (on first file as sample)
    if config.compression.len() > 1 && !file_info.is_empty() {
        info!("Step 4: Comparing compression algorithms on sample file");

        let (sample_path, sample_info) = &file_info[0];
        let compression_results = compare_compressions(
            sample_path,
            &config.compression,
            &config,
        ).await?;

        info!("  Compression comparison:");
        for result in &compression_results {
            info!("    {:?}: {:.2} MB, ratio: {:.1}%, time: {:.2}s",
                  result.compression,
                  result.output_size_mb,
                  result.compression_ratio * 100.0,
                  result.processing_time.as_secs_f64());
        }

        // Select best compression based on size/time tradeoff
        let best = compression_results.iter()
            .min_by(|a, b| {
                let score_a = a.output_size_mb * (1.0 + a.processing_time.as_secs_f64() / 10.0);
                let score_b = b.output_size_mb * (1.0 + b.processing_time.as_secs_f64() / 10.0);
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or("No compression results")?;

        info!("  Selected compression: {:?}", best.compression);
    } else {
        info!("Step 4: Skipping compression comparison");
    }

    // Step 5: Process files in parallel
    info!("Step 5: Processing {} files with {} parallel jobs",
          optimized_configs.len(), config.parallel_jobs);

    std::fs::create_dir_all(&config.output_dir)?;

    let start_time = Instant::now();

    let processing_results: Vec<ProcessingResult> = optimized_configs
        .par_iter()
        .map(|(input_path, tile_size, dataset_info)| {
            process_single_file(
                input_path,
                &config,
                *tile_size,
                dataset_info,
            )
        })
        .collect();

    let total_time = start_time.elapsed();

    // Step 6: Validate outputs
    if config.validate_output {
        info!("Step 6: Validating COG outputs");

        let validator = CogValidator::new();

        for result in &processing_results {
            if let Ok(ref output_path) = result.output_path {
                info!("  Validating: {}", output_path.display());

                match validator.validate_cog(output_path).await {
                    Ok(validation) => {
                        if validation.is_valid {
                            info!("    ✓ Valid COG");
                        } else {
                            warn!("    ✗ Invalid COG:");
                            for issue in &validation.issues {
                                warn!("      - {}", issue);
                            }
                        }
                    }
                    Err(e) => {
                        error!("    Validation failed: {}", e);
                    }
                }
            }
        }
    } else {
        info!("Step 6: Skipping validation");
    }

    // Step 7: Upload to cloud storage
    if config.cloud_upload.enabled {
        info!("Step 7: Uploading COGs to cloud storage");

        let cloud_backend = create_cloud_backend(&config.cloud_upload).await?;

        for result in &processing_results {
            if let Ok(ref output_path) = result.output_path {
                let key = format!("{}{}",
                    config.cloud_upload.prefix,
                    output_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                );

                info!("  Uploading: {} -> {}", output_path.display(), key);

                match cloud_backend.upload_file(output_path, &key).await {
                    Ok(_) => info!("    ✓ Uploaded successfully"),
                    Err(e) => error!("    ✗ Upload failed: {}", e),
                }
            }
        }
    } else {
        info!("Step 7: Skipping cloud upload");
    }

    // Step 8: Generate processing report
    if config.generate_report {
        info!("Step 8: Generating processing report");

        let report = generate_report(
            &processing_results,
            &config,
            total_time,
        )?;

        let report_path = config.output_dir.join("processing_report.json");
        let report_json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&report_path, report_json)?;

        info!("  Report saved to: {}", report_path.display());

        // Print summary
        info!("");
        info!("=== Processing Summary ===");
        info!("  Total files: {}", report.total_files);
        info!("  Successful: {}", report.successful);
        info!("  Failed: {}", report.failed);
        info!("  Total input size: {:.2} GB", report.total_input_size_gb);
        info!("  Total output size: {:.2} GB", report.total_output_size_gb);
        info!("  Average compression ratio: {:.1}%", report.avg_compression_ratio * 100.0);
        info!("  Total processing time: {:.2}s", report.total_time_secs);
        info!("  Average time per file: {:.2}s", report.avg_time_per_file_secs);
    }

    info!("");
    info!("✓ COG pipeline completed successfully!");

    Ok(())
}

/// Configuration for the COG generation pipeline
#[derive(Debug, Clone)]
struct PipelineConfig {
    input_pattern: &'static str,
    output_dir: PathBuf,
    tile_size: TileSizeStrategy,
    compression: Vec<CompressionConfig>,
    overview_strategy: OverviewStrategy,
    overview_levels: Vec<u32>,
    resampling_method: String,
    validate_output: bool,
    cloud_upload: CloudUploadConfig,
    parallel_jobs: usize,
    generate_report: bool,
}

#[derive(Debug, Clone)]
enum TileSizeStrategy {
    Auto,
    Fixed(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CompressionConfig {
    None,
    Deflate { level: u8 },
    Zstd { level: i32 },
    Lzw,
    Webp { quality: u8 },
    Jpeg { quality: u8 },
}

#[derive(Debug, Clone)]
enum OverviewStrategy {
    Internal,
    External,
    Both,
}

#[derive(Debug, Clone)]
struct CloudUploadConfig {
    enabled: bool,
    provider: CloudProvider,
    bucket: String,
    prefix: String,
}

#[derive(Debug, Clone)]
enum CloudProvider {
    S3,
    Azure,
    Gcs,
}

#[derive(Debug)]
struct DatasetInfo {
    width: u32,
    height: u32,
    bands: u32,
    data_type: DataType,
    file_size_mb: f64,
    has_overviews: bool,
    has_geotransform: bool,
    has_projection: bool,
}

#[derive(Debug)]
struct ProcessingResult {
    input_path: PathBuf,
    output_path: Result<PathBuf, String>,
    processing_time: std::time::Duration,
    input_size_mb: f64,
    output_size_mb: Option<f64>,
    compression_ratio: Option<f64>,
}

#[derive(Debug)]
struct CompressionResult {
    compression: CompressionConfig,
    output_size_mb: f64,
    compression_ratio: f64,
    processing_time: std::time::Duration,
}

#[derive(Debug, serde::Serialize)]
struct ProcessingReport {
    total_files: usize,
    successful: usize,
    failed: usize,
    total_input_size_gb: f64,
    total_output_size_gb: f64,
    avg_compression_ratio: f64,
    total_time_secs: f64,
    avg_time_per_file_secs: f64,
    files: Vec<FileReport>,
}

#[derive(Debug, serde::Serialize)]
struct FileReport {
    input_file: String,
    output_file: Option<String>,
    success: bool,
    input_size_mb: f64,
    output_size_mb: Option<f64>,
    compression_ratio: Option<f64>,
    processing_time_secs: f64,
}

/// Discover input files matching the pattern
fn discover_input_files(pattern: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let paths = glob::glob(pattern)?
        .filter_map(|result| result.ok())
        .collect();

    Ok(paths)
}

/// Analyze a dataset and extract metadata
async fn analyze_dataset(path: &Path) -> Result<DatasetInfo, Box<dyn std::error::Error>> {
    let dataset = Dataset::open(path).await?;

    let metadata = std::fs::metadata(path)?;
    let file_size_mb = metadata.len() as f64 / 1_000_000.0;

    Ok(DatasetInfo {
        width: dataset.width(),
        height: dataset.height(),
        bands: dataset.band_count(),
        data_type: dataset.band(1)?.data_type(),
        file_size_mb,
        has_overviews: dataset.overview_count()? > 0,
        has_geotransform: dataset.geo_transform().is_ok(),
        has_projection: dataset.spatial_ref().is_ok(),
    })
}

/// Optimize tile sizes based on dataset characteristics
fn optimize_tile_sizes(
    file_info: &[(PathBuf, DatasetInfo)],
    config: &PipelineConfig,
) -> Result<Vec<(PathBuf, u32, DatasetInfo)>, Box<dyn std::error::Error>> {
    let mut result = Vec::new();

    for (path, info) in file_info {
        let tile_size = match config.tile_size {
            TileSizeStrategy::Auto => {
                // Optimize based on image size
                let pixels = info.width * info.height;
                if pixels < 1_000_000 {
                    256
                } else if pixels < 10_000_000 {
                    512
                } else {
                    1024
                }
            }
            TileSizeStrategy::Fixed(size) => size,
        };

        result.push((path.clone(), tile_size, info.clone()));
    }

    Ok(result)
}

/// Compare different compression algorithms
async fn compare_compressions(
    input_path: &Path,
    compressions: &[CompressionConfig],
    config: &PipelineConfig,
) -> Result<Vec<CompressionResult>, Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir().join("cog_compression_test");
    std::fs::create_dir_all(&temp_dir)?;

    let dataset = Dataset::open(input_path).await?;
    let driver = GeoTiffDriver::new();

    let mut results = Vec::new();

    for compression in compressions {
        let output_path = temp_dir.join(format!("test_{:?}.tif", compression));

        let start_time = Instant::now();

        let cog_options = create_cog_options(compression, config);
        driver.create_cog(&dataset, &output_path, cog_options).await?;

        let processing_time = start_time.elapsed();

        let metadata = std::fs::metadata(&output_path)?;
        let output_size_mb = metadata.len() as f64 / 1_000_000.0;

        let input_metadata = std::fs::metadata(input_path)?;
        let input_size_mb = input_metadata.len() as f64 / 1_000_000.0;

        let compression_ratio = 1.0 - (output_size_mb / input_size_mb);

        results.push(CompressionResult {
            compression: *compression,
            output_size_mb,
            compression_ratio,
            processing_time,
        });

        // Clean up
        std::fs::remove_file(&output_path)?;
    }

    std::fs::remove_dir_all(&temp_dir)?;

    Ok(results)
}

/// Process a single file to COG
fn process_single_file(
    input_path: &Path,
    config: &PipelineConfig,
    tile_size: u32,
    dataset_info: &DatasetInfo,
) -> ProcessingResult {
    let start_time = Instant::now();

    let output_filename = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| format!("{}_cog.tif", s))
        .unwrap_or_else(|| "output_cog.tif".to_string());

    let output_path = config.output_dir.join(output_filename);

    // Process using async runtime
    let result = tokio::runtime::Runtime::new()
        .and_then(|rt| {
            rt.block_on(async {
                let dataset = Dataset::open(input_path).await?;
                let driver = GeoTiffDriver::new();

                let compression = config.compression.first()
                    .copied()
                    .unwrap_or(CompressionConfig::Deflate { level: 6 });

                let mut cog_options = create_cog_options(&compression, config);
                cog_options.tile_size = tile_size;

                driver.create_cog(&dataset, &output_path, cog_options).await?;

                Ok::<_, Box<dyn std::error::Error>>(output_path.clone())
            })
        });

    let processing_time = start_time.elapsed();

    let (output_path_result, output_size_mb, compression_ratio) = match result {
        Ok(path) => {
            let output_size_mb = std::fs::metadata(&path)
                .map(|m| m.len() as f64 / 1_000_000.0)
                .ok();

            let compression_ratio = output_size_mb.map(|out_size| {
                1.0 - (out_size / dataset_info.file_size_mb)
            });

            (Ok(path), output_size_mb, compression_ratio)
        }
        Err(e) => (Err(e.to_string()), None, None),
    };

    ProcessingResult {
        input_path: input_path.to_path_buf(),
        output_path: output_path_result,
        processing_time,
        input_size_mb: dataset_info.file_size_mb,
        output_size_mb,
        compression_ratio,
    }
}

/// Create COG options from compression config
fn create_cog_options(
    compression: &CompressionConfig,
    config: &PipelineConfig,
) -> CogOptions {
    let (compression_type, level) = match compression {
        CompressionConfig::None => ("NONE".to_string(), None),
        CompressionConfig::Deflate { level } => ("DEFLATE".to_string(), Some(*level as i32)),
        CompressionConfig::Zstd { level } => ("ZSTD".to_string(), Some(*level)),
        CompressionConfig::Lzw => ("LZW".to_string(), None),
        CompressionConfig::Webp { quality } => ("WEBP".to_string(), Some(*quality as i32)),
        CompressionConfig::Jpeg { quality } => ("JPEG".to_string(), Some(*quality as i32)),
    };

    CogOptions {
        tile_size: 512,
        compression: compression_type,
        compression_level: level,
        overview_levels: config.overview_levels.clone(),
        resampling: config.resampling_method.clone(),
        predictor: Some(2),
        ..Default::default()
    }
}

/// Create cloud backend based on configuration
async fn create_cloud_backend(
    config: &CloudUploadConfig,
) -> Result<Box<dyn CloudBackend>, Box<dyn std::error::Error>> {
    match config.provider {
        CloudProvider::S3 => {
            let backend = S3Backend::new(&config.bucket, "us-west-2").await?;
            Ok(Box::new(backend))
        }
        CloudProvider::Azure => {
            let backend = AzureBackend::new(&config.bucket).await?;
            Ok(Box::new(backend))
        }
        CloudProvider::Gcs => {
            let backend = GcsBackend::new(&config.bucket).await?;
            Ok(Box::new(backend))
        }
    }
}

/// Generate processing report
fn generate_report(
    results: &[ProcessingResult],
    config: &PipelineConfig,
    total_time: std::time::Duration,
) -> Result<ProcessingReport, Box<dyn std::error::Error>> {
    let successful = results.iter().filter(|r| r.output_path.is_ok()).count();
    let failed = results.len() - successful;

    let total_input_size_gb: f64 = results.iter()
        .map(|r| r.input_size_mb)
        .sum::<f64>() / 1000.0;

    let total_output_size_gb: f64 = results.iter()
        .filter_map(|r| r.output_size_mb)
        .sum::<f64>() / 1000.0;

    let avg_compression_ratio = results.iter()
        .filter_map(|r| r.compression_ratio)
        .sum::<f64>() / successful.max(1) as f64;

    let files = results.iter()
        .map(|r| FileReport {
            input_file: r.input_path.display().to_string(),
            output_file: r.output_path.as_ref().ok()
                .map(|p| p.display().to_string()),
            success: r.output_path.is_ok(),
            input_size_mb: r.input_size_mb,
            output_size_mb: r.output_size_mb,
            compression_ratio: r.compression_ratio,
            processing_time_secs: r.processing_time.as_secs_f64(),
        })
        .collect();

    Ok(ProcessingReport {
        total_files: results.len(),
        successful,
        failed,
        total_input_size_gb,
        total_output_size_gb,
        avg_compression_ratio,
        total_time_secs: total_time.as_secs_f64(),
        avg_time_per_file_secs: total_time.as_secs_f64() / results.len() as f64,
        files,
    })
}
