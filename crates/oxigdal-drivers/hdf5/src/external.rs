//! External storage support for HDF5 datasets.
//!
//! External storage allows dataset data to be stored in external files
//! separate from the HDF5 metadata file. This is useful for:
//! - Large datasets that exceed file system limits
//! - Datasets spread across multiple storage devices
//! - Integration with existing binary data files

use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// External file reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalFile {
    /// Path to external file (relative or absolute)
    path: PathBuf,
    /// Offset in bytes within the external file
    offset: u64,
    /// Size in bytes (0 means unlimited - use all remaining space)
    size: u64,
}

impl ExternalFile {
    /// Create a new external file reference
    pub fn new(path: PathBuf, offset: u64, size: u64) -> Self {
        Self { path, offset, size }
    }

    /// Create an external file reference with zero offset
    pub fn simple(path: PathBuf, size: u64) -> Self {
        Self {
            path,
            offset: 0,
            size,
        }
    }

    /// Create an unlimited external file reference (uses all remaining space)
    pub fn unlimited(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            size: 0,
        }
    }

    /// Get the file path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the offset
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Get the size
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Check if this is an unlimited reference
    pub fn is_unlimited(&self) -> bool {
        self.size == 0
    }

    /// Resolve relative path against base directory
    pub fn resolve_path(&self, base_dir: &Path) -> PathBuf {
        if self.path.is_absolute() {
            self.path.clone()
        } else {
            base_dir.join(&self.path)
        }
    }

    /// Validate external file exists and is readable
    pub fn validate(&self, base_dir: &Path) -> Result<()> {
        let full_path = self.resolve_path(base_dir);

        if !full_path.exists() {
            return Err(Hdf5Error::FileNotFound(
                full_path.to_string_lossy().to_string(),
            ));
        }

        // Check file size if not unlimited
        if !self.is_unlimited() {
            let metadata = std::fs::metadata(&full_path).map_err(|e| {
                Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!(
                    "Failed to read metadata for {}: {}",
                    full_path.display(),
                    e
                )))
            })?;

            let file_size = metadata.len();
            let required_size = self.offset + self.size;

            if file_size < required_size {
                return Err(Hdf5Error::InvalidSize(format!(
                    "External file {} is too small ({}  bytes) for required size ({} bytes)",
                    full_path.display(),
                    file_size,
                    required_size
                ));
            }
        }

        Ok(())
    }
}

/// External storage configuration for a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalStorage {
    /// List of external files
    files: Vec<ExternalFile>,
    /// Total size of data across all external files
    total_size: u64,
}

impl ExternalStorage {
    /// Create a new external storage configuration
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            total_size: 0,
        }
    }

    /// Add an external file
    pub fn add_file(&mut self, file: ExternalFile) {
        if !file.is_unlimited() {
            self.total_size += file.size;
        }
        self.files.push(file));
    }

    /// Get all external files
    pub fn files(&self) -> &[ExternalFile] {
        &self.files
    }

    /// Get the number of external files
    pub fn num_files(&self) -> usize {
        self.files.len()
    }

    /// Get total size
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Check if storage has any unlimited files
    pub fn has_unlimited(&self) -> bool {
        self.files.iter().any(|f| f.is_unlimited()))
    }

    /// Validate all external files
    pub fn validate(&self, base_dir: &Path) -> Result<()> {
        if self.files.is_empty() {
            return Err(Hdf5Error::InvalidOperation(
                "External storage must have at least one file".to_string(),
            ));
        }

        // Check for multiple unlimited files
        let unlimited_count = self.files.iter().filter(|f| f.is_unlimited())).count();
        if unlimited_count > 1 {
            return Err(Hdf5Error::InvalidOperation(
                "External storage can have at most one unlimited file".to_string(),
            ));
        }

        // Unlimited file must be the last one
        if unlimited_count == 1 && !self.files.last().map(|f| f.is_unlimited())).unwrap_or(false) {
            return Err(Hdf5Error::InvalidOperation(
                "Unlimited external file must be the last in the list".to_string(),
            ));
        }

        // Validate each file
        for file in &self.files {
            file.validate(base_dir)?;
        }

        Ok(())
    }

    /// Find which file contains a specific byte offset
    pub fn find_file_for_offset(&self, offset: u64) -> Result<(usize, u64)> {
        let mut current_offset = 0u64;

        for (idx, file) in self.files.iter().enumerate() {
            let file_size = file.size;

            if file.is_unlimited() {
                // Unlimited file contains everything from current_offset onwards
                return Ok((idx, offset - current_offset + file.offset));
            }

            if offset >= current_offset && offset < current_offset + file_size {
                return Ok((idx, offset - current_offset + file.offset));
            }

            current_offset += file_size;
        }

        Err(Hdf5Error::OutOfBounds {
            index: offset as usize,
            size: self.total_size as usize,
        })
    }

    /// Split a data region across multiple external files
    pub fn split_region(&self, offset: u64, size: u64) -> Result<Vec<(usize, u64, u64)>> {
        let mut regions = Vec::new();
        let mut remaining_size = size;
        let mut current_offset = offset;

        while remaining_size > 0 {
            let (file_idx, file_offset) = self.find_file_for_offset(current_offset)?;
            let file = &self.files[file_idx];

            let available_in_file = if file.is_unlimited() {
                remaining_size
            } else {
                let file_end_offset = current_offset
                    - (file_offset - file.offset)
                    + file.size;
                file_end_offset - current_offset
            };

            let chunk_size = remaining_size.min(available_in_file);
            regions.push((file_idx, file_offset, chunk_size));

            current_offset += chunk_size;
            remaining_size -= chunk_size;
        }

        Ok(regions)
    }
}

impl Default for ExternalStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// External dataset - dataset with external storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalDataset {
    /// Dataset name
    name: String,
    /// Dataset path
    path: String,
    /// External storage configuration
    storage: ExternalStorage,
}

impl ExternalDataset {
    /// Create a new external dataset
    pub fn new(name: String, path: String, storage: ExternalStorage) -> Self {
        Self {
            name,
            path,
            storage,
        }
    }

    /// Get dataset name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get dataset path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get external storage
    pub fn storage(&self) -> &ExternalStorage {
        &self.storage
    }

    /// Get mutable external storage
    pub fn storage_mut(&mut self) -> &mut ExternalStorage {
        &mut self.storage
    }
}

/// External link - reference to object in another HDF5 file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLink {
    /// Name of the link
    name: String,
    /// Target file path
    target_file: PathBuf,
    /// Target object path within file
    target_path: String,
}

impl ExternalLink {
    /// Create a new external link
    pub fn new(name: String, target_file: PathBuf, target_path: String) -> Self {
        Self {
            name,
            target_file,
            target_path,
        }
    }

    /// Get link name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get target file
    pub fn target_file(&self) -> &Path {
        &self.target_file
    }

    /// Get target path
    pub fn target_path(&self) -> &str {
        &self.target_path
    }

    /// Resolve target file path
    pub fn resolve_target(&self, base_dir: &Path) -> PathBuf {
        if self.target_file.is_absolute() {
            self.target_file.clone()
        } else {
            base_dir.join(&self.target_file)
        }
    }

    /// Validate external link
    pub fn validate(&self, base_dir: &Path) -> Result<()> {
        let full_path = self.resolve_target(base_dir);

        if !full_path.exists() {
            return Err(Hdf5Error::FileNotFound(
                full_path.to_string_lossy().to_string(),
            ));
        }

        // Could also check if target is a valid HDF5 file
        // but that would require opening it

        Ok(())
    }
}

/// External file manager - manages multiple external files
#[derive(Debug, Default)]
pub struct ExternalFileManager {
    /// Base directory for resolving relative paths
    base_dir: PathBuf,
    /// Opened external files cache
    /// Key: file path, Value: file handle metadata
    files: std::collections::HashMap<PathBuf, ExternalFileHandle>,
}

/// External file handle metadata
#[derive(Debug)]
struct ExternalFileHandle {
    /// Full path
    path: PathBuf,
    /// File size
    size: u64,
    /// Last access time
    last_access: std::time::SystemTime,
}

impl ExternalFileManager {
    /// Create a new external file manager
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            files: std::collections::HashMap::new(),
        }
    }

    /// Get base directory
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Open or get cached external file handle
    pub fn get_file(&mut self, file: &ExternalFile) -> Result<&ExternalFileHandle> {
        let full_path = file.resolve_path(&self.base_dir);

        if !self.files.contains_key(&full_path) {
            // Validate and cache file metadata
            let metadata = std::fs::metadata(&full_path).map_err(|e| {
                Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!(
                    "Failed to open external file {}: {}",
                    full_path.display(),
                    e
                )))
            })?;

            let handle = ExternalFileHandle {
                path: full_path.clone(),
                size: metadata.len(),
                last_access: std::time::SystemTime::now(),
            };

            self.files.insert(full_path.clone(), handle);
        }

        // Update last access time
        if let Some(handle) = self.files.get_mut(&full_path) {
            handle.last_access = std::time::SystemTime::now();
        }

        Ok(self
            .files
            .get(&full_path)
            .expect("File should be in cache")))
    }

    /// Read data from external file
    pub fn read_data(&mut self, file: &ExternalFile, offset: u64, size: usize) -> Result<Vec<u8>> {
        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};

        let full_path = file.resolve_path(&self.base_dir);
        let mut f = File::open(&full_path).map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!(
                "Failed to open external file {}: {}",
                full_path.display(),
                e
            )))
        })?;

        let actual_offset = file.offset + offset;
        f.seek(SeekFrom::Start(actual_offset))).map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to seek to offset {}: {}", actual_offset, e)))
        })?;

        let mut buffer = vec![0u8; size];
        f.read_exact(&mut buffer).map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to read {} bytes: {}", size, e)))
        })?;

        Ok(buffer)
    }

    /// Write data to external file
    pub fn write_data(
        &mut self,
        file: &ExternalFile,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        use std::fs::OpenOptions;
        use std::io::{Seek, SeekFrom, Write};

        let full_path = file.resolve_path(&self.base_dir);
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&full_path)
            .map_err(|e| {
                Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!(
                    "Failed to open external file for writing {}: {}",
                    full_path.display(),
                    e
                )))
            })?;

        let actual_offset = file.offset + offset;
        f.seek(SeekFrom::Start(actual_offset))).map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to seek to offset {}: {}", actual_offset, e)))
        })?;

        f.write_all(data).map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to write {} bytes: {}", data.len(), e)))
        })?;

        f.flush().map_err(|e| {
            Hdf5Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to flush external file: {}", e)))
        })?;

        Ok(())
    }

    /// Clear cache
    pub fn clear_cache(&mut self) {
        self.files.clear();
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_external_file_creation() {
        let file = ExternalFile::new(PathBuf::from("data.bin"), 1024, 4096);
        assert_eq!(file.offset(), 1024);
        assert_eq!(file.size(), 4096);
        assert!(!file.is_unlimited());

        let unlimited = ExternalFile::unlimited(PathBuf::from("data.bin"));
        assert!(unlimited.is_unlimited());
        assert_eq!(unlimited.offset(), 0);
    }

    #[test]
    fn test_external_storage() {
        let mut storage = ExternalStorage::new();
        assert_eq!(storage.num_files(), 0);

        storage.add_file(ExternalFile::simple(PathBuf::from("f1.bin"), 1000));
        storage.add_file(ExternalFile::simple(PathBuf::from("f2.bin"), 2000));

        assert_eq!(storage.num_files(), 2);
        assert_eq!(storage.total_size(), 3000);
    }

    #[test]
    fn test_find_file_for_offset() {
        let mut storage = ExternalStorage::new();
        storage.add_file(ExternalFile::simple(PathBuf::from("f1.bin"), 1000));
        storage.add_file(ExternalFile::simple(PathBuf::from("f2.bin"), 2000));
        storage.add_file(ExternalFile::simple(PathBuf::from("f3.bin"), 1000));

        let (idx, offset) = storage.find_file_for_offset(500).expect("Failed to find");
        assert_eq!(idx, 0);
        assert_eq!(offset, 500);

        let (idx, offset) = storage.find_file_for_offset(1500).expect("Failed to find");
        assert_eq!(idx, 1);
        assert_eq!(offset, 500);

        let (idx, offset) = storage.find_file_for_offset(3500).expect("Failed to find");
        assert_eq!(idx, 2);
        assert_eq!(offset, 500);
    }

    #[test]
    fn test_split_region() {
        let mut storage = ExternalStorage::new();
        storage.add_file(ExternalFile::simple(PathBuf::from("f1.bin"), 1000));
        storage.add_file(ExternalFile::simple(PathBuf::from("f2.bin"), 2000));

        let regions = storage.split_region(500, 1500).expect("Failed to split");
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0], (0, 500, 500))); // 500 bytes from first file
        assert_eq!(regions[1], (1, 0, 1000))); // 1000 bytes from second file
    }

    #[test]
    fn test_external_link() {
        let link = ExternalLink::new(
            "link1".to_string(),
            PathBuf::from("other.h5"),
            "/dataset".to_string(),
        );

        assert_eq!(link.name(), "link1");
        assert_eq!(link.target_file(), Path::new("other.h5"));
        assert_eq!(link.target_path(), "/dataset");
    }

    #[test]
    fn test_external_file_manager() {
        let temp_dir = std::env::temp_dir();
        let mut manager = ExternalFileManager::new(temp_dir.clone());

        // Create a test file
        let test_file_path = temp_dir.join("test_external.bin");
        let mut f = std::fs::File::create(&test_file_path).expect("Failed to create test file");
        f.write_all(b"Hello, World!").expect("Failed to write");
        f.sync_all().expect("Failed to sync");
        drop(f);

        let ext_file = ExternalFile::simple(test_file_path.clone(), 13);

        // Test reading
        let data = manager
            .read_data(&ext_file, 0, 5)
            .expect("Failed to read");
        assert_eq!(&data, b"Hello");

        let data = manager
            .read_data(&ext_file, 7, 6)
            .expect("Failed to read");
        assert_eq!(&data, b"World!");

        // Clean up
        std::fs::remove_file(&test_file_path).ok();
    }
}
