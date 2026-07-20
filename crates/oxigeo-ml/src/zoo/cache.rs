//! Model cache management

use crate::error::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

/// Cache policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Keep all models
    KeepAll,
    /// LRU eviction
    Lru,
    /// Size-based eviction
    SizeBased {
        /// Maximum cache size in megabytes
        max_size_mb: u64,
    },
}

/// Model cache
pub struct ModelCache {
    cache_dir: PathBuf,
    models: HashMap<String, PathBuf>,
    policy: CachePolicy,
    cache_hits: u64,
    cache_misses: u64,
}

impl ModelCache {
    /// Creates a new model cache
    #[must_use]
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            models: HashMap::new(),
            policy: CachePolicy::KeepAll,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// Gets a cached model path
    pub fn get(&mut self, name: &str) -> Option<PathBuf> {
        if let Some(path) = self.models.get(name).cloned() {
            self.cache_hits += 1;
            Some(path)
        } else {
            self.cache_misses += 1;
            None
        }
    }

    /// Adds a model to the cache
    ///
    /// # Errors
    /// Returns an error if file operations fail
    pub fn add(&mut self, name: &str, source_path: PathBuf) -> Result<PathBuf> {
        let dest_path = self.cache_dir.join(name);

        // Ensure cache directory exists
        std::fs::create_dir_all(&self.cache_dir)?;

        // Copy model to cache
        if source_path != dest_path {
            std::fs::copy(&source_path, &dest_path)?;
        }

        self.models.insert(name.to_string(), dest_path.clone());
        debug!("Added model to cache: {} -> {:?}", name, dest_path);

        Ok(dest_path)
    }

    /// Clears the cache
    ///
    /// # Errors
    /// Returns an error if file operations fail
    pub fn clear(&mut self) -> Result<()> {
        info!("Clearing model cache");

        for path in self.models.values() {
            let _ = std::fs::remove_file(path);
        }

        self.models.clear();
        Ok(())
    }

    /// Returns cache statistics
    #[must_use]
    pub fn stats(&self) -> super::CacheStats {
        let total_size = self
            .models
            .values()
            .filter_map(|path| std::fs::metadata(path).ok())
            .map(|m| m.len())
            .sum();

        let total_requests = self.cache_hits + self.cache_misses;
        let hit_rate = if total_requests > 0 {
            self.cache_hits as f32 / total_requests as f32
        } else {
            0.0
        };

        super::CacheStats {
            num_models: self.models.len(),
            total_size,
            hit_rate,
        }
    }

    /// Returns the number of cache hits
    #[must_use]
    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    /// Returns the number of cache misses
    #[must_use]
    pub fn cache_misses(&self) -> u64 {
        self.cache_misses
    }

    /// Returns the current hit rate
    #[must_use]
    pub fn hit_rate(&self) -> f32 {
        let total = self.cache_hits + self.cache_misses;
        if total > 0 {
            self.cache_hits as f32 / total as f32
        } else {
            0.0
        }
    }
}
