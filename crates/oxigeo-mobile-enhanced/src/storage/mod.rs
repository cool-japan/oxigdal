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

/// Widen a `libc::statvfs` block-size/block-count field to `u64`.
///
/// These fields are already `u64` on some Unix targets (x86_64 Linux, where
/// `c_ulong` and `fsblkcnt64_t` are both 64-bit) and narrower on others (32-bit
/// Android ABIs, where `c_ulong` is `u32`; macOS, where `fsblkcnt_t` is
/// `c_uint`). Written inline, neither spelling is portable *and* lint-clean:
/// `x as u64` trips `clippy::unnecessary_cast` on the wide targets, and
/// `u64::from(x)` trips `clippy::useless_conversion` there instead. Routing
/// both cases through a single `Into<u64>` bound satisfies every target
/// without a lint suppression, and — unlike a cast — it fails to compile
/// rather than silently truncating should a field ever be wider than `u64`.
#[cfg(unix)]
fn widen_to_u64<T: Into<u64>>(field: T) -> u64 {
    field.into()
}

/// Query real `(total_bytes, available_bytes, used_bytes)` for the
/// filesystem containing `path` via the POSIX `statvfs(2)` syscall.
///
/// `available_bytes` uses `f_bavail` (blocks available to an unprivileged
/// process), matching what a mobile app can actually consume; `used_bytes`
/// is derived from `f_bfree` (blocks free including those reserved for the
/// superuser) so it reflects genuinely occupied space rather than
/// unprivileged-available headroom.
///
/// # Errors
/// Returns [`MobileError::StorageError`] if `path` cannot be converted to a
/// NUL-free C string, or if the underlying `statvfs` call fails (e.g. the
/// path does not exist).
#[cfg(unix)]
#[allow(unsafe_code)]
fn statvfs_bytes(path: &Path) -> Result<(u64, u64, u64)> {
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| MobileError::StorageError(format!("path contains a NUL byte: {e}")))?;

    let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
    // SAFETY: `c_path` is a valid, NUL-terminated C string for the lifetime
    // of this call, and `stat.as_mut_ptr()` points at enough space for a
    // `libc::statvfs` (guaranteed by `MaybeUninit::<libc::statvfs>`).
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        return Err(MobileError::StorageError(format!(
            "statvfs failed for {}: {}",
            path.display(),
            err
        )));
    }

    // SAFETY: `rc == 0` guarantees the kernel fully populated `stat`.
    let stat = unsafe { stat.assume_init() };

    // `f_frsize` is the fundamental fragment size (the unit `f_blocks` etc.
    // are counted in); fall back to `f_bsize` if a platform reports zero.
    // See `widen_to_u64` for why the widening goes through a generic bound
    // rather than a cast.
    let block_size: u64 = if stat.f_frsize > 0 {
        widen_to_u64(stat.f_frsize)
    } else {
        widen_to_u64(stat.f_bsize)
    };

    let total_bytes = block_size.saturating_mul(widen_to_u64(stat.f_blocks));
    let available_bytes = block_size.saturating_mul(widen_to_u64(stat.f_bavail));
    let free_bytes = block_size.saturating_mul(widen_to_u64(stat.f_bfree));
    let used_bytes = total_bytes.saturating_sub(free_bytes);

    Ok((total_bytes, available_bytes, used_bytes))
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

    /// Get storage information for the filesystem backing this storage
    /// manager's `cache_dir` (falling back to `temp_dir`, then `/`).
    ///
    /// On Unix platforms (Linux, Android, macOS, iOS -- i.e. every mobile
    /// target this crate supports, plus the typical desktop dev/CI host)
    /// this queries the **real** filesystem capacity via `statvfs(2)`. On
    /// non-Unix hosts (e.g. Windows), where no `statvfs`-equivalent is
    /// wired up, this returns [`MobileError::StorageError`] rather than a
    /// fabricated capacity, so `is_low_storage`/`is_critical_storage` never
    /// silently evaluate against fake numbers.
    pub fn storage_info(&self) -> Result<StorageInfo> {
        #[cfg(unix)]
        {
            let probe_path: &Path = if self.cache_dir.exists() {
                &self.cache_dir
            } else if self.temp_dir.exists() {
                &self.temp_dir
            } else {
                Path::new("/")
            };

            let (total_bytes, available_bytes, used_bytes) = statvfs_bytes(probe_path)?;
            Ok(StorageInfo {
                total_bytes,
                available_bytes,
                used_bytes,
            })
        }

        #[cfg(not(unix))]
        {
            Err(MobileError::StorageError(
                "real filesystem storage introspection is only implemented for Unix targets \
                 (Linux/Android/macOS/iOS) via statvfs(2); no equivalent is wired up for this \
                 platform"
                    .to_string(),
            ))
        }
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

    /// Verifies the critical/high fix: `storage_info()` reports the *real*
    /// capacity of the host filesystem (via `statvfs`), not the previous
    /// hardcoded 64GB/10GB-free/54GB-used constants -- on this dev/CI host
    /// (any Unix), the real filesystem backing `std::env::temp_dir()` is
    /// essentially never exactly those fixed numbers.
    #[cfg(unix)]
    #[test]
    fn test_storage_info_reports_real_unix_filesystem_capacity() {
        let temp_dir = std::env::temp_dir();
        let storage = MobileStorage::new(temp_dir.clone(), temp_dir);

        let info = storage
            .storage_info()
            .expect("statvfs must succeed for a real, existing temp directory");

        assert!(
            info.total_bytes > 0,
            "real filesystem must report nonzero total capacity"
        );
        assert!(
            info.available_bytes <= info.total_bytes,
            "available must not exceed total"
        );
        assert!(
            info.used_bytes <= info.total_bytes,
            "used must not exceed total"
        );

        // The old mock constants were an exact 64GiB/10GiB/54GiB triple;
        // a real host filesystem reporting exactly that is astronomically
        // unlikely, so this also acts as a regression guard against the
        // fabricated values silently coming back.
        const MOCK_TOTAL: u64 = 64 * 1024 * 1024 * 1024;
        const MOCK_AVAILABLE: u64 = 10 * 1024 * 1024 * 1024;
        const MOCK_USED: u64 = 54 * 1024 * 1024 * 1024;
        assert!(
            !(info.total_bytes == MOCK_TOTAL
                && info.available_bytes == MOCK_AVAILABLE
                && info.used_bytes == MOCK_USED),
            "storage_info() must not return the old hardcoded mock triple"
        );
    }

    /// `statvfs_bytes` must return an honest error (not silently succeed
    /// with zeros) for a path that does not exist.
    #[cfg(unix)]
    #[test]
    fn test_statvfs_bytes_errors_on_nonexistent_path() {
        let bogus = std::env::temp_dir().join("oxigeo_mobile_enhanced_definitely_missing_dir_xyz");
        let _ = std::fs::remove_dir_all(&bogus);
        assert!(statvfs_bytes(&bogus).is_err());
    }
}
