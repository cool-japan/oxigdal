//! Model registry with pre-trained model metadata

use std::collections::HashMap;

/// Model task categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTask {
    /// Image classification
    Classification,
    /// Semantic segmentation
    Segmentation,
    /// Object detection
    Detection,
    /// Change detection
    ChangeDetection,
    /// Super-resolution
    SuperResolution,
}

/// Model source information
#[derive(Debug, Clone)]
pub enum ModelSource {
    /// HTTP(S) URL
    Url(String),
    /// Hugging Face Hub
    HuggingFace {
        /// Repository ID
        repo_id: String,
        /// Filename
        filename: String,
    },
    /// Local file path
    Local(String),
}

/// Pre-trained model information
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Model name
    pub name: String,
    /// Model version
    pub version: String,
    /// Model task
    pub task: ModelTask,
    /// Model description
    pub description: String,
    /// Input shape (channels, height, width)
    pub input_shape: (usize, usize, usize),
    /// Number of output classes
    pub num_classes: usize,
    /// Model source
    pub source: ModelSource,
    /// Model format (onnx, tflite, etc.)
    pub format: String,
    /// Model size in bytes
    pub size_bytes: u64,
    /// Model accuracy (if known)
    pub accuracy: Option<f32>,
    /// SHA256 checksum for validation
    pub checksum: Option<String>,
}

/// Model registry
pub struct ModelRegistry {
    models: HashMap<String, ModelInfo>,
}

impl ModelRegistry {
    /// Creates a new registry with pre-populated models
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
            models: HashMap::new(),
        };
        registry.populate_models();
        registry
    }

    /// Populates the registry with pre-trained models
    fn populate_models(&mut self) {
        // ResNet50 for land cover classification
        self.register(ModelInfo {
            name: "resnet50_landcover".to_string(),
            version: "1.0.0".to_string(),
            task: ModelTask::Classification,
            description: "ResNet50 model for land cover classification (10 classes)".to_string(),
            input_shape: (3, 224, 224),
            num_classes: 10,
            source: ModelSource::Url(
                "https://example.com/models/resnet50_landcover.onnx".to_string(),
            ),
            format: "onnx".to_string(),
            size_bytes: 97_800_000, // ~98 MB
            accuracy: Some(92.5),
            checksum: None, // Would be actual SHA256 in production
        });

        // U-Net for building segmentation
        self.register(ModelInfo {
            name: "unet_buildings".to_string(),
            version: "1.0.0".to_string(),
            task: ModelTask::Segmentation,
            description: "U-Net model for building segmentation from satellite imagery".to_string(),
            input_shape: (3, 256, 256),
            num_classes: 2,
            source: ModelSource::Url("https://example.com/models/unet_buildings.onnx".to_string()),
            format: "onnx".to_string(),
            size_bytes: 31_000_000, // ~31 MB
            accuracy: Some(88.3),
            checksum: None,
        });

        // YOLOv5 for vehicle detection
        self.register(ModelInfo {
            name: "yolov5_vehicles".to_string(),
            version: "5.0.0".to_string(),
            task: ModelTask::Detection,
            description: "YOLOv5 model for vehicle detection in aerial imagery".to_string(),
            input_shape: (3, 640, 640),
            num_classes: 3,
            source: ModelSource::Url("https://example.com/models/yolov5_vehicles.onnx".to_string()),
            format: "onnx".to_string(),
            size_bytes: 14_100_000, // ~14 MB
            accuracy: Some(85.7),
            checksum: None,
        });

        // DeepLabV3 for agricultural field segmentation
        self.register(ModelInfo {
            name: "deeplabv3_fields".to_string(),
            version: "3.0.0".to_string(),
            task: ModelTask::Segmentation,
            description: "DeepLabV3 for agricultural field boundary segmentation".to_string(),
            input_shape: (3, 512, 512),
            num_classes: 5,
            source: ModelSource::Url(
                "https://example.com/models/deeplabv3_fields.onnx".to_string(),
            ),
            format: "onnx".to_string(),
            size_bytes: 168_000_000, // ~168 MB
            accuracy: Some(91.2),
            checksum: None,
        });

        // EfficientNet for crop type classification
        self.register(ModelInfo {
            name: "efficientnet_crops".to_string(),
            version: "1.0.0".to_string(),
            task: ModelTask::Classification,
            description: "EfficientNet-B0 for crop type classification".to_string(),
            input_shape: (3, 224, 224),
            num_classes: 15,
            source: ModelSource::Url(
                "https://example.com/models/efficientnet_crops.onnx".to_string(),
            ),
            format: "onnx".to_string(),
            size_bytes: 20_500_000, // ~20 MB
            accuracy: Some(89.8),
            checksum: None,
        });

        // Change detection model
        self.register(ModelInfo {
            name: "siamese_change".to_string(),
            version: "1.0.0".to_string(),
            task: ModelTask::ChangeDetection,
            description: "Siamese network for change detection in multi-temporal imagery"
                .to_string(),
            input_shape: (6, 256, 256), // Two 3-channel images concatenated
            num_classes: 2,
            source: ModelSource::Url("https://example.com/models/siamese_change.onnx".to_string()),
            format: "onnx".to_string(),
            size_bytes: 45_000_000, // ~45 MB
            accuracy: Some(86.5),
            checksum: None,
        });

        // Add more pretrained models
        // MobileNetV3 for lightweight classification
        self.register(ModelInfo {
            name: "mobilenet_v3_landcover".to_string(),
            version: "3.0.0".to_string(),
            task: ModelTask::Classification,
            description: "MobileNetV3 for efficient land cover classification".to_string(),
            input_shape: (3, 224, 224),
            num_classes: 10,
            source: ModelSource::Url(
                "https://example.com/models/mobilenet_v3_landcover.onnx".to_string(),
            ),
            format: "onnx".to_string(),
            size_bytes: 5_400_000, // ~5 MB
            accuracy: Some(88.5),
            checksum: None,
        });

        // SegFormer for semantic segmentation
        self.register(ModelInfo {
            name: "segformer_b0_roads".to_string(),
            version: "1.0.0".to_string(),
            task: ModelTask::Segmentation,
            description: "SegFormer-B0 for road segmentation from satellite imagery".to_string(),
            input_shape: (3, 512, 512),
            num_classes: 2,
            source: ModelSource::Url(
                "https://example.com/models/segformer_b0_roads.onnx".to_string(),
            ),
            format: "onnx".to_string(),
            size_bytes: 13_200_000, // ~13 MB
            accuracy: Some(90.1),
            checksum: None,
        });

        // ESRGAN for super-resolution
        self.register(ModelInfo {
            name: "esrgan_2x".to_string(),
            version: "1.0.0".to_string(),
            task: ModelTask::SuperResolution,
            description: "ESRGAN for 2x image super-resolution".to_string(),
            input_shape: (3, 256, 256),
            num_classes: 1, // Not applicable for super-resolution
            source: ModelSource::Url("https://example.com/models/esrgan_2x.onnx".to_string()),
            format: "onnx".to_string(),
            size_bytes: 16_700_000, // ~17 MB
            accuracy: None,         // PSNR/SSIM would be more appropriate
            checksum: None,
        });
    }

    /// Registers a model in the registry
    pub fn register(&mut self, model: ModelInfo) {
        self.models.insert(model.name.clone(), model);
    }

    /// Gets a model by name
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ModelInfo> {
        self.models.get(name)
    }

    /// Lists all models
    #[must_use]
    pub fn list_all(&self) -> Vec<&ModelInfo> {
        self.models.values().collect()
    }

    /// Finds models by task
    #[must_use]
    pub fn find_by_task(&self, task: ModelTask) -> Vec<&ModelInfo> {
        self.models.values().filter(|m| m.task == task).collect()
    }

    /// Returns the number of registered models
    #[must_use]
    pub fn count(&self) -> usize {
        self.models.len()
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_population() {
        let registry = ModelRegistry::new();
        assert!(registry.count() > 0);
    }

    #[test]
    fn test_find_by_task() {
        let registry = ModelRegistry::new();
        let classifiers = registry.find_by_task(ModelTask::Classification);
        assert!(!classifiers.is_empty());

        let segmenters = registry.find_by_task(ModelTask::Segmentation);
        assert!(!segmenters.is_empty());
    }

    #[test]
    fn test_get_model() {
        let registry = ModelRegistry::new();
        let model = registry.get("resnet50_landcover");
        assert!(model.is_some());

        if let Some(m) = model {
            assert_eq!(m.task, ModelTask::Classification);
            assert_eq!(m.num_classes, 10);
        }
    }
}
