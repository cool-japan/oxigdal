//! Advanced chunking support for HDF5 datasets.
//!
//! This module provides comprehensive chunked dataset functionality including:
//! - Chunk indexing and lookup
//! - Partial chunk writes
//! - Optimal chunk size calculations
//! - Chunk allocation strategies

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Chunk index - represents the position of a chunk in the chunked dataset
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkIndex {
    /// Chunk coordinates (one per dimension)
    coords: Vec<usize>,
}

impl ChunkIndex {
    /// Create a new chunk index
    pub fn new(coords: Vec<usize>) -> Self {
        Self { coords }
    }

    /// Get the coordinates
    pub fn coords(&self) -> &[usize] {
        &self.coords
    }

    /// Calculate flat index for chunk (row-major order)
    pub fn flat_index(&self, chunk_grid: &ChunkGrid) -> usize {
        let mut index = 0;
        let mut multiplier = 1;

        for (i, &coord) in self.coords.iter().enumerate().rev() {
            index += coord * multiplier;
            multiplier *= chunk_grid.num_chunks[i];
        }

        index
    }

    /// Create chunk index from flat index
    pub fn from_flat_index(flat_index: usize, chunk_grid: &ChunkGrid) -> Self {
        let ndims = chunk_grid.ndims();
        let mut coords = vec![0; ndims];
        let mut idx = flat_index;

        for i in (0..ndims).rev() {
            coords[i] = idx % chunk_grid.num_chunks[i];
            idx /= chunk_grid.num_chunks[i];
        }

        Self { coords }
    }

    /// Calculate chunk index from dataset element coordinates
    pub fn from_element_coords(element_coords: &[usize], chunk_grid: &ChunkGrid) -> Result<Self> {
        if element_coords.len() != chunk_grid.ndims() {
            return Err(Hdf5Error::invalid_dimensions(format!(
                "Element coordinates ({}) must match dataset dimensions ({})",
                element_coords.len(),
                chunk_grid.ndims()
            )));
        }

        let coords: Vec<usize> = element_coords
            .iter()
            .zip(chunk_grid.chunk_dims.iter())
            .map(|(&elem, &chunk_size)| elem / chunk_size)
            .collect();

        Ok(Self { coords })
    }
}

/// Chunk grid - defines the chunking layout of a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkGrid {
    /// Dataset dimensions
    dataset_dims: Vec<usize>,
    /// Chunk dimensions
    chunk_dims: Vec<usize>,
    /// Number of chunks in each dimension
    num_chunks: Vec<usize>,
    /// Total number of chunks
    total_chunks: usize,
}

impl ChunkGrid {
    /// Create a new chunk grid
    pub fn new(dataset_dims: Vec<usize>, chunk_dims: Vec<usize>) -> Result<Self> {
        if dataset_dims.len() != chunk_dims.len() {
            return Err(Hdf5Error::invalid_dimensions(format!(
                "Dataset dimensions ({}) must match chunk dimensions ({})",
                dataset_dims.len(),
                chunk_dims.len()
            )));
        }

        for (i, (&chunk_size, &dim_size)) in
            chunk_dims.iter().zip(dataset_dims.iter()).enumerate()
        {
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

        let num_chunks: Vec<usize> = dataset_dims
            .iter()
            .zip(chunk_dims.iter())
            .map(|(&dim, &chunk)| (dim + chunk - 1) / chunk)
            .collect();

        let total_chunks = num_chunks.iter().product();

        Ok(Self {
            dataset_dims,
            chunk_dims,
            num_chunks,
            total_chunks,
        })
    }

    /// Get the number of dimensions
    pub fn ndims(&self) -> usize {
        self.dataset_dims.len()
    }

    /// Get the dataset dimensions
    pub fn dataset_dims(&self) -> &[usize] {
        &self.dataset_dims
    }

    /// Get the chunk dimensions
    pub fn chunk_dims(&self) -> &[usize] {
        &self.chunk_dims
    }

    /// Get the number of chunks in each dimension
    pub fn num_chunks_per_dim(&self) -> &[usize] {
        &self.num_chunks
    }

    /// Get the total number of chunks
    pub fn total_chunks(&self) -> usize {
        self.total_chunks
    }

    /// Calculate chunk size in bytes
    pub fn chunk_size_bytes(&self, datatype: &Datatype) -> usize {
        self.chunk_dims.iter().product::<usize>() * datatype.size()
    }

    /// Calculate actual chunk dimensions (may be smaller at edges)
    pub fn actual_chunk_dims(&self, chunk_index: &ChunkIndex) -> Vec<usize> {
        self.chunk_dims
            .iter()
            .zip(chunk_index.coords.iter())
            .zip(self.dataset_dims.iter())
            .map(|((&chunk_size, &chunk_coord), &dim_size)| {
                let start = chunk_coord * chunk_size;
                let end = (start + chunk_size).min(dim_size);
                end - start
            })
            .collect()
    }

    /// Check if a chunk index is valid
    pub fn is_valid_chunk(&self, chunk_index: &ChunkIndex) -> bool {
        if chunk_index.coords.len() != self.ndims() {
            return false;
        }

        chunk_index
            .coords
            .iter()
            .zip(self.num_chunks.iter())
            .all(|(&coord, &max)| coord < max)
    }

    /// Get all chunk indices that intersect with a hyperslab
    pub fn intersecting_chunks(
        &self,
        start: &[usize],
        count: &[usize],
    ) -> Result<Vec<ChunkIndex>> {
        if start.len() != self.ndims() || count.len() != self.ndims() {
            return Err(Hdf5Error::invalid_dimensions(
                "Start and count must match dataset dimensions",
            ));
        }

        // Calculate chunk ranges for each dimension
        let mut ranges: Vec<Vec<usize>> = Vec::new();

        for (i, (&s, &c)) in start.iter().zip(count.iter()).enumerate() {
            let end = s + c;
            let start_chunk = s / self.chunk_dims[i];
            let end_chunk = (end - 1) / self.chunk_dims[i];
            ranges.push((start_chunk..=end_chunk).collect());
        }

        // Generate all combinations of chunk indices
        let mut indices = Vec::new();
        self.generate_chunk_indices(&ranges, &mut vec![], 0, &mut indices);

        Ok(indices)
    }

    /// Helper function to generate all chunk index combinations
    fn generate_chunk_indices(
        &self,
        ranges: &[Vec<usize>],
        current: &mut Vec<usize>,
        dim: usize,
        result: &mut Vec<ChunkIndex>,
    ) {
        if dim == ranges.len() {
            result.push(ChunkIndex::new(current.clone()));
            return;
        }

        for &coord in &ranges[dim] {
            current.push(coord);
            self.generate_chunk_indices(ranges, current, dim + 1, result);
            current.pop();
        }
    }
}

/// Chunk allocation strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkAllocStrategy {
    /// Allocate all chunks upfront
    Early,
    /// Allocate chunks incrementally as needed
    Incremental,
    /// Allocate chunks in late stage (better for sparse datasets)
    Late,
}

/// Chunk metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Chunk index
    pub index: ChunkIndex,
    /// File offset where chunk data starts
    pub file_offset: u64,
    /// Size of chunk data in bytes (after compression/filters)
    pub size: usize,
    /// Whether chunk has been written
    pub is_written: bool,
    /// Filter mask (indicates which filters were applied)
    pub filter_mask: u32,
}

impl ChunkMetadata {
    /// Create new chunk metadata
    pub fn new(index: ChunkIndex, file_offset: u64, size: usize) -> Self {
        Self {
            index,
            file_offset,
            size,
            is_written: false,
            filter_mask: 0,
        }
    }

    /// Mark chunk as written
    pub fn mark_written(&mut self) {
        self.is_written = true;
    }

    /// Set filter mask
    pub fn set_filter_mask(&mut self, mask: u32) {
        self.filter_mask = mask;
    }
}

/// Chunk index structure for fast lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkIndexStructure {
    /// Chunk grid
    grid: ChunkGrid,
    /// Chunk metadata indexed by flat chunk index
    chunks: HashMap<usize, ChunkMetadata>,
    /// Allocation strategy
    alloc_strategy: ChunkAllocStrategy,
}

impl ChunkIndexStructure {
    /// Create a new chunk index structure
    pub fn new(grid: ChunkGrid, alloc_strategy: ChunkAllocStrategy) -> Self {
        Self {
            grid,
            chunks: HashMap::new(),
            alloc_strategy,
        }
    }

    /// Get the chunk grid
    pub fn grid(&self) -> &ChunkGrid {
        &self.grid
    }

    /// Get chunk metadata by chunk index
    pub fn get_chunk(&self, chunk_index: &ChunkIndex) -> Option<&ChunkMetadata> {
        let flat_idx = chunk_index.flat_index(&self.grid);
        self.chunks.get(&flat_idx)
    }

    /// Get mutable chunk metadata by chunk index
    pub fn get_chunk_mut(&mut self, chunk_index: &ChunkIndex) -> Option<&mut ChunkMetadata> {
        let flat_idx = chunk_index.flat_index(&self.grid);
        self.chunks.get_mut(&flat_idx)
    }

    /// Insert chunk metadata
    pub fn insert_chunk(&mut self, metadata: ChunkMetadata) {
        let flat_idx = metadata.index.flat_index(&self.grid);
        self.chunks.insert(flat_idx, metadata);
    }

    /// Check if chunk exists
    pub fn has_chunk(&self, chunk_index: &ChunkIndex) -> bool {
        let flat_idx = chunk_index.flat_index(&self.grid);
        self.chunks.contains_key(&flat_idx)
    }

    /// Get all written chunks
    pub fn written_chunks(&self) -> Vec<&ChunkMetadata> {
        self.chunks
            .values()
            .filter(|c| c.is_written)
            .collect()
    }

    /// Get number of written chunks
    pub fn num_written_chunks(&self) -> usize {
        self.chunks.values().filter(|c| c.is_written).count()
    }

    /// Get allocation strategy
    pub fn alloc_strategy(&self) -> ChunkAllocStrategy {
        self.alloc_strategy
    }
}

/// Calculate optimal chunk size for a dataset
pub fn calculate_optimal_chunk_size(
    dataset_dims: &[usize],
    datatype: &Datatype,
    target_chunk_bytes: usize,
) -> Result<Vec<usize>> {
    if dataset_dims.is_empty() {
        return Err(Hdf5Error::invalid_dimensions(
            "Dataset must have at least one dimension",
        ));
    }

    let element_size = datatype.size();
    let target_elements = target_chunk_bytes / element_size;

    if target_elements == 0 {
        return Err(Hdf5Error::InvalidChunkSize(
            "Target chunk size is too small".to_string(),
        ));
    }

    // Start with equal division across dimensions
    let ndims = dataset_dims.len();
    let elements_per_dim = (target_elements as f64).powf(1.0 / ndims as f64) as usize;

    let mut chunk_dims: Vec<usize> = dataset_dims
        .iter()
        .map(|&dim| elements_per_dim.min(dim).max(1))
        .collect();

    // Adjust to get closer to target
    let mut current_size: usize = chunk_dims.iter().product();

    if current_size < target_elements {
        // Increase dimensions starting from the last (fastest varying)
        for i in (0..ndims).rev() {
            let factor = target_elements / current_size;
            let new_size = (chunk_dims[i] * factor).min(dataset_dims[i]);
            if new_size > chunk_dims[i] {
                chunk_dims[i] = new_size;
                current_size = chunk_dims.iter().product();
                if current_size >= target_elements {
                    break;
                }
            }
        }
    } else if current_size > target_elements {
        // Decrease dimensions starting from the first
        for i in 0..ndims {
            let factor = current_size / target_elements;
            let new_size = (chunk_dims[i] / factor).max(1);
            if new_size < chunk_dims[i] {
                chunk_dims[i] = new_size;
                current_size = chunk_dims.iter().product();
                if current_size <= target_elements {
                    break;
                }
            }
        }
    }

    Ok(chunk_dims)
}

/// Chunk size recommendation based on access pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPattern {
    /// Sequential read/write along fastest dimension
    Sequential,
    /// Random access to small regions
    Random,
    /// Access entire dataset at once
    FullDataset,
    /// Access slices along specific dimensions
    Sliced { dim: usize },
}

/// Get recommended chunk size based on access pattern
pub fn recommend_chunk_size(
    dataset_dims: &[usize],
    datatype: &Datatype,
    access_pattern: AccessPattern,
) -> Result<Vec<usize>> {
    let ndims = dataset_dims.len();

    match access_pattern {
        AccessPattern::Sequential => {
            // Favor larger chunks in the fastest dimension (last)
            let mut chunk_dims = vec![1; ndims];
            chunk_dims[ndims - 1] = dataset_dims[ndims - 1].min(8192);
            Ok(chunk_dims)
        }
        AccessPattern::Random => {
            // Use smaller, more balanced chunks
            calculate_optimal_chunk_size(dataset_dims, datatype, 64 * 1024)
        }
        AccessPattern::FullDataset => {
            // No chunking needed, but HDF5 requires it for filters
            calculate_optimal_chunk_size(dataset_dims, datatype, 1024 * 1024)
        }
        AccessPattern::Sliced { dim } => {
            if dim >= ndims {
                return Err(Hdf5Error::invalid_dimensions(format!(
                    "Slice dimension {} exceeds dataset dimensions {}",
                    dim, ndims
                )));
            }
            // Make chunks that align with slice dimension
            let mut chunk_dims = vec![1; ndims];
            chunk_dims[dim] = dataset_dims[dim].min(1024);
            for i in 0..ndims {
                if i != dim {
                    chunk_dims[i] = dataset_dims[i].min(16);
                }
            }
            Ok(chunk_dims)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_index_creation() {
        let index = ChunkIndex::new(vec![1, 2, 3]);
        assert_eq!(index.coords(), &[1, 2, 3]);
    }

    #[test]
    fn test_chunk_index_flat_index() {
        let grid = ChunkGrid::new(vec![100, 100, 100], vec![10, 10, 10])
            .expect("Failed to create chunk grid");
        let index = ChunkIndex::new(vec![2, 3, 4]);
        let flat = index.flat_index(&grid);
        assert_eq!(flat, 2 * 100 + 3 * 10 + 4);
    }

    #[test]
    fn test_chunk_index_from_flat_index() {
        let grid = ChunkGrid::new(vec![100, 100, 100], vec![10, 10, 10])
            .expect("Failed to create chunk grid");
        let flat = 234;
        let index = ChunkIndex::from_flat_index(flat, &grid);
        assert_eq!(index.flat_index(&grid), flat);
    }

    #[test]
    fn test_chunk_index_from_element_coords() {
        let grid = ChunkGrid::new(vec![100, 100], vec![10, 10])
            .expect("Failed to create chunk grid");
        let index = ChunkIndex::from_element_coords(&[25, 37], &grid)
            .expect("Failed to create chunk index");
        assert_eq!(index.coords(), &[2, 3]);
    }

    #[test]
    fn test_chunk_grid_creation() {
        let grid =
            ChunkGrid::new(vec![100, 200, 300], vec![10, 20, 30]).expect("Failed to create grid");
        assert_eq!(grid.ndims(), 3);
        assert_eq!(grid.dataset_dims(), &[100, 200, 300]);
        assert_eq!(grid.chunk_dims(), &[10, 20, 30]);
        assert_eq!(grid.num_chunks_per_dim(), &[10, 10, 10]);
        assert_eq!(grid.total_chunks(), 1000);
    }

    #[test]
    fn test_chunk_grid_actual_dims() {
        let grid = ChunkGrid::new(vec![25, 25], vec![10, 10]).expect("Failed to create grid");

        let index = ChunkIndex::new(vec![0, 0]);
        assert_eq!(grid.actual_chunk_dims(&index), vec![10, 10]);

        let index = ChunkIndex::new(vec![2, 2]);
        assert_eq!(grid.actual_chunk_dims(&index), vec![5, 5]);
    }

    #[test]
    fn test_chunk_grid_intersecting_chunks() {
        let grid = ChunkGrid::new(vec![100, 100], vec![10, 10]).expect("Failed to create grid");

        let chunks = grid
            .intersecting_chunks(&[5, 5], &[10, 10])
            .expect("Failed to get intersecting chunks");
        assert_eq!(chunks.len(), 4);

        let chunks = grid
            .intersecting_chunks(&[0, 0], &[5, 5])
            .expect("Failed to get intersecting chunks");
        assert_eq!(chunks.len(), 1);

        let chunks = grid
            .intersecting_chunks(&[5, 5], &[20, 20])
            .expect("Failed to get intersecting chunks");
        assert_eq!(chunks.len(), 9);
    }

    #[test]
    fn test_chunk_index_structure() {
        let grid = ChunkGrid::new(vec![100, 100], vec![10, 10]).expect("Failed to create grid");
        let mut index_struct = ChunkIndexStructure::new(grid, ChunkAllocStrategy::Incremental);

        let chunk_idx = ChunkIndex::new(vec![1, 2]);
        let metadata = ChunkMetadata::new(chunk_idx.clone(), 1024, 400);
        index_struct.insert_chunk(metadata);

        assert!(index_struct.has_chunk(&chunk_idx));
        assert_eq!(index_struct.num_written_chunks(), 0);

        index_struct.get_chunk_mut(&chunk_idx).map(|m| m.mark_written());
        assert_eq!(index_struct.num_written_chunks(), 1);
    }

    #[test]
    fn test_calculate_optimal_chunk_size() {
        let dims = vec![1000, 2000, 3000];
        let datatype = Datatype::Float32;
        let target_bytes = 1024 * 1024; // 1 MB

        let chunk_dims = calculate_optimal_chunk_size(&dims, &datatype, target_bytes)
            .expect("Failed to calculate optimal chunk size");

        let chunk_size_bytes: usize = chunk_dims.iter().product::<usize>() * datatype.size();
        assert!(chunk_size_bytes <= target_bytes * 2);
        assert!(chunk_size_bytes >= target_bytes / 2);
    }

    #[test]
    fn test_recommend_chunk_size_sequential() {
        let dims = vec![100, 200];
        let datatype = Datatype::Float64;

        let chunks = recommend_chunk_size(&dims, &datatype, AccessPattern::Sequential)
            .expect("Failed to recommend chunk size");
        assert_eq!(chunks[0], 1);
        assert!(chunks[1] > 1);
    }

    #[test]
    fn test_recommend_chunk_size_random() {
        let dims = vec![1000, 2000];
        let datatype = Datatype::Int32;

        let chunks = recommend_chunk_size(&dims, &datatype, AccessPattern::Random)
            .expect("Failed to recommend chunk size");
        assert!(chunks[0] > 1 && chunks[0] < 1000);
        assert!(chunks[1] > 1 && chunks[1] < 2000);
    }
}
