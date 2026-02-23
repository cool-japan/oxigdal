//! SWMR (Single Writer Multiple Reader) support for HDF5.
//!
//! SWMR mode allows concurrent access to HDF5 files with one writer
//! and multiple readers. This is useful for real-time data acquisition
//! and streaming scenarios.

use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// SWMR access mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwmrMode {
    /// Single writer mode
    Writer,
    /// Multiple reader mode
    Reader,
}

/// SWMR configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwmrConfig {
    /// Access mode
    mode: SwmrMode,
    /// Metadata cache size in bytes
    metadata_cache_size: usize,
    /// Page buffer size in bytes
    page_buffer_size: usize,
    /// Metadata flush interval
    flush_interval: Duration,
    /// Enable checksums
    enable_checksums: bool,
}

impl SwmrConfig {
    /// Create a new SWMR configuration for writer
    pub fn writer() -> Self {
        Self {
            mode: SwmrMode::Writer,
            metadata_cache_size: 32 * 1024 * 1024, // 32 MB
            page_buffer_size: 4 * 1024 * 1024,      // 4 MB
            flush_interval: Duration::from_secs(1),
            enable_checksums: true,
        }
    }

    /// Create a new SWMR configuration for reader
    pub fn reader() -> Self {
        Self {
            mode: SwmrMode::Reader,
            metadata_cache_size: 16 * 1024 * 1024, // 16 MB
            page_buffer_size: 4 * 1024 * 1024,      // 4 MB
            flush_interval: Duration::from_secs(1),
            enable_checksums: true,
        }
    }

    /// Set metadata cache size
    pub fn with_metadata_cache_size(mut self, size: usize) -> Self {
        self.metadata_cache_size = size;
        self
    }

    /// Set page buffer size
    pub fn with_page_buffer_size(mut self, size: usize) -> Self {
        self.page_buffer_size = size;
        self
    }

    /// Set flush interval
    pub fn with_flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = interval;
        self
    }

    /// Enable or disable checksums
    pub fn with_checksums(mut self, enable: bool) -> Self {
        self.enable_checksums = enable;
        self
    }

    /// Get the access mode
    pub fn mode(&self) -> SwmrMode {
        self.mode
    }

    /// Get metadata cache size
    pub fn metadata_cache_size(&self) -> usize {
        self.metadata_cache_size
    }

    /// Get page buffer size
    pub fn page_buffer_size(&self) -> usize {
        self.page_buffer_size
    }

    /// Get flush interval
    pub fn flush_interval(&self) -> Duration {
        self.flush_interval
    }

    /// Check if checksums are enabled
    pub fn checksums_enabled(&self) -> bool {
        self.enable_checksums
    }
}

/// File lock for SWMR coordination
#[derive(Debug)]
pub struct FileLock {
    /// Path to the lock file
    lock_path: PathBuf,
    /// Lock acquisition time
    acquired_at: SystemTime,
    /// Lock owner process ID
    owner_pid: u32,
}

impl FileLock {
    /// Create a new file lock
    pub fn new(file_path: &Path) -> Self {
        let lock_path = file_path.with_extension("lock");
        Self {
            lock_path,
            acquired_at: SystemTime::now(),
            owner_pid: std::process::id(),
        }
    }

    /// Acquire the lock
    pub fn acquire(&mut self, timeout: Duration) -> Result<()> {
        let start = SystemTime::now();

        loop {
            if self.try_acquire()? {
                return Ok(());
            }

            let elapsed = SystemTime::now()
                .duration_since(start)
                .unwrap_or(Duration::from_secs(0));

            if elapsed >= timeout {
                return Err(Hdf5Error::LockTimeout {
                    path: self.lock_path.to_string_lossy().to_string(),
                    timeout_secs: timeout.as_secs(),
                });
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Try to acquire the lock (non-blocking)
    pub fn try_acquire(&mut self) -> Result<bool> {
        use std::fs::OpenOptions;
        use std::io::Write;

        // Check if lock file exists
        if self.lock_path.exists() {
            // Check if lock is stale
            if self.is_stale_lock()? {
                self.remove_lock()?;
            } else {
                return Ok(false);
            }
        }

        // Create lock file
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.lock_path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    return Hdf5Error::LockExists {
                        path: self.lock_path.to_string_lossy().to_string(),
                    };
                }
                Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to create lock file: {}", e)))
            })?;

        // Write PID to lock file
        writeln!(file, "{}", self.owner_pid).map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to write to lock file: {}", e)))
        })?;

        file.sync_all().map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to sync lock file: {}", e)))
        })?;

        self.acquired_at = SystemTime::now();
        Ok(true)
    }

    /// Release the lock
    pub fn release(&self) -> Result<()> {
        self.remove_lock()
    }

    /// Check if lock is stale (e.g., process no longer exists)
    fn is_stale_lock(&self) -> Result<bool> {
        use std::fs::File;
        use std::io::Read;

        let mut file = File::open(&self.lock_path).map_err(|e| {
            Hdf5Error::Io(e)
        })?;

        let mut content = String::new();
        file.read_to_string(&mut content).map_err(|e| {
            Hdf5Error::Io(e)
        })?;

        let lock_pid: u32 = content.trim().parse().map_err(|_| {
            Hdf5Error::InvalidFormat("Invalid PID in lock file".to_string()))
        })?;

        // Check if process still exists
        // This is platform-specific and simplified
        // Simple implementation: check if file is older than a threshold
        // In production, this would use platform-specific process checking
        let metadata = std::fs::metadata(&self.lock_path).map_err(|e| {
            Hdf5Error::Io(e)
        })?;

        let modified = metadata.modified().map_err(|e| {
            Hdf5Error::Io(e)
        })?;

        let elapsed = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::from_secs(0));

        // Consider stale if older than 1 hour
        Ok(elapsed > Duration::from_secs(3600)))
    }

    /// Remove lock file
    fn remove_lock(&self) -> Result<()> {
        if self.lock_path.exists() {
            std::fs::remove_file(&self.lock_path).map_err(|e| {
                Hdf5Error::Io(e)
            })?;
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Best effort to release lock on drop
        let _ = self.release();
    }
}

/// SWMR metadata tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataVersion {
    /// Version number
    version: u64,
    /// Timestamp
    timestamp: u64,
    /// Checksum of metadata
    checksum: u32,
}

impl MetadataVersion {
    /// Create a new metadata version
    pub fn new(version: u64, timestamp: u64, checksum: u32) -> Self {
        Self {
            version,
            timestamp,
            checksum,
        }
    }

    /// Get version number
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Get timestamp
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Get checksum
    pub fn checksum(&self) -> u32 {
        self.checksum
    }
}

/// SWMR writer handle
pub struct SwmrWriter {
    /// File path
    file_path: PathBuf,
    /// Configuration
    config: SwmrConfig,
    /// File lock
    lock: FileLock,
    /// Current metadata version
    metadata_version: u64,
    /// Last flush time
    last_flush: SystemTime,
}

impl SwmrWriter {
    /// Create a new SWMR writer
    pub fn new(file_path: PathBuf, config: SwmrConfig) -> Result<Self> {
        if config.mode() != SwmrMode::Writer {
            return Err(Hdf5Error::InvalidOperation(
                "Config must be in writer mode".to_string(),
            ));
        }

        let mut lock = FileLock::new(&file_path);
        lock.acquire(Duration::from_secs(10)))?;

        Ok(Self {
            file_path,
            config,
            lock,
            metadata_version: 0,
            last_flush: SystemTime::now(),
        })
    }

    /// Get file path
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Get configuration
    pub fn config(&self) -> &SwmrConfig {
        &self.config
    }

    /// Get current metadata version
    pub fn metadata_version(&self) -> u64 {
        self.metadata_version
    }

    /// Flush metadata to disk
    pub fn flush(&mut self) -> Result<()> {
        // Increment metadata version
        self.metadata_version += 1;
        self.last_flush = SystemTime::now();

        // In a real implementation, this would flush metadata to disk
        // and update the metadata version in the file

        tracing::debug!(
            "Flushed metadata version {} for {:?}",
            self.metadata_version,
            self.file_path
        );

        Ok(())
    }

    /// Check if flush is needed based on interval
    pub fn should_flush(&self) -> bool {
        let elapsed = SystemTime::now()
            .duration_since(self.last_flush)
            .unwrap_or(Duration::from_secs(0));

        elapsed >= self.config.flush_interval()
    }

    /// Auto-flush if interval has elapsed
    pub fn auto_flush(&mut self) -> Result<()> {
        if self.should_flush() {
            self.flush()?;
        }
        Ok(())
    }
}

impl Drop for SwmrWriter {
    fn drop(&mut self) {
        // Flush on drop
        let _ = self.flush();
    }
}

/// SWMR reader handle
pub struct SwmrReader {
    /// File path
    file_path: PathBuf,
    /// Configuration
    config: SwmrConfig,
    /// Last known metadata version
    metadata_version: u64,
    /// Last refresh time
    last_refresh: SystemTime,
}

impl SwmrReader {
    /// Create a new SWMR reader
    pub fn new(file_path: PathBuf, config: SwmrConfig) -> Result<Self> {
        if config.mode() != SwmrMode::Reader {
            return Err(Hdf5Error::InvalidOperation(
                "Config must be in reader mode".to_string(),
            ));
        }

        Ok(Self {
            file_path,
            config,
            metadata_version: 0,
            last_refresh: SystemTime::now(),
        })
    }

    /// Get file path
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Get configuration
    pub fn config(&self) -> &SwmrConfig {
        &self.config
    }

    /// Get current metadata version
    pub fn metadata_version(&self) -> u64 {
        self.metadata_version
    }

    /// Refresh metadata from disk
    pub fn refresh(&mut self) -> Result<bool> {
        // In a real implementation, this would read the latest metadata
        // version from disk and update internal caches

        // For now, just simulate version check
        let new_version = self.read_metadata_version()?;

        if new_version > self.metadata_version {
            self.metadata_version = new_version;
            self.last_refresh = SystemTime::now();

            tracing::debug!(
                "Refreshed metadata to version {} for {:?}",
                self.metadata_version,
                self.file_path
            );

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Read current metadata version from file
    fn read_metadata_version(&self) -> Result<u64> {
        // In a real implementation, this would read from the file
        // For now, return a dummy value
        Ok(0)
    }

    /// Check if refresh is needed
    pub fn should_refresh(&self) -> bool {
        let elapsed = SystemTime::now()
            .duration_since(self.last_refresh)
            .unwrap_or(Duration::from_secs(0));

        elapsed >= self.config.flush_interval()
    }

    /// Auto-refresh if needed
    pub fn auto_refresh(&mut self) -> Result<bool> {
        if self.should_refresh() {
            self.refresh()
        } else {
            Ok(false)
        }
    }
}

/// SWMR statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SwmrStatistics {
    /// Number of flushes
    num_flushes: u64,
    /// Number of refreshes
    num_refreshes: u64,
    /// Total bytes written
    bytes_written: u64,
    /// Total bytes read
    bytes_read: u64,
    /// Number of lock acquisitions
    lock_acquisitions: u64,
    /// Average flush time in microseconds
    avg_flush_time_us: u64,
}

impl SwmrStatistics {
    /// Create new statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a flush
    pub fn record_flush(&mut self, duration: Duration) {
        self.num_flushes += 1;
        let us = duration.as_micros() as u64;
        self.avg_flush_time_us =
            (self.avg_flush_time_us * (self.num_flushes - 1) + us) / self.num_flushes;
    }

    /// Record a refresh
    pub fn record_refresh(&mut self) {
        self.num_refreshes += 1;
    }

    /// Record bytes written
    pub fn record_write(&mut self, bytes: u64) {
        self.bytes_written += bytes;
    }

    /// Record bytes read
    pub fn record_read(&mut self, bytes: u64) {
        self.bytes_read += bytes;
    }

    /// Record lock acquisition
    pub fn record_lock(&mut self) {
        self.lock_acquisitions += 1;
    }

    /// Get statistics
    pub fn num_flushes(&self) -> u64 {
        self.num_flushes
    }

    pub fn num_refreshes(&self) -> u64 {
        self.num_refreshes
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    pub fn lock_acquisitions(&self) -> u64 {
        self.lock_acquisitions
    }

    pub fn avg_flush_time_us(&self) -> u64 {
        self.avg_flush_time_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swmr_config_writer() {
        let config = SwmrConfig::writer();
        assert_eq!(config.mode(), SwmrMode::Writer);
        assert!(config.checksums_enabled());
        assert!(config.metadata_cache_size() > 0);
    }

    #[test]
    fn test_swmr_config_reader() {
        let config = SwmrConfig::reader();
        assert_eq!(config.mode(), SwmrMode::Reader);
        assert!(config.checksums_enabled());
    }

    #[test]
    fn test_swmr_config_builder() {
        let config = SwmrConfig::writer()
            .with_metadata_cache_size(64 * 1024 * 1024)
            .with_page_buffer_size(8 * 1024 * 1024)
            .with_flush_interval(Duration::from_secs(5)))
            .with_checksums(false);

        assert_eq!(config.metadata_cache_size(), 64 * 1024 * 1024);
        assert_eq!(config.page_buffer_size(), 8 * 1024 * 1024);
        assert_eq!(config.flush_interval(), Duration::from_secs(5));
        assert!(!config.checksums_enabled());
    }

    #[test]
    fn test_metadata_version() {
        let version = MetadataVersion::new(42, 1234567890, 0xDEADBEEF);
        assert_eq!(version.version(), 42);
        assert_eq!(version.timestamp(), 1234567890);
        assert_eq!(version.checksum(), 0xDEADBEEF);
    }

    #[test]
    fn test_file_lock() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("swmr_test.h5");

        let mut lock = FileLock::new(&test_file);
        assert!(lock.try_acquire().is_ok());

        // Clean up
        let _ = lock.release();
    }

    #[test]
    fn test_swmr_statistics() {
        let mut stats = SwmrStatistics::new();
        assert_eq!(stats.num_flushes(), 0);

        stats.record_flush(Duration::from_micros(100));
        assert_eq!(stats.num_flushes(), 1);
        assert_eq!(stats.avg_flush_time_us(), 100);

        stats.record_flush(Duration::from_micros(200));
        assert_eq!(stats.num_flushes(), 2);
        assert_eq!(stats.avg_flush_time_us(), 150);

        stats.record_write(1024);
        assert_eq!(stats.bytes_written(), 1024);

        stats.record_read(512);
        assert_eq!(stats.bytes_read(), 512);
    }
}
