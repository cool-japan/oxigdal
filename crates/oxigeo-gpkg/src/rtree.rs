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
//! encodes a SQLite R-tree node in **big-endian** byte order. Note that the
//! node's own identifier is *not* stored inside the BLOB — it is the `nodeno`
//! column value, tracked separately by the caller (see [`GpkgRTreeReader`]'s
//! `nodes: HashMap<i64, Vec<u8>>`):
//!
//! | offset | size | field |
//! |--------|------|-------|
//! | 0      | 2    | root only: tree depth (u16 BE); other nodes: unused |
//! | 2      | 2    | number of cells (u16 BE) |
//! | 4+     | 24×n | cell array |
//!
//! Each 24-byte cell contains:
//!
//! | offset | size | field |
//! |--------|------|-------|
//! | 0      | 8    | id (i64 BE) — rowid for leaf cells, child_node for interior |
//! | 8      | 4    | min_x (f32 BE) |
//! | 12     | 4    | max_x (f32 BE) |
//! | 16     | 4    | min_y (f32 BE) |
//! | 20     | 4    | max_y (f32 BE) |
//!
//! **Leaf vs interior discrimination:** SQLite's R-tree does *not* mark
//! individual nodes as leaf/interior in their own bytes; instead the
//! **root** node's header (`nodeno == 1`) stores the tree's total depth as a
//! big-endian `u16` at offset 0 (`iDepth` in SQLite's own source). A
//! traversal starts at the root with `depth = iDepth`; a node at `depth ==
//! 0` is a leaf (cells are feature rowids), otherwise it is interior (cells
//! are child node ids) and each child is visited with `depth - 1`. This
//! guarantees termination in bounded recursion depth even against corrupt
//! or adversarial files, unlike a heuristic that peeks at cell id values
//! (which can misclassify a leaf node as interior whenever a feature rowid
//! happens to collide with a live node id — for example the extremely
//! common case of a single-leaf tree whose first feature has rowid 1 and
//! whose root is also node 1, previously causing unbounded recursion here).
//!
//! Reference: SQLite R-tree module source (`ext/rtree/rtree.c`, function
//! `nodeAcquire`/`rtreeDepth`), verified empirically against `rt_node.data`
//! BLOBs produced by SQLite 3.51.0 for both a single-leaf
//! `CREATE VIRTUAL TABLE rt USING rtree(id, minx, maxx, miny, maxy)` table
//! and a 2 000-row table that forces a 3-level tree (root header `0002 0002`
//! — depth 2, 2 cells — while every non-root node's first 2 bytes are
//! `0000`), and OGC GeoPackage Encoding Standard v1.3.1 Appendix F.

use std::collections::HashMap;

use crate::btree::CellValue;
use crate::error::GpkgError;
use crate::gpkg::GeoPackage;

// ── Byte count of a single cell entry in the node BLOB ─────────────────────

/// Size of a single 2-D R-tree cell inside a node BLOB (bytes).
///
/// `pub(crate)` so [`crate::writer::rtree_writer`] (the writer-side
/// counterpart of this reader) can encode node BLOBs using the exact same
/// constant rather than duplicating the magic number.
pub(crate) const CELL_BYTES: usize = 24;

/// Minimum node BLOB size: 2 reserved bytes + 2-byte (u16 BE) cell count.
///
/// SQLite's R-tree node BLOB does *not* embed the node's own identifier —
/// only a 2-byte reserved field (unused by this reader) followed by the
/// cell count. See the module-level "Wire format" section for the full
/// layout, verified against real SQLite-generated `_node` shadow tables.
///
/// `pub(crate)` — see [`CELL_BYTES`].
pub(crate) const NODE_HEADER_BYTES: usize = 4;

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

/// Extract the cell count from the 4-byte node BLOB header.
///
/// The first 2 bytes are a reserved field (unused; not the node's own
/// identifier — that comes from the `nodeno` column, tracked separately by
/// the caller) and are discarded. Returns `num_cells` as read from the
/// big-endian `u16` at offset 2.
///
/// # Errors
/// Returns [`GpkgError::InvalidFormat`] when the blob is shorter than
/// [`NODE_HEADER_BYTES`].
fn parse_node_header(blob: &[u8]) -> Result<u16, GpkgError> {
    if blob.len() < NODE_HEADER_BYTES {
        return Err(GpkgError::InvalidFormat(format!(
            "R-tree node blob is only {} bytes; need at least {NODE_HEADER_BYTES}",
            blob.len()
        )));
    }
    // Safety: we just verified blob.len() >= 4, so the slice is valid.
    // Bytes 0-1 are a reserved field and are intentionally discarded.
    let num_cells = u16::from_be_bytes([blob[2], blob[3]]);
    Ok(num_cells)
}

/// Extract one raw cell at `offset` within the post-header cell array.
///
/// The cell layout (2-D, 24 bytes) is:
/// `id(8) min_x(4) max_x(4) min_y(4) max_y(4)` — all big-endian.
///
/// Returns `(min_x, max_x, min_y, max_y, id)`, or `None` if `offset +
/// CELL_BYTES > data.len()`.
fn parse_raw_cell(data: &[u8], offset: usize) -> Option<(f32, f32, f32, f32, i64)> {
    if offset + CELL_BYTES > data.len() {
        return None;
    }
    let id = i64::from_be_bytes(data[offset..offset + 8].try_into().ok()?);
    let min_x = f32::from_be_bytes(data[offset + 8..offset + 12].try_into().ok()?);
    let max_x = f32::from_be_bytes(data[offset + 12..offset + 16].try_into().ok()?);
    let min_y = f32::from_be_bytes(data[offset + 16..offset + 20].try_into().ok()?);
    let max_y = f32::from_be_bytes(data[offset + 20..offset + 24].try_into().ok()?);
    Some((min_x, max_x, min_y, max_y, id))
}

/// Read the tree-depth field (`iDepth`) from a **root** node's header.
///
/// Only meaningful for the root node (`nodeno == 1`); the corresponding
/// bytes in every other node are unused. Returns `0` (leaf-only tree) when
/// the blob is too short to contain the field, which cannot happen for any
/// blob that has already passed the [`NODE_HEADER_BYTES`] length check
/// performed before a blob is stored in [`GpkgRTreeReader::nodes`], but the
/// `#[doc(hidden)]` `for_testing` constructor accepts arbitrary blobs so this
/// stays a safe fallback rather than a panic.
fn read_root_depth(blob: &[u8]) -> u16 {
    match blob.first_chunk::<2>() {
        Some(bytes) => u16::from_be_bytes(*bytes),
        None => 0,
    }
}

/// Decode a full R-tree node BLOB into an [`RTreeNode`].
///
/// `depth` is the node's distance from the leaf level: `0` means this node
/// **is** a leaf (cells are feature rowids); any other value means this node
/// is interior (cells are child node ids), and each child must be decoded
/// with `depth - 1`. The caller starts the traversal at the root with
/// `depth` equal to the root's declared `iDepth` (see [`read_root_depth`]).
fn decode_node(blob: &[u8], depth: u16) -> Result<RTreeNode, GpkgError> {
    let num_cells = parse_node_header(blob)? as usize;

    // Cell array begins immediately after the 4-byte header.
    let cell_data = &blob[NODE_HEADER_BYTES..];

    if depth == 0 {
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
    } else {
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
    /// Tree depth (`iDepth`) read from the root node's header (node 1).
    ///
    /// `0` when the tree has a single, leaf-only root, or when no root node
    /// (`nodeno == 1`) is present at all (in which case [`Self::search`] and
    /// [`Self::all_entries`] short-circuit to empty results regardless).
    root_depth: u16,
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

        let root_depth = nodes.get(&1).map(|blob| read_root_depth(blob)).unwrap_or(0);
        Ok(Self {
            nodes,
            max_node_id,
            root_depth,
        })
    }

    /// Create a reader directly from a pre-built node map.
    ///
    /// Intended for tests that need to exercise the search logic without a real
    /// SQLite file on disk.  Marked `#[doc(hidden)]` so it does not appear in
    /// the public API reference.
    ///
    /// The tree depth used to discriminate leaf vs. interior nodes during
    /// traversal is read from the root node's (`nodeno == 1`) header, exactly
    /// as it would be for a real SQLite-produced shadow table; there is no
    /// separate depth parameter to keep this constructor's signature stable.
    #[doc(hidden)]
    pub fn for_testing(nodes: HashMap<i64, Vec<u8>>, max_node_id: i64) -> Self {
        let root_depth = nodes.get(&1).map(|blob| read_root_depth(blob)).unwrap_or(0);
        Self {
            nodes,
            max_node_id,
            root_depth,
        }
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
            self.search_node(
                root_blob,
                self.root_depth,
                min_x,
                min_y,
                max_x,
                max_y,
                &mut results,
            );
        }

        results
    }

    /// Recursively search a single node blob and append matching rowids to
    /// `results`.
    ///
    /// `depth` is this node's distance from the leaf level (0 = leaf); it is
    /// decremented by exactly 1 on every recursive descent into a child, so
    /// recursion is bounded by the root's declared tree depth regardless of
    /// the blob's cell contents — this cannot loop even on a corrupt or
    /// adversarial file.
    #[allow(clippy::too_many_arguments)]
    fn search_node(
        &self,
        blob: &[u8],
        depth: u16,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        results: &mut Vec<i64>,
    ) {
        let node = match decode_node(blob, depth) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("Failed to decode R-tree node: {e}");
                return;
            }
        };

        match node {
            RTreeNode::Interior(cells) => {
                for cell in cells {
                    if cell.intersects(min_x, min_y, max_x, max_y)
                        && let Some(child_blob) = self.nodes.get(&cell.child_node)
                    {
                        self.search_node(
                            child_blob,
                            depth.saturating_sub(1),
                            min_x,
                            min_y,
                            max_x,
                            max_y,
                            results,
                        );
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
            self.collect_leaf_entries(root_blob, self.root_depth, &mut entries);
        }
        entries
    }

    /// Recursively collect all leaf entries from a subtree rooted at `blob`.
    ///
    /// `depth` is this node's distance from the leaf level (0 = leaf); see
    /// [`Self::search_node`] for why decrementing it on every descent bounds
    /// recursion even against a corrupt file.
    fn collect_leaf_entries(&self, blob: &[u8], depth: u16, out: &mut Vec<RTreeEntry>) {
        let node = match decode_node(blob, depth) {
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
                        self.collect_leaf_entries(child_blob, depth.saturating_sub(1), out);
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
/// `node_number` is accepted for API/call-site compatibility (callers
/// typically also use it as the key under which the returned blob is
/// inserted into the node map) but, matching the real SQLite wire format,
/// it is **not** embedded in the blob itself.
///
/// The header's first 2 bytes are written as `0` (tree depth 0), which is
/// only meaningful when the returned blob is inserted as the **root** node
/// (`nodeno == 1`) — see [`read_root_depth`]. A leaf node used anywhere else
/// in the tree ignores this field entirely, matching real SQLite behaviour.
///
/// This is a `#[doc(hidden)]` test-support function; integration tests access
/// it as `oxigeo_gpkg::rtree::build_leaf_node_blob`.
#[doc(hidden)]
pub fn build_leaf_node_blob(node_number: i32, entries: &[(i64, f32, f32, f32, f32)]) -> Vec<u8> {
    let _ = node_number; // not part of the wire format; kept for call-site compatibility
    let mut blob = Vec::with_capacity(NODE_HEADER_BYTES + entries.len() * CELL_BYTES);
    blob.extend_from_slice(&0u16.to_be_bytes()); // depth 0 (only meaningful as root)
    blob.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    write_node_cells(&mut blob, entries);
    blob
}

/// Build a raw node BLOB for an **interior** node from a slice of
/// `(child_node_id, min_x, max_x, min_y, max_y)` cells.
///
/// `depth` is written into the header's first 2 bytes and is only consulted
/// by [`decode_node`] when this blob is the **root** node (`nodeno == 1`);
/// pass the tree's total height (number of edges from root to a leaf) when
/// building a root, or any value when building a non-root interior node
/// (real SQLite leaves that field unused for non-root nodes too).
///
/// `#[doc(hidden)]` test-support function.
#[doc(hidden)]
pub fn build_interior_node_blob(
    node_number: i32,
    depth: u16,
    cells: &[(i64, f32, f32, f32, f32)],
) -> Vec<u8> {
    let _ = node_number; // not part of the wire format; kept for call-site compatibility
    let mut blob = Vec::with_capacity(NODE_HEADER_BYTES + cells.len() * CELL_BYTES);
    blob.extend_from_slice(&depth.to_be_bytes());
    blob.extend_from_slice(&(cells.len() as u16).to_be_bytes());
    write_node_cells(&mut blob, cells);
    blob
}

/// Append `cells` (each `(id, min_x, max_x, min_y, max_y)`) to `blob` in the
/// real SQLite wire-format cell layout (id first, then the four bounds).
/// Shared by [`build_leaf_node_blob`] and [`build_interior_node_blob`], whose
/// cell encoding is identical — only the caller's interpretation of `id`
/// (rowid vs. child node) differs.
fn write_node_cells(blob: &mut Vec<u8>, cells: &[(i64, f32, f32, f32, f32)]) {
    for (id, min_x, max_x, min_y, max_y) in cells {
        blob.extend_from_slice(&id.to_be_bytes());
        blob.extend_from_slice(&min_x.to_be_bytes());
        blob.extend_from_slice(&max_x.to_be_bytes());
        blob.extend_from_slice(&min_y.to_be_bytes());
        blob.extend_from_slice(&max_y.to_be_bytes());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Regression tests — real SQLite-generated wire format
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The leading 76 bytes of an `rt_node.data` BLOB captured with `sqlite3
    /// 3.51.0` from a real R-tree shadow table:
    ///
    /// ```sql
    /// CREATE VIRTUAL TABLE rt USING rtree(id, minx, maxx, miny, maxy);
    /// INSERT INTO rt VALUES (1, 0.0, 1.0, 0.0, 1.0);
    /// INSERT INTO rt VALUES (2, 2.0, 3.0, 2.0, 3.0);
    /// INSERT INTO rt VALUES (3, 4.0, 5.0, 4.0, 5.0);
    /// SELECT hex(data) FROM rt_node;
    /// ```
    ///
    /// The real page is padded with trailing zero bytes to the fixed node
    /// size; only the header + 3 cells (4 + 3×24 = 76 bytes) are meaningful
    /// and reproduced here, since `decode_node` only reads `num_cells` cells
    /// past the header and ignores anything after.
    ///
    /// This fixture exists specifically because the crate's own
    /// `build_leaf_node_blob`/`build_interior_node_blob` test builders
    /// previously encoded the *wrong* wire format (an 8-byte header with a
    /// fabricated node-number field, and coords-then-id cell order), which
    /// made the rest of the test suite blind to the mismatch against real
    /// GeoPackage/SQLite files. See the module-level "Wire format" docs.
    #[rustfmt::skip]
    const REAL_SQLITE_RTREE_NODE_BLOB: [u8; 76] = [
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x3F, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x3F, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        0x40, 0x00, 0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
        0x40, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03,
        0x40, 0x80, 0x00, 0x00, 0x40, 0xA0, 0x00, 0x00, 0x40, 0x80, 0x00, 0x00,
        0x40, 0xA0, 0x00, 0x00,
    ];

    #[test]
    fn parse_node_header_reads_real_sqlite_cell_count() {
        // Real SQLite blobs use a 4-byte header: 2 reserved bytes then a
        // big-endian u16 cell count — NOT an 8-byte header with a fabricated
        // i32 node-number field.
        let num_cells = parse_node_header(&REAL_SQLITE_RTREE_NODE_BLOB).expect("header must parse");
        assert_eq!(num_cells, 3, "real fixture has exactly 3 inserted rows");
        assert_eq!(NODE_HEADER_BYTES, 4);
    }

    #[test]
    fn parse_raw_cell_reads_real_sqlite_id_first_layout() {
        let cell_data = &REAL_SQLITE_RTREE_NODE_BLOB[NODE_HEADER_BYTES..];

        let (min_x, max_x, min_y, max_y, id) =
            parse_raw_cell(cell_data, 0).expect("cell 0 must parse");
        assert_eq!(id, 1, "id field must be read FIRST, not last");
        assert_eq!((min_x, max_x, min_y, max_y), (0.0, 1.0, 0.0, 1.0));

        let (min_x, max_x, min_y, max_y, id) =
            parse_raw_cell(cell_data, CELL_BYTES).expect("cell 1 must parse");
        assert_eq!(id, 2);
        assert_eq!((min_x, max_x, min_y, max_y), (2.0, 3.0, 2.0, 3.0));

        let (min_x, max_x, min_y, max_y, id) =
            parse_raw_cell(cell_data, 2 * CELL_BYTES).expect("cell 2 must parse");
        assert_eq!(id, 3);
        assert_eq!((min_x, max_x, min_y, max_y), (4.0, 5.0, 4.0, 5.0));
    }

    #[test]
    fn gpkg_rtree_reader_decodes_real_sqlite_node_blob_correctly() {
        let mut nodes = HashMap::new();
        nodes.insert(1i64, REAL_SQLITE_RTREE_NODE_BLOB.to_vec());
        let reader = GpkgRTreeReader::for_testing(nodes, 1);

        assert_eq!(reader.len(), 1);

        // A query window covering all three rows must return all three
        // rowids (order-independent).
        let mut results = reader.search(-10.0, -10.0, 10.0, 10.0);
        results.sort_unstable();
        assert_eq!(results, vec![1, 2, 3]);

        // A tight window around row 2's bbox must return only row 2.
        let results = reader.search(2.0, 2.0, 3.0, 3.0);
        assert_eq!(results, vec![2]);

        // A window disjoint from every bbox must return nothing.
        let results = reader.search(100.0, 100.0, 200.0, 200.0);
        assert!(results.is_empty());

        // Full-scan API must also see all three entries with correct bboxes.
        let mut entries = reader.all_entries();
        entries.sort_by_key(|e| e.rowid);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].rowid, 1);
        assert_eq!((entries[0].min_x, entries[0].max_x), (0.0, 1.0));
        assert_eq!(entries[1].rowid, 2);
        assert_eq!((entries[1].min_x, entries[1].max_x), (2.0, 3.0));
        assert_eq!(entries[2].rowid, 3);
        assert_eq!((entries[2].min_x, entries[2].max_x), (4.0, 5.0));
    }

    #[test]
    fn parse_node_header_rejects_short_blob() {
        let err = parse_node_header(&[0u8, 0u8, 0u8]).expect_err("3 bytes is too short");
        match err {
            GpkgError::InvalidFormat(msg) => assert!(msg.contains("only 3 bytes")),
            other => panic!("expected InvalidFormat, got {other:?}"),
        }
    }
}
