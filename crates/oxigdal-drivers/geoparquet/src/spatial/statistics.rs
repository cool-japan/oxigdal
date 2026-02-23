//! Row group statistics and spatial filtering

use crate::metadata::GeometryStatistics;
use oxigdal_core::types::BoundingBox;

/// Statistics for a row group
#[derive(Debug, Clone)]
pub struct RowGroupStatistics {
    /// Row group index
    pub row_group: usize,
    /// Geometry statistics
    pub geometry_stats: GeometryStatistics,
    /// Bounding box of all geometries in this row group
    pub bbox: Option<BoundingBox>,
    /// Total number of rows
    pub row_count: u64,
}

impl RowGroupStatistics {
    /// Creates new row group statistics
    pub fn new(row_group: usize, row_count: u64) -> Self {
        Self {
            row_group,
            geometry_stats: GeometryStatistics::new(),
            bbox: None,
            row_count,
        }
    }

    /// Sets the bounding box
    pub fn with_bbox(mut self, bbox: BoundingBox) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Sets the geometry statistics
    pub fn with_geometry_stats(mut self, stats: GeometryStatistics) -> Self {
        self.geometry_stats = stats;
        self
    }

    /// Returns true if this row group might contain geometries intersecting the filter
    pub fn matches_filter(&self, filter: &SpatialFilter) -> bool {
        match filter {
            SpatialFilter::BoundingBox(bbox) => {
                if let Some(ref rg_bbox) = self.bbox {
                    rg_bbox.intersects(bbox)
                } else {
                    true // No bbox info, assume it might match
                }
            }
            SpatialFilter::All => true,
        }
    }

    /// Returns the selectivity estimate for a filter (0.0 to 1.0)
    pub fn estimate_selectivity(&self, filter: &SpatialFilter) -> f64 {
        match filter {
            SpatialFilter::BoundingBox(query_bbox) => {
                if let Some(ref rg_bbox) = self.bbox {
                    if !rg_bbox.intersects(query_bbox) {
                        0.0
                    } else if rg_bbox.is_within(query_bbox) {
                        1.0
                    } else {
                        // Estimate based on bbox overlap
                        let overlap = Self::compute_overlap_ratio(rg_bbox, query_bbox);
                        overlap.clamp(0.0, 1.0)
                    }
                } else {
                    0.5 // No info, assume 50% selectivity
                }
            }
            SpatialFilter::All => 1.0,
        }
    }

    /// Computes the ratio of overlap between two bounding boxes
    fn compute_overlap_ratio(bbox1: &BoundingBox, bbox2: &BoundingBox) -> f64 {
        let intersection_area = {
            let min_x = bbox1.min_x().max(bbox2.min_x());
            let min_y = bbox1.min_y().max(bbox2.min_y());
            let max_x = bbox1.max_x().min(bbox2.max_x());
            let max_y = bbox1.max_y().min(bbox2.max_y());

            if min_x < max_x && min_y < max_y {
                (max_x - min_x) * (max_y - min_y)
            } else {
                0.0
            }
        };

        let bbox1_area = bbox1.width() * bbox1.height();
        if bbox1_area > 0.0 {
            intersection_area / bbox1_area
        } else {
            0.0
        }
    }
}

/// Spatial filter for row group selection
#[derive(Debug, Clone)]
pub enum SpatialFilter {
    /// Filter by bounding box intersection
    BoundingBox(BoundingBox),
    /// No filter (all geometries)
    All,
}

impl SpatialFilter {
    /// Creates a bounding box filter
    pub fn bbox(bbox: BoundingBox) -> Self {
        Self::BoundingBox(bbox)
    }

    /// Creates a filter that matches all geometries
    pub fn all() -> Self {
        Self::All
    }

    /// Returns true if this is a bounding box filter
    pub fn is_bbox(&self) -> bool {
        matches!(self, Self::BoundingBox(_))
    }

    /// Returns the bounding box if this is a bbox filter
    pub fn as_bbox(&self) -> Option<&BoundingBox> {
        match self {
            Self::BoundingBox(bbox) => Some(bbox),
            Self::All => None,
        }
    }
}

/// Statistics collector for a set of row groups
#[derive(Debug, Clone)]
pub struct StatisticsCollector {
    row_group_stats: Vec<RowGroupStatistics>,
}

impl StatisticsCollector {
    /// Creates a new statistics collector
    pub fn new() -> Self {
        Self {
            row_group_stats: Vec::new(),
        }
    }

    /// Adds statistics for a row group
    pub fn add_row_group(&mut self, stats: RowGroupStatistics) {
        self.row_group_stats.push(stats);
    }

    /// Returns statistics for a specific row group
    pub fn get_stats(&self, row_group: usize) -> Option<&RowGroupStatistics> {
        self.row_group_stats.get(row_group)
    }

    /// Returns all row group statistics
    pub fn all_stats(&self) -> &[RowGroupStatistics] {
        &self.row_group_stats
    }

    /// Filters row groups by a spatial filter
    pub fn filter_row_groups(&self, filter: &SpatialFilter) -> Vec<usize> {
        self.row_group_stats
            .iter()
            .filter(|stats| stats.matches_filter(filter))
            .map(|stats| stats.row_group)
            .collect()
    }

    /// Estimates total selectivity for a filter
    pub fn estimate_total_selectivity(&self, filter: &SpatialFilter) -> f64 {
        if self.row_group_stats.is_empty() {
            return 0.0;
        }

        let total_selectivity: f64 = self
            .row_group_stats
            .iter()
            .map(|stats| stats.estimate_selectivity(filter))
            .sum();

        total_selectivity / self.row_group_stats.len() as f64
    }

    /// Returns the total number of rows across all row groups
    pub fn total_row_count(&self) -> u64 {
        self.row_group_stats.iter().map(|s| s.row_count).sum()
    }

    /// Returns the global bounding box covering all row groups
    pub fn global_bbox(&self) -> Option<BoundingBox> {
        let bboxes: Vec<&BoundingBox> = self
            .row_group_stats
            .iter()
            .filter_map(|s| s.bbox.as_ref())
            .collect();

        if bboxes.is_empty() {
            return None;
        }

        let min_x = bboxes
            .iter()
            .map(|b| b.min_x())
            .fold(f64::INFINITY, f64::min);
        let min_y = bboxes
            .iter()
            .map(|b| b.min_y())
            .fold(f64::INFINITY, f64::min);
        let max_x = bboxes
            .iter()
            .map(|b| b.max_x())
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = bboxes
            .iter()
            .map(|b| b.max_y())
            .fold(f64::NEG_INFINITY, f64::max);

        BoundingBox::new(min_x, min_y, max_x, max_y).ok()
    }
}

impl Default for StatisticsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_group_statistics() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
        let stats = RowGroupStatistics::new(0, 1000).with_bbox(bbox);

        let filter = SpatialFilter::bbox(BoundingBox::new(5.0, 5.0, 15.0, 15.0).expect("valid"));
        assert!(stats.matches_filter(&filter));

        let selectivity = stats.estimate_selectivity(&filter);
        assert!(selectivity > 0.0 && selectivity < 1.0);
    }

    #[test]
    fn test_spatial_filter() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
        let filter = SpatialFilter::bbox(bbox);

        assert!(filter.is_bbox());
        assert!(filter.as_bbox().is_some());

        let all_filter = SpatialFilter::all();
        assert!(!all_filter.is_bbox());
    }

    #[test]
    fn test_statistics_collector() {
        let mut collector = StatisticsCollector::new();

        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid");
        let bbox2 = BoundingBox::new(10.0, 10.0, 20.0, 20.0).expect("valid");

        collector.add_row_group(RowGroupStatistics::new(0, 100).with_bbox(bbox1));
        collector.add_row_group(RowGroupStatistics::new(1, 100).with_bbox(bbox2));

        let filter = SpatialFilter::bbox(BoundingBox::new(5.0, 5.0, 15.0, 15.0).expect("valid"));
        let matching = collector.filter_row_groups(&filter);

        assert_eq!(matching.len(), 2);
        assert_eq!(collector.total_row_count(), 200);
    }

    #[test]
    fn test_global_bbox() {
        let mut collector = StatisticsCollector::new();

        collector.add_row_group(
            RowGroupStatistics::new(0, 100)
                .with_bbox(BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid")),
        );
        collector.add_row_group(
            RowGroupStatistics::new(1, 100)
                .with_bbox(BoundingBox::new(20.0, 20.0, 30.0, 30.0).expect("valid")),
        );

        let global = collector.global_bbox();
        assert!(global.is_some());

        let bbox = global.expect("should have global bbox");
        assert_eq!(bbox.min_x(), 0.0);
        assert_eq!(bbox.min_y(), 0.0);
        assert_eq!(bbox.max_x(), 30.0);
        assert_eq!(bbox.max_y(), 30.0);
    }
}
