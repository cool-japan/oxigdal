//! Pre-built model wrappers

/// ResNet classifier wrapper
pub struct ResNetClassifier {
    /// Model name
    pub name: String,
}

/// U-Net segmenter wrapper
pub struct UNetSegmenter {
    /// Model name
    pub name: String,
}

/// YOLO detector wrapper
pub struct YoloDetector {
    /// Model name
    pub name: String,
}

/// DeepLabV3 segmenter wrapper
pub struct DeepLabV3Segmenter {
    /// Model name
    pub name: String,
}

/// EfficientNet classifier wrapper
pub struct EfficientNetClassifier {
    /// Model name
    pub name: String,
}
