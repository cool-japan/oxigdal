//! Cookbook: Batch Processing Large Datasets
//!
//! Efficient workflow for processing many files:
//! - Parallel processing with Rayon
//! - Progress tracking and reporting
//! - Error handling and recovery
//! - Performance comparison (sequential vs. parallel)
//!
//! Real-world scenarios:
//! - Processing Landsat archive (1000+ scenes)
//! - Sentinel-1 SAR processing pipelines
//! - Time series analysis across regions
//!
//! Run with:
//! ```bash
//! cargo run --example cookbook_batch_processing --release
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use rayon::prelude::*;
use std::env;
use std::fs;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Batch Processing Large Datasets ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("batch_output");
    fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    println!("Scenario: Batch NDVI Processing (Landsat-style scenes)");
    println!("========================================================\n");

    let num_files = 40;
    let width = 128u64;
    let height = 128u64;

    // Step 1: Create synthetic dataset
    println!("Step 1: Prepare Dataset");
    println!("----------------------");

    println!("Creating {} synthetic scene identifiers...", num_files);
    let scenes: Vec<String> = (0..num_files).map(|i| format!("scene_{i:04}")).collect();
    println!("  Created {} scene identifiers", scenes.len());

    // Step 2: Sequential processing (baseline)
    println!("\n\nStep 2: Sequential Processing");
    println!("----------------------------");

    let start = Instant::now();

    println!("Processing scenes sequentially...");

    let mut sequential_results = Vec::with_capacity(scenes.len());
    for (idx, scene) in scenes.iter().enumerate() {
        let ndvi_path = output_dir.join(format!("{scene}_ndvi.tif"));
        let result = process_single_scene(scene, &ndvi_path, width, height, idx)?;
        sequential_results.push(result);

        if sequential_results.len() % 10 == 0 {
            println!(
                "  Processed {}/{} scenes",
                sequential_results.len(),
                num_files
            );
        }
    }

    let sequential_time = start.elapsed();

    println!(
        "  Sequential processing completed in {:.3}s",
        sequential_time.as_secs_f32()
    );

    // Step 3: Parallel processing with Rayon
    println!("\n\nStep 3: Parallel Processing (Rayon)");
    println!("-----------------------------------");

    let start = Instant::now();

    println!("Processing scenes in parallel...");

    let parallel_results: Vec<ProcessingResult> = scenes
        .par_iter()
        .enumerate()
        .map(|(idx, scene)| {
            let ndvi_path = output_dir.join(format!("{scene}_ndvi.tif"));
            process_single_scene(scene, &ndvi_path, width, height, idx).unwrap_or_else(|_| {
                ProcessingResult {
                    scene: scene.clone(),
                    ndvi_mean: 0.0,
                    processing_time_ms: 0,
                    success: false,
                    error_message: "Processing failed".to_string(),
                }
            })
        })
        .collect();

    let parallel_time = start.elapsed();

    println!(
        "  Parallel processing completed in {:.3}s",
        parallel_time.as_secs_f32()
    );

    // Step 4: Report speedup
    println!("\n\nStep 4: Performance Analysis");
    println!("---------------------------");

    let speedup = sequential_time.as_secs_f32() / parallel_time.as_secs_f32().max(1e-6);
    let cores = rayon::current_num_threads().max(1);
    let efficiency = speedup / cores as f32;

    println!("Processing time comparison:");
    println!("  Sequential: {:.3}s", sequential_time.as_secs_f32());
    println!("  Parallel:   {:.3}s", parallel_time.as_secs_f32());
    println!("  Speedup:    {:.2}x", speedup);
    println!("  Rayon threads: {}", cores);
    println!("  Efficiency: {:.1}%", efficiency * 100.0);

    // Step 5: Detailed Results
    println!("\n\nStep 5: Processing Results");
    println!("-------------------------");

    let mut successful = 0;
    let mut failed = 0;
    let mut total_ndvi = 0.0f32;

    for result in &parallel_results {
        if result.success {
            successful += 1;
            total_ndvi += result.ndvi_mean;
        } else {
            failed += 1;
            println!("  FAILED {}: {}", result.scene, result.error_message);
        }
    }

    println!("  Successful: {}", successful);
    println!("  Failed: {}", failed);
    println!(
        "  Success rate: {:.1}%",
        (successful as f32 / num_files as f32) * 100.0
    );

    let mean_ndvi = if successful > 0 {
        total_ndvi / successful as f32
    } else {
        0.0
    };

    println!("  Mean NDVI across all scenes: {:.4}", mean_ndvi);

    // Step 6: Generate statistics report
    println!("\n\nStep 6: Batch Statistics");
    println!("------------------------");

    let mut processing_times: Vec<u32> = parallel_results
        .iter()
        .filter(|r| r.success)
        .map(|r| r.processing_time_ms)
        .collect();

    processing_times.sort_unstable();

    let avg_time =
        processing_times.iter().sum::<u32>() as f32 / processing_times.len().max(1) as f32;
    let min_time = processing_times.first().copied().unwrap_or(0) as f32;
    let max_time = processing_times.last().copied().unwrap_or(0) as f32;

    println!("Processing time per scene:");
    println!("  Average: {:.2} ms", avg_time);
    println!("  Min:     {:.2} ms", min_time);
    println!("  Max:     {:.2} ms", max_time);

    let throughput = num_files as f32 / parallel_time.as_secs_f32().max(1e-6);
    println!("  Throughput: {:.1} scenes/second", throughput);

    // Step 7: Generate batch report
    println!("\n\nStep 7: Generate Batch Report");
    println!("-----------------------------");

    let report = generate_batch_report(
        num_files,
        successful,
        failed,
        &parallel_results,
        sequential_time.as_secs_f32(),
        parallel_time.as_secs_f32(),
    );

    let report_path = output_dir.join("batch_report.txt");
    fs::write(&report_path, &report)?;
    println!("Batch report saved to: {:?}", report_path);

    // Step 8: Quality control
    println!("\n\nStep 8: Quality Control Checks");
    println!("------------------------------");

    let mut output_valid = 0;
    for result in &parallel_results {
        let output_path = output_dir.join(format!("{}_ndvi.tif", result.scene));
        if output_path.exists() {
            output_valid += 1;
        }
    }

    println!(
        "  Output file validation: {}/{} files present",
        output_valid, successful
    );

    println!("\nSummary");
    println!("=======");
    println!("Total scenes processed: {}", num_files);
    println!("Successful: {}", successful);
    println!(
        "Processing time: {:.3}s (parallel) vs {:.3}s (sequential)",
        parallel_time.as_secs_f32(),
        sequential_time.as_secs_f32()
    );
    println!("Throughput: {:.1} scenes/second", throughput);
    println!("\nOutput directory: {:?}", output_dir);

    Ok(())
}

/// Processing result for a single scene
#[derive(Clone, Debug)]
struct ProcessingResult {
    scene: String,
    ndvi_mean: f32,
    processing_time_ms: u32,
    success: bool,
    error_message: String,
}

/// Process a single synthetic scene: generate Red/NIR data, compute NDVI, and write it out.
fn process_single_scene(
    scene: &str,
    output_path: &Path,
    width: u64,
    height: u64,
    seed: usize,
) -> Result<ProcessingResult, Box<dyn std::error::Error>> {
    let start = Instant::now();

    let mut ndvi_data: Vec<f32> = Vec::with_capacity((width * height) as usize);
    let mut ndvi_sum = 0.0f32;

    let offset = seed as f32 * 0.01;

    for y in 0..height {
        for x in 0..width {
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            let red = ((nx.sin() + offset) * 0.3).clamp(0.0, 1.0);
            let nir = ((ny.cos() + offset) * 0.4).clamp(0.0, 1.0);

            let sum = red + nir;
            let ndvi = if sum > 1e-6 { (nir - red) / sum } else { 0.0 };

            ndvi_sum += ndvi;
            ndvi_data.push(ndvi);
        }
    }

    let ndvi_mean = ndvi_sum / (width * height) as f32;

    let ndvi_buf = RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        ndvi_data,
        RasterDataType::Float32,
    )?;

    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 30.0, height as f64 * 30.0)?;
    let gt = GeoTransform::from_bounds(&bbox, width, height)?;

    let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
        .with_compression(Compression::Deflate)
        .with_tile_size(64, 64)
        .with_geo_transform(gt);

    let mut writer = GeoTiffWriter::create(output_path, config, GeoTiffWriterOptions::default())?;
    writer.write(ndvi_buf.as_bytes())?;

    let elapsed = start.elapsed();

    Ok(ProcessingResult {
        scene: scene.to_string(),
        ndvi_mean,
        processing_time_ms: elapsed.as_millis() as u32,
        success: true,
        error_message: String::new(),
    })
}

fn generate_batch_report(
    total_files: usize,
    successful: usize,
    failed: usize,
    results: &[ProcessingResult],
    sequential_time: f32,
    parallel_time: f32,
) -> String {
    let mut report = String::new();

    report.push_str("BATCH PROCESSING REPORT\n");
    report.push_str("=======================\n\n");

    report.push_str("SUMMARY\n");
    report.push_str("-------\n");
    report.push_str(&format!("Total files: {}\n", total_files));
    report.push_str(&format!("Successful: {}\n", successful));
    report.push_str(&format!("Failed: {}\n", failed));
    report.push_str(&format!(
        "Success rate: {:.1}%\n\n",
        (successful as f32 / total_files as f32) * 100.0
    ));

    report.push_str("PERFORMANCE\n");
    report.push_str("-----------\n");
    report.push_str(&format!("Sequential time: {:.3}s\n", sequential_time));
    report.push_str(&format!("Parallel time: {:.3}s\n", parallel_time));
    report.push_str(&format!(
        "Speedup: {:.2}x\n\n",
        sequential_time / parallel_time.max(1e-6)
    ));

    report.push_str("PROCESSING DETAILS\n");
    report.push_str("------------------\n");

    for result in results.iter().take(10) {
        if result.success {
            report.push_str(&format!(
                "{}: NDVI={:.4}, Time={}ms\n",
                result.scene, result.ndvi_mean, result.processing_time_ms
            ));
        }
    }

    if results.len() > 10 {
        report.push_str(&format!("... and {} more files\n", results.len() - 10));
    }

    report
}
