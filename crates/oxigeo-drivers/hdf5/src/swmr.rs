//! SWMR (Single Writer Multiple Reader) **coordination primitive** for HDF5.
//!
//! Real HDF5 SWMR (as libhdf5 implements it) is a file-format-level protocol:
//! a specific object-header/page-buffer write ordering that lets a reader
//! observe a live-growing `.h5` file's dataset content without ever seeing a
//! torn read, backed by metadata-cache flush ordering and (optionally) the
//! page buffer. **That protocol is not implemented here.**
//!
//! What this module actually provides: [`FileLock`] (a PID-stamped, staleness-
//! detecting file lock) plus [`SwmrWriter`]/[`SwmrReader`], which coordinate
//! through a `<file>.swmr-version` JSON sidecar — [`SwmrWriter::flush`] atomically
//! increments and republishes a monotonic version+timestamp+checksum record,
//! and [`SwmrReader::refresh`] polls it. `SwmrWriter` holds **no handle to any
//! [`crate::writer::Hdf5Writer`]** and has no method to write dataset bytes at
//! all — so `flush()` never touches the real `.h5` file's contents; it only
//! tells readers "a new version exists," on whatever protocol the caller
//! builds for actually publishing dataset data (e.g. write a new file and
//! rename it into place, then call `flush()`). [`SwmrConfig`]'s
//! `metadata_cache_size`/`page_buffer_size` fields are accepted for
//! API-compatibility with real HDF5 SWMR tuning knobs but are not read by
//! anything — no cache or page buffer exists here to tune.
//!
//! Useful for real-time data acquisition / streaming scenarios that need a
//! lock + "has anything changed" coordination signal — not a drop-in
//! replacement for libhdf5 SWMR's live dataset-content visibility guarantees.

use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Build the sidecar path used to publish the current SWMR metadata version.
///
/// The version is stored next to the data file (e.g. `data.h5` ->
/// `data.h5.swmr-version`) so that a single writer can publish a monotonically
/// increasing version that any number of readers can poll.
fn version_file_path(base: &Path) -> PathBuf {
    let mut os = base.as_os_str().to_owned();
    os.push(".swmr-version");
    PathBuf::from(os)
}

/// Temporary path used for atomic (write-then-rename) publication.
fn version_tmp_path(base: &Path) -> PathBuf {
    let mut os = base.as_os_str().to_owned();
    os.push(".swmr-version.tmp");
    PathBuf::from(os)
}

/// Seconds since the Unix epoch, saturating to 0 before the epoch.
fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Deterministic FNV-1a checksum over a metadata version's identifying fields.
///
/// Readers can compare this against a recomputed value to detect a torn or
/// corrupted version record.
fn version_checksum(version: u64, timestamp: u64) -> u32 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in version
        .to_le_bytes()
        .iter()
        .chain(timestamp.to_le_bytes().iter())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash ^ (hash >> 32)) as u32
}

/// Atomically publish `version` to the sidecar version file for `base`.
fn persist_metadata_version(base: &Path, version: &MetadataVersion) -> Result<()> {
    let json = serde_json::to_vec(version).map_err(|e| {
        Hdf5Error::internal(format!("Failed to serialize SWMR metadata version: {}", e))
    })?;
    let tmp = version_tmp_path(base);
    let final_path = version_file_path(base);
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &final_path)?;
    Ok(())
}

/// Read the currently published metadata version for `base`.
///
/// Returns `MetadataVersion::new(0, 0, ..)` when no version has been published
/// yet (the sidecar file is absent).
fn load_metadata_version(base: &Path) -> Result<MetadataVersion> {
    let path = version_file_path(base);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let version: MetadataVersion = serde_json::from_slice(&bytes).map_err(|e| {
                Hdf5Error::invalid_format(format!("Failed to parse SWMR version file: {}", e))
            })?;
            let expected = version_checksum(version.version(), version.timestamp());
            if version.checksum() != expected {
                return Err(Hdf5Error::ChecksumMismatch {
                    expected,
                    actual: version.checksum(),
                });
            }
            Ok(version)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(MetadataVersion::new(0, 0, version_checksum(0, 0)))
        }
        Err(e) => Err(Hdf5Error::Io(e)),
    }
}

/// SWMR access mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwmrMode {
    /// Single writer mode
    Writer,
    /// Multiple reader mode
    Reader,
}

/// SWMR configuration.
///
/// `metadata_cache_size`/`page_buffer_size` mirror real HDF5 SWMR tuning
/// knobs for API-compatibility, but **nothing in this module reads them** —
/// there is no metadata cache or page buffer here to size (see the module
/// doc). They're stored/returned via their setters/getters only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwmrConfig {
    /// Access mode
    mode: SwmrMode,
    /// Metadata cache size in bytes (accepted, not currently consulted by
    /// anything — see struct doc).
    metadata_cache_size: usize,
    /// Page buffer size in bytes (accepted, not currently consulted by
    /// anything — see struct doc).
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
            page_buffer_size: 4 * 1024 * 1024,     // 4 MB
            flush_interval: Duration::from_secs(1),
            enable_checksums: true,
        }
    }

    /// Create a new SWMR configuration for reader
    pub fn reader() -> Self {
        Self {
            mode: SwmrMode::Reader,
            metadata_cache_size: 16 * 1024 * 1024, // 16 MB
            page_buffer_size: 4 * 1024 * 1024,     // 4 MB
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
                Hdf5Error::Io(std::io::Error::other(format!(
                    "Failed to create lock file: {}",
                    e
                )))
            })?;

        // Write PID to lock file
        writeln!(file, "{}", self.owner_pid).map_err(|e| {
            Hdf5Error::Io(std::io::Error::other(format!(
                "Failed to write to lock file: {}",
                e
            )))
        })?;

        file.sync_all().map_err(|e| {
            Hdf5Error::Io(std::io::Error::other(format!(
                "Failed to sync lock file: {}",
                e
            )))
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

        let mut file = File::open(&self.lock_path)?;

        let mut content = String::new();
        file.read_to_string(&mut content)?;

        // Parse the recorded PID; a malformed lock file is treated as stale so a
        // stuck lock can be recovered rather than blocking forever.
        let _lock_pid: u32 = match content.trim().parse() {
            Ok(pid) => pid,
            Err(_) => return Ok(true),
        };

        // Check if process still exists.
        // This is platform-specific and simplified: we consider a lock stale if
        // its file has not been modified within the staleness threshold. In
        // production, this would use platform-specific process checking.
        let metadata = std::fs::metadata(&self.lock_path)?;

        let modified = metadata.modified()?;

        let elapsed = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::from_secs(0));

        // Consider stale if older than 1 hour
        Ok(elapsed > Duration::from_secs(3600))
    }

    /// Remove lock file
    fn remove_lock(&self) -> Result<()> {
        if self.lock_path.exists() {
            std::fs::remove_file(&self.lock_path)?;
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

/// SWMR writer handle.
///
/// Coordinates concurrent access via a [`FileLock`] plus an atomically
/// published version-sidecar (see the module doc) — it does **not** hold a
/// handle to any [`crate::writer::Hdf5Writer`] and has no method to write
/// dataset bytes. [`SwmrWriter::flush`] publishes a new version number; it
/// never touches the real `.h5` file's dataset content. Callers that need
/// real dataset content published to SWMR readers must write it themselves
/// (e.g. via [`crate::writer::Hdf5Writer`], atomically renamed into place)
/// and call `flush()` only to signal that a new version is ready.
pub struct SwmrWriter {
    /// File path
    file_path: PathBuf,
    /// Configuration
    config: SwmrConfig,
    /// File lock held for the writer's lifetime. Read only through its `Drop`
    /// impl, which releases the on-disk lock when the writer is dropped.
    #[allow(dead_code)]
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
        lock.acquire(Duration::from_secs(10))?;

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

    /// Publish a new coordination version.
    ///
    /// Increments the in-memory version counter and atomically publishes it
    /// (version + timestamp + checksum) to the `<file>.swmr-version` sidecar
    /// so that concurrent [`SwmrReader`]s can observe the change via
    /// [`SwmrReader::refresh`]. Publication uses a write-to-temp-then-rename
    /// sequence so readers never observe a torn sidecar record.
    ///
    /// This does **not** write, flush, or otherwise touch the real `.h5`
    /// file's dataset content — see the module doc. Call it only after the
    /// real dataset content a reader should observe next has already been
    /// durably written by whatever means the caller uses.
    pub fn flush(&mut self) -> Result<()> {
        // Increment the coordination version counter.
        self.metadata_version += 1;
        self.last_flush = SystemTime::now();

        // Persist the new version so readers can pick it up (durably and
        // atomically) — this is the sidecar coordination signal, not a
        // dataset-content publication.
        let timestamp = unix_now_secs();
        let checksum = version_checksum(self.metadata_version, timestamp);
        let version = MetadataVersion::new(self.metadata_version, timestamp, checksum);
        persist_metadata_version(&self.file_path, &version)?;

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

    /// Refresh metadata from disk.
    ///
    /// Reads the latest published metadata version from the sidecar version file
    /// and, if it is newer than the last observed version, advances the reader's
    /// view. Returns `true` when a newer version was observed.
    pub fn refresh(&mut self) -> Result<bool> {
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

    /// Read current metadata version from the sidecar version file.
    ///
    /// Returns `0` when no writer has published a version yet.
    fn read_metadata_version(&self) -> Result<u64> {
        Ok(load_metadata_version(&self.file_path)?.version())
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

    /// Get the number of flushes recorded
    pub fn num_flushes(&self) -> u64 {
        self.num_flushes
    }

    /// Get the number of refreshes recorded
    pub fn num_refreshes(&self) -> u64 {
        self.num_refreshes
    }

    /// Get the total number of bytes written
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// Get the total number of bytes read
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    /// Get the number of lock acquisitions recorded
    pub fn lock_acquisitions(&self) -> u64 {
        self.lock_acquisitions
    }

    /// Get the average flush time in microseconds
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
            .with_flush_interval(Duration::from_secs(5))
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

    #[test]
    fn test_metadata_version_persistence_roundtrip() {
        // A writer's flush must durably publish the coordination version to
        // the sidecar, and a reader's refresh must read it back. This
        // exercises the real (not stubbed) sidecar version-publication path —
        // it does NOT demonstrate real HDF5 dataset-content SWMR visibility,
        // since SwmrWriter never writes any dataset bytes (see module doc).
        let dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!(
            "swmr_roundtrip_{}_{}.h5",
            std::process::id(),
            nanos
        ));

        // Clean any leftovers from a prior run.
        let _ = std::fs::remove_file(version_file_path(&path));

        let mut writer =
            SwmrWriter::new(path.clone(), SwmrConfig::writer()).expect("create writer");
        writer.flush().expect("first flush");
        writer.flush().expect("second flush");
        let published = writer.metadata_version();
        assert_eq!(published, 2);

        let mut reader =
            SwmrReader::new(path.clone(), SwmrConfig::reader()).expect("create reader");
        assert_eq!(reader.metadata_version(), 0);
        let updated = reader.refresh().expect("refresh");
        assert!(updated, "reader should observe the published version");
        assert_eq!(reader.metadata_version(), published);

        // A second refresh with no new writes reports no change.
        let updated_again = reader.refresh().expect("refresh again");
        assert!(!updated_again);

        // Clean up (writer still holds the lock until dropped).
        drop(writer);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(version_file_path(&path));
        let _ = std::fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn test_reader_without_published_version_reads_zero() {
        let dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!(
            "swmr_noversion_{}_{}.h5",
            std::process::id(),
            nanos
        ));
        let _ = std::fs::remove_file(version_file_path(&path));

        let mut reader = SwmrReader::new(path.clone(), SwmrConfig::reader()).expect("reader");
        let updated = reader.refresh().expect("refresh");
        assert!(!updated);
        assert_eq!(reader.metadata_version(), 0);
    }
}
