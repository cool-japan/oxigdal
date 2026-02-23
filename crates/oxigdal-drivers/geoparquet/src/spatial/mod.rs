//! Spatial partitioning and indexing for GeoParquet
//!
//! This module provides spatial partitioning strategies and row group
//! statistics for efficient spatial queries on GeoParquet files.

mod partition;
mod rtree;
mod statistics;

pub use partition::{PartitionStrategy, RowGroupPartition, SpatialPartitioner};
pub use rtree::{BoundingBox as RTreeBBox, RTreeIndex};
pub use statistics::{RowGroupStatistics, SpatialFilter};

use crate::error::Result;
use oxigdal_core::types::BoundingBox;

/// Spatial reference for row group bounds
#[derive(Debug, Clone)]
pub struct RowGroupBounds {
    /// Row group index
    pub row_group: usize,
    /// Bounding box of geometries in this row group
    pub bbox: BoundingBox,
    /// Number of rows in this row group
    pub row_count: u64,
}

impl RowGroupBounds {
    /// Creates new row group bounds
    pub fn new(row_group: usize, bbox: BoundingBox, row_count: u64) -> Self {
        Self {
            row_group,
            bbox,
            row_count,
        }
    }

    /// Returns true if this row group intersects the given bounding box
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.bbox.intersects(other)
    }

    /// Returns true if this row group is fully contained within the given bounding box
    pub fn contained_by(&self, other: &BoundingBox) -> bool {
        self.bbox.is_within(other)
    }
}

/// Spatial index for GeoParquet files
pub struct SpatialIndex {
    row_groups: Vec<RowGroupBounds>,
    rtree: Option<RTreeIndex>,
}

impl SpatialIndex {
    /// Creates a new spatial index
    pub fn new(row_groups: Vec<RowGroupBounds>) -> Self {
        Self {
            row_groups,
            rtree: None,
        }
    }

    /// Builds an R-tree index for faster spatial queries
    pub fn build_rtree(&mut self) -> Result<()> {
        let mut rtree = RTreeIndex::new();
        for rg_bounds in &self.row_groups {
            let bbox = RTreeBBox::from_oxigdal(&rg_bounds.bbox);
            rtree.insert(rg_bounds.row_group, bbox);
        }
        self.rtree = Some(rtree);
        Ok(())
    }

    /// Queries row groups that intersect the given bounding box
    pub fn query(&self, bbox: &BoundingBox) -> Vec<usize> {
        if let Some(ref rtree) = self.rtree {
            let rtree_bbox = RTreeBBox::from_oxigdal(bbox);
            rtree.query(&rtree_bbox)
        } else {
            // Linear scan fallback
            self.row_groups
                .iter()
                .filter(|rg| rg.intersects(bbox))
                .map(|rg| rg.row_group)
                .collect()
        }
    }

    /// Returns row groups that are fully contained within the given bounding box
    pub fn query_contained(&self, bbox: &BoundingBox) -> Vec<usize> {
        self.row_groups
            .iter()
            .filter(|rg| rg.contained_by(bbox))
            .map(|rg| rg.row_group)
            .collect()
    }

    /// Returns all row groups
    pub fn all_row_groups(&self) -> Vec<usize> {
        (0..self.row_groups.len()).collect()
    }

    /// Returns the number of row groups
    pub fn num_row_groups(&self) -> usize {
        self.row_groups.len()
    }

    /// Gets the bounds for a specific row group
    pub fn get_bounds(&self, row_group: usize) -> Option<&RowGroupBounds> {
        self.row_groups.get(row_group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_group_bounds() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
        let rg_bounds = RowGroupBounds::new(0, bbox, 1000);

        let query_bbox = BoundingBox::new(5.0, 5.0, 15.0, 15.0).expect("valid bbox");
        assert!(rg_bounds.intersects(&query_bbox));

        let contained_bbox = BoundingBox::new(2.0, 2.0, 8.0, 8.0).expect("valid bbox");
        assert!(!rg_bounds.contained_by(&contained_bbox));

        let containing_bbox = BoundingBox::new(-5.0, -5.0, 15.0, 15.0).expect("valid bbox");
        assert!(rg_bounds.contained_by(&containing_bbox));
    }

    #[test]
    fn test_spatial_index_query() {
        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
        let bbox2 = BoundingBox::new(10.0, 10.0, 20.0, 20.0).expect("valid bbox");
        let bbox3 = BoundingBox::new(20.0, 20.0, 30.0, 30.0).expect("valid bbox");

        let row_groups = vec![
            RowGroupBounds::new(0, bbox1, 100),
            RowGroupBounds::new(1, bbox2, 100),
            RowGroupBounds::new(2, bbox3, 100),
        ];

        let index = SpatialIndex::new(row_groups);

        let query = BoundingBox::new(5.0, 5.0, 15.0, 15.0).expect("valid bbox");
        let results = index.query(&query);

        assert_eq!(results.len(), 2);
        assert!(results.contains(&0));
        assert!(results.contains(&1));
    }
}
