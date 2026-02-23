//! Cookbook: Land Cover Classification with ML
//!
//! Complete workflow for ML-based image classification:
//! - Data preprocessing and normalization
//! - Model inference with ONNX Runtime
//! - Post-processing and classification
//! - Validation and accuracy assessment
//! - Export to standard formats
//!
//! Real-world scenarios:
//! - Land cover mapping from Sentinel-2
//! - Urban area classification
//! - Crop type identification
//! - Forest vs. non-forest mapping
//!
//! Run with:
//! ```bash
//! cargo run --example ml_classification
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Land Cover Classification with ML ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("ml_classification_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    // Scenario: Land cover classification using Sentinel-2 data
    println!("Scenario: Land Cover Classification from Sentinel-2");
    println!("==================================================\n");

    let width = 512;
    let height = 512;

    // Step 1: Load and prepare input data
    println!("Step 1: Load Input Data");
    println!("----------------------");

    // Simulate loading Sentinel-2 bands
    let band_2_blue = create_synthetic_band(width, height, 0.15)?;
    let band_3_green = create_synthetic_band(width, height, 0.20)?;
    let band_4_red = create_synthetic_band(width, height, 0.18)?;
    let band_8_nir = create_synthetic_band(width, height, 0.40)?;
    let band_11_swir = create_synthetic_band(width, height, 0.25)?;

    println!("Loaded Sentinel-2 bands:");
    println!("  ✓ Band 2 (Blue): {}x{}", width, height);
    println!("  ✓ Band 3 (Green): {}x{}", width, height);
    println!("  ✓ Band 4 (Red): {}x{}", width, height);
    println!("  ✓ Band 8 (NIR): {}x{}", width, height);
    println!("  ✓ Band 11 (SWIR): {}x{}", width, height);

    // Step 2: Preprocessing
    println!("\n\nStep 2: Data Preprocessing");
    println!("--------------------------");

    println!("Normalizing bands to [0, 1] range...");

    let blue_norm = normalize_band(&band_2_blue)?;
    let green_norm = normalize_band(&band_3_green)?;
    let red_norm = normalize_band(&band_4_red)?;
    let nir_norm = normalize_band(&band_8_nir)?;
    let swir_norm = normalize_band(&band_11_swir)?;

    println!("  ✓ Normalization complete");

    // Calculate spectral indices
    println!("\nCalculating spectral indices...");

    let ndvi = calculate_ndvi(&nir_norm, &red_norm)?;
    let ndbi = calculate_ndbi(&swir_norm, &nir_norm)?; // Normalized Difference Built-up Index
    let ndmi = calculate_ndmi(&nir_norm, &swir_norm)?; // Normalized Difference Moisture Index
    let ndwi = calculate_ndwi(&green_norm, &nir_norm)?; // Normalized Difference Water Index

    println!("  ✓ NDVI calculated");
    println!("  ✓ NDBI calculated");
    println!("  ✓ NDMI calculated");
    println!("  ✓ NDWI calculated");

    // Step 3: Create input feature vector
    println!("\n\nStep 3: Prepare Model Input");
    println!("---------------------------");

    // Stack all bands into feature vector
    let features = create_feature_stack(
        &blue_norm,
        &green_norm,
        &red_norm,
        &nir_norm,
        &swir_norm,
        &ndvi,
        &ndbi,
        &ndmi,
        &ndwi,
    )?;

    println!("Feature vector created: {} input features", features.len() / (width * height));

    // Step 4: Model-based Classification (simulated)
    println!("\n\nStep 4: Model-Based Classification");
    println!("----------------------------------");

    println!("Loading ONNX model: land_cover_classifier.onnx");
    println!("  ✓ Model loaded (simulated)");

    // Simulated inference: create probability maps for each class
    let num_classes = 6;

    let classes = vec![
        (0, "Water", 0.1),
        (1, "Forest", 0.35),
        (2, "Grassland", 0.25),
        (3, "Agriculture", 0.2),
        (4, "Urban", 0.05),
        (5, "Bare Soil", 0.05),
    ];

    println!("Classes: {}", num_classes);
    for (_, name, _) in &classes {
        println!("  - {}", name);
    }

    // Simulate model inference
    let mut probabilities = vec![RasterBuffer::zeros(width, height, RasterDataType::Float32)?; num_classes];

    // Assign class probabilities based on spectral indices
    for (class_idx, class_name, base_prob) in &classes {
        let mut class_probs = match *class_name {
            "Water" => compute_water_probability(&ndwi, &ndvi),
            "Forest" => compute_forest_probability(&ndvi, &ndmi),
            "Grassland" => compute_grassland_probability(&ndvi, &ndbi),
            "Agriculture" => compute_agriculture_probability(&ndvi, &ndmi),
            "Urban" => compute_urban_probability(&ndbi, &ndvi),
            "Bare Soil" => compute_bare_soil_probability(&ndvi, &ndbi),
            _ => vec![*base_prob; width * height],
        };

        // Add some spatial variation
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let noise = ((x as f32 * 73.0 + y as f32 * 137.0).sin() * 0.1).max(0.0);
                class_probs[idx] = (class_probs[idx] + noise).min(1.0).max(0.0);
            }
        }

        probabilities[*class_idx] = RasterBuffer::from_vec(
            class_probs,
            width,
            height,
            RasterDataType::Float32,
        )?;
    }

    println!("  ✓ Model inference completed");

    // Step 5: Post-processing
    println!("\n\nStep 5: Post-Processing");
    println!("----------------------");

    // Generate maximum probability classification
    println!("Generating classification map from probabilities...");

    let classification = create_classification_map(&probabilities)?;

    // Apply confidence filtering
    let confidence = calculate_confidence(&probabilities)?;
    let filtered_classification = apply_confidence_filter(&classification, &confidence, 0.6)?;

    println!("  ✓ Classification map created");
    println!("  ✓ Confidence threshold applied (>60%)");

    // Smooth classification with morphological operations
    println!("Applying spatial smoothing...");

    let smoothed = apply_modal_filter(&filtered_classification, 1)?;

    println!("  ✓ Spatial smoothing completed");

    // Step 6: Accuracy Assessment
    println!("\n\nStep 6: Accuracy Assessment");
    println!("---------------------------");

    // Create reference classification for validation
    let reference = create_reference_classification(width, height)?;

    let confusion_matrix = compute_confusion_matrix(&filtered_classification, &reference, 6)?;

    let overall_accuracy = compute_overall_accuracy(&confusion_matrix)?;
    let producer_accuracy = compute_producer_accuracy(&confusion_matrix)?;
    let user_accuracy = compute_user_accuracy(&confusion_matrix)?;

    println!("Overall Accuracy: {:.2}%", overall_accuracy * 100.0);
    println!("\nPer-class Producer Accuracy (Sensitivity):");

    for (class_idx, class_name, _) in &classes {
        println!("  {}: {:.2}%", class_name, producer_accuracy[*class_idx] * 100.0);
    }

    println!("\nPer-class User Accuracy (Precision):");

    for (class_idx, class_name, _) in &classes {
        println!("  {}: {:.2}%", class_name, user_accuracy[*class_idx] * 100.0);
    }

    // Kappa coefficient
    let kappa = compute_kappa(&confusion_matrix)?;
    println!("\nCohen's Kappa: {:.4}", kappa);

    // Step 7: Class statistics
    println!("\n\nStep 7: Classification Statistics");
    println!("--------------------------------");

    let class_counts = compute_class_statistics(&filtered_classification, 6)?;

    println!("Area coverage by class:");

    for (class_idx, class_name, _) in &classes {
        let count = class_counts[*class_idx];
        let percentage = (count as f32 / (width * height) as f32) * 100.0;
        let area_km2 = (count as f64 * 30.0 * 30.0) / 1_000_000.0;

        println!("  {}: {:.2}% ({:.2} km²)", class_name, percentage, area_km2);
    }

    // Step 8: Export results
    println!("\n\nStep 8: Export Results");
    println!("---------------------");

    let gt = create_geotransform(width, height)?;

    save_raster(&filtered_classification, &output_dir.join("classification.tif"), &gt)?;
    save_raster(&smoothed, &output_dir.join("classification_smoothed.tif"), &gt)?;
    save_raster(&confidence, &output_dir.join("confidence.tif"), &gt)?;

    // Export probability maps for each class
    for (class_idx, class_name, _) in &classes {
        let prob_file = output_dir.join(format!("probability_{}.tif", class_name.to_lowercase()));
        save_raster(&probabilities[*class_idx], &prob_file, &gt)?;
    }

    // Step 9: Generate classification quality report
    println!("\n\nStep 9: Quality Report");
    println!("---------------------");

    let mut report = String::new();
    report.push_str("LAND COVER CLASSIFICATION REPORT\n");
    report.push_str("=================================\n\n");

    report.push_str("ACCURACY METRICS\n");
    report.push_str("----------------\n");
    report.push_str(&format!("Overall Accuracy: {:.2}%\n", overall_accuracy * 100.0));
    report.push_str(&format!("Cohen's Kappa: {:.4}\n\n", kappa));

    report.push_str("CLASSIFICATION RESULTS\n");
    report.push_str("---------------------\n");
    for (class_idx, class_name, _) in &classes {
        let count = class_counts[*class_idx];
        let percentage = (count as f32 / (width * height) as f32) * 100.0;
        report.push_str(&format!("{}: {:.2}%\n", class_name, percentage));
    }

    let report_path = output_dir.join("classification_report.txt");
    std::fs::write(&report_path, &report)?;
    println!("  ✓ Report saved");

    println!("\nAll outputs saved to: {:?}", output_dir);

    Ok(())
}

// Helper functions

fn create_synthetic_band(
    width: usize,
    height: usize,
    base_value: f32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            let pattern = (nx.sin() + ny.cos()) / 2.0;
            data[idx] = (base_value + pattern * 0.15).clamp(0.0, 1.0);
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn normalize_band(
    raster: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let stats = raster.compute_statistics()?;

    let normalized: Vec<f32> = data
        .iter()
        .map(|&x| {
            if stats.max > stats.min {
                (x - stats.min) / (stats.max - stats.min)
            } else {
                x
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        normalized,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_ndvi(
    nir: &RasterBuffer,
    red: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let nir_data = nir.get_data_as_f32()?;
    let red_data = red.get_data_as_f32()?;

    let ndvi: Vec<f32> = nir_data
        .iter()
        .zip(red_data.iter())
        .map(|(&n, &r)| {
            let sum = n + r;
            if sum > 1e-6 {
                (n - r) / sum
            } else {
                0.0
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        ndvi,
        nir.width(),
        nir.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_ndbi(
    swir: &RasterBuffer,
    nir: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let swir_data = swir.get_data_as_f32()?;
    let nir_data = nir.get_data_as_f32()?;

    let ndbi: Vec<f32> = swir_data
        .iter()
        .zip(nir_data.iter())
        .map(|(&s, &n)| {
            let sum = s + n;
            if sum > 1e-6 {
                (s - n) / sum
            } else {
                0.0
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        ndbi,
        swir.width(),
        swir.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_ndmi(
    nir: &RasterBuffer,
    swir: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let nir_data = nir.get_data_as_f32()?;
    let swir_data = swir.get_data_as_f32()?;

    let ndmi: Vec<f32> = nir_data
        .iter()
        .zip(swir_data.iter())
        .map(|(&n, &s)| {
            let sum = n + s;
            if sum > 1e-6 {
                (n - s) / sum
            } else {
                0.0
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        ndmi,
        nir.width(),
        nir.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_ndwi(
    green: &RasterBuffer,
    nir: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let green_data = green.get_data_as_f32()?;
    let nir_data = nir.get_data_as_f32()?;

    let ndwi: Vec<f32> = green_data
        .iter()
        .zip(nir_data.iter())
        .map(|(&g, &n)| {
            let sum = g + n;
            if sum > 1e-6 {
                (g - n) / sum
            } else {
                0.0
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        ndwi,
        green.width(),
        green.height(),
        RasterDataType::Float32,
    )?)
}

fn create_feature_stack(
    blue: &RasterBuffer,
    green: &RasterBuffer,
    red: &RasterBuffer,
    nir: &RasterBuffer,
    swir: &RasterBuffer,
    ndvi: &RasterBuffer,
    ndbi: &RasterBuffer,
    ndmi: &RasterBuffer,
    ndwi: &RasterBuffer,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let blue_data = blue.get_data_as_f32()?;
    let green_data = green.get_data_as_f32()?;
    let red_data = red.get_data_as_f32()?;
    let nir_data = nir.get_data_as_f32()?;
    let swir_data = swir.get_data_as_f32()?;
    let ndvi_data = ndvi.get_data_as_f32()?;
    let ndbi_data = ndbi.get_data_as_f32()?;
    let ndmi_data = ndmi.get_data_as_f32()?;
    let ndwi_data = ndwi.get_data_as_f32()?;

    let mut features = Vec::new();
    let size = blue_data.len();

    for i in 0..size {
        features.push(blue_data[i]);
        features.push(green_data[i]);
        features.push(red_data[i]);
        features.push(nir_data[i]);
        features.push(swir_data[i]);
        features.push(ndvi_data[i]);
        features.push(ndbi_data[i]);
        features.push(ndmi_data[i]);
        features.push(ndwi_data[i]);
    }

    Ok(features)
}

fn compute_water_probability(ndwi: &RasterBuffer, ndvi: &RasterBuffer) -> Vec<f32> {
    let ndwi_data = ndwi.get_data_as_f32().unwrap_or_default();
    let ndvi_data = ndvi.get_data_as_f32().unwrap_or_default();

    ndwi_data
        .iter()
        .zip(ndvi_data.iter())
        .map(|(&w, &v)| ((w + 1.0) / 2.0 * (1.0 - v).max(0.0)).min(1.0))
        .collect()
}

fn compute_forest_probability(ndvi: &RasterBuffer, ndmi: &RasterBuffer) -> Vec<f32> {
    let ndvi_data = ndvi.get_data_as_f32().unwrap_or_default();
    let ndmi_data = ndmi.get_data_as_f32().unwrap_or_default();

    ndvi_data
        .iter()
        .zip(ndmi_data.iter())
        .map(|(&v, &m)| {
            if v > 0.4 && m > 0.1 {
                0.8
            } else if v > 0.3 {
                0.5
            } else {
                0.1
            }
        })
        .collect()
}

fn compute_grassland_probability(ndvi: &RasterBuffer, ndbi: &RasterBuffer) -> Vec<f32> {
    let ndvi_data = ndvi.get_data_as_f32().unwrap_or_default();
    let ndbi_data = ndbi.get_data_as_f32().unwrap_or_default();

    ndvi_data
        .iter()
        .zip(ndbi_data.iter())
        .map(|(&v, &b)| {
            if v > 0.2 && v < 0.4 && b < 0.1 {
                0.7
            } else if v > 0.15 && v < 0.45 {
                0.4
            } else {
                0.1
            }
        })
        .collect()
}

fn compute_agriculture_probability(ndvi: &RasterBuffer, ndmi: &RasterBuffer) -> Vec<f32> {
    let ndvi_data = ndvi.get_data_as_f32().unwrap_or_default();
    let ndmi_data = ndmi.get_data_as_f32().unwrap_or_default();

    ndvi_data
        .iter()
        .zip(ndmi_data.iter())
        .map(|(&v, &m)| {
            if v > 0.3 && v < 0.5 && m > 0.0 {
                0.7
            } else if v > 0.25 && v < 0.55 {
                0.4
            } else {
                0.1
            }
        })
        .collect()
}

fn compute_urban_probability(ndbi: &RasterBuffer, ndvi: &RasterBuffer) -> Vec<f32> {
    let ndbi_data = ndbi.get_data_as_f32().unwrap_or_default();
    let ndvi_data = ndvi.get_data_as_f32().unwrap_or_default();

    ndbi_data
        .iter()
        .zip(ndvi_data.iter())
        .map(|(&b, &v)| {
            if b > 0.1 && v < 0.2 {
                0.8
            } else if b > 0.05 && v < 0.3 {
                0.5
            } else {
                0.1
            }
        })
        .collect()
}

fn compute_bare_soil_probability(ndvi: &RasterBuffer, ndbi: &RasterBuffer) -> Vec<f32> {
    let ndvi_data = ndvi.get_data_as_f32().unwrap_or_default();
    let ndbi_data = ndbi.get_data_as_f32().unwrap_or_default();

    ndvi_data
        .iter()
        .zip(ndbi_data.iter())
        .map(|(&v, &b)| {
            if v < 0.2 && b < 0.05 {
                0.7
            } else if v < 0.3 && b < 0.2 {
                0.4
            } else {
                0.1
            }
        })
        .collect()
}

fn create_classification_map(
    probabilities: &[RasterBuffer],
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let num_classes = probabilities.len();
    let size = probabilities[0].width() * probabilities[0].height();

    let mut classification = vec![0.0f32; size];

    for i in 0..size {
        let mut max_prob = 0.0f32;
        let mut best_class = 0usize;

        for class_idx in 0..num_classes {
            let prob_data = probabilities[class_idx].get_data_as_f32()?;
            if prob_data[i] > max_prob {
                max_prob = prob_data[i];
                best_class = class_idx;
            }
        }

        classification[i] = best_class as f32;
    }

    Ok(RasterBuffer::from_vec(
        classification,
        probabilities[0].width(),
        probabilities[0].height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_confidence(
    probabilities: &[RasterBuffer],
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let size = probabilities[0].width() * probabilities[0].height();
    let mut confidence = vec![0.0f32; size];

    for i in 0..size {
        let mut max_prob = 0.0f32;
        let mut second_max = 0.0f32;

        for prob_raster in probabilities {
            let data = prob_raster.get_data_as_f32()?;
            if data[i] > max_prob {
                second_max = max_prob;
                max_prob = data[i];
            } else if data[i] > second_max {
                second_max = data[i];
            }
        }

        confidence[i] = max_prob - second_max;
    }

    Ok(RasterBuffer::from_vec(
        confidence,
        probabilities[0].width(),
        probabilities[0].height(),
        RasterDataType::Float32,
    )?)
}

fn apply_confidence_filter(
    classification: &RasterBuffer,
    confidence: &RasterBuffer,
    threshold: f32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let class_data = classification.get_data_as_f32()?;
    let conf_data = confidence.get_data_as_f32()?;

    let filtered: Vec<f32> = class_data
        .iter()
        .zip(conf_data.iter())
        .map(|(&c, &conf)| if conf > threshold { c } else { -1.0 })
        .collect();

    Ok(RasterBuffer::from_vec(
        filtered,
        classification.width(),
        classification.height(),
        RasterDataType::Float32,
    )?)
}

fn apply_modal_filter(
    classification: &RasterBuffer,
    radius: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = classification.get_data_as_f32()?;
    let mut smoothed = data.clone();

    for y in radius..classification.height() - radius {
        for x in radius..classification.width() - radius {
            let mut counts: HashMap<i32, usize> = HashMap::new();

            for ky in -(radius as i32)..=(radius as i32) {
                for kx in -(radius as i32)..=(radius as i32) {
                    let ny = (y as i32 + ky) as usize;
                    let nx = (x as i32 + kx) as usize;

                    let val = data[ny * classification.width() + nx] as i32;
                    *counts.entry(val).or_insert(0) += 1;
                }
            }

            if let Some((&modal_class, _)) = counts.iter().max_by_key(|(_, &count)| count) {
                smoothed[y * classification.width() + x] = modal_class as f32;
            }
        }
    }

    Ok(RasterBuffer::from_vec(
        smoothed,
        classification.width(),
        classification.height(),
        RasterDataType::Float32,
    )?)
}

fn create_reference_classification(
    width: usize,
    height: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            data[idx] = if nx < 0.3 {
                0.0 // Water
            } else if nx < 0.6 && ny > 0.3 {
                1.0 // Forest
            } else if ny < 0.5 {
                2.0 // Grassland
            } else {
                3.0 // Agriculture
            };
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn compute_confusion_matrix(
    classified: &RasterBuffer,
    reference: &RasterBuffer,
    num_classes: usize,
) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
    let class_data = classified.get_data_as_f32()?;
    let ref_data = reference.get_data_as_f32()?;

    let mut matrix = vec![vec![0.0f32; num_classes]; num_classes];

    for (&c, &r) in class_data.iter().zip(ref_data.iter()) {
        let ci = c as usize;
        let ri = r as usize;

        if ci < num_classes && ri < num_classes && c >= 0.0 && r >= 0.0 {
            matrix[ri][ci] += 1.0;
        }
    }

    // Normalize
    let total: f32 = matrix.iter().flatten().sum();
    for row in &mut matrix {
        for val in row {
            *val /= total;
        }
    }

    Ok(matrix)
}

fn compute_overall_accuracy(matrix: &[Vec<f32>]) -> Result<f32, Box<dyn std::error::Error>> {
    let mut sum = 0.0f32;
    for (i, row) in matrix.iter().enumerate() {
        sum += row[i];
    }
    Ok(sum)
}

fn compute_producer_accuracy(matrix: &[Vec<f32>]) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut accuracies = vec![0.0f32; matrix.len()];

    for i in 0..matrix.len() {
        let mut col_sum = 0.0f32;
        for row in matrix {
            col_sum += row[i];
        }

        if col_sum > 0.0 {
            accuracies[i] = matrix[i][i] / col_sum;
        }
    }

    Ok(accuracies)
}

fn compute_user_accuracy(matrix: &[Vec<f32>]) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut accuracies = vec![0.0f32; matrix.len()];

    for i in 0..matrix.len() {
        let row_sum: f32 = matrix[i].iter().sum();

        if row_sum > 0.0 {
            accuracies[i] = matrix[i][i] / row_sum;
        }
    }

    Ok(accuracies)
}

fn compute_kappa(matrix: &[Vec<f32>]) -> Result<f32, Box<dyn std::error::Error>> {
    let po = compute_overall_accuracy(matrix)?; // Observed agreement

    let mut pe = 0.0f32;
    for i in 0..matrix.len() {
        let mut row_sum = 0.0f32;
        let mut col_sum = 0.0f32;

        for j in 0..matrix[i].len() {
            row_sum += matrix[i][j];
            col_sum += matrix[j][i];
        }

        pe += row_sum * col_sum;
    }

    let kappa = if pe < 1.0 {
        (po - pe) / (1.0 - pe)
    } else {
        0.0
    };

    Ok(kappa)
}

fn compute_class_statistics(
    classification: &RasterBuffer,
    num_classes: usize,
) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
    let data = classification.get_data_as_f32()?;
    let mut counts = vec![0usize; num_classes];

    for &val in data.iter() {
        let idx = val as usize;
        if idx < num_classes {
            counts[idx] += 1;
        }
    }

    Ok(counts)
}

fn create_geotransform(
    width: usize,
    height: usize,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 10.0, height as f64 * 10.0)?;
    Ok(GeoTransform::from_bounds(&bbox, width, height)?)
}

fn save_raster(
    raster: &RasterBuffer,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;

    let options = GeoTiffWriterOptions {
        compression: CompressionType::Deflate,
        ..Default::default()
    };

    let mut writer = GeoTiffWriter::new(file, options)?;
    writer.write(raster, gt)?;

    println!("  ✓ Saved: {}", path.display());
    Ok(())
}
