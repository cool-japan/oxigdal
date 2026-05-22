//! GeoPackage R-tree spatial index shadow-table reader.
//!
//! SQLite GeoPackage files use the `gpkg_rtree_index` extension to store a
//! 2-D R-tree index in a set of shadow tables:
//!
//! * `rtree_<table>_<geom>_node`   — raw B-tree pages (node_data BLOB)
//! * `rtree_<table>_<geom>_rowid`  — rowid → node mapping
//! * `rtree_<table>_<geom>_parent` — node → parent mapping
//!
//! This module reads the `_node` shadow table directly using the
//! [`GeoPackage::scan_table_by_name`] B-tree scanner and exposes an in-memory
//! R-tree reader that supports bounding-box intersection queries in O(log n)
//! time for a balanced tree.
//!
//! ## Wire format
//!
//! Each row in the `_node` table is `(nodeno INTEGER, data BLOB)`. The BLOB
//! encodes a SQLite R-tree node in **big-endian** byte order:
//!
//! | offset | size | field |
//! |--------|------|-------|
//! | 0      | 4    | node number (i32 BE) — 1-indexed; node 1 is root |
//! | 4      | 4    | number of cells (i32 BE) |
//! | 8+     | 24×n | cell array |
//!
//! Each 24-byte cell contains:
//!
//! | offset | size | field |
//! |--------|------|-------|
//! | 0      | 4    | min_x (f32 BE) |
//! | 4      | 4    | max_x (f32 BE) |
//! | 8      | 4    | min_y (f32 BE) |
//! | 12     | 4    | max_y (f32 BE) |
//! | 16     | 8    | id (i64 BE) — rowid for leaf cells, child_node for interior |
//!
//! **Leaf vs interior discrimination:** if the 8-byte `id` field is ≤
//! `max_node_id` *and* corresponds to an existing entry in the node map, the
//! cell is treated as an interior node pointer; otherwise it is a feature rowid
//! (leaf entry). This matches the strategy used by SQLite's own R-tree module.
//!
//! Reference: SQLite R-tree module source (`ext/rtree/rtree.c`) and OGC
//! GeoPackage Encoding Standard v1.3.1 Appendix F.

use std::collections::HashMap;

use crate::btree::CellValue;
use crate::error::GpkgError;
use crate::gpkg::GeoPackage;

// ── Byte count of a single cell entry in the node BLOB ─────────────────────

/// Size of a single 2-D R-tree cell inside a node BLOB (bytes).
const CELL_BYTES: usize = 24;

/// Minimum node BLOB size: 4-byte node number + 4-byte cell count.
const NODE_HEADER_BYTES: usize = 8;

// ─────────────────────────────────────────────────────────────────────────────
// Public data structures
// ─────────────────────────────────────────────────────────────────────────────

/// A single entry from the R-tree leaf level.
///
/// Each entry maps a feature rowid to its axis-aligned bounding box (AABB)
/// expressed in the coordinate system of the associated geometry column.
#[derive(Debug, Clone)]
pub struct RTreeEntry {
    /// The rowid of the feature in the user-data table.
    pub rowid: i64,
    /// Western / minimum-X bound of the feature's bounding box.
    pub min_x: f32,
    /// Eastern / maximum-X bound.
    pub max_x: f32,
    /// Southern / minimum-Y bound.
    pub min_y: f32,
    /// Northern / maximum-Y bound.
    pub max_y: f32,
}

impl RTreeEntry {
    /// Return `true` when this entry's bounding box overlaps `[min_x, max_x] ×
    /// [min_y, max_y]`.
    ///
    /// Touching boundaries are considered overlapping (inclusive test).
    #[inline]
    pub fn intersects(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        (self.max_x as f64) >= min_x
            && (self.min_x as f64) <= max_x
            && (self.max_y as f64) >= min_y
            && (self.min_y as f64) <= max_y
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal node-cell types
// ─────────────────────────────────────────────────────────────────────────────

/// A cell from an R-tree interior node.
///
/// Interior cells hold the MBR that bounds *all* entries in the subtree rooted
/// at `child_node`, plus the child node identifier.
#[derive(Debug, Clone)]
struct InteriorCell {
    /// 1-indexed identifier of the child node.
    child_node: i64,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

impl InteriorCell {
    /// Return `true` when this cell's bounding box overlaps the query window.
    #[inline]
    fn intersects(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        (self.max_x as f64) >= min_x
            && (self.min_x as f64) <= max_x
            && (self.max_y as f64) >= min_y
            && (self.min_y as f64) <= max_y
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Parsed node representation
// ─────────────────────────────────────────────────────────────────────────────

/// A fully decoded R-tree node — either an interior node or a leaf node.
#[derive(Debug)]
enum RTreeNode {
    /// Interior node: each cell points to a child node, not a feature rowid.
    Interior(Vec<InteriorCell>),
    /// Leaf node: each cell holds a feature rowid and its bounding box.
    Leaf(Vec<RTreeEntry>),
}

// ─────────────────────────────────────────────────────────────────────────────
// Raw-blob parsing helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the node number and cell count from the 8-byte node BLOB header.
///
/// Returns `(node_number, num_cells)`.
///
/// # Errors
/// Returns [`GpkgError::InvalidFormat`] when the blob is shorter than
/// [`NODE_HEADER_BYTES`].
fn parse_node_header(blob: &[u8]) -> Result<(i32, i32), GpkgError> {
    if blob.len() < NODE_HEADER_BYTES {
        return Err(GpkgError::InvalidFormat(format!(
            "R-tree node blob is only {} bytes; need at least {NODE_HEADER_BYTES}",
            blob.len()
        )));
    }
    // Safety: we just verified blob.len() >= 8, so both slices are valid.
    let node_number = i32::from_be_bytes([blob[0], blob[1], blob[2], blob[3]]);
    let num_cells = i32::from_be_bytes([blob[4], blob[5], blob[6], blob[7]]);
    Ok((node_number, num_cells))
}

/// Extract one raw cell at `offset` within the post-header cell array.
///
/// The cell layout (2-D, 24 bytes) is:
/// `min_x(4) max_x(4) min_y(4) max_y(4) id(8)` — all big-endian.
///
/// Returns `(min_x, max_x, min_y, max_y, id)`, or `None` if `offset +
/// CELL_BYTES > data.len()`.
fn parse_raw_cell(data: &[u8], offset: usize) -> Option<(f32, f32, f32, f32, i64)> {
    if offset + CELL_BYTES > data.len() {
        return None;
    }
    let min_x = f32::from_be_bytes(data[offset..offset + 4].try_into().ok()?);
    let max_x = f32::from_be_bytes(data[offset + 4..offset + 8].try_into().ok()?);
    let min_y = f32::from_be_bytes(data[offset + 8..offset + 12].try_into().ok()?);
    let max_y = f32::from_be_bytes(data[offset + 12..offset + 16].try_into().ok()?);
    let id = i64::from_be_bytes(data[offset + 16..offset + 24].try_into().ok()?);
    Some((min_x, max_x, min_y, max_y, id))
}

/// Decode a full R-tree node BLOB into an [`RTreeNode`].
///
/// `max_node_id` is used to discriminate interior cells (id ≤ max_node_id and
/// present in the node map) from leaf entries (feature rowids).
///
/// `node_ids` is the set of all known node identifiers; used together with
/// `max_node_id` for reliable leaf/interior classification.
fn decode_node(
    blob: &[u8],
    max_node_id: i64,
    node_ids: &HashMap<i64, Vec<u8>>,
) -> Result<RTreeNode, GpkgError> {
    let (_, num_cells) = parse_node_header(blob)?;
    let num_cells = num_cells.max(0) as usize;

    // Cell array begins immediately after the 8-byte header.
    let cell_data = &blob[NODE_HEADER_BYTES..];

    // Classify the node as interior or leaf by probing the first cell's id
    // field.  A node is interior when its cells' id values are valid node
    // identifiers (≤ max_node_id and present in the node map).
    //
    // Edge case: an empty node (num_cells == 0) is treated as a leaf because
    // there is nothing to recurse into.
    if num_cells == 0 {
        return Ok(RTreeNode::Leaf(Vec::new()));
    }

    // Peek at the id of the first cell to decide node type.
    let first_id = {
        let (_, _, _, _, id) = parse_raw_cell(cell_data, 0).ok_or_else(|| {
            GpkgError::InvalidFormat("R-tree node BLOB too short to hold even one cell".into())
        })?;
        id
    };

    let is_interior = first_id > 0 && first_id <= max_node_id && node_ids.contains_key(&first_id);

    if is_interior {
        let mut cells = Vec::with_capacity(num_cells);
        for i in 0..num_cells {
            let Some((min_x, max_x, min_y, max_y, child_node)) =
                parse_raw_cell(cell_data, i * CELL_BYTES)
            else {
                break;
            };
            cells.push(InteriorCell {
                child_node,
                min_x,
                max_x,
                min_y,
                max_y,
            });
        }
        Ok(RTreeNode::Interior(cells))
    } else {
        let mut entries = Vec::with_capacity(num_cells);
        for i in 0..num_cells {
            let Some((min_x, max_x, min_y, max_y, rowid)) =
                parse_raw_cell(cell_data, i * CELL_BYTES)
            else {
                break;
            };
            entries.push(RTreeEntry {
                rowid,
                min_x,
                max_x,
                min_y,
                max_y,
            });
        }
        Ok(RTreeNode::Leaf(entries))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgRTreeReader — main public API
// ─────────────────────────────────────────────────────────────────────────────

/// Reader for a GeoPackage R-tree spatial index stored in the shadow-table
/// `rtree_<table>_<geom>_node`.
///
/// After construction via [`GpkgRTreeReader::open`], the reader holds all node
/// BLOBs in memory and performs bbox intersection queries by walking the R-tree
/// from its root (node 1).
///
/// # Shadow-table schema
///
/// ```sql
/// CREATE VIRTUAL TABLE rtree_features_geom
///     USING rtree(id, minx, maxx, miny, maxy);
/// -- SQLite automatically creates:
/// --   rtree_features_geom_node   (nodeno INTEGER PRIMARY KEY, data BLOB)
/// --   rtree_features_geom_rowid  (rowid INTEGER, nodeno INTEGER, ...)
/// --   rtree_features_geom_parent (nodeno INTEGER, parentno INTEGER, ...)
/// ```
pub struct GpkgRTreeReader {
    /// All R-tree node BLOBs keyed by 1-indexed node id.
    nodes: HashMap<i64, Vec<u8>>,
    /// Largest node id seen when the reader was opened.
    max_node_id: i64,
}

impl GpkgRTreeReader {
    // ── Constructors ──────────────────────────────────────────────────────────

    /// Open the R-tree index for a feature table by scanning the `_node` shadow
    /// table from the GeoPackage.
    ///
    /// `table_name` is the name of the user-data (feature) table.
    /// `geom_column` is the name of the geometry column whose index is needed.
    ///
    /// The shadow table queried is `rtree_<table_name>_<geom_column>_node`.
    ///
    /// # Errors
    /// * [`GpkgError::TableNotFound`] — the shadow table does not exist in the
    ///   GeoPackage (the extension was declared but no index was built).
    /// * Other variants for SQLite B-tree parse errors.
    pub fn open(gpkg: &GeoPackage, table_name: &str, geom_column: &str) -> Result<Self, GpkgError> {
        // SQLite virtual-table shadow names follow the pattern
        // rtree_<name>_node (no double underscores).
        let node_table = format!("rtree_{table_name}_{geom_column}_node");

        let rows = gpkg
            .scan_table_by_name(&node_table)?
            .ok_or_else(|| GpkgError::TableNotFound(node_table.clone()))?;

        Self::build_from_rows(&rows, &node_table)
    }

    /// Construct a reader from a pre-scanned set of `(rowid, columns)` rows as
    /// returned by [`GeoPackage::scan_table_by_name`].
    ///
    /// The expected column layout is `(nodeno INTEGER, data BLOB)`.
    fn build_from_rows(
        rows: &[(i64, Vec<CellValue>)],
        table_name: &str,
    ) -> Result<Self, GpkgError> {
        let mut nodes: HashMap<i64, Vec<u8>> = HashMap::with_capacity(rows.len());
        let mut max_node_id: i64 = 0;

        for (rowid, values) in rows {
            // Row schema: (nodeno INTEGER PRIMARY KEY, data BLOB)
            // The rowid column is the nodeno; values[0] is also nodeno (stored
            // redundantly), values[1] is the data BLOB.
            //
            // Tolerate both layouts:
            //   a) values has 2 columns: [nodeno, data]
            //   b) values has 1 column:  [data]   (rowid serves as nodeno)
            let (node_id, blob) = match values.len() {
                0 => {
                    // Only rowid available — no data.
                    tracing::warn!(
                        table = table_name,
                        rowid = rowid,
                        "R-tree node row has no columns; skipping"
                    );
                    continue;
                }
                1 => {
                    // Single column must be the BLOB; rowid is the node id.
                    let blob = extract_blob(&values[0], table_name, *rowid)?;
                    (*rowid, blob)
                }
                _ => {
                    // Two or more columns: first is nodeno, second is data.
                    let node_id = extract_integer(&values[0], *rowid);
                    let blob = extract_blob(&values[1], table_name, node_id)?;
                    (node_id, blob)
                }
            };

            if blob.len() < NODE_HEADER_BYTES {
                tracing::warn!(
                    table = table_name,
                    node_id = node_id,
                    blob_len = blob.len(),
                    "R-tree node BLOB too short; skipping"
                );
                continue;
            }

            max_node_id = max_node_id.max(node_id);
            nodes.insert(node_id, blob);
        }

        Ok(Self { nodes, max_node_id })
    }

    /// Create a reader directly from a pre-built node map.
    ///
    /// Intended for tests that need to exercise the search logic without a real
    /// SQLite file on disk.  Marked `#[doc(hidden)]` so it does not appear in
    /// the public API reference.
    #[doc(hidden)]
    pub fn for_testing(nodes: HashMap<i64, Vec<u8>>, max_node_id: i64) -> Self {
        Self { nodes, max_node_id }
    }

    // ── Query API ─────────────────────────────────────────────────────────────

    /// Return the rowids of all features whose bounding box intersects the
    /// query window `[min_x, max_x] × [min_y, max_y]`.
    ///
    /// The search is performed by a depth-first traversal of the R-tree starting
    /// from the root (node 1). Interior cells whose MBR does not overlap the
    /// query window are pruned without descending into the subtree.
    ///
    /// Returns an empty `Vec` when the reader has no nodes (e.g. built from an
    /// empty shadow table) or when no features intersect the query window.
    pub fn search(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<i64> {
        let mut results = Vec::new();

        // R-tree root is always node 1.
        if let Some(root_blob) = self.nodes.get(&1) {
            self.search_node(root_blob, min_x, min_y, max_x, max_y, &mut results);
        }

        results
    }

    /// Recursively search a single node blob and append matching rowids to
    /// `results`.
    fn search_node(
        &self,
        blob: &[u8],
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        results: &mut Vec<i64>,
    ) {
        let node = match decode_node(blob, self.max_node_id, &self.nodes) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("Failed to decode R-tree node: {e}");
                return;
            }
        };

        match node {
            RTreeNode::Interior(cells) => {
                for cell in cells {
                    if cell.intersects(min_x, min_y, max_x, max_y) {
                        if let Some(child_blob) = self.nodes.get(&cell.child_node) {
                            self.search_node(child_blob, min_x, min_y, max_x, max_y, results);
                        }
                    }
                }
            }
            RTreeNode::Leaf(entries) => {
                for entry in entries {
                    if entry.intersects(min_x, min_y, max_x, max_y) {
                        results.push(entry.rowid);
                    }
                }
            }
        }
    }

    // ── Informational helpers ─────────────────────────────────────────────────

    /// Return the number of R-tree nodes held in memory.
    ///
    /// For an empty GeoPackage layer or one without an R-tree index this will
    /// be 0.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Return `true` when the reader contains no R-tree nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Return the largest node id seen in the shadow table.
    ///
    /// Returns 0 when the reader is empty.
    pub fn max_node_id(&self) -> i64 {
        self.max_node_id
    }

    /// Iterate over all leaf entries in the R-tree by performing a full tree
    /// walk.
    ///
    /// This is equivalent to `search(f64::NEG_INFINITY, f64::NEG_INFINITY,
    /// f64::INFINITY, f64::INFINITY)` but avoids the bbox-intersection test for
    /// every entry.
    pub fn all_entries(&self) -> Vec<RTreeEntry> {
        let mut entries = Vec::new();
        if let Some(root_blob) = self.nodes.get(&1) {
            self.collect_leaf_entries(root_blob, &mut entries);
        }
        entries
    }

    /// Recursively collect all leaf entries from a subtree rooted at `blob`.
    fn collect_leaf_entries(&self, blob: &[u8], out: &mut Vec<RTreeEntry>) {
        let node = match decode_node(blob, self.max_node_id, &self.nodes) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("Failed to decode R-tree node during full scan: {e}");
                return;
            }
        };
        match node {
            RTreeNode::Interior(cells) => {
                for cell in cells {
                    if let Some(child_blob) = self.nodes.get(&cell.child_node) {
                        self.collect_leaf_entries(child_blob, out);
                    }
                }
            }
            RTreeNode::Leaf(entries) => out.extend(entries),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Cell-value coercion helpers (private)
// ─────────────────────────────────────────────────────────────────────────────

/// Extract an `i64` node id from a `CellValue`, falling back to `rowid_hint`
/// when the value is not an integer.
fn extract_integer(v: &CellValue, rowid_hint: i64) -> i64 {
    match v {
        CellValue::Integer(i) => *i,
        _ => rowid_hint,
    }
}

/// Extract a BLOB payload from a `CellValue`.
///
/// # Errors
/// Returns [`GpkgError::InvalidFormat`] when the cell is not a BLOB.
fn extract_blob(v: &CellValue, table: &str, node_id: i64) -> Result<Vec<u8>, GpkgError> {
    match v {
        CellValue::Blob(b) => Ok(b.clone()),
        other => Err(GpkgError::InvalidFormat(format!(
            "Expected BLOB for node {node_id} in {table}, got {other:?}"
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Builder helper — construct a single-leaf-node BLOB for tests
// ─────────────────────────────────────────────────────────────────────────────

/// Build a raw node BLOB for a **leaf** node from a slice of
/// `(rowid, min_x, max_x, min_y, max_y)` entries.
///
/// The node number embedded in the header is set to `node_number`.
///
/// This is a `#[doc(hidden)]` test-support function; integration tests access
/// it as `oxigdal_gpkg::rtree::build_leaf_node_blob`.
#[doc(hidden)]
pub fn build_leaf_node_blob(node_number: i32, entries: &[(i64, f32, f32, f32, f32)]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(NODE_HEADER_BYTES + entries.len() * CELL_BYTES);
    blob.extend_from_slice(&node_number.to_be_bytes());
    blob.extend_from_slice(&(entries.len() as i32).to_be_bytes());
    for (rowid, min_x, max_x, min_y, max_y) in entries {
        blob.extend_from_slice(&min_x.to_be_bytes());
        blob.extend_from_slice(&max_x.to_be_bytes());
        blob.extend_from_slice(&min_y.to_be_bytes());
        blob.extend_from_slice(&max_y.to_be_bytes());
        blob.extend_from_slice(&rowid.to_be_bytes());
    }
    blob
}

/// Build a raw node BLOB for an **interior** node from a slice of
/// `(child_node_id, min_x, max_x, min_y, max_y)` cells.
///
/// `#[doc(hidden)]` test-support function.
#[doc(hidden)]
pub fn build_interior_node_blob(node_number: i32, cells: &[(i64, f32, f32, f32, f32)]) -> Vec<u8> {
    // Interior cells share the same wire format as leaf cells; the
    // interpretation of the 8-byte id field differs (child node vs rowid).
    build_leaf_node_blob(node_number, cells)
}
