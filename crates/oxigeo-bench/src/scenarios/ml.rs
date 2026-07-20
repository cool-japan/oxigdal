//! ML inference benchmark scenarios.
//!
//! This module provides benchmark scenarios for ML operations including:
//! - ONNX model inference
//! - Batch processing performance
//! - Preprocessing overhead
//! - Postprocessing performance
//! - End-to-end inference pipeline

use crate::error::{BenchError, Result};
use crate::scenarios::BenchmarkScenario;
use std::path::PathBuf;

/// ML task types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlTask {
    /// Image classification.
    Classification,
    /// Object detection.
    Detection,
    /// Semantic segmentation.
    Segmentation,
    /// Instance segmentation.
    InstanceSegmentation,
}

/// ONNX inference benchmark scenario.
pub struct OnnxInferenceScenario {
    model_path: PathBuf,
    input_shape: Vec<usize>,
    batch_size: usize,
    task_type: MlTask,
    warmup_iterations: usize,
    benchmark_iterations: usize,
}

impl OnnxInferenceScenario {
    /// Creates a new ONNX inference benchmark scenario.
    pub fn new<P>(model_path: P, input_shape: Vec<usize>) -> Self
    where
        P: Into<PathBuf>,
    {
        Self {
            model_path: model_path.into(),
            input_shape,
            batch_size: 1,
            task_type: MlTask::Classification,
            warmup_iterations: 10,
            benchmark_iterations: 100,
        }
    }

    /// Sets the batch size.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Sets the task type.
    pub fn with_task_type(mut self, task_type: MlTask) -> Self {
        self.task_type = task_type;
        self
    }

    /// Sets the warmup iterations.
    pub fn with_warmup_iterations(mut self, iterations: usize) -> Self {
        self.warmup_iterations = iterations;
        self
    }

    /// Sets the benchmark iterations.
    pub fn with_benchmark_iterations(mut self, iterations: usize) -> Self {
        self.benchmark_iterations = iterations;
        self
    }
}

impl BenchmarkScenario for OnnxInferenceScenario {
    fn name(&self) -> &str {
        "onnx_inference"
    }

    fn description(&self) -> &str {
        "Benchmark ONNX model inference performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.model_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Model file does not exist: {}", self.model_path.display()),
            ));
        }

        if self.input_shape.is_empty() {
            return Err(BenchError::InvalidConfiguration(
                "Input shape cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "ml")]
        {
            // Simulate ONNX inference with synthetic tensors
            let input_size: usize = self.input_shape.iter().product();
            let input_data: Vec<f32> = (0..input_size * self.batch_size)
                .map(|i| (i as f32 * 0.001_f32).sin())
                .collect();

            // Warmup iterations: simulate lightweight forward pass
            for _ in 0..self.warmup_iterations {
                let _out: Vec<f32> = input_data
                    .iter()
                    .map(|&x| 1.0_f32 / (1.0_f32 + (-x).exp())) // sigmoid activation
                    .collect();
            }

            // Benchmark iterations: simulate full matmul-like operation
            let hidden_size = 256usize;
            for _ in 0..self.benchmark_iterations {
                let _out: Vec<f32> = (0..hidden_size)
                    .map(|j| {
                        input_data.iter().enumerate().fold(0.0_f32, |acc, (i, &x)| {
                            acc + x * ((i + j) as f32 * 0.001_f32).cos()
                        })
                    })
                    .collect();
            }
        }

        #[cfg(not(feature = "ml"))]
        {
            return Err(BenchError::missing_dependency("oxigeo-ml", "ml"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Batch processing benchmark scenario.
pub struct BatchProcessingScenario {
    model_path: PathBuf,
    input_dir: PathBuf,
    batch_sizes: Vec<usize>,
    task_type: MlTask,
}

impl BatchProcessingScenario {
    /// Creates a new batch processing benchmark scenario.
    pub fn new<P1, P2>(model_path: P1, input_dir: P2) -> Self
    where
        P1: Into<PathBuf>,
        P2: Into<PathBuf>,
    {
        Self {
            model_path: model_path.into(),
            input_dir: input_dir.into(),
            batch_sizes: vec![1, 4, 8, 16, 32],
            task_type: MlTask::Classification,
        }
    }

    /// Sets the batch sizes to benchmark.
    pub fn with_batch_sizes(mut self, sizes: Vec<usize>) -> Self {
        self.batch_sizes = sizes;
        self
    }

    /// Sets the task type.
    pub fn with_task_type(mut self, task_type: MlTask) -> Self {
        self.task_type = task_type;
        self
    }
}

impl BenchmarkScenario for BatchProcessingScenario {
    fn name(&self) -> &str {
        "batch_processing"
    }

    fn description(&self) -> &str {
        "Benchmark batch processing performance with different batch sizes"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.model_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Model file does not exist: {}", self.model_path.display()),
            ));
        }

        if !self.input_dir.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!(
                    "Input directory does not exist: {}",
                    self.input_dir.display()
                ),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "ml")]
        {
            // Simulate batch processing with varying batch sizes
            // Generate synthetic image data (3-channel, 224x224 = 150528 floats per image)
            let image_size = 3 * 224 * 224usize;
            let total_images = 32usize;
            let images: Vec<Vec<f32>> = (0..total_images)
                .map(|img_idx| {
                    (0..image_size)
                        .map(|i| ((img_idx + i) as f32 * 0.001_f32).sin())
                        .collect()
                })
                .collect();

            for &batch_size in &self.batch_sizes {
                for chunk in images.chunks(batch_size) {
                    // Flatten batch into contiguous tensor
                    let batch_tensor: Vec<f32> =
                        chunk.iter().flat_map(|img| img.iter().copied()).collect();
                    // Simulate softmax over 1000 classes for each image in batch
                    let logits: Vec<f32> = (0..chunk.len() * 1000)
                        .map(|i| batch_tensor[i % batch_tensor.len()])
                        .collect();
                    let _max_class: usize = logits
                        .iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| {
                            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
        }

        #[cfg(not(feature = "ml"))]
        {
            return Err(BenchError::missing_dependency("oxigeo-ml", "ml"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Preprocessing benchmark scenario.
pub struct PreprocessingScenario {
    input_dir: PathBuf,
    preprocessing_steps: Vec<PreprocessingStep>,
    image_count: usize,
}

/// Preprocessing steps.
#[derive(Debug, Clone, Copy)]
pub enum PreprocessingStep {
    /// Resize to target dimensions.
    Resize,
    /// Normalize pixel values.
    Normalize,
    /// Convert color space.
    ColorConversion,
    /// Apply data augmentation.
    Augmentation,
}

impl PreprocessingScenario {
    /// Creates a new preprocessing benchmark scenario.
    pub fn new<P>(input_dir: P) -> Self
    where
        P: Into<PathBuf>,
    {
        Self {
            input_dir: input_dir.into(),
            preprocessing_steps: vec![PreprocessingStep::Resize, PreprocessingStep::Normalize],
            image_count: 100,
        }
    }

    /// Sets the preprocessing steps.
    pub fn with_steps(mut self, steps: Vec<PreprocessingStep>) -> Self {
        self.preprocessing_steps = steps;
        self
    }

    /// Sets the number of images to process.
    pub fn with_image_count(mut self, count: usize) -> Self {
        self.image_count = count;
        self
    }
}

impl BenchmarkScenario for PreprocessingScenario {
    fn name(&self) -> &str {
        "preprocessing"
    }

    fn description(&self) -> &str {
        "Benchmark image preprocessing performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_dir.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!(
                    "Input directory does not exist: {}",
                    self.input_dir.display()
                ),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "ml")]
        {
            // Simulate image preprocessing pipeline
            let image_width = 640usize;
            let image_height = 480usize;
            let channels = 3usize;
            for img_idx in 0..self.image_count {
                let raw: Vec<u8> = (0..image_width * image_height * channels)
                    .map(|i| ((img_idx * 7 + i) % 256) as u8)
                    .collect();
                let mut processed: Vec<f32> = raw.iter().map(|&b| b as f32).collect();
                for step in &self.preprocessing_steps {
                    processed = match step {
                        PreprocessingStep::Resize => {
                            // Bilinear downsample to 224x224x3
                            let target = 224usize;
                            (0..target * target * channels)
                                .map(|i| {
                                    let c = i % channels;
                                    let px = (i / channels) % target;
                                    let py = i / channels / target;
                                    let src_x = px * image_width / target;
                                    let src_y = py * image_height / target;
                                    processed[(src_y * image_width + src_x) * channels + c]
                                })
                                .collect()
                        }
                        PreprocessingStep::Normalize => {
                            // ImageNet normalization: (x/255 - mean) / std
                            let mean = [0.485_f32, 0.456, 0.406];
                            let std_vals = [0.229_f32, 0.224, 0.225];
                            processed
                                .iter()
                                .enumerate()
                                .map(|(i, &v)| {
                                    let c = i % channels;
                                    (v / 255.0 - mean[c]) / std_vals[c]
                                })
                                .collect()
                        }
                        PreprocessingStep::ColorConversion => {
                            // RGB → BGR swap
                            let mut bgr = processed.clone();
                            for px in 0..(bgr.len() / channels) {
                                bgr.swap(px * channels, px * channels + 2);
                            }
                            bgr
                        }
                        PreprocessingStep::Augmentation => {
                            // Horizontal flip
                            let w = if processed.len() > channels {
                                processed.len() / channels
                            } else {
                                1
                            };
                            let mut flipped = processed.clone();
                            for row in 0..(flipped.len() / channels / w.max(1)) {
                                for col in 0..(w / 2) {
                                    for c in 0..channels {
                                        let a = (row * w + col) * channels + c;
                                        let b_idx = (row * w + (w - 1 - col)) * channels + c;
                                        if a < flipped.len() && b_idx < flipped.len() {
                                            flipped.swap(a, b_idx);
                                        }
                                    }
                                }
                            }
                            flipped
                        }
                    };
                }
                let _ = processed;
            }
        }

        #[cfg(not(feature = "ml"))]
        {
            return Err(BenchError::missing_dependency("oxigeo-ml", "ml"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Postprocessing benchmark scenario.
pub struct PostprocessingScenario {
    #[allow(dead_code)]
    task_type: MlTask,
    result_count: usize,
    nms_threshold: f32,
}

impl PostprocessingScenario {
    /// Creates a new postprocessing benchmark scenario.
    pub fn new(task_type: MlTask) -> Self {
        Self {
            task_type,
            result_count: 1000,
            nms_threshold: 0.5,
        }
    }

    /// Sets the number of results to process.
    pub fn with_result_count(mut self, count: usize) -> Self {
        self.result_count = count;
        self
    }

    /// Sets the NMS threshold for detection tasks.
    pub fn with_nms_threshold(mut self, threshold: f32) -> Self {
        self.nms_threshold = threshold;
        self
    }
}

impl BenchmarkScenario for PostprocessingScenario {
    fn name(&self) -> &str {
        "postprocessing"
    }

    fn description(&self) -> &str {
        "Benchmark postprocessing performance (NMS, etc.)"
    }

    fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "ml")]
        {
            // Generate dummy predictions for postprocessing benchmark
            let num_classes = 1000usize;
            match self.task_type {
                MlTask::Classification => {
                    // Softmax + argmax over result_count independent predictions
                    for _ in 0..self.result_count {
                        let logits: Vec<f32> = (0..num_classes)
                            .map(|i| (i as f32 * 0.01_f32).sin())
                            .collect();
                        // Numerically stable softmax
                        let max_logit = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                        let exp_sum: f32 = logits.iter().map(|&x| (x - max_logit).exp()).sum();
                        let probs: Vec<f32> = logits
                            .iter()
                            .map(|&x| (x - max_logit).exp() / exp_sum)
                            .collect();
                        let _argmax = probs
                            .iter()
                            .enumerate()
                            .max_by(|(_, a), (_, b)| {
                                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                            })
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                    }
                }
                MlTask::Detection => {
                    // Non-maximum suppression simulation
                    let boxes: Vec<(f32, f32, f32, f32)> = (0..self.result_count)
                        .map(|i| {
                            let x = (i % 10) as f32 * 100.0;
                            let y = (i / 10) as f32 * 100.0;
                            (x, y, x + 80.0, y + 80.0)
                        })
                        .collect();
                    let scores: Vec<f32> = (0..self.result_count)
                        .map(|i| i as f32 / self.result_count as f32)
                        .collect();
                    // Greedy NMS
                    let mut kept = vec![true; boxes.len()];
                    for i in 0..boxes.len() {
                        if !kept[i] {
                            continue;
                        }
                        for j in (i + 1)..boxes.len() {
                            if !kept[j] {
                                continue;
                            }
                            let iou = compute_iou(boxes[i], boxes[j]);
                            if iou > self.nms_threshold {
                                // Suppress lower-score box
                                if scores[i] >= scores[j] {
                                    kept[j] = false;
                                } else {
                                    kept[i] = false;
                                    break;
                                }
                            }
                        }
                    }
                    let _ = kept;
                }
                MlTask::Segmentation | MlTask::InstanceSegmentation => {
                    // Argmax per pixel for a segmentation map
                    let h = 224usize;
                    let w = 224usize;
                    let logit_map: Vec<f32> = (0..h * w * num_classes)
                        .map(|i| ((i % num_classes) as f32 * 0.001_f32).sin())
                        .collect();
                    let _mask: Vec<usize> = (0..h * w)
                        .map(|px| {
                            let base = px * num_classes;
                            logit_map[base..base + num_classes]
                                .iter()
                                .enumerate()
                                .max_by(|(_, a), (_, b)| {
                                    a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                                })
                                .map(|(c, _)| c)
                                .unwrap_or(0)
                        })
                        .collect();
                }
            }
        }

        #[cfg(not(feature = "ml"))]
        {
            return Err(BenchError::missing_dependency("oxigeo-ml", "ml"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// End-to-end inference pipeline benchmark.
pub struct EndToEndPipelineScenario {
    model_path: PathBuf,
    input_dir: PathBuf,
    #[allow(dead_code)]
    task_type: MlTask,
    batch_size: usize,
    pipeline_count: usize,
}

impl EndToEndPipelineScenario {
    /// Creates a new end-to-end pipeline benchmark scenario.
    pub fn new<P1, P2>(model_path: P1, input_dir: P2, task_type: MlTask) -> Self
    where
        P1: Into<PathBuf>,
        P2: Into<PathBuf>,
    {
        Self {
            model_path: model_path.into(),
            input_dir: input_dir.into(),
            task_type,
            batch_size: 4,
            pipeline_count: 50,
        }
    }

    /// Sets the batch size.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Sets the number of pipeline iterations.
    pub fn with_pipeline_count(mut self, count: usize) -> Self {
        self.pipeline_count = count;
        self
    }
}

impl BenchmarkScenario for EndToEndPipelineScenario {
    fn name(&self) -> &str {
        "end_to_end_pipeline"
    }

    fn description(&self) -> &str {
        "Benchmark end-to-end inference pipeline (preprocessing + inference + postprocessing)"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.model_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Model file does not exist: {}", self.model_path.display()),
            ));
        }

        if !self.input_dir.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!(
                    "Input directory does not exist: {}",
                    self.input_dir.display()
                ),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "ml")]
        {
            // Simulate end-to-end pipeline: preprocessing + inference + postprocessing
            let image_size = 3 * self.batch_size * (self.batch_size * 64) * (self.batch_size * 64);
            let raw_images: Vec<u8> = (0..image_size).map(|i| (i % 256) as u8).collect();
            for _pipeline_iter in 0..self.pipeline_count {
                // Step 1: Preprocessing — normalize to f32
                let preprocessed: Vec<f32> =
                    raw_images.iter().map(|&b| b as f32 / 255.0 - 0.5).collect();
                // Step 2: Inference — dot product "forward pass"
                let hidden_size = 128usize;
                let layer_out: Vec<f32> = (0..hidden_size)
                    .map(|j| {
                        preprocessed
                            .iter()
                            .enumerate()
                            .fold(0.0_f32, |acc, (i, &x)| {
                                acc + x * ((i + j) as f32 * 0.001_f32).cos()
                            })
                            / preprocessed.len() as f32
                    })
                    .collect();
                // Step 3: Postprocessing — softmax
                let max_val = layer_out.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let exp_sum: f32 = layer_out.iter().map(|&x| (x - max_val).exp()).sum();
                let _probs: Vec<f32> = layer_out
                    .iter()
                    .map(|&x| (x - max_val).exp() / exp_sum)
                    .collect();
            }
        }

        #[cfg(not(feature = "ml"))]
        {
            return Err(BenchError::missing_dependency("oxigeo-ml", "ml"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Compute IoU (Intersection over Union) of two axis-aligned bounding boxes.
fn compute_iou(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> f32 {
    let ix1 = a.0.max(b.0);
    let iy1 = a.1.max(b.1);
    let ix2 = a.2.min(b.2);
    let iy2 = a.3.min(b.3);
    let inter_w = (ix2 - ix1).max(0.0);
    let inter_h = (iy2 - iy1).max(0.0);
    let intersection = inter_w * inter_h;
    let area_a = (a.2 - a.0).max(0.0) * (a.3 - a.1).max(0.0);
    let area_b = (b.2 - b.0).max(0.0) * (b.3 - b.1).max(0.0);
    let union = area_a + area_b - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onnx_inference_scenario_creation() {
        let scenario = OnnxInferenceScenario::new(
            std::env::temp_dir().join("model.onnx"),
            vec![1, 3, 224, 224],
        )
        .with_batch_size(8)
        .with_task_type(MlTask::Segmentation)
        .with_warmup_iterations(20);

        assert_eq!(scenario.name(), "onnx_inference");
        assert_eq!(scenario.batch_size, 8);
        assert_eq!(scenario.warmup_iterations, 20);
    }

    #[test]
    fn test_batch_processing_scenario_creation() {
        let scenario = BatchProcessingScenario::new(
            std::env::temp_dir().join("model.onnx"),
            std::env::temp_dir().join("images"),
        )
        .with_batch_sizes(vec![2, 4, 8])
        .with_task_type(MlTask::Detection);

        assert_eq!(scenario.name(), "batch_processing");
        assert_eq!(scenario.batch_sizes.len(), 3);
    }

    #[test]
    fn test_preprocessing_scenario_creation() {
        let scenario = PreprocessingScenario::new(std::env::temp_dir().join("images"))
            .with_steps(vec![
                PreprocessingStep::Resize,
                PreprocessingStep::Normalize,
                PreprocessingStep::ColorConversion,
            ])
            .with_image_count(50);

        assert_eq!(scenario.name(), "preprocessing");
        assert_eq!(scenario.preprocessing_steps.len(), 3);
        assert_eq!(scenario.image_count, 50);
    }

    #[test]
    fn test_postprocessing_scenario_creation() {
        let scenario = PostprocessingScenario::new(MlTask::Detection)
            .with_result_count(500)
            .with_nms_threshold(0.4);

        assert_eq!(scenario.name(), "postprocessing");
        assert_eq!(scenario.result_count, 500);
        assert_eq!(scenario.nms_threshold, 0.4);
    }

    #[test]
    fn test_end_to_end_pipeline_scenario_creation() {
        let scenario = EndToEndPipelineScenario::new(
            std::env::temp_dir().join("model.onnx"),
            std::env::temp_dir().join("images"),
            MlTask::Classification,
        )
        .with_batch_size(16)
        .with_pipeline_count(100);

        assert_eq!(scenario.name(), "end_to_end_pipeline");
        assert_eq!(scenario.batch_size, 16);
        assert_eq!(scenario.pipeline_count, 100);
    }
}
