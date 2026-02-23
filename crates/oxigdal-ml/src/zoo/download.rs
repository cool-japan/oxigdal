//! Model download functionality

use super::cache::ModelCache;
use super::{ModelInfo, ModelSource};
use crate::error::{MlError, ModelError, Result};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Download progress callback
#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    /// Downloaded bytes
    pub downloaded: u64,
    /// Total bytes (if known)
    pub total: Option<u64>,
}

impl DownloadProgress {
    /// Returns download progress percentage
    #[must_use]
    pub fn percent(&self) -> Option<f32> {
        self.total
            .map(|total| (self.downloaded as f32 / total as f32) * 100.0)
    }
}

/// Model downloader
pub struct ModelDownloader {
    client: Client,
    show_progress: bool,
}

impl ModelDownloader {
    /// Creates a new downloader
    #[must_use]
    pub fn new() -> Self {
        Self::with_progress(true)
    }

    /// Creates a new downloader with configurable progress display
    #[must_use]
    pub fn with_progress(show_progress: bool) -> Self {
        let client = Client::builder()
            .user_agent("oxigdal-ml/0.1.0")
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            show_progress,
        }
    }

    /// Downloads a model and adds it to the cache
    ///
    /// # Errors
    /// Returns an error if download fails
    pub fn download(&self, model: &ModelInfo, cache: &mut ModelCache) -> Result<PathBuf> {
        info!("Downloading model: {}", model.name);

        match &model.source {
            ModelSource::Url(url) => self.download_from_url(url, model, cache),
            ModelSource::HuggingFace { repo_id, filename } => {
                self.download_from_huggingface(repo_id, filename, model, cache)
            }
            ModelSource::Local(path) => {
                let local_path = PathBuf::from(path);
                if !local_path.exists() {
                    return Err(ModelError::NotFound { path: path.clone() }.into());
                }
                cache.add(&model.name, local_path)
            }
        }
    }

    /// Downloads from HTTP(S) URL
    fn download_from_url(
        &self,
        url: &str,
        model: &ModelInfo,
        cache: &mut ModelCache,
    ) -> Result<PathBuf> {
        debug!("Downloading from URL: {}", url);

        // Create temporary file
        let temp_file = std::env::temp_dir().join(format!("{}.download", model.name));

        // Send GET request
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|e| ModelError::LoadFailed {
                reason: format!("HTTP request failed: {}", e),
            })?;

        if !response.status().is_success() {
            return Err(MlError::Model(ModelError::LoadFailed {
                reason: format!("HTTP error: {}", response.status()),
            }));
        }

        let total_size = response.content_length().unwrap_or(model.size_bytes);

        // Create progress bar
        let progress = if self.show_progress {
            let pb = ProgressBar::new(total_size);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}) {msg}")
                    .map_err(|e| ModelError::LoadFailed {
                        reason: format!("Progress bar setup failed: {}", e),
                    })?,
            );
            pb.set_message(format!("Downloading {}", model.name));
            Some(pb)
        } else {
            None
        };

        // Download with progress tracking
        let mut file = File::create(&temp_file).map_err(|e| ModelError::LoadFailed {
            reason: format!("Failed to create temporary file: {}", e),
        })?;

        let mut downloaded = 0u64;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];

        let mut reader = response;
        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .map_err(|e| ModelError::LoadFailed {
                    reason: format!("Download failed: {}", e),
                })?;

            if bytes_read == 0 {
                break;
            }

            file.write_all(&buffer[..bytes_read])
                .map_err(|e| ModelError::LoadFailed {
                    reason: format!("Failed to write to file: {}", e),
                })?;

            hasher.update(&buffer[..bytes_read]);
            downloaded += bytes_read as u64;

            if let Some(ref pb) = progress {
                pb.set_position(downloaded);
            }
        }

        if let Some(pb) = progress {
            pb.finish_with_message(format!("Downloaded {}", model.name));
        }

        // Verify checksum if provided
        if let Some(ref expected_checksum) = model.checksum {
            let computed_checksum = format!("{:x}", hasher.finalize());
            if &computed_checksum != expected_checksum {
                std::fs::remove_file(&temp_file).ok();
                return Err(MlError::Model(ModelError::LoadFailed {
                    reason: format!(
                        "Checksum mismatch: expected {}, got {}",
                        expected_checksum, computed_checksum
                    ),
                }));
            }
            info!("Checksum verified for model: {}", model.name);
        } else {
            debug!(
                "No checksum provided for model: {}, skipping verification",
                model.name
            );
        }

        cache.add(&model.name, temp_file)
    }

    /// Downloads from Hugging Face Hub
    fn download_from_huggingface(
        &self,
        repo_id: &str,
        filename: &str,
        model: &ModelInfo,
        cache: &mut ModelCache,
    ) -> Result<PathBuf> {
        debug!("Downloading from Hugging Face: {}/{}", repo_id, filename);

        // Construct Hugging Face Hub URL
        // Format: https://huggingface.co/{repo_id}/resolve/main/{filename}
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            repo_id, filename
        );

        info!("Downloading from Hugging Face Hub: {}", url);

        // Use the same download logic as HTTP
        self.download_from_url(&url, model, cache)
    }

    /// Computes the SHA256 checksum of a file
    ///
    /// # Errors
    /// Returns an error if file reading fails
    pub fn compute_checksum<P: AsRef<std::path::Path>>(path: P) -> Result<String> {
        let mut file = File::open(path).map_err(|e| ModelError::LoadFailed {
            reason: format!("Failed to open file for checksum: {}", e),
        })?;

        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];

        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| ModelError::LoadFailed {
                reason: format!("Failed to read file for checksum: {}", e),
            })?;

            if bytes_read == 0 {
                break;
            }

            hasher.update(&buffer[..bytes_read]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new()
    }
}
