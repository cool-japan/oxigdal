//! HDF5 file writer backed by the real Pure-Rust [`oxih5`] crate.
//!
//! [`Hdf5Writer`] accumulates groups, datasets, and attributes through the same
//! public API as before, and on [`Hdf5Writer::finalize`] emits a genuine HDF5
//! file via `oxih5`'s writer. The legacy `OXIGDAL_HDF5_METADATA_V1` JSON
//! sidecar writer (the 0.1.x-era on-disk format identifier, removed) has been
//! retired.
//!
//! `oxih5`'s writer covers a real but bounded surface: root-level datasets
//! (`i32`/`i64`/`f32`/`f64`/`u8`, or zero-filled fixed-size numeric datatypes),
//! single-level sub-groups, `f64`/`i32` datasets inside sub-groups, string root
//! attributes, and `string`/`f64`/`i64`/`i32` scalar attributes on datasets.
//! Anything beyond that (nested groups, sub-group attributes, non-string root
//! attributes, unsupported element types) returns a typed error at
//! [`Hdf5Writer::finalize`] rather than silently degrading.

use crate::attribute::{Attribute, AttributeValue};
use crate::convert;
use crate::dataset::{Dataset, DatasetProperties};
use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use crate::group::{Group, ObjectRef, ObjectType, PathUtils};
use oxih5::FileWriter;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// HDF5 file version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hdf5Version {
    /// HDF5 1.0 (Superblock Version 0)
    V10,
    /// HDF5 1.2 (Superblock Version 1)
    V12,
}

/// HDF5 file writer (real HDF5, backed by `oxih5`).
pub struct Hdf5Writer {
    /// File handle (created eagerly; the real bytes are written on `finalize`).
    file: File,
    /// Destination path (used to emit the real file via `oxih5`).
    path: PathBuf,
    /// HDF5 version
    version: Hdf5Version,
    /// Size of offsets (4 or 8 bytes) — accepted for API compatibility;
    /// the real `oxih5` writer always emits 8-byte offsets.
    #[allow(dead_code)]
    size_of_offsets: u8,
    /// Size of lengths (4 or 8 bytes) — accepted for API compatibility;
    /// the real `oxih5` writer always emits 8-byte lengths.
    #[allow(dead_code)]
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

// Implement Write trait for Hdf5Writer to delegate to the file field.
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

// Implement Seek trait for Hdf5Writer to delegate to the file field.
impl Seek for Hdf5Writer {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = self.file.seek(pos)?;
        self.current_position = new_pos;
        Ok(new_pos)
    }
}

impl Hdf5Writer {
    /// Create a new HDF5 file for writing.
    pub fn create<P: AsRef<Path>>(path: P, version: Hdf5Version) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::create(&path)?;

        let mut writer = Self {
            file,
            path,
            version,
            size_of_offsets: 8,
            size_of_lengths: 8,
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

    /// Write dataset data (raw little-endian element bytes).
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
        let raw_data: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.write_dataset(path, &raw_data)
    }

    /// Write f32 array to dataset
    pub fn write_f32(&mut self, path: &str, data: &[f32]) -> Result<()> {
        let raw_data: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.write_dataset(path, &raw_data)
    }

    /// Write f64 array to dataset
    pub fn write_f64(&mut self, path: &str, data: &[f64]) -> Result<()> {
        let raw_data: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
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

    /// Finalize and write the real HDF5 file.
    pub fn finalize(&mut self) -> Result<()> {
        if self.finalized {
            return Err(Hdf5Error::internal("File already finalized"));
        }

        self.write_real()?;
        self.file.flush()?;
        self.finalized = true;

        Ok(())
    }

    /// Translate the accumulated groups/datasets/attributes into an `oxih5`
    /// [`FileWriter`] and emit a genuine HDF5 file.
    fn write_real(&self) -> Result<()> {
        if self.version == Hdf5Version::V12 {
            return Err(Hdf5Error::feature_not_available(
                "HDF5 1.2 (superblock v1) writing — the real writer emits superblock v0 (use V10)",
            ));
        }

        let mut fw = FileWriter::new();

        // 1. Sub-groups (single-level only; sub-group attributes unsupported).
        let mut group_names: Vec<String> = Vec::new();
        for (path, group) in &self.groups {
            if path == "/" {
                continue;
            }
            let segments = path_segments(path);
            if segments.len() != 1 {
                return Err(Hdf5Error::feature_not_available(format!(
                    "nested group '{path}' — the real HDF5 writer supports only single-level groups"
                )));
            }
            if !group.attributes().is_empty() {
                return Err(Hdf5Error::feature_not_available(format!(
                    "attributes on sub-group '{path}' are not supported by the real HDF5 writer"
                )));
            }
            group_names.push(segments[0].clone());
        }
        group_names.sort();
        group_names.dedup();
        for name in &group_names {
            fw.create_group(name).map_err(convert::map_oxih5_err)?;
        }

        // 2. Root-group attributes (string only).
        if let Some(root) = self.groups.get("/") {
            for attr in root.attributes().iter() {
                match attr.value() {
                    AttributeValue::String(s) => fw.write_root_str_attr(attr.name(), s),
                    other => {
                        return Err(Hdf5Error::feature_not_available(format!(
                            "root attribute '{}' of type {} — the real HDF5 writer supports only string root attributes",
                            attr.name(),
                            other.datatype().name()
                        )));
                    }
                }
            }
        }

        // 3. Datasets (root-level and single-level-group).
        for (path, ds) in &self.datasets {
            let (parent, name) = PathUtils::split(path)?;
            if parent == "/" {
                write_root_dataset(&mut fw, &name, ds)?;
            } else {
                let group_segments = path_segments(&parent);
                if group_segments.len() != 1 {
                    return Err(Hdf5Error::feature_not_available(format!(
                        "dataset '{path}' nested deeper than one group level is not supported by the real HDF5 writer"
                    )));
                }
                write_group_dataset(&mut fw, &group_segments[0], &name, ds)?;
            }
        }

        // 4. Dataset attributes (after datasets exist).
        for (path, ds) in &self.datasets {
            let (parent, name) = PathUtils::split(path)?;
            let group_name = if parent == "/" {
                None
            } else {
                Some(path_first_segment(&parent))
            };
            for attr in ds.attributes().iter() {
                write_dataset_attr(&mut fw, group_name.as_deref(), &name, attr)?;
            }
        }

        fw.build(&self.path).map_err(convert::map_oxih5_err)?;
        Ok(())
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
            // Try to finalize on drop; ignore errors (nothing to propagate to).
            let _ = self.finalize();
        }
    }
}

/// Split a normalized path into its non-empty segments.
fn path_segments(path: &str) -> Vec<String> {
    path.trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Return the first path segment (or the whole trimmed path if it has none).
fn path_first_segment(path: &str) -> String {
    path.trim_start_matches('/')
        .split('/')
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| path.trim_start_matches('/'))
        .to_string()
}

/// Write a root-level dataset (with data, or zero-filled) into the `FileWriter`.
fn write_root_dataset(fw: &mut FileWriter, name: &str, ds: &Dataset) -> Result<()> {
    let dims = ds.dims();
    match ds.data() {
        Some(bytes) => match ds.datatype() {
            Datatype::Int32 => {
                let v = bytes_to_i32(bytes);
                fw.write_dataset_i32(name, &v, dims)
                    .map_err(convert::map_oxih5_err)?;
            }
            Datatype::Int64 => {
                let v = bytes_to_i64(bytes);
                fw.write_dataset_i64(name, &v, dims)
                    .map_err(convert::map_oxih5_err)?;
            }
            Datatype::Float32 => {
                let v = bytes_to_f32(bytes);
                fw.write_dataset_f32(name, &v, dims)
                    .map_err(convert::map_oxih5_err)?;
            }
            Datatype::Float64 => {
                let v = bytes_to_f64(bytes);
                fw.write_dataset_f64(name, &v, dims)
                    .map_err(convert::map_oxih5_err)?;
            }
            Datatype::UInt8 => {
                fw.write_dataset_u8(name, bytes, dims)
                    .map_err(convert::map_oxih5_err)?;
            }
            other => {
                return Err(Hdf5Error::feature_not_available(format!(
                    "writing dataset '{name}' with data of type {} — the real HDF5 writer supports i32, i64, f32, f64, u8",
                    other.name()
                )));
            }
        },
        None => {
            let dtype = convert::to_oxih5_dtype(ds.datatype())?;
            fw.create_dataset(name, dims, &dtype)
                .map_err(convert::map_oxih5_err)?;
        }
    }
    Ok(())
}

/// Write a dataset inside a single-level sub-group into the `FileWriter`.
fn write_group_dataset(fw: &mut FileWriter, group: &str, name: &str, ds: &Dataset) -> Result<()> {
    match ds.data() {
        Some(bytes) => match ds.datatype() {
            Datatype::Float64 => {
                let v = bytes_to_f64(bytes);
                fw.write_group_dataset_f64(group, name, &v, ds.dims())
                    .map_err(convert::map_oxih5_err)?;
            }
            Datatype::Int32 => {
                let v = bytes_to_i32(bytes);
                fw.write_group_dataset_i32(group, name, &v, ds.dims())
                    .map_err(convert::map_oxih5_err)?;
            }
            other => {
                return Err(Hdf5Error::feature_not_available(format!(
                    "dataset '{name}' in group '{group}' of type {} — the real HDF5 writer supports f64 and i32 in sub-groups",
                    other.name()
                )));
            }
        },
        None => {
            return Err(Hdf5Error::feature_not_available(format!(
                "zero-filled dataset '{name}' in group '{group}' is not supported by the real HDF5 writer (write its data)"
            )));
        }
    }
    Ok(())
}

/// Write one dataset attribute into the `FileWriter`.
///
/// `group` is `Some(group_name)` for datasets inside a single-level sub-group,
/// or `None` for root-level datasets.
fn write_dataset_attr(
    fw: &mut FileWriter,
    group: Option<&str>,
    dataset: &str,
    attr: &Attribute,
) -> Result<()> {
    match (group, attr.value()) {
        (None, AttributeValue::String(s)) => {
            fw.write_string_attr(dataset, attr.name(), s)
                .map_err(convert::map_oxih5_err)?;
        }
        (None, AttributeValue::Float64(v)) => {
            fw.write_f64_attr(dataset, attr.name(), *v)
                .map_err(convert::map_oxih5_err)?;
        }
        (None, AttributeValue::Int64(v)) => {
            fw.write_i64_attr(dataset, attr.name(), *v)
                .map_err(convert::map_oxih5_err)?;
        }
        (None, AttributeValue::Int32(v)) => {
            fw.write_i32_attr(dataset, attr.name(), *v)
                .map_err(convert::map_oxih5_err)?;
        }
        (Some(g), AttributeValue::String(s)) => {
            fw.write_group_string_attr(g, dataset, attr.name(), s)
                .map_err(convert::map_oxih5_err)?;
        }
        (_, other) => {
            return Err(Hdf5Error::feature_not_available(format!(
                "attribute '{}' of type {} on dataset '{}' — the real HDF5 writer supports string/f64/i64/i32 on root datasets and string on sub-group datasets",
                attr.name(),
                other.datatype().name(),
                dataset
            )));
        }
    }
    Ok(())
}

fn bytes_to_i32(bytes: &[u8]) -> Vec<i32> {
    bytes
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn bytes_to_i64(bytes: &[u8]) -> Vec<i64> {
    bytes
        .chunks_exact(8)
        .map(|c| {
            let mut a = [0u8; 8];
            a.copy_from_slice(c);
            i64::from_le_bytes(a)
        })
        .collect()
}

fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn bytes_to_f64(bytes: &[u8]) -> Vec<f64> {
    bytes
        .chunks_exact(8)
        .map(|c| {
            let mut a = [0u8; 8];
            a.copy_from_slice(c);
            f64::from_le_bytes(a)
        })
        .collect()
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
        assert!(writer.create_group("/nonexistent/subgroup").is_err()); // Parent missing
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

    /// Finalizing a real writer must emit a genuine HDF5 file (with the standard
    /// signature) that round-trips its values through `oxih5`.
    #[test]
    fn test_finalize_emits_real_hdf5() {
        let dir = std::env::temp_dir();
        let path = dir.join("oxigeo_hdf5_writer_real_signature.h5");

        {
            let mut writer = Hdf5Writer::create(&path, Hdf5Version::V10).expect("create writer");
            writer
                .create_dataset("/x", Datatype::Float64, vec![3], DatasetProperties::new())
                .expect("create dataset");
            writer.write_f64("/x", &[1.0, 2.0, 3.0]).expect("write f64");
            writer.finalize().expect("finalize");
        }

        // Real HDF5 signature.
        let bytes = std::fs::read(&path).expect("read back");
        assert!(
            bytes.starts_with(b"\x89HDF\r\n\x1a\n"),
            "finalized file must carry the real HDF5 signature"
        );

        // Real values via oxih5's own reader.
        let file = oxih5::open(&path).expect("oxih5 open");
        let ds = file.dataset("x").expect("dataset x");
        assert_eq!(ds.shape, vec![3]);
        assert_eq!(ds.as_f64().expect("as_f64"), vec![1.0, 2.0, 3.0]);

        let _ = std::fs::remove_file(&path);
    }

    /// Non-string root attributes are beyond `oxih5`'s writer surface and must
    /// fail loud rather than silently degrade.
    #[test]
    fn test_unsupported_root_attr_fails_loud() {
        let dir = std::env::temp_dir();
        let path = dir.join("oxigeo_hdf5_writer_unsupported_root_attr.h5");

        let mut writer = Hdf5Writer::create(&path, Hdf5Version::V10).expect("create writer");
        writer
            .add_group_attribute("/", Attribute::f64("scale", 0.5))
            .expect("buffer attr");
        let result = writer.finalize();
        assert!(matches!(result, Err(Hdf5Error::FeatureNotAvailable { .. })));

        let _ = std::fs::remove_file(&path);
    }
}
