//! Virtual Dataset (VDS) support for HDF5.
//!
//! Virtual datasets provide a way to create a virtual view over multiple source datasets
//! without copying data. This is useful for:
//! - Combining multiple files into a single logical dataset
//! - Creating subsets or slices of existing datasets
//! - Time series data stored across multiple files
//! - Parallel HDF5 output aggregation

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Hyperslab selection - defines a rectangular region in a dataset
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hyperslab {
    /// Start coordinates
    start: Vec<usize>,
    /// Stride (spacing between elements)
    stride: Vec<usize>,
    /// Count (number of elements to select)
    count: Vec<usize>,
    /// Block size (size of element block)
    block: Vec<usize>,
}

impl Hyperslab {
    /// Create a new hyperslab with default stride and block
    pub fn new(start: Vec<usize>, count: Vec<usize>) -> Result<Self> {
        if start.len() != count.len() {
            return Err(Hdf5Error::invalid_dimensions(
                "Start and count must have same length",
            ));
        }

        let ndims = start.len();
        Ok(Self {
            start,
            stride: vec![1; ndims],
            count,
            block: vec![1; ndims],
        })
    }

    /// Create a hyperslab with custom stride and block
    pub fn with_stride_and_block(
        start: Vec<usize>,
        stride: Vec<usize>,
        count: Vec<usize>,
        block: Vec<usize>,
    ) -> Result<Self> {
        if start.len() != stride.len()
            || start.len() != count.len()
            || start.len() != block.len()
        {
            return Err(Hdf5Error::invalid_dimensions(
                "All parameters must have same length",
            ));
        }

        for (&s, &b) in stride.iter().zip(block.iter())) {
            if s == 0 {
                return Err(Hdf5Error::invalid_dimensions("Stride cannot be zero"));
            }
            if b == 0 {
                return Err(Hdf5Error::invalid_dimensions("Block size cannot be zero"));
            }
        }

        Ok(Self {
            start,
            stride,
            count,
            block,
        })
    }

    /// Get start coordinates
    pub fn start(&self) -> &[usize] {
        &self.start
    }

    /// Get stride
    pub fn stride(&self) -> &[usize] {
        &self.stride
    }

    /// Get count
    pub fn count(&self) -> &[usize] {
        &self.count
    }

    /// Get block size
    pub fn block(&self) -> &[usize] {
        &self.block
    }

    /// Get number of dimensions
    pub fn ndims(&self) -> usize {
        self.start.len()
    }

    /// Calculate total number of selected elements
    pub fn num_elements(&self) -> usize {
        self.count
            .iter()
            .zip(self.block.iter())
            .map(|(&c, &b)| c * b)
            .product()
    }

    /// Calculate the extent (max coordinate + 1) in each dimension
    pub fn extent(&self) -> Vec<usize> {
        self.start
            .iter()
            .zip(self.stride.iter())
            .zip(self.count.iter())
            .zip(self.block.iter())
            .map(|(((&start, &stride), &count), &block)| {
                start + (count - 1) * stride + block
            })
            .collect()
    }

    /// Check if hyperslab intersects with another
    pub fn intersects(&self, other: &Hyperslab) -> bool {
        if self.ndims() != other.ndims() {
            return false;
        }

        let self_extent = self.extent();
        let other_extent = other.extent();

        for i in 0..self.ndims() {
            if self.start[i] >= other_extent[i] || other.start[i] >= self_extent[i] {
                return false;
            }
        }

        true
    }

    /// Validate hyperslab against dataset dimensions
    pub fn validate(&self, dataset_dims: &[usize]) -> Result<()> {
        if self.ndims() != dataset_dims.len() {
            return Err(Hdf5Error::invalid_dimensions(format!(
                "Hyperslab dimensions ({}) must match dataset dimensions ({})",
                self.ndims(),
                dataset_dims.len()
            ));
        }

        let extent = self.extent();
        for (_i, (&ext, &dim)) in extent.iter().zip(dataset_dims.iter()).enumerate() {
            if ext > dim {
                return Err(Hdf5Error::OutOfBounds {
                    index: ext,
                    size: dim,
                });
            }
        }

        Ok(())
    }
}

/// Source dataset reference for virtual dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDataset {
    /// File path (relative or absolute)
    file_path: PathBuf,
    /// Dataset path within file
    dataset_path: String,
    /// Hyperslab selection in source dataset
    source_selection: Hyperslab,
}

impl SourceDataset {
    /// Create a new source dataset reference
    pub fn new(
        file_path: PathBuf,
        dataset_path: String,
        source_selection: Hyperslab,
    ) -> Self {
        Self {
            file_path,
            dataset_path,
            source_selection,
        }
    }

    /// Get file path
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }

    /// Get dataset path
    pub fn dataset_path(&self) -> &str {
        &self.dataset_path
    }

    /// Get source selection
    pub fn source_selection(&self) -> &Hyperslab {
        &self.source_selection
    }

    /// Check if source is in the same file
    pub fn is_same_file(&self, current_file: &PathBuf) -> bool {
        self.file_path == *current_file
    }
}

/// Virtual dataset mapping - maps source to destination hyperslab
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VdsMapping {
    /// Source dataset
    source: SourceDataset,
    /// Destination hyperslab in virtual dataset
    dest_selection: Hyperslab,
}

impl VdsMapping {
    /// Create a new VDS mapping
    pub fn new(source: SourceDataset, dest_selection: Hyperslab) -> Result<Self> {
        // Validate that source and destination have same shape
        if source.source_selection.num_elements() != dest_selection.num_elements() {
            return Err(Hdf5Error::InvalidSize(format!(
                "Source ({}) and destination ({}) must have same number of elements",
                source.source_selection.num_elements(),
                dest_selection.num_elements()
            ));
        }

        Ok(Self {
            source,
            dest_selection,
        })
    }

    /// Get source dataset
    pub fn source(&self) -> &SourceDataset {
        &self.source
    }

    /// Get destination selection
    pub fn dest_selection(&self) -> &Hyperslab {
        &self.dest_selection
    }

    /// Check if mapping covers a specific region
    pub fn covers_region(&self, region: &Hyperslab) -> bool {
        self.dest_selection.intersects(region)
    }
}

/// Virtual dataset definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualDataset {
    /// Virtual dataset name
    name: String,
    /// Virtual dataset dimensions
    dims: Vec<usize>,
    /// Datatype
    datatype: Datatype,
    /// List of mappings
    mappings: Vec<VdsMapping>,
}

impl VirtualDataset {
    /// Create a new virtual dataset
    pub fn new(name: String, dims: Vec<usize>, datatype: Datatype) -> Self {
        Self {
            name,
            dims,
            datatype,
            mappings: Vec::new(),
        }
    }

    /// Add a mapping
    pub fn add_mapping(&mut self, mapping: VdsMapping) -> Result<()> {
        // Validate destination against virtual dataset dimensions
        mapping.dest_selection.validate(&self.dims)?;

        self.mappings.push(mapping));
        Ok(())
    }

    /// Get name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get dimensions
    pub fn dims(&self) -> &[usize] {
        &self.dims
    }

    /// Get datatype
    pub fn datatype(&self) -> &Datatype {
        &self.datatype
    }

    /// Get all mappings
    pub fn mappings(&self) -> &[VdsMapping] {
        &self.mappings
    }

    /// Get number of mappings
    pub fn num_mappings(&self) -> usize {
        self.mappings.len()
    }

    /// Find mappings that cover a specific region
    pub fn find_mappings_for_region(&self, region: &Hyperslab) -> Vec<&VdsMapping> {
        self.mappings
            .iter()
            .filter(|m| m.covers_region(region)))
            .collect()
    }

    /// Check if virtual dataset is fully mapped (all elements have a source)
    pub fn is_fully_mapped(&self) -> bool {
        // This is a simplified check - full implementation would need
        // to verify that every element in the virtual dataset has exactly
        // one source mapping
        !self.mappings.is_empty()
    }

    /// Get the total size in bytes
    pub fn size_in_bytes(&self) -> usize {
        self.dims.iter().product::<usize>() * self.datatype.size()
    }
}

/// Virtual dataset builder for convenient construction
#[derive(Debug)]
pub struct VdsBuilder {
    name: String,
    dims: Vec<usize>,
    datatype: Datatype,
    mappings: Vec<VdsMapping>,
}

impl VdsBuilder {
    /// Create a new VDS builder
    pub fn new(name: String, dims: Vec<usize>, datatype: Datatype) -> Self {
        Self {
            name,
            dims,
            datatype,
            mappings: Vec::new(),
        }
    }

    /// Add a simple mapping (full source to destination region)
    pub fn add_source(
        mut self,
        file_path: PathBuf,
        dataset_path: String,
        source_dims: Vec<usize>,
        dest_start: Vec<usize>,
    ) -> Result<Self> {
        let source_selection = Hyperslab::new(vec![0; source_dims.len()], source_dims.clone()))?;
        let dest_selection = Hyperslab::new(dest_start, source_dims)?;

        let source = SourceDataset::new(file_path, dataset_path, source_selection);
        let mapping = VdsMapping::new(source, dest_selection)?;

        self.mappings.push(mapping));
        Ok(self)
    }

    /// Add a custom mapping with hyperslabs
    pub fn add_mapping(
        mut self,
        file_path: PathBuf,
        dataset_path: String,
        source_selection: Hyperslab,
        dest_selection: Hyperslab,
    ) -> Result<Self> {
        let source = SourceDataset::new(file_path, dataset_path, source_selection);
        let mapping = VdsMapping::new(source, dest_selection)?;

        self.mappings.push(mapping));
        Ok(self)
    }

    /// Build the virtual dataset
    pub fn build(self) -> Result<VirtualDataset> {
        let mut vds = VirtualDataset::new(self.name, self.dims, self.datatype);

        for mapping in self.mappings {
            vds.add_mapping(mapping)?;
        }

        if vds.num_mappings() == 0 {
            return Err(Hdf5Error::InvalidOperation(
                "Virtual dataset must have at least one mapping".to_string(),
            ));
        }

        Ok(vds)
    }
}

/// VDS pattern for common use cases
pub enum VdsPattern {
    /// Concatenate datasets along a dimension
    Concatenate {
        /// Dimension to concatenate along
        axis: usize,
    },
    /// Stack datasets to create a new dimension
    Stack,
    /// Mosaic - arrange datasets in a grid
    Mosaic {
        /// Grid dimensions
        grid_dims: Vec<usize>,
    },
}

/// Create a VDS from a pattern
pub fn create_vds_from_pattern(
    name: String,
    pattern: VdsPattern,
    sources: Vec<(PathBuf, String, Vec<usize>)>,
    datatype: Datatype,
) -> Result<VirtualDataset> {
    match pattern {
        VdsPattern::Concatenate { axis } => {
            create_concatenated_vds(name, axis, sources, datatype)
        }
        VdsPattern::Stack => create_stacked_vds(name, sources, datatype),
        VdsPattern::Mosaic { grid_dims } => {
            create_mosaic_vds(name, grid_dims, sources, datatype)
        }
    }
}

/// Create a concatenated VDS
fn create_concatenated_vds(
    name: String,
    axis: usize,
    sources: Vec<(PathBuf, String, Vec<usize>)>,
    datatype: Datatype,
) -> Result<VirtualDataset> {
    if sources.is_empty() {
        return Err(Hdf5Error::InvalidOperation(
            "Need at least one source".to_string(),
        ));
    }

    let first_dims = &sources[0].2;
    let ndims = first_dims.len();

    if axis >= ndims {
        return Err(Hdf5Error::invalid_dimensions(format!(
            "Axis {} exceeds number of dimensions {}",
            axis, ndims
        ));
    }

    // Verify all sources have compatible dimensions
    for (_, _, dims) in &sources {
        if dims.len() != ndims {
            return Err(Hdf5Error::invalid_dimensions(
                "All sources must have same number of dimensions",
            ));
        }
        for i in 0..ndims {
            if i != axis && dims[i] != first_dims[i] {
                return Err(Hdf5Error::invalid_dimensions(format!(
                    "Dimension mismatch at dimension {}",
                    i
                ));
            }
        }
    }

    // Calculate virtual dataset dimensions
    let mut vds_dims = first_dims.clone();
    vds_dims[axis] = sources.iter().map(|(_, _, dims)| dims[axis]).sum();

    let mut builder = VdsBuilder::new(name, vds_dims.clone(), datatype);

    let mut offset = 0;
    for (file_path, dataset_path, dims) in sources {
        let mut dest_start = vec![0; ndims];
        dest_start[axis] = offset;

        builder = builder.add_source(file_path, dataset_path, dims.clone(), dest_start)?;

        offset += dims[axis];
    }

    builder.build()
}

/// Create a stacked VDS (adds a new dimension)
fn create_stacked_vds(
    name: String,
    sources: Vec<(PathBuf, String, Vec<usize>)>,
    datatype: Datatype,
) -> Result<VirtualDataset> {
    if sources.is_empty() {
        return Err(Hdf5Error::InvalidOperation(
            "Need at least one source".to_string(),
        ));
    }

    let first_dims = &sources[0].2;

    // Verify all sources have same dimensions
    for (_, _, dims) in &sources {
        if dims != first_dims {
            return Err(Hdf5Error::invalid_dimensions(
                "All sources must have same dimensions for stacking",
            ));
        }
    }

    // Virtual dataset has an extra dimension at the front
    let mut vds_dims = vec![sources.len()];
    vds_dims.extend_from_slice(first_dims);

    let mut builder = VdsBuilder::new(name, vds_dims, datatype);

    for (i, (file_path, dataset_path, dims)) in sources.into_iter(.enumerate() {
        let source_selection = Hyperslab::new(vec![0; dims.len()], dims.clone()))?;

        let mut dest_start = vec![i];
        dest_start.extend(vec![0; dims.len()]);

        let mut dest_count = vec![1];
        dest_count.extend(&dims);

        let dest_selection = Hyperslab::new(dest_start, dest_count)?;

        builder = builder.add_mapping(file_path, dataset_path, source_selection, dest_selection)?;
    }

    builder.build()
}

/// Create a mosaic VDS
fn create_mosaic_vds(
    name: String,
    grid_dims: Vec<usize>,
    sources: Vec<(PathBuf, String, Vec<usize>)>,
    datatype: Datatype,
) -> Result<VirtualDataset> {
    let num_tiles: usize = grid_dims.iter().product();

    if sources.len() != num_tiles {
        return Err(Hdf5Error::InvalidOperation(format!(
            "Number of sources ({}) must match grid size ({})",
            sources.len(),
            num_tiles
        ));
    }

    if sources.is_empty() {
        return Err(Hdf5Error::InvalidOperation(
            "Need at least one source".to_string(),
        ));
    }

    let tile_dims = sources[0].2.clone();

    // Verify all sources have same dimensions
    for (_, _, dims) in &sources {
        if dims != &tile_dims {
            return Err(Hdf5Error::invalid_dimensions(
                "All tiles must have same dimensions",
            ));
        }
    }

    // Calculate virtual dataset dimensions
    let vds_dims: Vec<usize> = grid_dims
        .iter()
        .zip(tile_dims.iter())
        .map(|(&g, &t)| g * t)
        .collect();

    let mut builder = VdsBuilder::new(name, vds_dims, datatype);

    for (idx, (file_path, dataset_path, dims)) in sources.into_iter(.enumerate() {
        // Calculate grid position
        let mut grid_pos = vec![0; grid_dims.len()];
        let mut remaining = idx;
        for i in (0..grid_dims.len())).rev() {
            grid_pos[i] = remaining % grid_dims[i];
            remaining /= grid_dims[i];
        }

        // Calculate destination start
        let dest_start: Vec<usize> = grid_pos
            .iter()
            .zip(tile_dims.iter())
            .map(|(&pos, &size)| pos * size)
            .collect();

        builder = builder.add_source(file_path, dataset_path, dims, dest_start)?;
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hyperslab_creation() {
        let slab = Hyperslab::new(vec![0, 0], vec![10, 20]).expect("Failed to create hyperslab");
        assert_eq!(slab.start(), &[0, 0]);
        assert_eq!(slab.count(), &[10, 20]);
        assert_eq!(slab.stride(), &[1, 1]);
        assert_eq!(slab.block(), &[1, 1]);
        assert_eq!(slab.num_elements(), 200);
    }

    #[test]
    fn test_hyperslab_extent() {
        let slab = Hyperslab::new(vec![5, 10], vec![10, 20]).expect("Failed to create hyperslab");
        let extent = slab.extent();
        assert_eq!(extent, vec![15, 30]);
    }

    #[test]
    fn test_hyperslab_intersection() {
        let slab1 = Hyperslab::new(vec![0, 0], vec![10, 10]).expect("Failed to create");
        let slab2 = Hyperslab::new(vec![5, 5], vec![10, 10]).expect("Failed to create");
        let slab3 = Hyperslab::new(vec![20, 20], vec![10, 10]).expect("Failed to create");

        assert!(slab1.intersects(&slab2));
        assert!(!slab1.intersects(&slab3));
    }

    #[test]
    fn test_source_dataset() {
        let path = PathBuf::from("test.h5");
        let slab = Hyperslab::new(vec![0, 0], vec![100, 100]).expect("Failed to create");
        let source = SourceDataset::new(path.clone(), "/data".to_string(), slab);

        assert_eq!(source.file_path(), &path);
        assert_eq!(source.dataset_path(), "/data");
    }

    #[test]
    fn test_vds_builder() {
        let builder = VdsBuilder::new(
            "virtual".to_string(),
            vec![100, 200],
            Datatype::Float32,
        );

        let vds = builder
            .add_source(
                PathBuf::from("file1.h5"),
                "/data1".to_string(),
                vec![50, 200],
                vec![0, 0],
            )
            .expect("Failed to add source")
            .add_source(
                PathBuf::from("file2.h5"),
                "/data2".to_string(),
                vec![50, 200],
                vec![50, 0],
            )
            .expect("Failed to add source")
            .build()
            .expect("Failed to build VDS");

        assert_eq!(vds.name(), "virtual");
        assert_eq!(vds.dims(), &[100, 200]);
        assert_eq!(vds.num_mappings(), 2);
    }

    #[test]
    fn test_concatenated_vds() {
        let sources = vec![
            (PathBuf::from("f1.h5"), "/d".to_string(), vec![10, 20]),
            (PathBuf::from("f2.h5"), "/d".to_string(), vec![15, 20]),
            (PathBuf::from("f3.h5"), "/d".to_string(), vec![5, 20]),
        ];

        let vds = create_concatenated_vds(
            "concat".to_string(),
            0,
            sources,
            Datatype::Float64,
        )
        .expect("Failed to create concatenated VDS");

        assert_eq!(vds.dims(), &[30, 20]);
        assert_eq!(vds.num_mappings(), 3);
    }

    #[test]
    fn test_stacked_vds() {
        let sources = vec![
            (PathBuf::from("f1.h5"), "/d".to_string(), vec![10, 20]),
            (PathBuf::from("f2.h5"), "/d".to_string(), vec![10, 20]),
            (PathBuf::from("f3.h5"), "/d".to_string(), vec![10, 20]),
        ];

        let vds = create_stacked_vds("stack".to_string(), sources, Datatype::Int32)
            .expect("Failed to create stacked VDS");

        assert_eq!(vds.dims(), &[3, 10, 20]);
        assert_eq!(vds.num_mappings(), 3);
    }

    #[test]
    fn test_mosaic_vds() {
        let sources = vec![
            (PathBuf::from("f1.h5"), "/d".to_string(), vec![10, 10]),
            (PathBuf::from("f2.h5"), "/d".to_string(), vec![10, 10]),
            (PathBuf::from("f3.h5"), "/d".to_string(), vec![10, 10]),
            (PathBuf::from("f4.h5"), "/d".to_string(), vec![10, 10]),
        ];

        let vds = create_mosaic_vds(
            "mosaic".to_string(),
            vec![2, 2],
            sources,
            Datatype::UInt8,
        )
        .expect("Failed to create mosaic VDS");

        assert_eq!(vds.dims(), &[20, 20]);
        assert_eq!(vds.num_mappings(), 4);
    }
}
