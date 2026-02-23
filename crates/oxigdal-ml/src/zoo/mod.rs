//! Pre-trained model zoo for geospatial ML
//!
//! This module provides access to a curated collection of pre-trained models
//! for common geospatial ML tasks including classification, segmentation,
//! detection, and change detection.

pub mod cache;
pub mod download;
pub mod models;
pub mod registry;

pub use cache::{CachePolicy, ModelCache};
pub use download::{DownloadProgress, ModelDownloader};
pub use models::{
    DeepLabV3Segmenter, EfficientNetClassifier, ResNetClassifier, UNetSegmenter, YoloDetector,
};
pub use registry::{ModelInfo, ModelRegistry, ModelSource, ModelTask};

use crate::error::{ModelError, Result};
use std::path::PathBuf;
use tracing::info;

/// Model zoo for geospatial machine learning
pub struct ModelZoo {
    registry: ModelRegistry,
    cache: ModelCache,
    downloader: ModelDownloader,
}

impl ModelZoo {
    /// Creates a new model zoo
    ///
    /// # Errors
    /// Returns an error if initialization fails
    pub fn new() -> Result<Self> {
        Self::with_cache_dir(Self::default_cache_dir()?)
    }

    /// Creates a model zoo with a custom cache directory
    ///
    /// # Errors
    /// Returns an error if initialization fails
    pub fn with_cache_dir<P: Into<PathBuf>>(cache_dir: P) -> Result<Self> {
        let cache_path = cache_dir.into();
        info!("Initializing model zoo with cache dir: {:?}", cache_path);

        Ok(Self {
            registry: ModelRegistry::new(),
            cache: ModelCache::new(cache_path),
            downloader: ModelDownloader::new(),
        })
    }

    /// Returns the default cache directory
    fn default_cache_dir() -> Result<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| ModelError::LoadFailed {
                reason: "Failed to determine home directory".to_string(),
            })?;

        Ok(PathBuf::from(home).join(".oxigdal").join("models"))
    }

    /// Lists all available models in the registry
    #[must_use]
    pub fn list_models(&self) -> Vec<&ModelInfo> {
        self.registry.list_all()
    }

    /// Searches for models by task
    #[must_use]
    pub fn find_by_task(&self, task: ModelTask) -> Vec<&ModelInfo> {
        self.registry.find_by_task(task)
    }

    /// Gets a model by name
    ///
    /// Downloads the model if not already cached.
    ///
    /// # Errors
    /// Returns an error if the model cannot be found or downloaded
    pub fn get_model(&mut self, name: &str) -> Result<PathBuf> {
        info!("Getting model: {}", name);

        // Check if model exists in registry
        let model_info = self
            .registry
            .get(name)
            .ok_or_else(|| ModelError::NotFound {
                path: name.to_string(),
            })?;

        // Check cache
        if let Some(path) = self.cache.get(name) {
            info!("Model found in cache: {:?}", path);
            return Ok(path);
        }

        // Download model
        info!("Downloading model from {:?}", model_info.source);
        let path = self.downloader.download(model_info, &mut self.cache)?;

        Ok(path)
    }

    /// Clears the model cache
    ///
    /// # Errors
    /// Returns an error if cache clearing fails
    pub fn clear_cache(&mut self) -> Result<()> {
        info!("Clearing model cache");
        self.cache.clear()
    }

    /// Returns cache statistics
    #[must_use]
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cached models
    pub num_models: usize,
    /// Total cache size in bytes
    pub total_size: u64,
    /// Cache hit rate
    pub hit_rate: f32,
}

impl CacheStats {
    /// Returns the cache size in megabytes
    #[must_use]
    pub fn size_mb(&self) -> f32 {
        self.total_size as f32 / (1024.0 * 1024.0)
    }

    /// Returns the cache size in gigabytes
    #[must_use]
    pub fn size_gb(&self) -> f32 {
        self.total_size as f32 / (1024.0 * 1024.0 * 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_stats_conversions() {
        let stats = CacheStats {
            num_models: 5,
            total_size: 2_147_483_648, // 2 GB
            hit_rate: 0.85,
        };

        assert!((stats.size_mb() - 2048.0).abs() < 1.0);
        assert!((stats.size_gb() - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_model_zoo_creation() {
        let temp_dir = std::env::temp_dir().join("oxigdal_test_zoo");
        let zoo_result = ModelZoo::with_cache_dir(&temp_dir);
        assert!(zoo_result.is_ok());

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
