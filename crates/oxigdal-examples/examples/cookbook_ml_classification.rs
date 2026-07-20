//! Cookbook: Land Cover Classification with ML
//!
//! Complete workflow for ML-based image classification:
//! - Data preprocessing and normalization
//! - Spectral index calculation (feature engineering)
//! - Rule-based "inference" standing in for a trained model
//! - Post-processing and classification
//! - Validation and accuracy assessment
//!
//! Real-world scenarios:
//! - Land cover mapping from Sentinel-2
//! - Urban area classification
//! - Crop type identification
//!
//! Run with:
//! ```bash
//! cargo run --example cookbook_ml_classification
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::collections::HashMap;
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Land Cover Classification with ML ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("ml_classification_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    println!("Scenario: Land Cover Classification from Sentinel-2");
    println!("==================================================\n");

    let width = 128u64;
    let height = 128u64;

    // Step 1: Load and prepare input data
    println!("Step 1: Load Input Data");
    println!("----------------------");

    let band_2_blue = create_synthetic_band(width, height, 0.15);
    let band_3_green = create_synthetic_band(width, height, 0.20);
    let band_4_red = create_synthetic_band(width, height, 0.18);
    let band_8_nir = create_synthetic_band(width, height, 0.40);
    let band_11_swir = create_synthetic_band(width, height, 0.25);

    println!("Loaded Sentinel-2 bands:");
    println!("  Band 2 (Blue): {}x{}", width, height);
    println!("  Band 3 (Green): {}x{}", width, height);
    println!("  Band 4 (Red): {}x{}", width, height);
    println!("  Band 8 (NIR): {}x{}", width, height);
    println!("  Band 11 (SWIR): {}x{}", width, height);

    // Step 2: Preprocessing
    println!("\n\nStep 2: Data Preprocessing");
    println!("--------------------------");

    println!("Normalizing bands to [0, 1] range...");

    let green_norm = normalize_band(&band_3_green)?;
    let red_norm = normalize_band(&band_4_red)?;
    let nir_norm = normalize_band(&band_8_nir)?;
    let swir_norm = normalize_band(&band_11_swir)?;
    let _blue_norm = normalize_band(&band_2_blue)?;

    println!("  Normalization complete");

    println!("\nCalculating spectral indices...");

    let ndvi = calculate_normalized_diff(&nir_norm, &red_norm)?;
    let ndbi = calculate_normalized_diff(&swir_norm, &nir_norm)?;
    let ndmi = calculate_normalized_diff(&nir_norm, &swir_norm)?;
    let ndwi = calculate_normalized_diff(&green_norm, &nir_norm)?;

    println!("  NDVI calculated");
    println!("  NDBI (built-up) calculated");
    println!("  NDMI (moisture) calculated");
    println!("  NDWI (water) calculated");

    // Step 3: Model-based Classification (rule-based simulation)
    println!("\n\nStep 3: Model-Based Classification");
    println!("----------------------------------");

    println!("Loading ONNX model: land_cover_classifier.onnx");
    println!("  Model loaded (simulated with rule-based probability functions)");

    let num_classes = 6;

    let classes = [
        (0usize, "Water", 0.1f32),
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

    let mut probabilities: Vec<RasterBuffer> = (0..num_classes)
        .map(|_| RasterBuffer::zeros(width, height, RasterDataType::Float32))
        .collect();

    for (class_idx, class_name, base_prob) in &classes {
        let mut class_probs = match *class_name {
            "Water" => compute_water_probability(&ndwi, &ndvi)?,
            "Forest" => compute_forest_probability(&ndvi, &ndmi)?,
            "Grassland" => compute_grassland_probability(&ndvi, &ndbi)?,
            "Agriculture" => compute_agriculture_probability(&ndvi, &ndmi)?,
            "Urban" => compute_urban_probability(&ndbi, &ndvi)?,
            "Bare Soil" => compute_bare_soil_probability(&ndvi, &ndbi)?,
            _ => vec![*base_prob; (width * height) as usize],
        };

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let noise = ((x as f32 * 73.0 + y as f32 * 137.0).sin() * 0.1).max(0.0);
                class_probs[idx] = (class_probs[idx] + noise).clamp(0.0, 1.0);
            }
        }

        probabilities[*class_idx] = RasterBuffer::from_typed_vec(
            width as usize,
            height as usize,
            class_probs,
            RasterDataType::Float32,
        )?;
    }

    println!("  Model inference completed");

    // Step 4: Post-processing
    println!("\n\nStep 4: Post-Processing");
    println!("----------------------");

    println!("Generating classification map from probabilities...");

    let classification = create_classification_map(&probabilities)?;

    let confidence = calculate_confidence(&probabilities)?;
    let filtered_classification = apply_confidence_filter(&classification, &confidence, 0.6)?;

    println!("  Classification map created");
    println!("  Confidence threshold applied (>60%)");

    println!("Applying spatial smoothing...");
    let smoothed = apply_modal_filter(&filtered_classification, 1)?;
    println!("  Spatial smoothing completed");

    // Step 5: Accuracy Assessment
    println!("\n\nStep 5: Accuracy Assessment");
    println!("---------------------------");

    let reference = create_reference_classification(width, height)?;

    let confusion_matrix = compute_confusion_matrix(&filtered_classification, &reference, 6)?;

    let overall_accuracy = compute_overall_accuracy(&confusion_matrix);
    let producer_accuracy = compute_producer_accuracy(&confusion_matrix);
    let user_accuracy = compute_user_accuracy(&confusion_matrix);

    println!("Overall Accuracy: {:.2}%", overall_accuracy * 100.0);
    println!("\nPer-class Producer Accuracy (Sensitivity):");
    for (class_idx, class_name, _) in &classes {
        println!(
            "  {}: {:.2}%",
            class_name,
            producer_accuracy[*class_idx] * 100.0
        );
    }

    println!("\nPer-class User Accuracy (Precision):");
    for (class_idx, class_name, _) in &classes {
        println!(
            "  {}: {:.2}%",
            class_name,
            user_accuracy[*class_idx] * 100.0
        );
    }

    let kappa = compute_kappa(&confusion_matrix);
    println!("\nCohen's Kappa: {:.4}", kappa);

    // Step 6: Class statistics
    println!("\n\nStep 6: Classification Statistics");
    println!("--------------------------------");

    let class_counts = compute_class_statistics(&filtered_classification, 6)?;

    println!("Area coverage by class:");
    for (class_idx, class_name, _) in &classes {
        let count = class_counts[*class_idx];
        let percentage = (count as f32 / (width * height) as f32) * 100.0;
        let area_km2 = (count as f64 * 30.0 * 30.0) / 1_000_000.0;
        println!("  {}: {:.2}% ({:.2} km2)", class_name, percentage, area_km2);
    }

    // Step 7: Export results
    println!("\n\nStep 7: Export Results");
    println!("---------------------");

    let gt = create_geotransform(width, height)?;

    save_raster(
        &filtered_classification,
        &output_dir.join("classification.tif"),
        &gt,
    )?;
    save_raster(
        &smoothed,
        &output_dir.join("classification_smoothed.tif"),
        &gt,
    )?;
    save_raster(&confidence, &output_dir.join("confidence.tif"), &gt)?;

    for (class_idx, class_name, _) in &classes {
        let prob_file = output_dir.join(format!("probability_{}.tif", class_name.to_lowercase()));
        save_raster(&probabilities[*class_idx], &prob_file, &gt)?;
    }

    // Step 8: Generate classification quality report
    println!("\n\nStep 8: Quality Report");
    println!("---------------------");

    let mut report = String::new();
    report.push_str("LAND COVER CLASSIFICATION REPORT\n");
    report.push_str("=================================\n\n");

    report.push_str("ACCURACY METRICS\n");
    report.push_str("----------------\n");
    report.push_str(&format!(
        "Overall Accuracy: {:.2}%\n",
        overall_accuracy * 100.0
    ));
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
    println!("  Report saved");

    println!("\nAll outputs saved to: {:?}", output_dir);

    Ok(())
}

// Helper functions

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

fn normalize_band(raster: &RasterBuffer) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.as_slice::<f32>()?;
    let stats = raster.compute_statistics()?;
    let (min, max) = (stats.min as f32, stats.max as f32);

    let normalized: Vec<f32> = data
        .iter()
        .map(|&x| {
            if max > min {
                (x - min) / (max - min)
            } else {
                x
            }
        })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        raster.width() as usize,
        raster.height() as usize,
        normalized,
        RasterDataType::Float32,
    )?)
}

/// Generic normalized-difference index: (a - b) / (a + b)
fn calculate_normalized_diff(
    a: &RasterBuffer,
    b: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let a_data = a.as_slice::<f32>()?;
    let b_data = b.as_slice::<f32>()?;

    let result: Vec<f32> = a_data
        .iter()
        .zip(b_data.iter())
        .map(|(&x, &y)| {
            let sum = x + y;
            if sum > 1e-6 { (x - y) / sum } else { 0.0 }
        })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        a.width() as usize,
        a.height() as usize,
        result,
        RasterDataType::Float32,
    )?)
}

fn compute_water_probability(
    ndwi: &RasterBuffer,
    ndvi: &RasterBuffer,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let ndwi_data = ndwi.as_slice::<f32>()?;
    let ndvi_data = ndvi.as_slice::<f32>()?;

    Ok(ndwi_data
        .iter()
        .zip(ndvi_data.iter())
        .map(|(&w, &v)| (((w + 1.0) / 2.0) * (1.0 - v).max(0.0)).min(1.0))
        .collect())
}

fn compute_forest_probability(
    ndvi: &RasterBuffer,
    ndmi: &RasterBuffer,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let ndvi_data = ndvi.as_slice::<f32>()?;
    let ndmi_data = ndmi.as_slice::<f32>()?;

    Ok(ndvi_data
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
        .collect())
}

fn compute_grassland_probability(
    ndvi: &RasterBuffer,
    ndbi: &RasterBuffer,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let ndvi_data = ndvi.as_slice::<f32>()?;
    let ndbi_data = ndbi.as_slice::<f32>()?;

    Ok(ndvi_data
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
        .collect())
}

fn compute_agriculture_probability(
    ndvi: &RasterBuffer,
    ndmi: &RasterBuffer,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let ndvi_data = ndvi.as_slice::<f32>()?;
    let ndmi_data = ndmi.as_slice::<f32>()?;

    Ok(ndvi_data
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
        .collect())
}

fn compute_urban_probability(
    ndbi: &RasterBuffer,
    ndvi: &RasterBuffer,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let ndbi_data = ndbi.as_slice::<f32>()?;
    let ndvi_data = ndvi.as_slice::<f32>()?;

    Ok(ndbi_data
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
        .collect())
}

fn compute_bare_soil_probability(
    ndvi: &RasterBuffer,
    ndbi: &RasterBuffer,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let ndvi_data = ndvi.as_slice::<f32>()?;
    let ndbi_data = ndbi.as_slice::<f32>()?;

    Ok(ndvi_data
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
        .collect())
}

fn create_classification_map(
    probabilities: &[RasterBuffer],
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let num_classes = probabilities.len();
    let width = probabilities[0].width();
    let height = probabilities[0].height();
    let size = (width * height) as usize;

    let prob_slices: Vec<&[f32]> = probabilities
        .iter()
        .map(|p| p.as_slice::<f32>())
        .collect::<Result<Vec<_>, _>>()?;

    let mut classification = vec![0.0f32; size];

    for i in 0..size {
        let mut max_prob = 0.0f32;
        let mut best_class = 0usize;

        for (class_idx, slice) in prob_slices.iter().enumerate().take(num_classes) {
            if slice[i] > max_prob {
                max_prob = slice[i];
                best_class = class_idx;
            }
        }

        classification[i] = best_class as f32;
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        classification,
        RasterDataType::Float32,
    )?)
}

fn calculate_confidence(
    probabilities: &[RasterBuffer],
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = probabilities[0].width();
    let height = probabilities[0].height();
    let size = (width * height) as usize;
    let mut confidence = vec![0.0f32; size];

    let prob_slices: Vec<&[f32]> = probabilities
        .iter()
        .map(|p| p.as_slice::<f32>())
        .collect::<Result<Vec<_>, _>>()?;

    for i in 0..size {
        let mut max_prob = 0.0f32;
        let mut second_max = 0.0f32;

        for slice in &prob_slices {
            if slice[i] > max_prob {
                second_max = max_prob;
                max_prob = slice[i];
            } else if slice[i] > second_max {
                second_max = slice[i];
            }
        }

        confidence[i] = max_prob - second_max;
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        confidence,
        RasterDataType::Float32,
    )?)
}

fn apply_confidence_filter(
    classification: &RasterBuffer,
    confidence: &RasterBuffer,
    threshold: f32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let class_data = classification.as_slice::<f32>()?;
    let conf_data = confidence.as_slice::<f32>()?;

    let filtered: Vec<f32> = class_data
        .iter()
        .zip(conf_data.iter())
        .map(|(&c, &conf)| if conf > threshold { c } else { -1.0 })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        classification.width() as usize,
        classification.height() as usize,
        filtered,
        RasterDataType::Float32,
    )?)
}

fn apply_modal_filter(
    classification: &RasterBuffer,
    radius: u64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = classification.width();
    let height = classification.height();
    let data = classification.as_slice::<f32>()?;
    let mut smoothed = data.to_vec();

    for y in radius..height - radius {
        for x in radius..width - radius {
            let mut counts: HashMap<i32, usize> = HashMap::new();

            for ky in -(radius as i64)..=(radius as i64) {
                for kx in -(radius as i64)..=(radius as i64) {
                    let ny = (y as i64 + ky) as u64;
                    let nx = (x as i64 + kx) as u64;

                    let val = data[(ny * width + nx) as usize] as i32;
                    *counts.entry(val).or_insert(0) += 1;
                }
            }

            if let Some((&modal_class, _)) = counts.iter().max_by_key(|&(_, &count)| count) {
                smoothed[(y * width + x) as usize] = modal_class as f32;
            }
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        smoothed,
        RasterDataType::Float32,
    )?)
}

fn create_reference_classification(
    width: u64,
    height: u64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = x as f64 / width as f64;
            let ny = y as f64 / height as f64;

            let class = if nx < 0.3 {
                0.0
            } else if nx < 0.6 && ny > 0.3 {
                1.0
            } else if ny < 0.5 {
                2.0
            } else {
                3.0
            };

            buffer.set_pixel(x, y, class)?;
        }
    }

    Ok(buffer)
}

fn compute_confusion_matrix(
    classified: &RasterBuffer,
    reference: &RasterBuffer,
    num_classes: usize,
) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
    let class_data = classified.as_slice::<f32>()?;
    let ref_data = reference.as_slice::<f32>()?;

    let mut matrix = vec![vec![0.0f32; num_classes]; num_classes];

    for (&c, &r) in class_data.iter().zip(ref_data.iter()) {
        let ci = c as usize;
        let ri = r as usize;

        if ci < num_classes && ri < num_classes && c >= 0.0 && r >= 0.0 {
            matrix[ri][ci] += 1.0;
        }
    }

    let total: f32 = matrix.iter().flatten().sum();
    if total > 0.0 {
        for row in &mut matrix {
            for val in row {
                *val /= total;
            }
        }
    }

    Ok(matrix)
}

fn compute_overall_accuracy(matrix: &[Vec<f32>]) -> f32 {
    matrix.iter().enumerate().map(|(i, row)| row[i]).sum()
}

fn compute_producer_accuracy(matrix: &[Vec<f32>]) -> Vec<f32> {
    let mut accuracies = vec![0.0f32; matrix.len()];

    for (i, acc) in accuracies.iter_mut().enumerate() {
        let col_sum: f32 = matrix.iter().map(|row| row[i]).sum();
        if col_sum > 0.0 {
            *acc = matrix[i][i] / col_sum;
        }
    }

    accuracies
}

fn compute_user_accuracy(matrix: &[Vec<f32>]) -> Vec<f32> {
    let mut accuracies = vec![0.0f32; matrix.len()];

    for (i, acc) in accuracies.iter_mut().enumerate() {
        let row_sum: f32 = matrix[i].iter().sum();
        if row_sum > 0.0 {
            *acc = matrix[i][i] / row_sum;
        }
    }

    accuracies
}

fn compute_kappa(matrix: &[Vec<f32>]) -> f32 {
    let po = compute_overall_accuracy(matrix);

    let mut pe = 0.0f32;
    for i in 0..matrix.len() {
        let row_sum: f32 = matrix[i].iter().sum();
        let col_sum: f32 = matrix.iter().map(|row| row[i]).sum();
        pe += row_sum * col_sum;
    }

    if pe < 1.0 {
        (po - pe) / (1.0 - pe)
    } else {
        0.0
    }
}

fn compute_class_statistics(
    classification: &RasterBuffer,
    num_classes: usize,
) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
    let data = classification.as_slice::<f32>()?;
    let mut counts = vec![0usize; num_classes];

    for &val in data {
        let idx = val as usize;
        if val >= 0.0 && idx < num_classes {
            counts[idx] += 1;
        }
    }

    Ok(counts)
}

fn create_geotransform(
    width: u64,
    height: u64,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 10.0, height as f64 * 10.0)?;
    Ok(GeoTransform::from_bounds(&bbox, width, height)?)
}

fn save_raster(
    raster: &RasterBuffer,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = WriterConfig::new(raster.width(), raster.height(), 1, raster.data_type())
        .with_compression(Compression::Deflate)
        .with_tile_size(64, 64)
        .with_geo_transform(*gt);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(raster.as_bytes())?;

    println!("  Saved: {}", path.display());
    Ok(())
}
