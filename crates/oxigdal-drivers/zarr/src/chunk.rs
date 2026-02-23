//! Chunk coordinate and grid utilities for Zarr arrays
//!
//! This module provides types and functions for working with chunked array storage,
//! including chunk coordinate calculation, chunk grid iteration, and chunk indexing.

use crate::dimension::{DimensionSeparator, Shape};
use crate::error::{ChunkError, Result, ZarrError};
use serde::{Deserialize, Serialize};

/// Chunk coordinates - position of a chunk in the chunk grid
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkCoord(Vec<usize>);

impl ChunkCoord {
    /// Creates new chunk coordinates
    ///
    /// # Errors
    /// Returns error if coordinates are empty
    pub fn new(coords: Vec<usize>) -> Result<Self> {
        if coords.is_empty() {
            return Err(ZarrError::Chunk(ChunkError::InvalidCoordinates {
                coords,
                shape: vec![],
            }));
        }
        Ok(Self(coords))
    }

    /// Creates chunk coordinates without validation
    #[must_use]
    pub fn new_unchecked(coords: Vec<usize>) -> Self {
        Self(coords)
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.0.len()
    }

    /// Returns coordinates as a slice
    #[must_use]
    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }

    /// Returns coordinates as a vector
    #[must_use]
    pub fn to_vec(&self) -> Vec<usize> {
        self.0.clone()
    }

    /// Formats chunk coordinates as a key string
    ///
    /// # Arguments
    /// * `separator` - Character to use between coordinates
    ///
    /// # Examples
    /// ```ignore
    /// let coord = ChunkCoord::new(vec![0, 1, 2]).unwrap();
    /// assert_eq!(coord.to_key('.'), "0.1.2");
    /// assert_eq!(coord.to_key('/'), "0/1/2");
    /// ```
    #[must_use]
    pub fn to_key(&self, separator: DimensionSeparator) -> String {
        self.0
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(&separator.as_char().to_string())
    }

    /// Parses chunk coordinates from a key string
    ///
    /// # Errors
    /// Returns error if parsing fails
    pub fn from_key(key: &str, separator: DimensionSeparator) -> Result<Self> {
        let parts: std::result::Result<Vec<usize>, _> =
            key.split(separator.as_char()).map(str::parse).collect();

        match parts {
            Ok(coords) => Self::new(coords),
            Err(e) => Err(ZarrError::Chunk(ChunkError::DecodeError {
                message: format!("Failed to parse chunk key '{key}': {e}"),
            })),
        }
    }

    /// Checks if these coordinates are valid for a chunk grid
    #[must_use]
    pub fn is_valid_for_grid(&self, grid: &ChunkGrid) -> bool {
        if self.ndim() != grid.ndim() {
            return false;
        }

        self.0
            .iter()
            .zip(grid.chunk_count.as_slice())
            .all(|(c, count)| c < count)
    }
}

impl From<Vec<usize>> for ChunkCoord {
    fn from(coords: Vec<usize>) -> Self {
        Self::new_unchecked(coords)
    }
}

impl AsRef<[usize]> for ChunkCoord {
    fn as_ref(&self) -> &[usize] {
        &self.0
    }
}

impl core::ops::Index<usize> for ChunkCoord {
    type Output = usize;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

/// Chunk grid - defines how an array is divided into chunks
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkGrid {
    /// Shape of the full array
    pub array_shape: Shape,
    /// Shape of each chunk
    pub chunk_shape: Shape,
    /// Number of chunks in each dimension
    pub chunk_count: Shape,
}

impl ChunkGrid {
    /// Creates a new chunk grid
    ///
    /// # Errors
    /// Returns error if shapes are incompatible
    pub fn new(array_shape: Shape, chunk_shape: Shape) -> Result<Self> {
        if array_shape.ndim() != chunk_shape.ndim() {
            return Err(ZarrError::Chunk(ChunkError::InvalidChunkShape {
                chunk_shape: chunk_shape.to_vec(),
                array_shape: array_shape.to_vec(),
            }));
        }

        // Calculate number of chunks in each dimension
        let chunk_count = Shape::new_unchecked(
            array_shape
                .as_slice()
                .iter()
                .zip(chunk_shape.as_slice())
                .map(|(&arr_size, &chunk_size)| {
                    if chunk_size == 0 {
                        0
                    } else {
                        arr_size.div_ceil(chunk_size)
                    }
                })
                .collect(),
        );

        Ok(Self {
            array_shape,
            chunk_shape,
            chunk_count,
        })
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.array_shape.ndim()
    }

    /// Returns the total number of chunks
    #[must_use]
    pub fn total_chunks(&self) -> usize {
        self.chunk_count.size()
    }

    /// Converts array indices to chunk coordinates
    #[must_use]
    pub fn array_index_to_chunk_coord(&self, indices: &[usize]) -> Option<ChunkCoord> {
        if indices.len() != self.ndim() {
            return None;
        }

        let coords: Vec<usize> = indices
            .iter()
            .zip(self.chunk_shape.as_slice())
            .map(|(&idx, &chunk_size)| idx / chunk_size)
            .collect();

        Some(ChunkCoord::new_unchecked(coords))
    }

    /// Returns the array slice covered by a chunk
    #[must_use]
    pub fn chunk_slice(&self, coord: &ChunkCoord) -> Option<ChunkSlice> {
        if coord.ndim() != self.ndim() {
            return None;
        }

        if !coord.is_valid_for_grid(self) {
            return None;
        }

        let mut starts = Vec::with_capacity(self.ndim());
        let mut ends = Vec::with_capacity(self.ndim());

        for i in 0..self.ndim() {
            let start = coord[i] * self.chunk_shape[i];
            let end = (start + self.chunk_shape[i]).min(self.array_shape[i]);
            starts.push(start);
            ends.push(end);
        }

        Some(ChunkSlice { starts, ends })
    }

    /// Returns the actual size of a chunk (may be smaller at boundaries)
    #[must_use]
    pub fn chunk_size(&self, coord: &ChunkCoord) -> Option<usize> {
        self.chunk_slice(coord).map(|slice| {
            slice
                .starts
                .iter()
                .zip(slice.ends.iter())
                .map(|(s, e)| e - s)
                .product()
        })
    }

    /// Returns an iterator over all chunk coordinates
    pub fn iter_coords(&self) -> ChunkGridIter<'_> {
        ChunkGridIter {
            grid: self,
            current: vec![0; self.ndim()],
            finished: false,
        }
    }

    /// Returns chunk coordinates that intersect with an array slice
    pub fn intersecting_chunks(
        &self,
        slice_starts: &[usize],
        slice_ends: &[usize],
    ) -> Vec<ChunkCoord> {
        if slice_starts.len() != self.ndim() || slice_ends.len() != self.ndim() {
            return Vec::new();
        }

        let mut chunk_ranges = Vec::with_capacity(self.ndim());

        for i in 0..self.ndim() {
            let start_chunk = slice_starts[i] / self.chunk_shape[i];
            let end_chunk =
                ((slice_ends[i] - 1) / self.chunk_shape[i]).min(self.chunk_count[i] - 1);
            chunk_ranges.push(start_chunk..=end_chunk);
        }

        // Generate all combinations
        let mut chunks = Vec::new();
        Self::generate_chunk_combinations(&chunk_ranges, &mut vec![0; self.ndim()], 0, &mut chunks);
        chunks
    }

    fn generate_chunk_combinations(
        ranges: &[core::ops::RangeInclusive<usize>],
        current: &mut [usize],
        dim: usize,
        result: &mut Vec<ChunkCoord>,
    ) {
        if dim == ranges.len() {
            result.push(ChunkCoord::new_unchecked(current.to_vec()));
            return;
        }

        for i in ranges[dim].clone() {
            current[dim] = i;
            Self::generate_chunk_combinations(ranges, current, dim + 1, result);
        }
    }
}

/// Slice of the array covered by a chunk
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSlice {
    /// Start indices for each dimension
    pub starts: Vec<usize>,
    /// End indices for each dimension (exclusive)
    pub ends: Vec<usize>,
}

impl ChunkSlice {
    /// Returns the shape of this slice
    #[must_use]
    pub fn shape(&self) -> Shape {
        Shape::new_unchecked(
            self.starts
                .iter()
                .zip(self.ends.iter())
                .map(|(s, e)| e - s)
                .collect(),
        )
    }

    /// Returns the total number of elements in this slice
    #[must_use]
    pub fn size(&self) -> usize {
        self.shape().size()
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.starts.len()
    }
}

/// Iterator over chunk grid coordinates
pub struct ChunkGridIter<'a> {
    grid: &'a ChunkGrid,
    current: Vec<usize>,
    finished: bool,
}

impl<'a> Iterator for ChunkGridIter<'a> {
    type Item = ChunkCoord;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let result = ChunkCoord::new_unchecked(self.current.clone());

        // Increment coordinates (C-order)
        for i in (0..self.grid.ndim()).rev() {
            self.current[i] += 1;
            if self.current[i] < self.grid.chunk_count[i] {
                return Some(result);
            }
            self.current[i] = 0;
        }

        self.finished = true;
        Some(result)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.finished {
            (0, Some(0))
        } else {
            let total = self.grid.total_chunks();
            // Calculate how many we've already returned
            let mut index = 0;
            let mut multiplier = 1;
            for i in (0..self.grid.ndim()).rev() {
                index += self.current[i] * multiplier;
                multiplier *= self.grid.chunk_count[i];
            }
            let remaining = total - index;
            (remaining, Some(remaining))
        }
    }
}

impl<'a> ExactSizeIterator for ChunkGridIter<'a> {}

/// Chunk index - linear index of a chunk
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkIndex(pub usize);

impl ChunkIndex {
    /// Creates a new chunk index
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Returns the index value
    #[must_use]
    pub const fn value(&self) -> usize {
        self.0
    }

    /// Converts to chunk coordinates using a grid
    #[must_use]
    pub fn to_coord(&self, grid: &ChunkGrid) -> Option<ChunkCoord> {
        grid.chunk_count
            .unravel_index(self.0)
            .map(ChunkCoord::new_unchecked)
    }
}

impl From<usize> for ChunkIndex {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_coord() {
        let coord = ChunkCoord::new(vec![0, 1, 2]).expect("valid coord");
        assert_eq!(coord.ndim(), 3);
        assert_eq!(coord.as_slice(), &[0, 1, 2]);
        assert_eq!(coord[0], 0);
        assert_eq!(coord[1], 1);
        assert_eq!(coord[2], 2);
    }

    #[test]
    fn test_chunk_coord_key() {
        let coord = ChunkCoord::new(vec![0, 1, 2]).expect("valid coord");
        assert_eq!(coord.to_key(DimensionSeparator::Dot), "0.1.2");
        assert_eq!(coord.to_key(DimensionSeparator::Slash), "0/1/2");

        let parsed = ChunkCoord::from_key("0.1.2", DimensionSeparator::Dot).expect("valid key");
        assert_eq!(parsed, coord);

        let parsed2 = ChunkCoord::from_key("0/1/2", DimensionSeparator::Slash).expect("valid key");
        assert_eq!(parsed2, coord);
    }

    #[test]
    fn test_chunk_grid() {
        let array_shape = Shape::new(vec![100, 200, 300]).expect("valid shape");
        let chunk_shape = Shape::new(vec![10, 20, 30]).expect("valid shape");

        let grid = ChunkGrid::new(array_shape, chunk_shape).expect("valid grid");

        assert_eq!(grid.ndim(), 3);
        assert_eq!(grid.chunk_count.as_slice(), &[10, 10, 10]);
        assert_eq!(grid.total_chunks(), 1000);
    }

    #[test]
    fn test_chunk_grid_with_remainder() {
        let array_shape = Shape::new(vec![105, 205]).expect("valid shape");
        let chunk_shape = Shape::new(vec![10, 20]).expect("valid shape");

        let grid = ChunkGrid::new(array_shape, chunk_shape).expect("valid grid");

        assert_eq!(grid.chunk_count.as_slice(), &[11, 11]);

        // Check last chunk size
        let last_coord = ChunkCoord::new(vec![10, 10]).expect("valid coord");
        let size = grid.chunk_size(&last_coord).expect("valid chunk");
        assert_eq!(size, 5 * 5); // 5x5 remainder chunk
    }

    #[test]
    fn test_array_index_to_chunk_coord() {
        let array_shape = Shape::new(vec![100, 200]).expect("valid shape");
        let chunk_shape = Shape::new(vec![10, 20]).expect("valid shape");
        let grid = ChunkGrid::new(array_shape, chunk_shape).expect("valid grid");

        assert_eq!(
            grid.array_index_to_chunk_coord(&[0, 0]),
            Some(ChunkCoord::new_unchecked(vec![0, 0]))
        );
        assert_eq!(
            grid.array_index_to_chunk_coord(&[15, 25]),
            Some(ChunkCoord::new_unchecked(vec![1, 1]))
        );
        assert_eq!(
            grid.array_index_to_chunk_coord(&[99, 199]),
            Some(ChunkCoord::new_unchecked(vec![9, 9]))
        );
    }

    #[test]
    fn test_chunk_slice() {
        let array_shape = Shape::new(vec![100, 200]).expect("valid shape");
        let chunk_shape = Shape::new(vec![10, 20]).expect("valid shape");
        let grid = ChunkGrid::new(array_shape, chunk_shape).expect("valid grid");

        let coord = ChunkCoord::new(vec![0, 0]).expect("valid coord");
        let slice = grid.chunk_slice(&coord).expect("valid slice");
        assert_eq!(slice.starts, vec![0, 0]);
        assert_eq!(slice.ends, vec![10, 20]);
        assert_eq!(slice.size(), 200);

        let coord2 = ChunkCoord::new(vec![5, 7]).expect("valid coord");
        let slice2 = grid.chunk_slice(&coord2).expect("valid slice");
        assert_eq!(slice2.starts, vec![50, 140]);
        assert_eq!(slice2.ends, vec![60, 160]);
    }

    #[test]
    fn test_chunk_grid_iter() {
        let array_shape = Shape::new(vec![20, 30]).expect("valid shape");
        let chunk_shape = Shape::new(vec![10, 10]).expect("valid shape");
        let grid = ChunkGrid::new(array_shape, chunk_shape).expect("valid grid");

        let coords: Vec<_> = grid.iter_coords().collect();
        assert_eq!(coords.len(), 6); // 2x3 chunks

        assert_eq!(coords[0], ChunkCoord::new_unchecked(vec![0, 0]));
        assert_eq!(coords[1], ChunkCoord::new_unchecked(vec![0, 1]));
        assert_eq!(coords[2], ChunkCoord::new_unchecked(vec![0, 2]));
        assert_eq!(coords[3], ChunkCoord::new_unchecked(vec![1, 0]));
        assert_eq!(coords[4], ChunkCoord::new_unchecked(vec![1, 1]));
        assert_eq!(coords[5], ChunkCoord::new_unchecked(vec![1, 2]));
    }

    #[test]
    fn test_intersecting_chunks() {
        let array_shape = Shape::new(vec![100, 100]).expect("valid shape");
        let chunk_shape = Shape::new(vec![10, 10]).expect("valid shape");
        let grid = ChunkGrid::new(array_shape, chunk_shape).expect("valid grid");

        // Slice that covers chunks [1,1] to [2,2] (2x2 = 4 chunks)
        let chunks = grid.intersecting_chunks(&[15, 15], &[30, 30]);
        assert_eq!(chunks.len(), 4); // 2x2 chunks

        assert!(chunks.contains(&ChunkCoord::new_unchecked(vec![1, 1])));
        assert!(chunks.contains(&ChunkCoord::new_unchecked(vec![1, 2])));
        assert!(chunks.contains(&ChunkCoord::new_unchecked(vec![2, 1])));
        assert!(chunks.contains(&ChunkCoord::new_unchecked(vec![2, 2])));
    }

    #[test]
    fn test_chunk_index() {
        let array_shape = Shape::new(vec![20, 30]).expect("valid shape");
        let chunk_shape = Shape::new(vec![10, 10]).expect("valid shape");
        let grid = ChunkGrid::new(array_shape, chunk_shape).expect("valid grid");

        let index = ChunkIndex::new(0);
        assert_eq!(
            index.to_coord(&grid),
            Some(ChunkCoord::new_unchecked(vec![0, 0]))
        );

        let index2 = ChunkIndex::new(3);
        assert_eq!(
            index2.to_coord(&grid),
            Some(ChunkCoord::new_unchecked(vec![1, 0]))
        );
    }
}
