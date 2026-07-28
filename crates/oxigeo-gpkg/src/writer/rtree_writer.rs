//! GeoPackage R-tree spatial index shadow-table *writer* — the write-side
//! counterpart of [`crate::rtree`]'s reader.
//!
//! Produces the `gpkg_rtree_index` extension's shadow tables
//! (`rtree_<table>_<geom>_node` / `_rowid` / `_parent`) plus the
//! `sqlite_master` entry for the virtual table itself and the
//! `gpkg_extensions` registration row, all readable by both
//! [`crate::rtree::GpkgRTreeReader`] and a real SQLite `rtree` module.
//!
//! # Scope
//!
//! [`GeoPackageBuilder`](crate::writer::builder::GeoPackageBuilder) only ever
//! writes a feature table whose entire row set fits on a **single** 4096-byte
//! leaf page (`write_table` returns [`GpkgError::RowOverflowsPage`]
//! otherwise). A single R-tree leaf node has a strictly *smaller* per-entry
//! cost (24 bytes/cell) than the smallest possible feature row (record
//! header + rowid + a 29-byte point GPB blob, well over 24 bytes), so any
//! feature table this builder can produce is always small enough for its
//! spatial index to also fit in a **single, depth-0 R-tree node** (the node
//! is simultaneously the root and the only leaf). This module therefore
//! implements exactly that case — a real, spec-conformant R-tree, just
//! without the multi-level bulk-loading a much larger table would need. If
//! the single-node budget is ever exceeded (should be unreachable given the
//! invariant above), [`build_rtree_index`] returns
//! [`GpkgError::RowOverflowsPage`] rather than silently truncating entries.

use crate::error::GpkgError;
use crate::rtree::{CELL_BYTES, NODE_HEADER_BYTES, build_leaf_node_blob};
use crate::sqlite_writer::{
    btree_writer::{MAX_PAYLOAD_INLINE, write_table},
    page_allocator::PageAllocator,
    record::{RecordValue, encode_record},
};

/// DDL for the `gpkg_extensions` system table (OGC 12-128r19 §F.4).
pub const DDL_GPKG_EXTENSIONS: &str = "CREATE TABLE gpkg_extensions (\n\
  table_name TEXT,\n\
  column_name TEXT,\n\
  extension_name TEXT NOT NULL,\n\
  definition TEXT NOT NULL,\n\
  scope TEXT NOT NULL,\n\
  CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name)\n\
)";

/// URI of the `gpkg_rtree_index` extension definition (OGC 12-128r19 Annex F).
pub const RTREE_EXTENSION_DEFINITION: &str = "http://www.geopackage.org/spec/#extension_rtree";

/// One `(rowid, min_x, max_x, min_y, max_y)` entry to be indexed.
pub type RTreeIndexEntry = (i64, f32, f32, f32, f32);

/// Everything needed to emit `sqlite_master` rows for one R-tree index.
pub struct RTreeTableRoots {
    /// Root page of the `_node` shadow table.
    pub node_root: u32,
    /// Root page of the `_rowid` shadow table.
    pub rowid_root: u32,
    /// Root page of the `_parent` shadow table (always empty for a
    /// single-node tree — no non-root node has a parent to record).
    pub parent_root: u32,
}

/// SQLite shadow-table name for the R-tree `_node` table.
pub fn node_table_name(table_name: &str, geom_column: &str) -> String {
    format!("rtree_{table_name}_{geom_column}_node")
}

/// SQLite shadow-table name for the R-tree `_rowid` table.
pub fn rowid_table_name(table_name: &str, geom_column: &str) -> String {
    format!("rtree_{table_name}_{geom_column}_rowid")
}

/// SQLite shadow-table name for the R-tree `_parent` table.
pub fn parent_table_name(table_name: &str, geom_column: &str) -> String {
    format!("rtree_{table_name}_{geom_column}_parent")
}

/// Name of the R-tree virtual table itself (as declared in `sqlite_master`).
pub fn virtual_table_name(table_name: &str, geom_column: &str) -> String {
    format!("rtree_{table_name}_{geom_column}")
}

/// DDL for the virtual table entry in `sqlite_master` (`rootpage = 0`, as SQLite
/// writes for every virtual table — the rtree module manages its own storage
/// via the three shadow tables instead of a conventional btree root page).
pub fn ddl_rtree_virtual_table(table_name: &str, geom_column: &str) -> String {
    let vt = virtual_table_name(table_name, geom_column);
    format!("CREATE VIRTUAL TABLE \"{vt}\" USING rtree(id, minx, maxx, miny, maxy)")
}

/// DDL for the `_node` shadow table.
pub fn ddl_rtree_node_table(table_name: &str, geom_column: &str) -> String {
    let name = node_table_name(table_name, geom_column);
    format!("CREATE TABLE \"{name}\"(nodeno INTEGER PRIMARY KEY, data BLOB)")
}

/// DDL for the `_rowid` shadow table.
pub fn ddl_rtree_rowid_table(table_name: &str, geom_column: &str) -> String {
    let name = rowid_table_name(table_name, geom_column);
    format!("CREATE TABLE \"{name}\"(rowid INTEGER PRIMARY KEY, nodeno INTEGER)")
}

/// DDL for the `_parent` shadow table.
pub fn ddl_rtree_parent_table(table_name: &str, geom_column: &str) -> String {
    let name = parent_table_name(table_name, geom_column);
    format!("CREATE TABLE \"{name}\"(nodeno INTEGER PRIMARY KEY, parentnode INTEGER)")
}

/// Encode a single-node (depth 0 — root and leaf are the same node) R-tree
/// node BLOB for `entries`, matching exactly the wire format documented and
/// parsed by [`crate::rtree`] (delegates the byte layout itself to
/// [`build_leaf_node_blob`], the same encoder the reader's own tests use to
/// build known-good fixtures, so writer and test fixtures can never drift
/// apart).
///
/// # Errors
/// Returns [`GpkgError::RowOverflowsPage`] if the encoded blob would not fit
/// in a single leaf-page row (see the module-level scope note for why this
/// should be unreachable in practice for this builder).
fn encode_single_node_blob(entries: &[RTreeIndexEntry]) -> Result<Vec<u8>, GpkgError> {
    let total = NODE_HEADER_BYTES + entries.len() * CELL_BYTES;
    if total > MAX_PAYLOAD_INLINE {
        return Err(GpkgError::RowOverflowsPage {
            size: total,
            max: MAX_PAYLOAD_INLINE,
        });
    }
    // node_number = 1: this node is both the root (nodeno 1) and the only
    // leaf; depth 0 is written into the header, matching a single-node tree.
    Ok(build_leaf_node_blob(1, entries))
}

/// Allocate and write the three R-tree shadow tables (`_node`, `_rowid`,
/// `_parent`) for one geometry column's spatial index.
///
/// `entries` is one `(rowid, min_x, max_x, min_y, max_y)` tuple per indexed
/// feature.
///
/// # Errors
/// Returns [`GpkgError::RowOverflowsPage`] if the node blob or any shadow
/// table's rows do not fit on a single leaf page (see module scope note).
pub fn build_rtree_index(
    allocator: &mut PageAllocator,
    entries: &[RTreeIndexEntry],
) -> Result<RTreeTableRoots, GpkgError> {
    // ── _node table: single row (nodeno = 1, data = node blob) ────────────
    let node_blob = encode_single_node_blob(entries)?;
    let node_payload = encode_record(&[RecordValue::Int(1), RecordValue::Blob(&node_blob)]);
    let node_root = write_table(allocator, &[(1, node_payload)], 0)?;

    // ── _rowid table: one row per feature (rowid = feature id, nodeno = 1) ─
    let rowid_rows: Vec<(i64, Vec<u8>)> = entries
        .iter()
        .map(|&(id, ..)| {
            let payload = encode_record(&[RecordValue::Int(id), RecordValue::Int(1)]);
            (id, payload)
        })
        .collect();
    let rowid_root = write_table(allocator, &rowid_rows, 0)?;

    // ── _parent table: always empty for a single-node (root-only) tree —
    // real SQLite never records a parent entry for the root itself. ────────
    let parent_root = write_table(allocator, &[], 0)?;

    Ok(RTreeTableRoots {
        node_root,
        rowid_root,
        parent_root,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn encode_single_node_blob_empty() {
        let blob = encode_single_node_blob(&[]).unwrap();
        assert_eq!(blob.len(), NODE_HEADER_BYTES);
        assert_eq!(&blob[0..2], &[0, 0], "depth must be 0");
        assert_eq!(&blob[2..4], &[0, 0], "cell count must be 0");
    }

    #[test]
    fn encode_single_node_blob_round_trips_via_reader_format() {
        let entries: Vec<RTreeIndexEntry> =
            vec![(1, -1.0, -1.0, -1.0, -1.0), (2, 5.0, 5.0, 5.0, 5.0)];
        let blob = encode_single_node_blob(&entries).unwrap();
        assert_eq!(blob.len(), NODE_HEADER_BYTES + 2 * CELL_BYTES);

        // depth = 0
        assert_eq!(u16::from_be_bytes([blob[0], blob[1]]), 0);
        // cell count = 2
        assert_eq!(u16::from_be_bytes([blob[2], blob[3]]), 2);

        // First cell: id=1
        let cell0 = &blob[NODE_HEADER_BYTES..NODE_HEADER_BYTES + CELL_BYTES];
        let id0 = i64::from_be_bytes(cell0[0..8].try_into().unwrap());
        assert_eq!(id0, 1);
        let min_x0 = f32::from_be_bytes(cell0[8..12].try_into().unwrap());
        assert_eq!(min_x0, -1.0);
    }

    #[test]
    fn ddl_names_follow_sqlite_rtree_module_convention() {
        assert_eq!(node_table_name("pts", "geom"), "rtree_pts_geom_node");
        assert_eq!(rowid_table_name("pts", "geom"), "rtree_pts_geom_rowid");
        assert_eq!(parent_table_name("pts", "geom"), "rtree_pts_geom_parent");
        assert_eq!(virtual_table_name("pts", "geom"), "rtree_pts_geom");
    }

    #[test]
    fn build_rtree_index_writes_three_tables() {
        let mut allocator = PageAllocator::new();
        let entries: Vec<RTreeIndexEntry> = vec![(1, 0.0, 0.0, 0.0, 0.0)];
        let roots = build_rtree_index(&mut allocator, &entries).unwrap();
        assert_ne!(roots.node_root, roots.rowid_root);
        assert_ne!(roots.node_root, roots.parent_root);
        assert_ne!(roots.rowid_root, roots.parent_root);
    }
}
