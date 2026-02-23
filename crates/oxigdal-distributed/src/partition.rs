//! Data partitioning strategies for distributed processing.
//!
//! This module provides various partitioning strategies for dividing geospatial
//! data across multiple worker nodes for parallel processing.

use crate::error::{DistributedError, Result};
use crate::task::PartitionId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Spatial extent for a partition.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpatialExtent {
    /// Minimum X coordinate.
    pub min_x: f64,
    /// Minimum Y coordinate.
    pub min_y: f64,
    /// Maximum X coordinate.
    pub max_x: f64,
    /// Maximum Y coordinate.
    pub max_y: f64,
}

impl SpatialExtent {
    /// Create a new spatial extent.
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<Self> {
        if min_x >= max_x || min_y >= max_y {
            return Err(DistributedError::partitioning(
                "Invalid extent: min must be less than max",
            ));
        }
        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Get the width of the extent.
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Get the height of the extent.
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Get the area of the extent.
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// Check if this extent contains a point.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Check if this extent intersects another extent.
    pub fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }
}

/// A partition of data with associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partition {
    /// Unique partition identifier.
    pub id: PartitionId,
    /// Spatial extent of this partition.
    pub extent: SpatialExtent,
    /// Estimated size in bytes.
    pub estimated_size: u64,
    /// Number of features/pixels in this partition.
    pub feature_count: Option<u64>,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
}

impl Partition {
    /// Create a new partition.
    pub fn new(id: PartitionId, extent: SpatialExtent) -> Self {
        Self {
            id,
            extent,
            estimated_size: 0,
            feature_count: None,
            metadata: HashMap::new(),
        }
    }

    /// Set the estimated size.
    pub fn with_estimated_size(mut self, size: u64) -> Self {
        self.estimated_size = size;
        self
    }

    /// Set the feature count.
    pub fn with_feature_count(mut self, count: u64) -> Self {
        self.feature_count = Some(count);
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Strategy for partitioning data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionStrategy {
    /// Partition data into regular tiles.
    Tile,
    /// Partition data into horizontal strips.
    Strip,
    /// Partition data based on hash of a key column.
    Hash,
    /// Partition data based on value ranges.
    Range,
    /// Partition data to balance load across workers.
    LoadBalanced,
}

/// Tile-based partitioner for regular spatial grids.
pub struct TilePartitioner {
    /// Total spatial extent.
    extent: SpatialExtent,
    /// Number of tiles in X direction.
    tiles_x: usize,
    /// Number of tiles in Y direction.
    tiles_y: usize,
}

impl TilePartitioner {
    /// Create a new tile partitioner.
    pub fn new(extent: SpatialExtent, tiles_x: usize, tiles_y: usize) -> Result<Self> {
        if tiles_x == 0 || tiles_y == 0 {
            return Err(DistributedError::partitioning(
                "Number of tiles must be greater than zero",
            ));
        }
        Ok(Self {
            extent,
            tiles_x,
            tiles_y,
        })
    }

    /// Generate partitions.
    pub fn partition(&self) -> Vec<Partition> {
        let tile_width = self.extent.width() / self.tiles_x as f64;
        let tile_height = self.extent.height() / self.tiles_y as f64;

        let mut partitions = Vec::with_capacity(self.tiles_x * self.tiles_y);
        let mut partition_id = 0;

        for y in 0..self.tiles_y {
            for x in 0..self.tiles_x {
                let min_x = self.extent.min_x + (x as f64 * tile_width);
                let min_y = self.extent.min_y + (y as f64 * tile_height);
                let max_x = min_x + tile_width;
                let max_y = min_y + tile_height;

                if let Ok(tile_extent) = SpatialExtent::new(min_x, min_y, max_x, max_y) {
                    let partition = Partition::new(PartitionId(partition_id), tile_extent)
                        .with_metadata("tile_x".to_string(), x.to_string())
                        .with_metadata("tile_y".to_string(), y.to_string());
                    partitions.push(partition);
                    partition_id += 1;
                }
            }
        }

        partitions
    }

    /// Get the total number of partitions.
    pub fn num_partitions(&self) -> usize {
        self.tiles_x * self.tiles_y
    }
}

/// Strip-based partitioner for horizontal bands.
pub struct StripPartitioner {
    /// Total spatial extent.
    extent: SpatialExtent,
    /// Number of strips.
    num_strips: usize,
}

impl StripPartitioner {
    /// Create a new strip partitioner.
    pub fn new(extent: SpatialExtent, num_strips: usize) -> Result<Self> {
        if num_strips == 0 {
            return Err(DistributedError::partitioning(
                "Number of strips must be greater than zero",
            ));
        }
        Ok(Self { extent, num_strips })
    }

    /// Generate partitions.
    pub fn partition(&self) -> Vec<Partition> {
        let strip_height = self.extent.height() / self.num_strips as f64;

        let mut partitions = Vec::with_capacity(self.num_strips);

        for i in 0..self.num_strips {
            let min_y = self.extent.min_y + (i as f64 * strip_height);
            let max_y = min_y + strip_height;

            if let Ok(strip_extent) =
                SpatialExtent::new(self.extent.min_x, min_y, self.extent.max_x, max_y)
            {
                let partition = Partition::new(PartitionId(i as u64), strip_extent)
                    .with_metadata("strip_index".to_string(), i.to_string());
                partitions.push(partition);
            }
        }

        partitions
    }

    /// Get the total number of partitions.
    pub fn num_partitions(&self) -> usize {
        self.num_strips
    }
}

/// Hash-based partitioner for key-based distribution.
pub struct HashPartitioner {
    /// Number of partitions.
    num_partitions: usize,
}

impl HashPartitioner {
    /// Create a new hash partitioner.
    pub fn new(num_partitions: usize) -> Result<Self> {
        if num_partitions == 0 {
            return Err(DistributedError::partitioning(
                "Number of partitions must be greater than zero",
            ));
        }
        Ok(Self { num_partitions })
    }

    /// Compute partition ID for a key.
    pub fn partition_for_key(&self, key: &[u8]) -> PartitionId {
        let hash = self.hash_key(key);
        PartitionId(hash % self.num_partitions as u64)
    }

    /// Hash a key using a simple FNV-1a hash.
    fn hash_key(&self, key: &[u8]) -> u64 {
        const FNV_OFFSET: u64 = 14695981039346656037;
        const FNV_PRIME: u64 = 1099511628211;

        let mut hash = FNV_OFFSET;
        for &byte in key {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    /// Get the total number of partitions.
    pub fn num_partitions(&self) -> usize {
        self.num_partitions
    }
}

/// Range-based partitioner for value-based distribution.
pub struct RangePartitioner {
    /// Partition boundaries.
    boundaries: Vec<f64>,
}

impl RangePartitioner {
    /// Create a new range partitioner with specified boundaries.
    pub fn new(boundaries: Vec<f64>) -> Result<Self> {
        if boundaries.is_empty() {
            return Err(DistributedError::partitioning("Boundaries cannot be empty"));
        }

        // Verify boundaries are sorted
        for i in 1..boundaries.len() {
            if boundaries[i] <= boundaries[i - 1] {
                return Err(DistributedError::partitioning(
                    "Boundaries must be sorted in ascending order",
                ));
            }
        }

        Ok(Self { boundaries })
    }

    /// Compute partition ID for a value.
    pub fn partition_for_value(&self, value: f64) -> PartitionId {
        // Binary search to find the partition
        let mut low = 0;
        let mut high = self.boundaries.len();

        while low < high {
            let mid = (low + high) / 2;
            if value < self.boundaries[mid] {
                high = mid;
            } else {
                low = mid + 1;
            }
        }

        PartitionId(low as u64)
    }

    /// Get the total number of partitions.
    pub fn num_partitions(&self) -> usize {
        self.boundaries.len() + 1
    }
}

/// Load-balanced partitioner that considers data size.
pub struct LoadBalancedPartitioner {
    /// Target size per partition in bytes.
    target_size: u64,
    /// Total data size.
    total_size: u64,
}

impl LoadBalancedPartitioner {
    /// Create a new load-balanced partitioner.
    pub fn new(total_size: u64, num_workers: usize) -> Result<Self> {
        if num_workers == 0 {
            return Err(DistributedError::partitioning(
                "Number of workers must be greater than zero",
            ));
        }

        let target_size = total_size.div_ceil(num_workers as u64);

        Ok(Self {
            target_size,
            total_size,
        })
    }

    /// Get the target size per partition.
    pub fn target_size(&self) -> u64 {
        self.target_size
    }

    /// Estimate the number of partitions needed.
    pub fn estimated_partitions(&self) -> usize {
        self.total_size.div_ceil(self.target_size) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_extent() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let extent = SpatialExtent::new(0.0, 0.0, 100.0, 100.0)?;
        assert_eq!(extent.width(), 100.0);
        assert_eq!(extent.height(), 100.0);
        assert_eq!(extent.area(), 10000.0);

        assert!(extent.contains(50.0, 50.0));
        assert!(!extent.contains(150.0, 50.0));

        let other = SpatialExtent::new(50.0, 50.0, 150.0, 150.0)?;
        assert!(extent.intersects(&other));
        Ok(())
    }

    #[test]
    fn test_tile_partitioner() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let extent = SpatialExtent::new(0.0, 0.0, 100.0, 100.0)?;
        let partitioner = TilePartitioner::new(extent, 2, 2)?;

        let partitions = partitioner.partition();
        assert_eq!(partitions.len(), 4);
        assert_eq!(partitioner.num_partitions(), 4);

        // Check first tile
        let first = &partitions[0];
        assert_eq!(first.extent.min_x, 0.0);
        assert_eq!(first.extent.min_y, 0.0);
        assert_eq!(first.extent.max_x, 50.0);
        assert_eq!(first.extent.max_y, 50.0);
        Ok(())
    }

    #[test]
    fn test_strip_partitioner() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let extent = SpatialExtent::new(0.0, 0.0, 100.0, 100.0)?;
        let partitioner = StripPartitioner::new(extent, 4)?;

        let partitions = partitioner.partition();
        assert_eq!(partitions.len(), 4);
        assert_eq!(partitioner.num_partitions(), 4);

        // Check first strip
        let first = &partitions[0];
        assert_eq!(first.extent.min_x, 0.0);
        assert_eq!(first.extent.max_x, 100.0);
        assert_eq!(first.extent.height(), 25.0);
        Ok(())
    }

    #[test]
    fn test_hash_partitioner() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let partitioner = HashPartitioner::new(4)?;
        assert_eq!(partitioner.num_partitions(), 4);

        let key1 = b"test_key_1";
        let key2 = b"test_key_2";

        let partition1 = partitioner.partition_for_key(key1);
        let partition2 = partitioner.partition_for_key(key2);

        // Same key should always go to same partition
        assert_eq!(partition1, partitioner.partition_for_key(key1));
        assert_eq!(partition2, partitioner.partition_for_key(key2));

        // Partitions should be in valid range
        assert!(partition1.0 < 4);
        assert!(partition2.0 < 4);
        Ok(())
    }

    #[test]
    fn test_range_partitioner() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let boundaries = vec![10.0, 20.0, 30.0];
        let partitioner = RangePartitioner::new(boundaries)?;

        assert_eq!(partitioner.num_partitions(), 4);

        assert_eq!(partitioner.partition_for_value(5.0), PartitionId(0));
        assert_eq!(partitioner.partition_for_value(15.0), PartitionId(1));
        assert_eq!(partitioner.partition_for_value(25.0), PartitionId(2));
        assert_eq!(partitioner.partition_for_value(35.0), PartitionId(3));
        Ok(())
    }

    #[test]
    fn test_load_balanced_partitioner() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let total_size = 1000 * 1024 * 1024; // 1000 MB
        let num_workers = 4;
        let partitioner = LoadBalancedPartitioner::new(total_size, num_workers)?;

        assert_eq!(partitioner.target_size(), 250 * 1024 * 1024);
        assert_eq!(partitioner.estimated_partitions(), 4);
        Ok(())
    }
}
