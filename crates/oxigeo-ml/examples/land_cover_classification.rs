//! Land cover classification using pre-trained ResNet50
//!
//! This example demonstrates how to:
//! - Load a pre-trained classification model from the model zoo
//! - Preprocess satellite imagery
//! - Run inference to predict land cover classes
//! - Visualize and export results

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::RasterDataType;
use oxigeo_ml::error::Result;
use oxigeo_ml::inference::{InferenceConfig, InferenceEngine};
use oxigeo_ml::models::OnnxModel;
use oxigeo_ml::preprocessing::NormalizationParams;
use oxigeo_ml::zoo::{ModelTask, ModelZoo};

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create model zoo
    let mut zoo = ModelZoo::new()?;

    // List available classification models
    let classifiers = zoo.find_by_task(ModelTask::Classification);
    println!("Available classification models:");
    for model in &classifiers {
        println!(
            "  - {} ({:.1}% accuracy)",
            model.name,
            model.accuracy.unwrap_or(0.0)
        );
    }

    // Get the ResNet50 land cover model
    let model_path = zoo.get_model("resnet50_landcover")?;
    println!("\nLoaded model from: {:?}", model_path);

    // Load the model
    let model = OnnxModel::from_file(model_path)?;

    // Create inference engine with ImageNet normalization
    let config = InferenceConfig {
        normalization: Some(NormalizationParams::imagenet()),
        tiling: None,
        confidence_threshold: 0.5,
        gpu_config: None,
    };
    let mut engine = InferenceEngine::new(model, config);

    // Load input raster (simulated for this example)
    let input = create_sample_input(224, 224);
    println!("\nInput raster: {}x{}", input.width(), input.height());

    // Run inference
    println!("Running inference...");
    let predictions = engine.predict(&input)?;

    // Get predicted class
    let class_id = get_predicted_class(&predictions)?;
    let class_name = get_class_name(class_id);
    println!(
        "\nPredicted land cover: {} (class {})",
        class_name, class_id
    );

    // Export results
    println!("\nInference complete!");

    Ok(())
}

/// Creates a sample input raster.
///
/// Note: `RasterBuffer` is single-band. A real 3-channel (RGB) classifier such
/// as ResNet50 expects a `[1, 3, H, W]` tensor, so running this example against
/// such a model now returns a clear `InferenceError::InvalidBandCount` instead of
/// silently building a `[1, 1, H, W]` tensor. Full multi-band inference support
/// (threading a multi-band buffer through the `Model` trait) is tracked as future
/// work.
fn create_sample_input(width: u64, height: u64) -> RasterBuffer {
    RasterBuffer::zeros(width, height, RasterDataType::Float32)
}

/// Gets the predicted class from probability distribution
fn get_predicted_class(_predictions: &RasterBuffer) -> Result<usize> {
    // Find max probability (simplified for example)
    Ok(0) // Placeholder
}

/// Maps class ID to land cover name
fn get_class_name(class_id: usize) -> &'static str {
    match class_id {
        0 => "Forest",
        1 => "Grassland",
        2 => "Water",
        3 => "Urban",
        4 => "Cropland",
        5 => "Barren",
        6 => "Wetland",
        7 => "Shrubland",
        8 => "Snow/Ice",
        9 => "Clouds",
        _ => "Unknown",
    }
}
