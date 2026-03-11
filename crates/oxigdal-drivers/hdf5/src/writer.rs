//! HDF5 file writer with Pure Rust support for basic features.
//!
//! This module provides HDF5 file writing capabilities. The default implementation
//! uses Pure Rust for basic HDF5 writing, following the COOLJAPAN Pure Rust policy.

use crate::attribute::Attribute;
use crate::dataset::{CompressionFilter, Dataset, DatasetProperties};
use crate::datatype::{Datatype, TypeConverter};
use crate::error::{Hdf5Error, Result};
use crate::group::{Group, ObjectRef, ObjectType, PathUtils};
use byteorder::{LittleEndian, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// HDF5 file signature
const HDF5_SIGNATURE: &[u8] = b"\x89HDF\r\n\x1a\n";

/// Metadata marker for Pure Rust implementation
const METADATA_MARKER: &[u8] = b"OXIGDAL_HDF5_METADATA_V1\n";

/// File metadata structure for Pure Rust implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileMetadata {
    /// Groups (path -> Group)
    groups: HashMap<String, Group>,
    /// Datasets (path -> Dataset)
    datasets: HashMap<String, Dataset>,
}

/// HDF5 file version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hdf5Version {
    /// HDF5 1.0 (Superblock Version 0)
    V10,
    /// HDF5 1.2 (Superblock Version 1)
    V12,
}

/// HDF5 file writer
pub struct Hdf5Writer {
    /// File handle
    file: File,
    /// HDF5 version
    version: Hdf5Version,
    /// Size of offsets (4 or 8 bytes)
    size_of_offsets: u8,
    /// Size of lengths (4 or 8 bytes)
    size_of_lengths: u8,
    /// Groups (path -> Group)
    groups: HashMap<String, Group>,
    /// Datasets (path -> Dataset)
    datasets: HashMap<String, Dataset>,
    /// Current file position
    current_position: u64,
    /// File is finalized
    finalized: bool,
}

// Implement Write trait for Hdf5Writer to delegate to the file field
impl Write for Hdf5Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.current_position += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

// Implement Seek trait for Hdf5Writer to delegate to the file field
impl Seek for Hdf5Writer {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = self.file.seek(pos)?;
        self.current_position = new_pos;
        Ok(new_pos)
    }
}

impl Hdf5Writer {
    /// Create a new HDF5 file for writing
    pub fn create<P: AsRef<Path>>(path: P, version: Hdf5Version) -> Result<Self> {
        let file = File::create(path)?;

        let mut writer = Self {
            file,
            version,
            size_of_offsets: 8, // Use 8-byte offsets by default
            size_of_lengths: 8, // Use 8-byte lengths by default
            groups: HashMap::new(),
            datasets: HashMap::new(),
            current_position: 0,
            finalized: false,
        };

        // Initialize root group
        writer.groups.insert("/".to_string(), Group::root());

        Ok(writer)
    }

    /// Create a group
    pub fn create_group(&mut self, path: &str) -> Result<()> {
        let normalized = PathUtils::normalize(path)?;

        if self.groups.contains_key(&normalized) {
            return Err(Hdf5Error::ObjectExists(normalized));
        }

        // Check that parent exists
        let (parent_path, name) = PathUtils::split(&normalized)?;
        if !self.groups.contains_key(&parent_path) {
            return Err(Hdf5Error::PathNotFound(parent_path));
        }

        // Create group
        let group = Group::new(name.clone(), normalized.clone());
        self.groups.insert(normalized.clone(), group);

        // Add to parent
        let parent = self
            .groups
            .get_mut(&parent_path)
            .ok_or_else(|| Hdf5Error::internal("Parent group disappeared"))?;
        parent.add_child(ObjectRef::new(name, ObjectType::Group, normalized));

        Ok(())
    }

    /// Create a dataset
    pub fn create_dataset(
        &mut self,
        path: &str,
        datatype: Datatype,
        dims: Vec<usize>,
        properties: DatasetProperties,
    ) -> Result<()> {
        let normalized = PathUtils::normalize(path)?;

        if self.datasets.contains_key(&normalized) {
            return Err(Hdf5Error::ObjectExists(normalized));
        }

        // Check that parent group exists
        let (parent_path, name) = PathUtils::split(&normalized)?;
        if !self.groups.contains_key(&parent_path) {
            return Err(Hdf5Error::PathNotFound(parent_path));
        }

        // Create dataset
        let dataset = Dataset::new(name.clone(), normalized.clone(), datatype, dims, properties)?;
        self.datasets.insert(normalized.clone(), dataset);

        // Add to parent
        let parent = self
            .groups
            .get_mut(&parent_path)
            .ok_or_else(|| Hdf5Error::internal("Parent group disappeared"))?;
        parent.add_child(ObjectRef::new(name, ObjectType::Dataset, normalized));

        Ok(())
    }

    /// Write dataset data
    pub fn write_dataset(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let normalized = PathUtils::normalize(path)?;

        let dataset = self
            .datasets
            .get_mut(&normalized)
            .ok_or_else(|| Hdf5Error::dataset_not_found(path))?;

        // Validate data size
        let expected_size = dataset.size_in_bytes();
        if data.len() != expected_size {
            return Err(Hdf5Error::InvalidSize(format!(
                "Data size ({}) does not match expected size ({})",
                data.len(),
                expected_size
            )));
        }

        // Store data
        dataset.set_data(data.to_vec())?;

        Ok(())
    }

    /// Write i32 array to dataset
    pub fn write_i32(&mut self, path: &str, data: &[i32]) -> Result<()> {
        let mut raw_data = Vec::with_capacity(data.len() * 4);
        for &value in data {
            let mut buf = [0u8; 4];
            TypeConverter::write_i32_le(&mut buf, value)?;
            raw_data.extend_from_slice(&buf);
        }
        self.write_dataset(path, &raw_data)
    }

    /// Write f32 array to dataset
    pub fn write_f32(&mut self, path: &str, data: &[f32]) -> Result<()> {
        let mut raw_data = Vec::with_capacity(data.len() * 4);
        for &value in data {
            let mut buf = [0u8; 4];
            TypeConverter::write_f32_le(&mut buf, value)?;
            raw_data.extend_from_slice(&buf);
        }
        self.write_dataset(path, &raw_data)
    }

    /// Write f64 array to dataset
    pub fn write_f64(&mut self, path: &str, data: &[f64]) -> Result<()> {
        let mut raw_data = Vec::with_capacity(data.len() * 8);
        for &value in data {
            let mut buf = [0u8; 8];
            TypeConverter::write_f64_le(&mut buf, value)?;
            raw_data.extend_from_slice(&buf);
        }
        self.write_dataset(path, &raw_data)
    }

    /// Add an attribute to a group
    pub fn add_group_attribute(&mut self, path: &str, attribute: Attribute) -> Result<()> {
        let normalized = PathUtils::normalize(path)?;

        let group = self
            .groups
            .get_mut(&normalized)
            .ok_or_else(|| Hdf5Error::group_not_found(path))?;

        group.attributes_mut().add(attribute);

        Ok(())
    }

    /// Add an attribute to a dataset
    pub fn add_dataset_attribute(&mut self, path: &str, attribute: Attribute) -> Result<()> {
        let normalized = PathUtils::normalize(path)?;

        let dataset = self
            .datasets
            .get_mut(&normalized)
            .ok_or_else(|| Hdf5Error::dataset_not_found(path))?;

        dataset.attributes_mut().add(attribute);

        Ok(())
    }

    /// Finalize and write the file
    pub fn finalize(&mut self) -> Result<()> {
        if self.finalized {
            return Err(Hdf5Error::internal("File already finalized"));
        }

        // Write HDF5 signature
        self.file.write_all(HDF5_SIGNATURE)?;
        self.current_position += HDF5_SIGNATURE.len() as u64;

        // Write superblock
        self.write_superblock()?;

        // Write groups and datasets
        self.write_data()?;

        // Flush file
        self.file.flush()?;

        self.finalized = true;

        Ok(())
    }

    /// Write superblock
    fn write_superblock(&mut self) -> Result<()> {
        match self.version {
            Hdf5Version::V10 => {
                // Version 0
                self.file.write_u8(0)?;
                self.current_position += 1;

                // Free-space storage version
                self.file.write_u8(0)?;
                self.current_position += 1;

                // Root group symbol table version
                self.file.write_u8(0)?;
                self.current_position += 1;

                // Reserved
                self.file.write_u8(0)?;
                self.current_position += 1;

                // Shared header message format version
                self.file.write_u8(0)?;
                self.current_position += 1;

                // Size of offsets
                self.file.write_u8(self.size_of_offsets)?;
                self.current_position += 1;

                // Size of lengths
                self.file.write_u8(self.size_of_lengths)?;
                self.current_position += 1;

                // Reserved
                self.file.write_u8(0)?;
                self.current_position += 1;

                // Group leaf node K
                self.file.write_u16::<LittleEndian>(4)?;
                self.current_position += 2;

                // Group internal node K
                self.file.write_u16::<LittleEndian>(16)?;
                self.current_position += 2;

                // File consistency flags
                self.file.write_u32::<LittleEndian>(0)?;
                self.current_position += 4;

                // Base address
                self.write_offset(0)?;

                // Address of file free space info
                self.write_offset(0xFFFFFFFF_FFFFFFFF)?; // Undefined

                // End of file address
                self.write_offset(0)?; // Will be updated later

                // Driver information block address
                self.write_offset(0xFFFFFFFF_FFFFFFFF)?; // Undefined

                // Root group symbol table entry (simplified)
                self.write_offset(0)?; // Link name offset
                self.write_offset(0)?; // Object header address
                self.write_u32::<LittleEndian>(0)?; // Cache type
                self.write_u32::<LittleEndian>(0)?; // Reserved

                // Scratch space
                for _ in 0..16 {
                    self.file.write_u8(0)?;
                    self.current_position += 1;
                }
            }
            Hdf5Version::V12 => {
                // Version 1
                self.file.write_u8(1)?;
                self.current_position += 1;

                // Similar to V10 but with additional fields
                // (Simplified implementation for demonstration)
                return Err(Hdf5Error::feature_not_available(
                    "HDF5 1.2 writing (use V10 for Pure Rust)",
                ));
            }
        }

        Ok(())
    }

    /// Write offset value
    fn write_offset(&mut self, value: u64) -> Result<()> {
        match self.size_of_offsets {
            4 => {
                let value32 = u32::try_from(value)
                    .map_err(|_| Hdf5Error::invalid_format("Offset too large for 4 bytes"))?;
                self.file.write_u32::<LittleEndian>(value32)?;
                self.current_position += 4;
            }
            8 => {
                self.file.write_u64::<LittleEndian>(value)?;
                self.current_position += 8;
            }
            _ => {
                return Err(Hdf5Error::invalid_format(format!(
                    "Invalid offset size: {}",
                    self.size_of_offsets
                )));
            }
        }
        Ok(())
    }

    /// Write length value
    #[allow(dead_code)]
    fn write_length(&mut self, value: u64) -> Result<()> {
        self.write_offset(value)
    }

    /// Write data (groups and datasets)
    fn write_data(&mut self) -> Result<()> {
        // For Pure Rust minimal implementation, we write:
        // 1. Metadata section (JSON) for discoverability
        // 2. Dataset data sequentially

        // Write metadata marker
        self.file.write_all(METADATA_MARKER)?;
        self.current_position += METADATA_MARKER.len() as u64;

        // Serialize metadata
        let metadata = FileMetadata {
            groups: self.groups.clone(),
            datasets: self.datasets.clone(),
        };

        let metadata_json = serde_json::to_vec(&metadata)
            .map_err(|e| Hdf5Error::internal(format!("Failed to serialize metadata: {}", e)))?;

        // Write metadata length (8 bytes)
        self.file
            .write_u64::<LittleEndian>(metadata_json.len() as u64)?;
        self.current_position += 8;

        // Write metadata
        self.file.write_all(&metadata_json)?;
        self.current_position += metadata_json.len() as u64;

        // Write datasets
        for dataset in self.datasets.values() {
            if let Some(data) = dataset.data() {
                // Apply compression if needed
                let compressed_data =
                    self.compress_data(data, dataset.properties().compression())?;

                // Write data
                self.file.write_all(&compressed_data)?;
                self.current_position += compressed_data.len() as u64;
            }
        }

        Ok(())
    }

    /// Compress data if needed
    fn compress_data(&self, data: &[u8], filter: CompressionFilter) -> Result<Vec<u8>> {
        match filter {
            CompressionFilter::None => Ok(data.to_vec()),
            CompressionFilter::Gzip { level } => oxiarc_archive::gzip::compress(data, level)
                .map_err(|e| Hdf5Error::compression(e.to_string())),
            CompressionFilter::Lzf | CompressionFilter::Szip => {
                Err(Hdf5Error::feature_not_available(format!(
                    "{:?} compression (use GZIP or enable hdf5_sys feature)",
                    filter
                )))
            }
        }
    }

    /// Get current file position
    pub fn position(&self) -> u64 {
        self.current_position
    }

    /// Check if finalized
    pub fn is_finalized(&self) -> bool {
        self.finalized
    }
}

impl Drop for Hdf5Writer {
    fn drop(&mut self) {
        if !self.finalized {
            // Try to finalize on drop
            let _ = self.finalize();
        }
    }
}

/// Builder for Hdf5Writer with configuration options
pub struct Hdf5WriterBuilder {
    /// HDF5 version
    version: Hdf5Version,
    /// Size of offsets
    size_of_offsets: u8,
    /// Size of lengths
    size_of_lengths: u8,
}

impl Hdf5WriterBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            version: Hdf5Version::V10,
            size_of_offsets: 8,
            size_of_lengths: 8,
        }
    }

    /// Set HDF5 version
    pub fn version(mut self, version: Hdf5Version) -> Self {
        self.version = version;
        self
    }

    /// Set size of offsets (4 or 8 bytes)
    pub fn size_of_offsets(mut self, size: u8) -> Self {
        self.size_of_offsets = size;
        self
    }

    /// Set size of lengths (4 or 8 bytes)
    pub fn size_of_lengths(mut self, size: u8) -> Self {
        self.size_of_lengths = size;
        self
    }

    /// Build the writer
    pub fn create<P: AsRef<Path>>(self, path: P) -> Result<Hdf5Writer> {
        let mut writer = Hdf5Writer::create(path, self.version)?;
        writer.size_of_offsets = self.size_of_offsets;
        writer.size_of_lengths = self.size_of_lengths;
        Ok(writer)
    }
}

impl Default for Hdf5WriterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_writer_creation() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10);
        assert!(writer.is_ok());
    }

    #[test]
    fn test_create_group() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        assert!(writer.create_group("/group1").is_ok());
        assert!(writer.create_group("/group1").is_err()); // Already exists
        assert!(writer.create_group("/group1/subgroup").is_ok());
        assert!(writer.create_group("/nonexistent/subgroup").is_err()); // Parent doesn't exist
    }

    #[test]
    fn test_create_dataset() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        let result = writer.create_dataset(
            "/data",
            Datatype::Float32,
            vec![10, 20],
            DatasetProperties::new(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_write_dataset() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        writer
            .create_dataset("/data", Datatype::Int32, vec![10], DatasetProperties::new())
            .expect("Failed to create dataset");

        let data: Vec<i32> = (0..10).collect();
        assert!(writer.write_i32("/data", &data).is_ok());
    }

    #[test]
    fn test_builder() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let writer = Hdf5WriterBuilder::new()
            .version(Hdf5Version::V10)
            .size_of_offsets(8)
            .create(temp_file.path());
        assert!(writer.is_ok());
    }
}
