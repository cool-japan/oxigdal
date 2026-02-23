//! Mobile storage optimization
//!
//! This module provides storage optimization strategies for mobile devices
//! with limited storage capacity.

pub mod cache;
pub mod compression;

pub use cache::{CachePolicy, CacheStats, MobileCache};
pub use compression::{CompressionStrategy, StorageCompressor};

use crate::error::{MobileError, Result};
use std::path::{Path, PathBuf};

/// Storage usage information
#[derive(Debug, Clone)]
pub struct StorageInfo {
    /// Total storage capacity in bytes
    pub total_bytes: u64,
    /// Available storage in bytes
    pub available_bytes: u64,
    /// Used storage in bytes
    pub used_bytes: u64,
}

impl StorageInfo {
    /// Get storage usage percentage (0.0 - 100.0)
    pub fn usage_percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    /// Check if storage is running low (< 10% available)
    pub fn is_low_storage(&self) -> bool {
        let available_percentage = (self.available_bytes as f64 / self.total_bytes as f64) * 100.0;
        available_percentage < 10.0
    }

    /// Check if storage is critically low (< 5% available)
    pub fn is_critical_storage(&self) -> bool {
        let available_percentage = (self.available_bytes as f64 / self.total_bytes as f64) * 100.0;
        available_percentage < 5.0
    }
}

/// Mobile storage manager
pub struct MobileStorage {
    cache_dir: PathBuf,
    temp_dir: PathBuf,
}

impl MobileStorage {
    /// Create a new mobile storage manager
    pub fn new(cache_dir: PathBuf, temp_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            temp_dir,
        }
    }

    /// Get storage information
    pub fn storage_info(&self) -> Result<StorageInfo> {
        // In a real implementation, this would use platform-specific APIs
        // For now, return mock values
        Ok(StorageInfo {
            total_bytes: 64 * 1024 * 1024 * 1024,     // 64 GB
            available_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
            used_bytes: 54 * 1024 * 1024 * 1024,      // 54 GB
        })
    }

    /// Get cache directory
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Get temp directory
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    /// Clean up temporary files
    pub fn cleanup_temp(&self) -> Result<u64> {
        if !self.temp_dir.exists() {
            return Ok(0);
        }

        let mut cleaned_bytes = 0u64;

        let entries = std::fs::read_dir(&self.temp_dir)
            .map_err(|e| MobileError::StorageError(format!("Failed to read temp dir: {}", e)))?;

        for entry in entries {
            let entry = entry
                .map_err(|e| MobileError::StorageError(format!("Failed to read entry: {}", e)))?;
            let metadata = entry.metadata().map_err(|e| {
                MobileError::StorageError(format!("Failed to read metadata: {}", e))
            })?;

            if metadata.is_file() {
                cleaned_bytes = cleaned_bytes.saturating_add(metadata.len());
                std::fs::remove_file(entry.path()).map_err(|e| {
                    MobileError::StorageError(format!("Failed to remove file: {}", e))
                })?;
            }
        }

        Ok(cleaned_bytes)
    }

    /// Get directory size
    pub fn directory_size(&self, path: &Path) -> Result<u64> {
        if !path.exists() {
            return Ok(0);
        }

        let mut total_size = 0u64;

        let entries = std::fs::read_dir(path)
            .map_err(|e| MobileError::StorageError(format!("Failed to read directory: {}", e)))?;

        for entry in entries {
            let entry = entry
                .map_err(|e| MobileError::StorageError(format!("Failed to read entry: {}", e)))?;
            let metadata = entry.metadata().map_err(|e| {
                MobileError::StorageError(format!("Failed to read metadata: {}", e))
            })?;

            if metadata.is_file() {
                total_size = total_size.saturating_add(metadata.len());
            } else if metadata.is_dir() {
                total_size = total_size.saturating_add(self.directory_size(&entry.path())?);
            }
        }

        Ok(total_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_info() {
        let info = StorageInfo {
            total_bytes: 100 * 1024 * 1024 * 1024,    // 100 GB
            available_bytes: 20 * 1024 * 1024 * 1024, // 20 GB
            used_bytes: 80 * 1024 * 1024 * 1024,      // 80 GB
        };

        assert_eq!(info.usage_percentage(), 80.0);
        assert!(!info.is_low_storage());
        assert!(!info.is_critical_storage());
    }

    #[test]
    fn test_storage_info_low() {
        let info = StorageInfo {
            total_bytes: 100 * 1024 * 1024 * 1024,   // 100 GB
            available_bytes: 5 * 1024 * 1024 * 1024, // 5 GB
            used_bytes: 95 * 1024 * 1024 * 1024,     // 95 GB
        };

        assert!(info.is_low_storage());
    }

    #[test]
    fn test_mobile_storage() {
        let temp_dir = std::env::temp_dir();
        let cache_dir = temp_dir.join("cache");

        let storage = MobileStorage::new(cache_dir.clone(), temp_dir.clone());
        assert_eq!(storage.cache_dir(), cache_dir.as_path());
        assert_eq!(storage.temp_dir(), temp_dir.as_path());
    }
}
