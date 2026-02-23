//! HDF5 dataset handling for multi-dimensional array storage.
//!
//! Datasets are multi-dimensional arrays with a fixed datatype and shape.
//! They can be chunked, compressed, and have associated metadata (attributes).

use crate::attribute::Attributes;
use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};

/// Dataset layout type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutType {
    /// Contiguous layout (all data in a single block)
    Contiguous,
    /// Chunked layout (data divided into fixed-size chunks)
    Chunked,
    /// Compact layout (data stored in the object header)
    Compact,
}

/// Compression filter type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionFilter {
    /// No compression
    None,
    /// GZIP/DEFLATE compression
    Gzip {
        /// Compression level (1-9)
        level: u8,
    },
    /// LZF compression
    Lzf,
    /// SZIP compression
    Szip,
}

/// Dataset creation properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetProperties {
    /// Layout type
    layout: LayoutType,
    /// Chunk dimensions (only for chunked layout)
    chunk_dims: Option<Vec<usize>>,
    /// Compression filter
    compression: CompressionFilter,
    /// Fill value
    fill_value: Option<Vec<u8>>,
}

impl DatasetProperties {
    /// Create default properties (contiguous layout, no compression)
    pub fn new() -> Self {
        Self {
            layout: LayoutType::Contiguous,
            chunk_dims: None,
            compression: CompressionFilter::None,
            fill_value: None,
        }
    }

    /// Set layout type
    pub fn with_layout(mut self, layout: LayoutType) -> Self {
        self.layout = layout;
        self
    }

    /// Set chunking (automatically sets layout to Chunked)
    pub fn with_chunks(mut self, chunk_dims: Vec<usize>) -> Self {
        self.layout = LayoutType::Chunked;
        self.chunk_dims = Some(chunk_dims);
        self
    }

    /// Set compression
    pub fn with_compression(mut self, compression: CompressionFilter) -> Self {
        self.compression = compression;
        self
    }

    /// Set GZIP compression
    pub fn with_gzip(mut self, level: u8) -> Self {
        let level = level.clamp(1, 9);
        self.compression = CompressionFilter::Gzip { level };
        self
    }

    /// Set fill value
    pub fn with_fill_value(mut self, fill_value: Vec<u8>) -> Self {
        self.fill_value = Some(fill_value);
        self
    }

    /// Get layout type
    pub fn layout(&self) -> LayoutType {
        self.layout
    }

    /// Get chunk dimensions
    pub fn chunk_dims(&self) -> Option<&[usize]> {
        self.chunk_dims.as_deref()
    }

    /// Get compression filter
    pub fn compression(&self) -> CompressionFilter {
        self.compression
    }

    /// Get fill value
    pub fn fill_value(&self) -> Option<&[u8]> {
        self.fill_value.as_deref()
    }

    /// Validate chunk dimensions against dataset dimensions
    pub fn validate_chunks(&self, dims: &[usize]) -> Result<()> {
        if let Some(chunks) = &self.chunk_dims {
            if chunks.len() != dims.len() {
                return Err(Hdf5Error::InvalidChunkSize(format!(
                    "Chunk dimensions ({}) must match dataset dimensions ({})",
                    chunks.len(),
                    dims.len()
                )));
            }

            for (i, (&chunk_size, &dim_size)) in chunks.iter().zip(dims.iter()).enumerate() {
                if chunk_size == 0 {
                    return Err(Hdf5Error::InvalidChunkSize(format!(
                        "Chunk size at dimension {} cannot be zero",
                        i
                    )));
                }
                if chunk_size > dim_size {
                    return Err(Hdf5Error::InvalidChunkSize(format!(
                        "Chunk size ({}) at dimension {} exceeds dataset size ({})",
                        chunk_size, i, dim_size
                    )));
                }
            }
        }
        Ok(())
    }
}

impl Default for DatasetProperties {
    fn default() -> Self {
        Self::new()
    }
}

/// HDF5 dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    /// Dataset name
    name: String,
    /// Full path from root
    path: String,
    /// Datatype
    datatype: Datatype,
    /// Dimensions (shape)
    dims: Vec<usize>,
    /// Dataset properties
    properties: DatasetProperties,
    /// Attributes
    attributes: Attributes,
    /// Raw data (for in-memory datasets)
    #[serde(skip)]
    data: Option<Vec<u8>>,
}

impl Dataset {
    /// Create a new dataset
    pub fn new(
        name: String,
        path: String,
        datatype: Datatype,
        dims: Vec<usize>,
        properties: DatasetProperties,
    ) -> Result<Self> {
        // Validate dimensions
        if dims.is_empty() {
            return Err(Hdf5Error::invalid_dimensions(
                "Dataset must have at least one dimension",
            ));
        }

        for (i, &dim) in dims.iter().enumerate() {
            if dim == 0 {
                return Err(Hdf5Error::invalid_dimensions(format!(
                    "Dimension {} cannot be zero",
                    i
                )));
            }
        }

        // Validate chunk dimensions
        properties.validate_chunks(&dims)?;

        Ok(Self {
            name,
            path,
            datatype,
            dims,
            properties,
            attributes: Attributes::new(),
            data: None,
        })
    }

    /// Create a dataset with default properties
    pub fn simple(
        name: String,
        path: String,
        datatype: Datatype,
        dims: Vec<usize>,
    ) -> Result<Self> {
        Self::new(name, path, datatype, dims, DatasetProperties::new())
    }

    /// Get the dataset name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the full path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the datatype
    pub fn datatype(&self) -> &Datatype {
        &self.datatype
    }

    /// Get the dimensions (shape)
    pub fn dims(&self) -> &[usize] {
        &self.dims
    }

    /// Get the number of dimensions
    pub fn ndims(&self) -> usize {
        self.dims.len()
    }

    /// Get the total number of elements
    pub fn len(&self) -> usize {
        self.dims.iter().product()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the total size in bytes
    pub fn size_in_bytes(&self) -> usize {
        self.len() * self.datatype.size()
    }

    /// Get the dataset properties
    pub fn properties(&self) -> &DatasetProperties {
        &self.properties
    }

    /// Get the attributes
    pub fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    /// Get mutable attributes
    pub fn attributes_mut(&mut self) -> &mut Attributes {
        &mut self.attributes
    }

    /// Set the raw data
    pub fn set_data(&mut self, data: Vec<u8>) -> Result<()> {
        let expected_size = self.size_in_bytes();
        if data.len() != expected_size {
            return Err(Hdf5Error::InvalidSize(format!(
                "Data size ({}) does not match expected size ({})",
                data.len(),
                expected_size
            )));
        }
        self.data = Some(data);
        Ok(())
    }

    /// Get the raw data
    pub fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }

    /// Take the raw data
    pub fn take_data(&mut self) -> Option<Vec<u8>> {
        self.data.take()
    }

    /// Validate slice parameters
    pub fn validate_slice(&self, start: &[usize], count: &[usize]) -> Result<()> {
        if start.len() != self.ndims() {
            return Err(Hdf5Error::invalid_dimensions(format!(
                "Start dimensions ({}) must match dataset dimensions ({})",
                start.len(),
                self.ndims()
            )));
        }

        if count.len() != self.ndims() {
            return Err(Hdf5Error::invalid_dimensions(format!(
                "Count dimensions ({}) must match dataset dimensions ({})",
                count.len(),
                self.ndims()
            )));
        }

        for (i, (&s, &c)) in start.iter().zip(count.iter()).enumerate() {
            if s + c > self.dims[i] {
                return Err(Hdf5Error::OutOfBounds {
                    index: s + c,
                    size: self.dims[i],
                });
            }
        }

        Ok(())
    }

    /// Calculate the number of elements in a slice
    pub fn slice_size(&self, count: &[usize]) -> usize {
        count.iter().product()
    }

    /// Calculate the size in bytes of a slice
    pub fn slice_size_bytes(&self, count: &[usize]) -> usize {
        self.slice_size(count) * self.datatype.size()
    }
}

/// Helper functions for creating datasets with common configurations
impl Dataset {
    /// Create a 1D dataset
    pub fn from_1d(name: String, path: String, datatype: Datatype, size: usize) -> Result<Self> {
        Self::simple(name, path, datatype, vec![size])
    }

    /// Create a 2D dataset
    pub fn from_2d(
        name: String,
        path: String,
        datatype: Datatype,
        rows: usize,
        cols: usize,
    ) -> Result<Self> {
        Self::simple(name, path, datatype, vec![rows, cols])
    }

    /// Create a 3D dataset
    pub fn from_3d(
        name: String,
        path: String,
        datatype: Datatype,
        depth: usize,
        rows: usize,
        cols: usize,
    ) -> Result<Self> {
        Self::simple(name, path, datatype, vec![depth, rows, cols])
    }

    /// Create a chunked dataset
    pub fn chunked(
        name: String,
        path: String,
        datatype: Datatype,
        dims: Vec<usize>,
        chunk_dims: Vec<usize>,
    ) -> Result<Self> {
        let properties = DatasetProperties::new().with_chunks(chunk_dims);
        Self::new(name, path, datatype, dims, properties)
    }

    /// Create a compressed dataset
    pub fn compressed(
        name: String,
        path: String,
        datatype: Datatype,
        dims: Vec<usize>,
        chunk_dims: Vec<usize>,
        compression: CompressionFilter,
    ) -> Result<Self> {
        let properties = DatasetProperties::new()
            .with_chunks(chunk_dims)
            .with_compression(compression);
        Self::new(name, path, datatype, dims, properties)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dataset_properties() {
        let props = DatasetProperties::new();
        assert_eq!(props.layout(), LayoutType::Contiguous);
        assert!(props.chunk_dims().is_none());
        assert_eq!(props.compression(), CompressionFilter::None);

        let props = DatasetProperties::new()
            .with_chunks(vec![10, 10])
            .with_gzip(6);
        assert_eq!(props.layout(), LayoutType::Chunked);
        assert_eq!(props.chunk_dims(), Some(&[10, 10][..]));
        assert_eq!(props.compression(), CompressionFilter::Gzip { level: 6 });
    }

    #[test]
    fn test_dataset_creation() {
        let dataset = Dataset::simple(
            "data".to_string(),
            "/data".to_string(),
            Datatype::Float32,
            vec![100, 200],
        )
        .expect("Failed to create dataset");

        assert_eq!(dataset.name(), "data");
        assert_eq!(dataset.path(), "/data");
        assert_eq!(dataset.datatype(), &Datatype::Float32);
        assert_eq!(dataset.dims(), &[100, 200]);
        assert_eq!(dataset.ndims(), 2);
        assert_eq!(dataset.len(), 20000);
        assert_eq!(dataset.size_in_bytes(), 80000); // 20000 * 4 bytes
    }

    #[test]
    fn test_dataset_1d() {
        let dataset = Dataset::from_1d(
            "data".to_string(),
            "/data".to_string(),
            Datatype::Int32,
            100,
        )
        .expect("Failed to create dataset");

        assert_eq!(dataset.dims(), &[100]);
        assert_eq!(dataset.len(), 100);
    }

    #[test]
    fn test_dataset_2d() {
        let dataset = Dataset::from_2d(
            "data".to_string(),
            "/data".to_string(),
            Datatype::Float64,
            50,
            100,
        )
        .expect("Failed to create dataset");

        assert_eq!(dataset.dims(), &[50, 100]);
        assert_eq!(dataset.len(), 5000);
    }

    #[test]
    fn test_dataset_3d() {
        let dataset = Dataset::from_3d(
            "data".to_string(),
            "/data".to_string(),
            Datatype::UInt8,
            10,
            20,
            30,
        )
        .expect("Failed to create dataset");

        assert_eq!(dataset.dims(), &[10, 20, 30]);
        assert_eq!(dataset.len(), 6000);
    }

    #[test]
    fn test_dataset_chunked() {
        let dataset = Dataset::chunked(
            "data".to_string(),
            "/data".to_string(),
            Datatype::Float32,
            vec![100, 200],
            vec![10, 20],
        )
        .expect("Failed to create dataset");

        assert_eq!(dataset.properties().layout(), LayoutType::Chunked);
        assert_eq!(dataset.properties().chunk_dims(), Some(&[10, 20][..]));
    }

    #[test]
    fn test_dataset_compressed() {
        let dataset = Dataset::compressed(
            "data".to_string(),
            "/data".to_string(),
            Datatype::Float64,
            vec![100, 200],
            vec![10, 20],
            CompressionFilter::Gzip { level: 6 },
        )
        .expect("Failed to create dataset");

        assert_eq!(dataset.properties().layout(), LayoutType::Chunked);
        assert_eq!(
            dataset.properties().compression(),
            CompressionFilter::Gzip { level: 6 }
        );
    }

    #[test]
    fn test_dataset_validate_slice() {
        let dataset = Dataset::from_2d(
            "data".to_string(),
            "/data".to_string(),
            Datatype::Int32,
            100,
            200,
        )
        .expect("Failed to create dataset");

        assert!(dataset.validate_slice(&[0, 0], &[50, 100]).is_ok());
        assert!(dataset.validate_slice(&[50, 100], &[50, 100]).is_ok());
        assert!(dataset.validate_slice(&[0, 0], &[100, 200]).is_ok());
        assert!(dataset.validate_slice(&[0, 0], &[101, 200]).is_err());
        assert!(dataset.validate_slice(&[50, 100], &[51, 100]).is_err());
    }

    #[test]
    fn test_dataset_slice_size() {
        let dataset = Dataset::from_2d(
            "data".to_string(),
            "/data".to_string(),
            Datatype::Int32,
            100,
            200,
        )
        .expect("Failed to create dataset");

        assert_eq!(dataset.slice_size(&[50, 100]), 5000);
        assert_eq!(dataset.slice_size_bytes(&[50, 100]), 20000); // 5000 * 4 bytes
    }

    #[test]
    fn test_dataset_set_data() {
        let mut dataset =
            Dataset::from_1d("data".to_string(), "/data".to_string(), Datatype::Int32, 10)
                .expect("Failed to create dataset");

        let data = vec![0u8; 40]; // 10 * 4 bytes
        assert!(dataset.set_data(data).is_ok());

        let wrong_size_data = vec![0u8; 50];
        assert!(dataset.set_data(wrong_size_data).is_err());
    }
}
