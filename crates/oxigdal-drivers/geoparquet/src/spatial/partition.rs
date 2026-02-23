//! Spatial partitioning strategies for GeoParquet
//!
//! This module provides different strategies for partitioning geometries
//! into row groups based on spatial properties.

use crate::error::Result;
use crate::geometry::Geometry;
use oxigdal_core::types::BoundingBox;

/// Strategy for partitioning geometries into row groups
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionStrategy {
    /// No spatial partitioning (sequential)
    None,
    /// Hilbert curve-based spatial ordering
    Hilbert,
    /// Grid-based partitioning
    Grid { cells_x: u32, cells_y: u32 },
    /// R-tree based partitioning (Sort-Tile-Recursive)
    STR { node_capacity: usize },
}

/// Partition information for a row group
#[derive(Debug, Clone)]
pub struct RowGroupPartition {
    /// Row group index
    pub index: usize,
    /// Bounding box of this partition
    pub bbox: Option<BoundingBox>,
    /// Starting row in the partition
    pub start_row: u64,
    /// Number of rows in this partition
    pub row_count: u64,
}

impl RowGroupPartition {
    /// Creates a new row group partition
    pub fn new(index: usize, start_row: u64, row_count: u64) -> Self {
        Self {
            index,
            bbox: None,
            start_row,
            row_count,
        }
    }

    /// Sets the bounding box for this partition
    pub fn with_bbox(mut self, bbox: BoundingBox) -> Self {
        self.bbox = Some(bbox);
        self
    }
}

/// Spatial partitioner for organizing geometries
pub struct SpatialPartitioner {
    strategy: PartitionStrategy,
    target_row_group_size: usize,
    global_bounds: Option<BoundingBox>,
}

impl SpatialPartitioner {
    /// Creates a new spatial partitioner
    pub fn new(strategy: PartitionStrategy, target_row_group_size: usize) -> Self {
        Self {
            strategy,
            target_row_group_size,
            global_bounds: None,
        }
    }

    /// Sets the global bounds for partitioning
    pub fn with_bounds(mut self, bounds: BoundingBox) -> Self {
        self.global_bounds = Some(bounds);
        self
    }

    /// Computes partitions for a set of geometries
    pub fn partition(&self, geometries: &[Geometry]) -> Result<Vec<RowGroupPartition>> {
        match self.strategy {
            PartitionStrategy::None => self.partition_sequential(geometries),
            PartitionStrategy::Hilbert => self.partition_hilbert(geometries),
            PartitionStrategy::Grid { cells_x, cells_y } => {
                self.partition_grid(geometries, cells_x, cells_y)
            }
            PartitionStrategy::STR { node_capacity } => {
                self.partition_str(geometries, node_capacity)
            }
        }
    }

    /// Sequential partitioning (no spatial ordering)
    fn partition_sequential(&self, geometries: &[Geometry]) -> Result<Vec<RowGroupPartition>> {
        let mut partitions = Vec::new();
        let mut current_row = 0u64;
        let num_geometries = geometries.len();

        for (idx, chunk_start) in (0..num_geometries)
            .step_by(self.target_row_group_size)
            .enumerate()
        {
            let chunk_end = (chunk_start + self.target_row_group_size).min(num_geometries);
            let row_count = (chunk_end - chunk_start) as u64;

            // Compute bounding box for this chunk
            let bbox = Self::compute_chunk_bbox(&geometries[chunk_start..chunk_end]);

            let mut partition = RowGroupPartition::new(idx, current_row, row_count);
            if let Some(bbox) = bbox {
                partition = partition.with_bbox(bbox);
            }

            partitions.push(partition);
            current_row += row_count;
        }

        Ok(partitions)
    }

    /// Hilbert curve-based partitioning
    fn partition_hilbert(&self, geometries: &[Geometry]) -> Result<Vec<RowGroupPartition>> {
        let bounds = self
            .global_bounds
            .or_else(|| Self::compute_global_bounds(geometries))
            .ok_or_else(|| crate::error::GeoParquetError::internal("No bounds for partitioning"))?;

        // Compute Hilbert codes for each geometry
        let mut geometry_codes: Vec<(u64, usize)> = geometries
            .iter()
            .enumerate()
            .filter_map(|(idx, geom)| {
                geom.bbox().map(|bbox| {
                    let center_x = (bbox[0] + bbox[2]) / 2.0;
                    let center_y = (bbox[1] + bbox[3]) / 2.0;
                    (Self::hilbert_code(center_x, center_y, &bounds, 16), idx)
                })
            })
            .collect();

        // Sort by Hilbert code
        geometry_codes.sort_by_key(|(code, _)| *code);

        // Create partitions
        let mut partitions = Vec::new();
        let mut current_row = 0u64;

        for (partition_idx, chunk) in geometry_codes
            .chunks(self.target_row_group_size)
            .enumerate()
        {
            let row_count = chunk.len() as u64;
            let indices: Vec<usize> = chunk.iter().map(|(_, idx)| *idx).collect();

            let chunk_geometries: Vec<&Geometry> =
                indices.iter().map(|&i| &geometries[i]).collect();
            let bbox = Self::compute_bbox_from_refs(&chunk_geometries);

            let mut partition = RowGroupPartition::new(partition_idx, current_row, row_count);
            if let Some(bbox) = bbox {
                partition = partition.with_bbox(bbox);
            }

            partitions.push(partition);
            current_row += row_count;
        }

        Ok(partitions)
    }

    /// Grid-based partitioning
    fn partition_grid(
        &self,
        geometries: &[Geometry],
        cells_x: u32,
        cells_y: u32,
    ) -> Result<Vec<RowGroupPartition>> {
        let bounds = self
            .global_bounds
            .or_else(|| Self::compute_global_bounds(geometries))
            .ok_or_else(|| crate::error::GeoParquetError::internal("No bounds for partitioning"))?;

        let cell_width = bounds.width() / cells_x as f64;
        let cell_height = bounds.height() / cells_y as f64;

        // Assign geometries to grid cells
        let mut cell_geometries: Vec<Vec<usize>> = vec![Vec::new(); (cells_x * cells_y) as usize];

        for (idx, geom) in geometries.iter().enumerate() {
            if let Some(bbox) = geom.bbox() {
                let center_x = (bbox[0] + bbox[2]) / 2.0;
                let center_y = (bbox[1] + bbox[3]) / 2.0;

                let cell_x = ((center_x - bounds.min_x()) / cell_width)
                    .floor()
                    .min((cells_x - 1) as f64) as u32;
                let cell_y = ((center_y - bounds.min_y()) / cell_height)
                    .floor()
                    .min((cells_y - 1) as f64) as u32;

                let cell_idx = (cell_y * cells_x + cell_x) as usize;
                cell_geometries[cell_idx].push(idx);
            }
        }

        // Create partitions from non-empty cells
        let mut partitions = Vec::new();
        let mut current_row = 0u64;
        let mut partition_idx = 0;

        for cell_indices in cell_geometries {
            if cell_indices.is_empty() {
                continue;
            }

            let row_count = cell_indices.len() as u64;
            let chunk_geometries: Vec<&Geometry> =
                cell_indices.iter().map(|&i| &geometries[i]).collect();
            let bbox = Self::compute_bbox_from_refs(&chunk_geometries);

            let mut partition = RowGroupPartition::new(partition_idx, current_row, row_count);
            if let Some(bbox) = bbox {
                partition = partition.with_bbox(bbox);
            }

            partitions.push(partition);
            current_row += row_count;
            partition_idx += 1;
        }

        Ok(partitions)
    }

    /// STR (Sort-Tile-Recursive) partitioning
    fn partition_str(
        &self,
        geometries: &[Geometry],
        _node_capacity: usize,
    ) -> Result<Vec<RowGroupPartition>> {
        // For now, use sequential partitioning
        // A full STR implementation would be more complex
        self.partition_sequential(geometries)
    }

    /// Computes Hilbert code for a point
    fn hilbert_code(x: f64, y: f64, bounds: &BoundingBox, order: u32) -> u64 {
        let max_val = (1u64 << order) - 1;

        let norm_x = ((x - bounds.min_x()) / bounds.width()).clamp(0.0, 1.0);
        let norm_y = ((y - bounds.min_y()) / bounds.height()).clamp(0.0, 1.0);

        let ix = (norm_x * max_val as f64) as u64;
        let iy = (norm_y * max_val as f64) as u64;

        Self::hilbert_encode(ix, iy, order)
    }

    /// Encodes x, y coordinates to Hilbert curve index
    fn hilbert_encode(mut x: u64, mut y: u64, order: u32) -> u64 {
        let mut d = 0u64;
        let mut s = 1u64 << (order - 1);

        while s > 0 {
            let rx = ((x & s) > 0) as u64;
            let ry = ((y & s) > 0) as u64;
            d += s * s * ((3 * rx) ^ ry);

            if ry == 0 {
                if rx == 1 {
                    x = s - 1 - x;
                    y = s - 1 - y;
                }
                std::mem::swap(&mut x, &mut y);
            }

            s >>= 1;
        }

        d
    }

    /// Computes global bounds from geometries
    fn compute_global_bounds(geometries: &[Geometry]) -> Option<BoundingBox> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for geom in geometries {
            if let Some(bbox) = geom.bbox() {
                min_x = min_x.min(bbox[0]);
                min_y = min_y.min(bbox[1]);
                max_x = max_x.max(bbox[2]);
                max_y = max_y.max(bbox[3]);
            }
        }

        if min_x.is_finite() {
            BoundingBox::new(min_x, min_y, max_x, max_y).ok()
        } else {
            None
        }
    }

    /// Computes bounding box for a chunk of geometries
    fn compute_chunk_bbox(geometries: &[Geometry]) -> Option<BoundingBox> {
        let bboxes: Vec<Vec<f64>> = geometries.iter().filter_map(|g| g.bbox()).collect();
        if bboxes.is_empty() {
            return None;
        }

        let min_x = bboxes.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min);
        let min_y = bboxes.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min);
        let max_x = bboxes
            .iter()
            .map(|b| b[2])
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = bboxes
            .iter()
            .map(|b| b[3])
            .fold(f64::NEG_INFINITY, f64::max);

        BoundingBox::new(min_x, min_y, max_x, max_y).ok()
    }

    /// Computes bounding box from geometry references
    fn compute_bbox_from_refs(geometries: &[&Geometry]) -> Option<BoundingBox> {
        let bboxes: Vec<Vec<f64>> = geometries.iter().filter_map(|g| g.bbox()).collect();
        if bboxes.is_empty() {
            return None;
        }

        let min_x = bboxes.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min);
        let min_y = bboxes.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min);
        let max_x = bboxes
            .iter()
            .map(|b| b[2])
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = bboxes
            .iter()
            .map(|b| b[3])
            .fold(f64::NEG_INFINITY, f64::max);

        BoundingBox::new(min_x, min_y, max_x, max_y).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    #[test]
    fn test_sequential_partitioning() {
        let geometries: Vec<Geometry> = (0..10)
            .map(|i| Geometry::Point(Point::new_2d(i as f64, i as f64)))
            .collect();

        let partitioner = SpatialPartitioner::new(PartitionStrategy::None, 3);
        let partitions = partitioner
            .partition(&geometries)
            .expect("should partition");

        assert_eq!(partitions.len(), 4); // 3, 3, 3, 1
        assert_eq!(partitions[0].row_count, 3);
        assert_eq!(partitions[3].row_count, 1);
    }

    #[test]
    fn test_hilbert_encoding() {
        let code1 = SpatialPartitioner::hilbert_encode(0, 0, 8);
        let code2 = SpatialPartitioner::hilbert_encode(255, 255, 8);
        assert_ne!(code1, code2);
    }
}
