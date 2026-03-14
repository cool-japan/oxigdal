//! Partitioned GeoParquet dataset support
//!
//! Supports:
//! - Hive-style partitioning: `data/year=2024/month=01/part-0.parquet`
//! - Spatial partitioning: by bbox grid, quadtree, or Z-order curve
//!
//! # Example
//!
//! ```rust
//! use oxigdal_geoparquet::partitioning::{PartitionKey, Partition, PartitionedDataset};
//!
//! let mut ds = PartitionedDataset::new("/data/tiles", vec!["year".into(), "month".into()]);
//! let keys = vec![
//!     PartitionKey::new("year", "2024"),
//!     PartitionKey::new("month", "01"),
//! ];
//! let path = ds.hive_path(&keys);
//! assert!(path.ends_with("year=2024/month=01"));
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Partition key
// ---------------------------------------------------------------------------

/// A single partition column = value pair (Hive-style).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartitionKey {
    /// Column name (e.g., `"year"`).
    pub column: String,
    /// Partition value (e.g., `"2024"`).
    pub value: String,
}

impl PartitionKey {
    /// Create a new partition key.
    pub fn new(column: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            value: value.into(),
        }
    }

    /// Parse a Hive-style path component such as `"year=2024"`.
    ///
    /// Returns `None` if the component does not contain `=`.
    #[must_use]
    pub fn from_hive_component(component: &str) -> Option<Self> {
        let (col, val) = component.split_once('=')?;
        Some(Self::new(col, val))
    }

    /// Render back as a Hive path component: `"year=2024"`.
    #[must_use]
    pub fn to_hive_component(&self) -> String {
        format!("{}={}", self.column, self.value)
    }
}

// ---------------------------------------------------------------------------
// Partition
// ---------------------------------------------------------------------------

/// A single partition — one Parquet file with its associated partition keys.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Path to the Parquet file.
    pub path: PathBuf,
    /// Partition key-value pairs.
    pub keys: Vec<PartitionKey>,
    /// Number of rows (if known).
    pub row_count: Option<u64>,
    /// File size in bytes (if known).
    pub file_size_bytes: Option<u64>,
    /// Spatial bounding box `[min_x, min_y, max_x, max_y]` (if known).
    pub spatial_extent: Option<[f64; 4]>,
}

impl Partition {
    /// Create a new partition at the given path with the given keys.
    #[must_use]
    pub fn new(path: impl AsRef<Path>, keys: Vec<PartitionKey>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            keys,
            row_count: None,
            file_size_bytes: None,
            spatial_extent: None,
        }
    }

    /// Builder: set row count.
    #[must_use]
    pub fn with_row_count(mut self, count: u64) -> Self {
        self.row_count = Some(count);
        self
    }

    /// Builder: set file size.
    #[must_use]
    pub fn with_file_size(mut self, bytes: u64) -> Self {
        self.file_size_bytes = Some(bytes);
        self
    }

    /// Builder: set spatial extent `[min_x, min_y, max_x, max_y]`.
    #[must_use]
    pub fn with_spatial_extent(mut self, extent: [f64; 4]) -> Self {
        self.spatial_extent = Some(extent);
        self
    }

    /// Returns true if `self.keys` contains the given key.
    #[must_use]
    pub fn has_key(&self, key: &PartitionKey) -> bool {
        self.keys.contains(key)
    }

    /// Returns the value for the given column name, or `None`.
    #[must_use]
    pub fn get_value(&self, column: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|k| k.column == column)
            .map(|k| k.value.as_str())
    }

    /// Returns true if the spatial extent overlaps the query bbox.
    /// If no extent is stored, returns `true` (conservative include-all).
    #[must_use]
    pub fn overlaps_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        match self.spatial_extent {
            None => true, // unknown extent → include conservatively
            Some([px0, py0, px1, py1]) => {
                // standard AABB overlap test
                !(max_x < px0 || min_x > px1 || max_y < py0 || min_y > py1)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Spatial partition strategy
// ---------------------------------------------------------------------------

/// Strategy for spatially partitioning a GeoParquet dataset.
#[derive(Debug, Clone, PartialEq)]
pub enum SpatialPartitionStrategy {
    /// Divide the spatial extent into an NxM regular grid.
    Grid {
        /// Number of columns.
        n_x: u32,
        /// Number of rows.
        n_y: u32,
    },
    /// Recursively subdivide via quadtree to the given depth.
    Quadtree {
        /// Maximum subdivision depth.
        depth: u8,
    },
    /// Space-filling Z-order (Morton) curve with given bit resolution.
    ZOrder {
        /// Number of bits per dimension.
        bits: u8,
    },
}

impl SpatialPartitionStrategy {
    /// Returns the maximum number of cells this strategy may produce.
    #[must_use]
    pub fn max_cells(&self) -> u64 {
        match self {
            Self::Grid { n_x, n_y } => (*n_x as u64) * (*n_y as u64),
            Self::Quadtree { depth } => 4u64.saturating_pow(*depth as u32),
            Self::ZOrder { bits } => 1u64 << (2 * (*bits as u64)),
        }
    }

    /// Returns a human-readable name for this strategy.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Grid { .. } => "Grid",
            Self::Quadtree { .. } => "Quadtree",
            Self::ZOrder { .. } => "Z-Order (Morton)",
        }
    }
}

// ---------------------------------------------------------------------------
// PartitionedDataset
// ---------------------------------------------------------------------------

/// A collection of GeoParquet files forming a logically partitioned dataset.
#[derive(Debug, Clone)]
pub struct PartitionedDataset {
    /// Root directory of the dataset.
    pub base_path: PathBuf,
    /// Registered partitions.
    pub partitions: Vec<Partition>,
    /// Column names used for partitioning (in order).
    pub partition_columns: Vec<String>,
    /// Non-partition column names in the schema.
    pub schema_columns: Vec<String>,
    /// Total row count across all partitions.
    pub total_row_count: u64,
    /// Spatial partition strategy (if spatial partitioning is used).
    pub spatial_strategy: Option<SpatialPartitionStrategy>,
}

impl PartitionedDataset {
    /// Create a new empty partitioned dataset.
    pub fn new(base_path: impl AsRef<Path>, partition_columns: Vec<String>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
            partitions: Vec::new(),
            partition_columns,
            schema_columns: Vec::new(),
            total_row_count: 0,
            spatial_strategy: None,
        }
    }

    /// Set the spatial partition strategy.
    #[must_use]
    pub fn with_spatial_strategy(mut self, strategy: SpatialPartitionStrategy) -> Self {
        self.spatial_strategy = Some(strategy);
        self
    }

    /// Set the non-partition schema columns.
    #[must_use]
    pub fn with_schema_columns(mut self, columns: Vec<String>) -> Self {
        self.schema_columns = columns;
        self
    }

    /// Register a partition.
    pub fn add_partition(&mut self, partition: Partition) {
        self.total_row_count += partition.row_count.unwrap_or(0);
        self.partitions.push(partition);
    }

    /// Number of registered partitions.
    #[must_use]
    pub fn partition_count(&self) -> usize {
        self.partitions.len()
    }

    /// Filter partitions that match **all** of the given key-value pairs
    /// (partition pruning).
    pub fn filter_partitions<'a>(&'a self, filters: &[PartitionKey]) -> Vec<&'a Partition> {
        self.partitions
            .iter()
            .filter(|p| filters.iter().all(|f| p.keys.contains(f)))
            .collect()
    }

    /// Return partitions whose spatial extent overlaps the query bounding box.
    ///
    /// Partitions without a stored spatial extent are conservatively included.
    pub fn partitions_for_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Vec<&Partition> {
        self.partitions
            .iter()
            .filter(|p| p.overlaps_bbox(min_x, min_y, max_x, max_y))
            .collect()
    }

    /// Build the Hive-style directory path for a given set of partition keys.
    ///
    /// E.g. `base/year=2024/month=01`
    #[must_use]
    pub fn hive_path(&self, keys: &[PartitionKey]) -> PathBuf {
        let mut path = self.base_path.clone();
        for key in keys {
            path = path.join(key.to_hive_component());
        }
        path
    }

    /// Parse partition keys from a Hive-style path relative to `base_path`.
    ///
    /// Returns an empty vec if no Hive components are found.
    #[must_use]
    pub fn parse_hive_path(&self, path: &Path) -> Vec<PartitionKey> {
        let rel = path.strip_prefix(&self.base_path).unwrap_or(path);
        rel.components()
            .filter_map(|c| {
                c.as_os_str()
                    .to_str()
                    .and_then(PartitionKey::from_hive_component)
            })
            .collect()
    }

    /// Collect all unique values seen for each partition column.
    #[must_use]
    pub fn unique_values(&self) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for p in &self.partitions {
            for k in &p.keys {
                map.entry(k.column.clone())
                    .or_default()
                    .push(k.value.clone());
            }
        }
        for vals in map.values_mut() {
            vals.sort();
            vals.dedup();
        }
        map
    }
}

// ---------------------------------------------------------------------------
// PartitionStats
// ---------------------------------------------------------------------------

/// Aggregate statistics for a partitioned dataset.
#[derive(Debug, Clone)]
pub struct PartitionStats {
    /// Number of partitions.
    pub n_partitions: usize,
    /// Total row count.
    pub total_rows: u64,
    /// Total file size in bytes (only partitions with known sizes).
    pub total_size_bytes: u64,
    /// Average rows per partition.
    pub avg_rows_per_partition: f64,
    /// Columns used for partitioning.
    pub partition_columns: Vec<String>,
    /// Unique values per partition column.
    pub unique_values: HashMap<String, Vec<String>>,
    /// Minimum rows in a single partition (among partitions with known count).
    pub min_rows: Option<u64>,
    /// Maximum rows in a single partition (among partitions with known count).
    pub max_rows: Option<u64>,
}

impl PartitionStats {
    /// Compute statistics from a `PartitionedDataset`.
    #[must_use]
    pub fn compute(dataset: &PartitionedDataset) -> Self {
        let n = dataset.partitions.len();
        let total_rows = dataset.total_row_count;
        let total_size = dataset
            .partitions
            .iter()
            .filter_map(|p| p.file_size_bytes)
            .sum();
        let avg = if n > 0 {
            total_rows as f64 / n as f64
        } else {
            0.0
        };

        let unique = dataset.unique_values();

        let known_counts: Vec<u64> = dataset
            .partitions
            .iter()
            .filter_map(|p| p.row_count)
            .collect();
        let min_rows = known_counts.iter().copied().reduce(u64::min);
        let max_rows = known_counts.iter().copied().reduce(u64::max);

        Self {
            n_partitions: n,
            total_rows,
            total_size_bytes: total_size,
            avg_rows_per_partition: avg,
            partition_columns: dataset.partition_columns.clone(),
            unique_values: unique,
            min_rows,
            max_rows,
        }
    }

    /// Returns the total size in mebibytes (MiB).
    #[must_use]
    pub fn total_size_mib(&self) -> f64 {
        self.total_size_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Returns a one-line summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} partitions, {} rows, {:.1} MiB, avg {:.0} rows/partition",
            self.n_partitions,
            self.total_rows,
            self.total_size_mib(),
            self.avg_rows_per_partition
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_ds() -> PartitionedDataset {
        PartitionedDataset::new("/data/tiles", vec!["year".into(), "month".into()])
    }

    fn part(year: &str, month: &str, rows: u64, size: u64) -> Partition {
        Partition::new(
            format!("/data/tiles/year={year}/month={month}/part-0.parquet"),
            vec![
                PartitionKey::new("year", year),
                PartitionKey::new("month", month),
            ],
        )
        .with_row_count(rows)
        .with_file_size(size)
    }

    // -- PartitionKey --

    #[test]
    fn test_partition_key_new() {
        let k = PartitionKey::new("year", "2024");
        assert_eq!(k.column, "year");
        assert_eq!(k.value, "2024");
    }

    #[test]
    fn test_partition_key_hive_component_roundtrip() {
        let k = PartitionKey::new("year", "2024");
        let comp = k.to_hive_component();
        assert_eq!(comp, "year=2024");
        let parsed = PartitionKey::from_hive_component(&comp).unwrap();
        assert_eq!(parsed, k);
    }

    #[test]
    fn test_partition_key_from_hive_no_eq() {
        assert!(PartitionKey::from_hive_component("year2024").is_none());
    }

    #[test]
    fn test_partition_key_equality() {
        let a = PartitionKey::new("x", "1");
        let b = PartitionKey::new("x", "1");
        let c = PartitionKey::new("x", "2");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -- Partition --

    #[test]
    fn test_partition_has_key() {
        let p = part("2024", "01", 100, 1024);
        assert!(p.has_key(&PartitionKey::new("year", "2024")));
        assert!(!p.has_key(&PartitionKey::new("year", "2023")));
    }

    #[test]
    fn test_partition_get_value() {
        let p = part("2024", "03", 50, 512);
        assert_eq!(p.get_value("year"), Some("2024"));
        assert_eq!(p.get_value("month"), Some("03"));
        assert!(p.get_value("day").is_none());
    }

    #[test]
    fn test_partition_overlaps_bbox_no_extent() {
        let p = Partition::new("/x", vec![]);
        // No extent → conservative true
        assert!(p.overlaps_bbox(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn test_partition_overlaps_bbox_overlap() {
        let p = Partition::new("/x", vec![]).with_spatial_extent([0.0, 0.0, 10.0, 10.0]);
        assert!(p.overlaps_bbox(5.0, 5.0, 15.0, 15.0));
    }

    #[test]
    fn test_partition_overlaps_bbox_no_overlap() {
        let p = Partition::new("/x", vec![]).with_spatial_extent([0.0, 0.0, 5.0, 5.0]);
        assert!(!p.overlaps_bbox(10.0, 10.0, 20.0, 20.0));
    }

    // -- SpatialPartitionStrategy --

    #[test]
    fn test_strategy_max_cells_grid() {
        let s = SpatialPartitionStrategy::Grid { n_x: 4, n_y: 4 };
        assert_eq!(s.max_cells(), 16);
    }

    #[test]
    fn test_strategy_max_cells_quadtree() {
        let s = SpatialPartitionStrategy::Quadtree { depth: 3 };
        assert_eq!(s.max_cells(), 64);
    }

    #[test]
    fn test_strategy_max_cells_zorder() {
        let s = SpatialPartitionStrategy::ZOrder { bits: 4 };
        assert_eq!(s.max_cells(), 256);
    }

    #[test]
    fn test_strategy_names() {
        assert!(
            !SpatialPartitionStrategy::Grid { n_x: 1, n_y: 1 }
                .name()
                .is_empty()
        );
        assert!(
            !SpatialPartitionStrategy::Quadtree { depth: 1 }
                .name()
                .is_empty()
        );
        assert!(
            !SpatialPartitionStrategy::ZOrder { bits: 1 }
                .name()
                .is_empty()
        );
    }

    // -- PartitionedDataset --

    #[test]
    fn test_add_partition_updates_row_count() {
        let mut ds = make_ds();
        ds.add_partition(part("2024", "01", 100, 1024));
        ds.add_partition(part("2024", "02", 200, 2048));
        assert_eq!(ds.total_row_count, 300);
        assert_eq!(ds.partition_count(), 2);
    }

    #[test]
    fn test_filter_partitions_by_year() {
        let mut ds = make_ds();
        ds.add_partition(part("2024", "01", 100, 1024));
        ds.add_partition(part("2023", "01", 50, 512));
        let filtered = ds.filter_partitions(&[PartitionKey::new("year", "2024")]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].get_value("year"), Some("2024"));
    }

    #[test]
    fn test_filter_partitions_no_match() {
        let mut ds = make_ds();
        ds.add_partition(part("2024", "01", 100, 1024));
        let filtered = ds.filter_partitions(&[PartitionKey::new("year", "2099")]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_hive_path() {
        let ds = make_ds();
        let keys = vec![
            PartitionKey::new("year", "2024"),
            PartitionKey::new("month", "01"),
        ];
        let p = ds.hive_path(&keys);
        assert!(p.ends_with(Path::new("year=2024/month=01")));
    }

    #[test]
    fn test_parse_hive_path() {
        let ds = make_ds();
        let full = PathBuf::from("/data/tiles/year=2024/month=01/part-0.parquet");
        let keys = ds.parse_hive_path(&full);
        // Should find year=2024, month=01 components (not the filename)
        let year_key = keys.iter().find(|k| k.column == "year");
        let month_key = keys.iter().find(|k| k.column == "month");
        assert!(year_key.is_some());
        assert_eq!(year_key.unwrap().value, "2024");
        assert!(month_key.is_some());
        assert_eq!(month_key.unwrap().value, "01");
    }

    #[test]
    fn test_partitions_for_bbox_spatial_filter() {
        let mut ds = make_ds();
        let p1 = Partition::new("/data/a.parquet", vec![])
            .with_spatial_extent([0.0, 0.0, 10.0, 10.0])
            .with_row_count(50);
        let p2 = Partition::new("/data/b.parquet", vec![])
            .with_spatial_extent([20.0, 20.0, 30.0, 30.0])
            .with_row_count(50);
        ds.add_partition(p1);
        ds.add_partition(p2);

        let hits = ds.partitions_for_bbox(5.0, 5.0, 15.0, 15.0);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_unique_values() {
        let mut ds = make_ds();
        ds.add_partition(part("2024", "01", 10, 100));
        ds.add_partition(part("2024", "02", 10, 100));
        ds.add_partition(part("2023", "12", 10, 100));
        let uv = ds.unique_values();
        let years = uv.get("year").expect("year key");
        assert_eq!(years, &vec!["2023".to_string(), "2024".to_string()]);
    }

    // -- PartitionStats --

    #[test]
    fn test_partition_stats_basic() {
        let mut ds = make_ds();
        ds.add_partition(part("2024", "01", 100, 1024));
        ds.add_partition(part("2024", "02", 200, 2048));
        let stats = PartitionStats::compute(&ds);
        assert_eq!(stats.n_partitions, 2);
        assert_eq!(stats.total_rows, 300);
        assert_eq!(stats.total_size_bytes, 3072);
        assert!((stats.avg_rows_per_partition - 150.0).abs() < 1e-6);
    }

    #[test]
    fn test_partition_stats_min_max_rows() {
        let mut ds = make_ds();
        ds.add_partition(part("2024", "01", 10, 100));
        ds.add_partition(part("2024", "02", 500, 2048));
        let stats = PartitionStats::compute(&ds);
        assert_eq!(stats.min_rows, Some(10));
        assert_eq!(stats.max_rows, Some(500));
    }

    #[test]
    fn test_partition_stats_empty_dataset() {
        let ds = make_ds();
        let stats = PartitionStats::compute(&ds);
        assert_eq!(stats.n_partitions, 0);
        assert_eq!(stats.total_rows, 0);
        assert_eq!(stats.avg_rows_per_partition, 0.0);
        assert!(stats.min_rows.is_none());
    }

    #[test]
    fn test_partition_stats_summary_non_empty() {
        let mut ds = make_ds();
        ds.add_partition(part("2024", "01", 100, 1_048_576)); // 1 MiB
        let stats = PartitionStats::compute(&ds);
        let summary = stats.summary();
        assert!(summary.contains("1 partitions"));
        assert!(summary.contains("100 rows"));
    }
}
