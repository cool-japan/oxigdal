//! GeoPackage spatial indexing with R-tree.

use super::connection::GpkgConnection;
use crate::error::Result;
use rstar::{AABB, RTree};

/// Spatial index interface.
pub trait SpatialIndex {
    /// Build index for a table.
    fn build(&mut self, conn: &GpkgConnection, table_name: &str) -> Result<()>;

    /// Query features by bounding box.
    fn query(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<i64>;

    /// Insert feature into index.
    fn insert(&mut self, fid: i64, min_x: f64, min_y: f64, max_x: f64, max_y: f64);

    /// Remove feature from index.
    fn remove(&mut self, fid: i64) -> bool;

    /// Clear index.
    fn clear(&mut self);
}

/// R-tree spatial index entry.
#[derive(Debug, Clone, PartialEq)]
struct IndexEntry {
    fid: i64,
    bounds: AABB<[f64; 2]>,
}

impl rstar::RTreeObject for IndexEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.bounds
    }
}

impl rstar::PointDistance for IndexEntry {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        self.bounds.distance_2(point)
    }
}

/// R-tree based spatial index.
pub struct RTreeIndex {
    tree: RTree<IndexEntry>,
}

impl RTreeIndex {
    /// Create new R-tree index.
    pub fn new() -> Self {
        Self { tree: RTree::new() }
    }

    /// Get number of entries.
    pub fn len(&self) -> usize {
        self.tree.size()
    }

    /// Check if index is empty.
    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }
}

impl Default for RTreeIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialIndex for RTreeIndex {
    fn build(&mut self, conn: &GpkgConnection, table_name: &str) -> Result<()> {
        self.clear();

        // Query all features with their bounding boxes
        // This is a simplified version - in practice you'd extract bounds from WKB geometry
        let sql = format!("SELECT fid FROM {} WHERE geom IS NOT NULL", table_name);

        let mut stmt = conn.connection().prepare(&sql)?;
        let _rows = stmt.query_map([], |row| {
            let fid: i64 = row.get(0)?;
            // Placeholder bounds - real implementation would calculate from geometry
            Ok((fid, 0.0, 0.0, 1.0, 1.0))
        })?;

        Ok(())
    }

    fn query(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<i64> {
        let bounds = AABB::from_corners([min_x, min_y], [max_x, max_y]);
        self.tree
            .locate_in_envelope_intersecting(&bounds)
            .map(|entry| entry.fid)
            .collect()
    }

    fn insert(&mut self, fid: i64, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        let bounds = AABB::from_corners([min_x, min_y], [max_x, max_y]);
        self.tree.insert(IndexEntry { fid, bounds });
    }

    fn remove(&mut self, fid: i64) -> bool {
        // Find entry with matching fid
        let entry = self.tree.iter().find(|e| e.fid == fid).cloned();

        // Remove entry if found
        if let Some(entry) = entry {
            self.tree.remove(&entry);
            return true;
        }
        false
    }

    fn clear(&mut self) {
        self.tree = RTree::new();
    }
}

/// Create R-tree extension for GeoPackage.
#[allow(dead_code)]
pub fn create_rtree_extension(conn: &GpkgConnection, table_name: &str) -> Result<()> {
    let rtree_name = format!("rtree_{}_geom", table_name);

    // Create R-tree virtual table
    let create_sql = format!(
        "CREATE VIRTUAL TABLE {} USING rtree(
            id, minx, maxx, miny, maxy
        )",
        rtree_name
    );
    conn.execute_batch(&create_sql)?;

    // Register extension
    conn.execute(
        "INSERT INTO gpkg_extensions (table_name, column_name, extension_name, definition, scope)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        &[
            &table_name as &dyn rusqlite::ToSql,
            &"geom",
            &"gpkg_rtree_index",
            &"GeoPackage 1.0 Specification Annex L",
            &"write-only",
        ],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtree_index_creation() {
        let index = RTreeIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_rtree_insert_query() {
        let mut index = RTreeIndex::new();

        index.insert(1, 0.0, 0.0, 10.0, 10.0);
        index.insert(2, 5.0, 5.0, 15.0, 15.0);
        index.insert(3, 20.0, 20.0, 30.0, 30.0);

        assert_eq!(index.len(), 3);

        let results = index.query(0.0, 0.0, 12.0, 12.0);
        assert!(!results.is_empty());

        let results = index.query(25.0, 25.0, 35.0, 35.0);
        assert!(results.contains(&3));
    }

    #[test]
    fn test_rtree_remove() {
        let mut index = RTreeIndex::new();

        index.insert(1, 0.0, 0.0, 10.0, 10.0);
        index.insert(2, 5.0, 5.0, 15.0, 15.0);

        assert_eq!(index.len(), 2);

        let removed = index.remove(1);
        assert!(removed);
        assert_eq!(index.len(), 1);

        let removed = index.remove(99);
        assert!(!removed);
    }
}
