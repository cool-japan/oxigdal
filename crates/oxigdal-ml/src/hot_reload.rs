//! ONNX model hot-reload with file watching
//!
//! Watches a model file for changes and automatically reloads it.
//! Uses atomic swapping so inference can continue during reload.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use crate::error::MlError;

/// Model reload event
#[derive(Debug, Clone)]
pub struct ReloadEvent {
    /// Path to the model file that changed
    pub path: PathBuf,
    /// Timestamp of the detected change
    pub timestamp: SystemTime,
    /// Version counter at the time of the event
    pub version: u64,
}

/// Configuration for hot-reload behavior
#[derive(Debug, Clone)]
pub struct HotReloadConfig {
    /// How often to check for file changes
    pub poll_interval: Duration,
    /// Maximum time to wait for a reload to complete
    pub reload_timeout: Duration,
    /// Whether to validate the model before swapping
    pub validate_before_swap: bool,
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            reload_timeout: Duration::from_secs(30),
            validate_before_swap: true,
        }
    }
}

/// Hot-reload state tracker (does NOT depend on wgpu or onnxruntime)
///
/// This struct watches a model file for modifications and tracks version
/// information for atomic model swapping during live inference.
pub struct ModelWatcher {
    path: PathBuf,
    config: HotReloadConfig,
    last_modified: Arc<RwLock<Option<SystemTime>>>,
    version: Arc<RwLock<u64>>,
    reload_count: Arc<RwLock<u64>>,
}

impl ModelWatcher {
    /// Create a new `ModelWatcher` for the given file path and configuration.
    pub fn new(path: impl AsRef<Path>, config: HotReloadConfig) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            config,
            last_modified: Arc::new(RwLock::new(None)),
            version: Arc::new(RwLock::new(0)),
            reload_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Check if the file has been modified since the last check.
    ///
    /// Returns `Ok(Some(ReloadEvent))` when a change is detected,
    /// `Ok(None)` when the file is unchanged or does not exist yet,
    /// and `Err` on lock-poisoning failures.
    pub fn check_for_update(&self) -> Result<Option<ReloadEvent>, MlError> {
        // If the file doesn't exist, treat as no update available
        let metadata = match std::fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(e) => return Err(MlError::Io(e)),
        };

        let current_mtime = metadata.modified().map_err(MlError::Io)?;

        let mut last_modified = self
            .last_modified
            .write()
            .map_err(|_| MlError::InvalidConfig("lock poisoned: last_modified".into()))?;

        let changed = match *last_modified {
            None => {
                // First check — record mtime but do not fire a reload event
                *last_modified = Some(current_mtime);
                false
            }
            Some(prev) => current_mtime > prev,
        };

        if changed {
            *last_modified = Some(current_mtime);
            let version = self
                .version
                .read()
                .map_err(|_| MlError::InvalidConfig("lock poisoned: version".into()))?;
            return Ok(Some(ReloadEvent {
                path: self.path.clone(),
                timestamp: current_mtime,
                version: *version,
            }));
        }

        Ok(None)
    }

    /// Mark a reload as completed, incrementing the version counter.
    ///
    /// Returns the new version number.
    pub fn mark_reloaded(&self) -> Result<u64, MlError> {
        let mut version = self
            .version
            .write()
            .map_err(|_| MlError::InvalidConfig("lock poisoned: version".into()))?;
        *version += 1;

        let mut reload_count = self
            .reload_count
            .write()
            .map_err(|_| MlError::InvalidConfig("lock poisoned: reload_count".into()))?;
        *reload_count += 1;

        Ok(*version)
    }

    /// Return the current model version counter.
    pub fn current_version(&self) -> Result<u64, MlError> {
        let version = self
            .version
            .read()
            .map_err(|_| MlError::InvalidConfig("lock poisoned: version".into()))?;
        Ok(*version)
    }

    /// Return the total number of completed reloads.
    pub fn reload_count(&self) -> Result<u64, MlError> {
        let count = self
            .reload_count
            .read()
            .map_err(|_| MlError::InvalidConfig("lock poisoned: reload_count".into()))?;
        Ok(*count)
    }

    /// Return the path being watched.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return a reference to the watcher configuration.
    pub fn config(&self) -> &HotReloadConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn default_watcher(path: impl AsRef<Path>) -> ModelWatcher {
        ModelWatcher::new(path, HotReloadConfig::default())
    }

    #[test]
    fn test_construction() {
        let watcher = default_watcher("/tmp/nonexistent_test_model.onnx");
        assert_eq!(
            watcher.path(),
            Path::new("/tmp/nonexistent_test_model.onnx")
        );
    }

    #[test]
    fn test_default_config() {
        let config = HotReloadConfig::default();
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert_eq!(config.reload_timeout, Duration::from_secs(30));
        assert!(config.validate_before_swap);
    }

    #[test]
    fn test_check_nonexistent_file() {
        let watcher = default_watcher("/tmp/this_file_absolutely_does_not_exist_oxigdal.onnx");
        let result = watcher.check_for_update();
        assert!(result.is_ok());
        assert!(result.expect("should be ok").is_none());
    }

    #[test]
    fn test_check_existing_file_first_call_no_event() {
        let dir = std::env::temp_dir();
        let path = dir.join("oxigdal_hot_reload_test_first_call.onnx");
        fs::write(&path, b"dummy model data").expect("write");

        let watcher = default_watcher(&path);
        // First call should record mtime but return None (no prior state)
        let result = watcher.check_for_update().expect("check");
        assert!(result.is_none(), "first check should not fire reload event");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_check_file_unchanged_returns_none() {
        let dir = std::env::temp_dir();
        let path = dir.join("oxigdal_hot_reload_unchanged.onnx");
        fs::write(&path, b"dummy model").expect("write");

        let watcher = default_watcher(&path);
        // First call — establish baseline
        let _ = watcher.check_for_update().expect("check 1");
        // Second call — same mtime, should be None
        let result = watcher.check_for_update().expect("check 2");
        assert!(result.is_none(), "unchanged file should return None");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_mark_reloaded_increments_version() {
        let watcher = default_watcher("/tmp/dummy.onnx");
        assert_eq!(watcher.current_version().expect("v"), 0);

        let v1 = watcher.mark_reloaded().expect("reload 1");
        assert_eq!(v1, 1);

        let v2 = watcher.mark_reloaded().expect("reload 2");
        assert_eq!(v2, 2);

        assert_eq!(watcher.current_version().expect("cv"), 2);
    }

    #[test]
    fn test_reload_count_tracking() {
        let watcher = default_watcher("/tmp/dummy.onnx");
        assert_eq!(watcher.reload_count().expect("rc"), 0);

        watcher.mark_reloaded().expect("r1");
        watcher.mark_reloaded().expect("r2");
        watcher.mark_reloaded().expect("r3");

        assert_eq!(watcher.reload_count().expect("rc"), 3);
    }

    #[test]
    fn test_poll_interval_accessor() {
        let config = HotReloadConfig {
            poll_interval: Duration::from_millis(500),
            ..Default::default()
        };
        let watcher = ModelWatcher::new("/tmp/dummy.onnx", config);
        assert_eq!(watcher.config().poll_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_reload_timeout_accessor() {
        let config = HotReloadConfig {
            reload_timeout: Duration::from_secs(60),
            ..Default::default()
        };
        let watcher = ModelWatcher::new("/tmp/dummy.onnx", config);
        assert_eq!(watcher.config().reload_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_validate_before_swap_default_true() {
        let config = HotReloadConfig::default();
        assert!(config.validate_before_swap);
    }

    #[test]
    fn test_version_starts_at_zero() {
        let watcher = default_watcher("/tmp/dummy.onnx");
        assert_eq!(watcher.current_version().expect("v"), 0);
    }

    #[test]
    fn test_reload_event_fields() {
        let now = SystemTime::now();
        let event = ReloadEvent {
            path: PathBuf::from("/tmp/model.onnx"),
            timestamp: now,
            version: 3,
        };
        assert_eq!(event.version, 3);
        assert_eq!(event.path, PathBuf::from("/tmp/model.onnx"));
        assert_eq!(event.timestamp, now);
    }
}
