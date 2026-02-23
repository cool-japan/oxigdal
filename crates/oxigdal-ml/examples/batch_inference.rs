//! High-throughput batch inference example

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use oxigdal_ml::batch::{BatchConfig, BatchProcessor};
use oxigdal_ml::error::Result;
use oxigdal_ml::models::OnnxModel;
use oxigdal_ml::zoo::ModelZoo;
use std::time::Instant;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("Batch Inference Example");
    println!("======================\n");

    // Load model
    let mut zoo = ModelZoo::new()?;
    let model_path = zoo.get_model("resnet50_landcover")?;
    let model = OnnxModel::from_file(model_path)?;

    // Configure batch processor
    let config = BatchConfig::builder()
        .max_batch_size(32)
        .batch_timeout_ms(100)
        .parallel_batches(4)
        .memory_pooling(true)
        .build();

    let processor = BatchProcessor::new(model, config);

    // Create batch of inputs
    let num_images = 100;
    let inputs: Vec<_> = (0..num_images)
        .map(|_| RasterBuffer::zeros(224, 224, RasterDataType::Float32))
        .collect();

    println!("Processing batch of {} images...", num_images);

    // Process batch
    let start = Instant::now();
    let results = processor.infer_batch(inputs)?;
    let elapsed = start.elapsed();

    println!("\nResults:");
    println!("  Processed: {} images", results.len());
    println!("  Time: {:.2}s", elapsed.as_secs_f32());
    println!(
        "  Throughput: {:.1} images/sec",
        results.len() as f32 / elapsed.as_secs_f32()
    );

    // Show statistics
    let stats = processor.stats();
    println!("\nBatch Statistics:");
    println!("  Total requests: {}", stats.total_requests);
    println!("  Total batches: {}", stats.total_batches);
    println!("  Avg batch size: {:.1}", stats.avg_batch_size());
    println!("  Avg latency: {:.1}ms", stats.avg_latency_ms());

    Ok(())
}
