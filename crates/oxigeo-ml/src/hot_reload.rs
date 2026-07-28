//! Model hot-reload with file watching and atomic swapping.
//!
//! This module has two layers:
//!
//! - [`ModelWatcher`] is a low-level **change-detection primitive**: it polls a
//!   file's modification time and reports when it changes, plus a version /
//!   reload counter. It does not itself load or swap models.
//! - [`HotReloadModel`] builds on the watcher to provide real **atomic model
//!   swapping**: it owns the live model behind an `Arc` and, on
//!   [`reload_if_changed`](HotReloadModel::reload_if_changed), loads the new
//!   model file and atomically replaces the live handle. Readers holding an
//!   `Arc` from [`current`](HotReloadModel::current) keep running on the old
//!   model until they next fetch it, so inference never blocks on a reload and
//!   never observes a half-loaded model.

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

/// A live model handle with atomic hot-swapping.
///
/// `HotReloadModel<T>` owns the current model behind an `Arc<T>` guarded by an
/// `RwLock`. Reads ([`current`](Self::current)) clone the `Arc` cheaply and run
/// lock-free thereafter; a reload builds the replacement model fully (which
/// validates it) *before* taking the brief write lock to swap it in. A failed
/// load leaves the previous model in place, so a bad model file can never take
/// the served model down.
///
/// The loader is supplied per call so this type stays agnostic to the concrete
/// model type — it works equally with `OnnxModel`, an `oxionnx::Session`, or any
/// user model.
///
/// # Example
///
/// ```no_run
/// use oxigeo_ml::hot_reload::{HotReloadModel, HotReloadConfig};
/// use oxigeo_ml::models::OnnxModel;
/// use std::path::Path;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let load = |p: &Path| OnnxModel::from_file(p);
/// let handle = HotReloadModel::new("model.onnx", HotReloadConfig::default(), &load)?;
///
/// // In the inference loop:
/// let model = handle.current()?;          // cheap Arc clone, never blocks on reload
/// // ... run inference with `model` ...
///
/// // Periodically (e.g. on a poll timer):
/// if let Some(version) = handle.reload_if_changed(&load)? {
///     println!("hot-reloaded to version {version}");
/// }
/// # Ok(())
/// # }
/// ```
pub struct HotReloadModel<T> {
    watcher: ModelWatcher,
    current: RwLock<Arc<T>>,
}

impl<T> HotReloadModel<T> {
    /// Loads the initial model and wraps it in a hot-reloadable handle.
    ///
    /// # Errors
    /// Returns an error if the initial model load fails.
    pub fn new<F>(path: impl AsRef<Path>, config: HotReloadConfig, load: F) -> Result<Self, MlError>
    where
        F: Fn(&Path) -> Result<T, MlError>,
    {
        let watcher = ModelWatcher::new(path, config);
        let model = load(watcher.path())?;
        // Establish the mtime baseline so the first poll does not spuriously
        // report a change.
        let _ = watcher.check_for_update()?;
        Ok(Self {
            watcher,
            current: RwLock::new(Arc::new(model)),
        })
    }

    /// Returns the currently-served model as a cheap `Arc` clone.
    ///
    /// # Errors
    /// Returns an error only if the internal lock is poisoned.
    pub fn current(&self) -> Result<Arc<T>, MlError> {
        let guard = self
            .current
            .read()
            .map_err(|_| MlError::InvalidConfig("lock poisoned: current model".into()))?;
        Ok(Arc::clone(&guard))
    }

    /// If the watched file changed since the last check, loads the new model and
    /// atomically swaps it in, returning the new version number. Returns
    /// `Ok(None)` when the file is unchanged.
    ///
    /// If loading the new model fails, the previously-served model is kept and
    /// the error is returned — the swap is all-or-nothing.
    ///
    /// # Errors
    /// Returns an error if the new model fails to load or a lock is poisoned.
    pub fn reload_if_changed<F>(&self, load: F) -> Result<Option<u64>, MlError>
    where
        F: Fn(&Path) -> Result<T, MlError>,
    {
        if self.watcher.check_for_update()?.is_none() {
            return Ok(None);
        }
        self.force_reload(load).map(Some)
    }

    /// Unconditionally reloads the model from disk and swaps it in atomically,
    /// returning the new version number.
    ///
    /// # Errors
    /// Returns an error if the model fails to load or a lock is poisoned.
    pub fn force_reload<F>(&self, load: F) -> Result<u64, MlError>
    where
        F: Fn(&Path) -> Result<T, MlError>,
    {
        // Build (and thereby validate) the replacement before touching the live
        // handle, so a failed load never disturbs the served model.
        let new_model = Arc::new(load(self.watcher.path())?);
        {
            let mut guard = self
                .current
                .write()
                .map_err(|_| MlError::InvalidConfig("lock poisoned: current model".into()))?;
            *guard = new_model;
        }
        self.watcher.mark_reloaded()
    }

    /// Returns the current model version counter.
    ///
    /// # Errors
    /// Returns an error if the internal lock is poisoned.
    pub fn version(&self) -> Result<u64, MlError> {
        self.watcher.current_version()
    }

    /// Returns the total number of completed reloads.
    ///
    /// # Errors
    /// Returns an error if the internal lock is poisoned.
    pub fn reload_count(&self) -> Result<u64, MlError> {
        self.watcher.reload_count()
    }

    /// Returns the watched model path.
    pub fn path(&self) -> &Path {
        self.watcher.path()
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
        let path = std::env::temp_dir().join("oxigeo_nonexistent_test_model_bx9f.onnx");
        let watcher = default_watcher(&path);
        assert_eq!(watcher.path(), path.as_path());
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
        let path =
            std::env::temp_dir().join("oxigeo_nonexistent_absolutely_does_not_exist_bx9f.onnx");
        let watcher = default_watcher(&path);
        let result = watcher.check_for_update();
        assert!(result.is_ok());
        assert!(result.expect("should be ok").is_none());
    }

    #[test]
    fn test_check_existing_file_first_call_no_event() {
        let dir = std::env::temp_dir();
        let path = dir.join("oxigeo_hot_reload_test_first_call.onnx");
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
        let path = dir.join("oxigeo_hot_reload_unchanged.onnx");
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
        let path = std::env::temp_dir().join("oxigeo_dummy_bx9f.onnx");
        let watcher = default_watcher(&path);
        assert_eq!(watcher.current_version().expect("v"), 0);

        let v1 = watcher.mark_reloaded().expect("reload 1");
        assert_eq!(v1, 1);

        let v2 = watcher.mark_reloaded().expect("reload 2");
        assert_eq!(v2, 2);

        assert_eq!(watcher.current_version().expect("cv"), 2);
    }

    #[test]
    fn test_reload_count_tracking() {
        let path = std::env::temp_dir().join("oxigeo_dummy_bx9f.onnx");
        let watcher = default_watcher(&path);
        assert_eq!(watcher.reload_count().expect("rc"), 0);

        watcher.mark_reloaded().expect("r1");
        watcher.mark_reloaded().expect("r2");
        watcher.mark_reloaded().expect("r3");

        assert_eq!(watcher.reload_count().expect("rc"), 3);
    }

    #[test]
    fn test_poll_interval_accessor() {
        let path = std::env::temp_dir().join("oxigeo_dummy_bx9f.onnx");
        let config = HotReloadConfig {
            poll_interval: Duration::from_millis(500),
            ..Default::default()
        };
        let watcher = ModelWatcher::new(&path, config);
        assert_eq!(watcher.config().poll_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_reload_timeout_accessor() {
        let path = std::env::temp_dir().join("oxigeo_dummy_bx9f.onnx");
        let config = HotReloadConfig {
            reload_timeout: Duration::from_secs(60),
            ..Default::default()
        };
        let watcher = ModelWatcher::new(&path, config);
        assert_eq!(watcher.config().reload_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_validate_before_swap_default_true() {
        let config = HotReloadConfig::default();
        assert!(config.validate_before_swap);
    }

    #[test]
    fn test_version_starts_at_zero() {
        let path = std::env::temp_dir().join("oxigeo_dummy_bx9f.onnx");
        let watcher = default_watcher(&path);
        assert_eq!(watcher.current_version().expect("v"), 0);
    }

    #[test]
    fn test_hot_reload_model_atomic_swap() {
        // Use a trivial "model" = the file's byte length, loaded via a closure.
        let dir = std::env::temp_dir();
        let path = dir.join("oxigeo_hotswap_model.bin");
        fs::write(&path, b"aaaa").expect("write v1"); // len 4

        let load = |p: &Path| -> Result<usize, MlError> {
            let bytes = std::fs::read(p).map_err(MlError::Io)?;
            Ok(bytes.len())
        };

        let handle = HotReloadModel::new(&path, HotReloadConfig::default(), load).expect("init");
        assert_eq!(*handle.current().expect("current"), 4);
        assert_eq!(handle.version().expect("v"), 0);

        // Hold an Arc from before the reload — it must keep the old value.
        let old = handle.current().expect("old");

        // No change yet.
        assert!(handle.reload_if_changed(load).expect("noop").is_none());

        // Change the file so mtime advances and content grows.
        std::thread::sleep(Duration::from_millis(1100));
        fs::write(&path, b"bbbbbbbb").expect("write v2"); // len 8

        let version = handle
            .reload_if_changed(load)
            .expect("reload")
            .expect("should have reloaded");
        assert_eq!(version, 1);
        assert_eq!(*handle.current().expect("current v2"), 8);
        // The previously-held Arc still points at the old model (atomic swap).
        assert_eq!(*old, 4);
        assert_eq!(handle.reload_count().expect("rc"), 1);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_hot_reload_bad_load_keeps_old_model() {
        let dir = std::env::temp_dir();
        let path = dir.join("oxigeo_hotswap_badload.bin");
        fs::write(&path, b"good").expect("write");

        // Loader fails whenever the file content starts with 'x'.
        let load = |p: &Path| -> Result<usize, MlError> {
            let bytes = std::fs::read(p).map_err(MlError::Io)?;
            if bytes.first() == Some(&b'x') {
                return Err(MlError::InvalidConfig("bad model".into()));
            }
            Ok(bytes.len())
        };

        let handle = HotReloadModel::new(&path, HotReloadConfig::default(), load).expect("init");
        assert_eq!(*handle.current().expect("current"), 4);

        // A failing force_reload must leave the served model intact.
        std::thread::sleep(Duration::from_millis(1100));
        fs::write(&path, b"xbad").expect("write bad");
        let result = handle.force_reload(load);
        assert!(result.is_err(), "bad load should error");
        assert_eq!(*handle.current().expect("still old"), 4);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_reload_event_fields() {
        let model_path = std::env::temp_dir().join("oxigeo_model_bx9f.onnx");
        let now = SystemTime::now();
        let event = ReloadEvent {
            path: model_path.clone(),
            timestamp: now,
            version: 3,
        };
        assert_eq!(event.version, 3);
        assert_eq!(event.path, model_path);
        assert_eq!(event.timestamp, now);
    }
}
