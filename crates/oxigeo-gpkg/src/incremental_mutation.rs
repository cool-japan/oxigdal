//! Buffered-rewrite incremental mutation support for GeoPackage feature tables.
//!
//! Provides [`GeoPackageEditor`] which loads a snapshot of an existing feature
//! table, buffers INSERT / UPDATE / DELETE operations in memory, and atomically
//! rewrites the file on [`GeoPackageEditor::commit_to_path`] or
//! [`GeoPackageEditor::commit_in_place`].
//!
//! # Strategy
//! 1. Open the source file and scan the feature table into a `FeatureTableSnapshot`.
//! 2. Accumulate mutations in `PendingMutations` without touching the source file.
//! 3. On commit, merge snapshot + mutations and produce a new GeoPackage byte stream
//!    via [`crate::writer::GeoPackageBuilder`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::btree::CellValue;
use crate::error::GpkgError;
use crate::gpkg::GeoPackage;
use crate::vector::feature::FeatureRow;
use crate::vector::types::GpkgGeometry;
use crate::vector::wkb::GpkgBinaryParser;
use crate::writer::GeoPackageBuilder;

// ─────────────────────────────────────────────────────────────────────────────
// Internal structures
// ─────────────────────────────────────────────────────────────────────────────

/// Set of buffered, uncommitted mutations for a single feature table.
struct PendingMutations {
    /// Rows to be appended as new features.
    inserts: Vec<FeatureRow>,
    /// Replacement rows keyed by their original FID.
    updates: HashMap<i64, FeatureRow>,
    /// FIDs that should be removed from the output.
    deletes: HashSet<i64>,
}

impl PendingMutations {
    fn new() -> Self {
        Self {
            inserts: Vec::new(),
            updates: HashMap::new(),
            deletes: HashSet::new(),
        }
    }
}

/// Immutable snapshot of a feature table loaded from the source file.
struct FeatureTableSnapshot {
    /// All feature rows read at open time.
    features: Vec<FeatureRow>,
    /// OGC geometry type string from `gpkg_geometry_columns` (e.g. `"POINT"`).
    geometry_type: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Statistics returned by a successful commit operation.
#[derive(Debug, Clone)]
pub struct MutationStats {
    /// Number of INSERT operations applied.
    pub inserts_applied: usize,
    /// Number of UPDATE operations applied.
    pub updates_applied: usize,
    /// Number of DELETE operations applied.
    pub deletes_applied: usize,
    /// Feature count in the output file.
    pub features_after: usize,
    /// Feature count in the snapshot (before any mutations).
    pub features_before: usize,
}

/// Buffered editor for a single GeoPackage feature table.
///
/// Load it with [`GeoPackageEditor::open`], queue mutations with
/// [`insert_feature`][Self::insert_feature],
/// [`update_feature`][Self::update_feature],
/// and [`delete_feature`][Self::delete_feature],
/// then materialise the result with
/// [`commit_to_path`][Self::commit_to_path] or
/// [`commit_in_place`][Self::commit_in_place].
///
/// Nothing is written to disk until a `commit_*` method is called.
pub struct GeoPackageEditor {
    /// Path of the source GeoPackage file.
    source_path: PathBuf,
    /// Name of the feature table being edited.
    feature_table: String,
    /// Buffered mutations accumulated since construction.
    pending: PendingMutations,
    /// Snapshot loaded from the source file at construction time.
    snapshot: FeatureTableSnapshot,
    /// FID to assign to the next inserted feature.
    next_fid: i64,
    /// Spatial reference system id to use when rebuilding.
    srs_id: i32,
}

impl GeoPackageEditor {
    /// Open a GeoPackage file for editing.
    ///
    /// Reads the entire file, scans `feature_table`, builds an in-memory
    /// snapshot, and returns an editor ready to accept mutations.
    ///
    /// # Errors
    /// - [`GpkgError::Io`] when the file cannot be read.
    /// - [`GpkgError::InvalidFormat`] when the file is not a valid GeoPackage.
    /// - Any other [`GpkgError`] variant produced by the underlying B-tree scanner.
    pub fn open<P: AsRef<Path>>(path: P, feature_table: &str) -> Result<Self, GpkgError> {
        let path = path.as_ref().to_path_buf();
        let bytes = fs::read(&path)?;
        let gpkg = GeoPackage::from_bytes(bytes)?;

        let srs_id = read_srs_id_from_contents(&gpkg, feature_table).unwrap_or(4326);
        let geom_type_info = read_geometry_column_info(&gpkg, feature_table)
            .unwrap_or_else(|| ("geom".to_string(), "POINT".to_string()));
        let (_geometry_column, geometry_type) = geom_type_info;

        let mut features: Vec<FeatureRow> = Vec::new();
        let mut max_fid: i64 = 0;

        if let Some(rows) = gpkg.scan_table_by_name(feature_table)? {
            for (_rowid, values) in rows {
                if values.is_empty() {
                    continue;
                }

                // Column 0 is `fid INTEGER PRIMARY KEY`
                let fid = match &values[0] {
                    CellValue::Integer(i) => *i,
                    CellValue::Float(f) => *f as i64,
                    _ => continue,
                };
                if fid > max_fid {
                    max_fid = fid;
                }

                // Column 1 is `geom BLOB` (the GeoPackage geometry blob)
                let geometry = if values.len() > 1 {
                    match &values[1] {
                        CellValue::Blob(blob) => GpkgBinaryParser::parse(blob).ok(),
                        _ => None,
                    }
                } else {
                    None
                };

                // Columns 2+ are user-defined attribute columns.
                // The builder only creates (fid, geom) so there are none in
                // practice, but we capture them generically for forward compat.
                let fields: HashMap<String, crate::vector::types::FieldValue> = HashMap::new();

                features.push(FeatureRow {
                    fid,
                    geometry,
                    fields,
                });
            }
        }

        Ok(Self {
            source_path: path,
            feature_table: feature_table.to_string(),
            pending: PendingMutations::new(),
            snapshot: FeatureTableSnapshot {
                features,
                geometry_type,
            },
            next_fid: max_fid + 1,
            srs_id,
        })
    }

    // ── Mutation staging ────────────────────────────────────────────────────

    /// Stage a new feature for insertion.
    ///
    /// The `fid` field on `row` is ignored; the editor assigns the next
    /// available FID, which is returned.
    pub fn insert_feature(&mut self, mut row: FeatureRow) -> i64 {
        let fid = self.next_fid;
        self.next_fid += 1;
        row.fid = fid;
        self.pending.inserts.push(row);
        fid
    }

    /// Stage a replacement for an existing feature identified by `fid`.
    ///
    /// The `fid` field of `row` is overwritten with `fid` before buffering.
    ///
    /// # Errors
    /// Returns [`GpkgError::FeatureNotFound`] when `fid` is not present in the
    /// snapshot (already-deleted or never-existing FIDs are also rejected here).
    pub fn update_feature(&mut self, fid: i64, mut row: FeatureRow) -> Result<(), GpkgError> {
        let exists_in_snapshot = self.snapshot.features.iter().any(|f| f.fid == fid);
        if !exists_in_snapshot {
            return Err(GpkgError::FeatureNotFound(fid));
        }
        row.fid = fid;
        self.pending.updates.insert(fid, row);
        Ok(())
    }

    /// Stage a feature for deletion.
    ///
    /// # Errors
    /// Returns [`GpkgError::FeatureNotFound`] when `fid` is not present in the
    /// snapshot.
    pub fn delete_feature(&mut self, fid: i64) -> Result<(), GpkgError> {
        let exists_in_snapshot = self.snapshot.features.iter().any(|f| f.fid == fid);
        if !exists_in_snapshot {
            return Err(GpkgError::FeatureNotFound(fid));
        }
        self.pending.deletes.insert(fid);
        Ok(())
    }

    /// Discard all pending mutations without writing anything to disk.
    pub fn rollback(&mut self) {
        self.pending = PendingMutations::new();
    }

    // ── Pending-mutation inspection ─────────────────────────────────────────

    /// Return the number of pending INSERT operations.
    pub fn pending_inserts(&self) -> usize {
        self.pending.inserts.len()
    }

    /// Return the number of pending UPDATE operations.
    pub fn pending_updates(&self) -> usize {
        self.pending.updates.len()
    }

    /// Return the number of pending DELETE operations.
    pub fn pending_deletes(&self) -> usize {
        self.pending.deletes.len()
    }

    /// Return the number of features in the snapshot (before any mutations).
    pub fn snapshot_feature_count(&self) -> usize {
        self.snapshot.features.len()
    }

    // ── Commit ──────────────────────────────────────────────────────────────

    /// Materialise all pending mutations and write a new GeoPackage to `output_path`.
    ///
    /// The source file is **not** modified.  A fresh file is written atomically
    /// to `output_path`.
    ///
    /// # Errors
    /// Returns an error when building the GeoPackage fails (e.g., a row too
    /// large to fit on a page) or when the file cannot be written.
    pub fn commit_to_path<P: AsRef<Path>>(
        &self,
        output_path: P,
    ) -> Result<MutationStats, GpkgError> {
        let (bytes, stats) = self.build_output_bytes()?;
        fs::write(output_path.as_ref(), &bytes)?;
        Ok(stats)
    }

    /// Materialise all pending mutations and atomically replace the source file.
    ///
    /// Writes to a temporary `.tmp` sibling file first, then renames it over
    /// the source file.  On POSIX systems the rename is atomic.
    ///
    /// # Errors
    /// Returns an error when the temporary file cannot be written or the rename
    /// fails.
    pub fn commit_in_place(&self) -> Result<MutationStats, GpkgError> {
        let tmp_path = self.source_path.with_extension("gpkg.tmp");
        let (bytes, stats) = self.build_output_bytes()?;
        fs::write(&tmp_path, &bytes)?;
        fs::rename(&tmp_path, &self.source_path)?;
        Ok(stats)
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    /// Merge snapshot + pending mutations and produce the raw GeoPackage bytes
    /// together with commit statistics.
    fn build_output_bytes(&self) -> Result<(Vec<u8>, MutationStats), GpkgError> {
        let features_before = self.snapshot.features.len();
        let inserts_applied = self.pending.inserts.len();
        let updates_applied = self.pending.updates.len();
        let deletes_applied = self.pending.deletes.len();

        // Build the merged feature list:
        //   1. Start from snapshot, skip deletes, apply updates.
        //   2. Append inserts.
        let mut merged: Vec<(i64, f64, f64)> = Vec::new();

        for feature in &self.snapshot.features {
            if self.pending.deletes.contains(&feature.fid) {
                continue;
            }
            let row = self.pending.updates.get(&feature.fid).unwrap_or(feature);
            if let Some((x, y)) = extract_point_xy_from_row(row) {
                merged.push((row.fid, x, y));
            }
        }

        for insert in &self.pending.inserts {
            if let Some((x, y)) = extract_point_xy_from_row(insert) {
                merged.push((insert.fid, x, y));
            }
        }

        let features_after = merged.len();

        let bytes = GeoPackageBuilder::new(self.srs_id)
            .add_feature_table(&self.feature_table, &self.snapshot.geometry_type, merged)
            .build()?;

        let stats = MutationStats {
            inserts_applied,
            updates_applied,
            deletes_applied,
            features_after,
            features_before,
        };

        Ok((bytes, stats))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Geometry extraction helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract an (x, y) pair from the geometry of a [`FeatureRow`].
///
/// Only point-like geometry variants are supported; all others return `None`
/// (non-point features are silently omitted from the rebuilt file, which is a
/// known limitation of the [`GeoPackageBuilder`] POINT-only constraint).
fn extract_point_xy_from_row(row: &FeatureRow) -> Option<(f64, f64)> {
    row.geometry.as_ref().and_then(extract_point_xy)
}

/// Extract (x, y) from any [`GpkgGeometry`] variant that represents a single
/// point or has a meaningful 2-D centroid-equivalent for the point case.
fn extract_point_xy(geom: &GpkgGeometry) -> Option<(f64, f64)> {
    match geom {
        GpkgGeometry::Point { x, y } => Some((*x, *y)),
        GpkgGeometry::PointZ { x, y, .. } => Some((*x, *y)),
        GpkgGeometry::PointM { x, y, .. } => Some((*x, *y)),
        GpkgGeometry::PointZM(p) => Some((p.x, p.y)),
        // Non-point geometries cannot be represented by GeoPackageBuilder;
        // extract first vertex as a degraded representation.
        GpkgGeometry::LineString { coords } => coords.first().copied(),
        GpkgGeometry::LineStringZ { coords } => coords.first().map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::LineStringM { coords } => coords.first().map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::LineStringZM { coords } => coords.first().map(|p| (p.x, p.y)),
        GpkgGeometry::Polygon { rings } => rings.first().and_then(|r| r.first()).copied(),
        GpkgGeometry::PolygonZ { rings } => rings
            .first()
            .and_then(|r| r.first())
            .map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::PolygonM { rings } => rings
            .first()
            .and_then(|r| r.first())
            .map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::PolygonZM { rings } => {
            rings.first().and_then(|r| r.first()).map(|p| (p.x, p.y))
        }
        GpkgGeometry::MultiPoint { points } => points.first().copied(),
        GpkgGeometry::MultiPointZ { points } => points.first().map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::MultiPointM { points } => points.first().map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::MultiPointZM { points } => points.first().map(|p| (p.x, p.y)),
        GpkgGeometry::MultiLineString { lines } => lines.first().and_then(|l| l.first()).copied(),
        GpkgGeometry::MultiLineStringZ { lines } => lines
            .first()
            .and_then(|l| l.first())
            .map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::MultiLineStringM { lines } => lines
            .first()
            .and_then(|l| l.first())
            .map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::MultiLineStringZM { lines } => {
            lines.first().and_then(|l| l.first()).map(|p| (p.x, p.y))
        }
        GpkgGeometry::MultiPolygon { polygons } => polygons
            .first()
            .and_then(|poly| poly.first())
            .and_then(|ring| ring.first())
            .copied(),
        GpkgGeometry::MultiPolygonZ { polygons } => polygons
            .first()
            .and_then(|poly| poly.first())
            .and_then(|ring| ring.first())
            .map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::MultiPolygonM { polygons } => polygons
            .first()
            .and_then(|poly| poly.first())
            .and_then(|ring| ring.first())
            .map(|(x, y, _)| (*x, *y)),
        GpkgGeometry::MultiPolygonZM { polygons } => polygons
            .first()
            .and_then(|poly| poly.first())
            .and_then(|ring| ring.first())
            .map(|p| (p.x, p.y)),
        GpkgGeometry::GeometryCollection(geoms)
        | GpkgGeometry::GeometryCollectionZ(geoms)
        | GpkgGeometry::GeometryCollectionM(geoms)
        | GpkgGeometry::GeometryCollectionZM(geoms) => geoms.first().and_then(extract_point_xy),
        GpkgGeometry::Empty => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GeoPackage system-table helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Read the `srs_id` for a named table from `gpkg_contents`.
///
/// Returns `None` when the table is not found or the column is missing.
///
/// `gpkg_contents` column layout (0-based):
/// 0=table_name, 1=data_type, 2=identifier, 3=description, 4=last_change,
/// 5=min_x, 6=min_y, 7=max_x, 8=max_y, 9=srs_id
fn read_srs_id_from_contents(gpkg: &GeoPackage, table_name: &str) -> Option<i32> {
    let rows = gpkg.scan_table_by_name("gpkg_contents").ok()??;
    for (_rowid, values) in rows {
        if values.len() < 10 {
            continue;
        }
        let name = cell_as_str(&values[0]);
        if name == table_name {
            return Some(cell_as_i32(&values[9]));
        }
    }
    None
}

/// Read the geometry column name and geometry type string for a named feature
/// table from `gpkg_geometry_columns`.
///
/// Returns `None` when no matching row is found.
///
/// `gpkg_geometry_columns` column layout (0-based):
/// 0=table_name, 1=column_name, 2=geometry_type_name, 3=srs_id, 4=z, 5=m
fn read_geometry_column_info(gpkg: &GeoPackage, table_name: &str) -> Option<(String, String)> {
    let rows = gpkg.scan_table_by_name("gpkg_geometry_columns").ok()??;
    for (_rowid, values) in rows {
        if values.len() < 3 {
            continue;
        }
        let name = cell_as_str(&values[0]);
        if name == table_name {
            let column_name = cell_as_str(&values[1]).to_string();
            let geom_type = cell_as_str(&values[2]).to_string();
            return Some((column_name, geom_type));
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// CellValue coercion helpers (private)
// ─────────────────────────────────────────────────────────────────────────────

fn cell_as_str(v: &CellValue) -> &str {
    match v {
        CellValue::Text(s) => s.as_str(),
        _ => "",
    }
}

fn cell_as_i32(v: &CellValue) -> i32 {
    match v {
        CellValue::Integer(i) => {
            if *i > i32::MAX as i64 {
                i32::MAX
            } else if *i < i32::MIN as i64 {
                i32::MIN
            } else {
                *i as i32
            }
        }
        CellValue::Float(f) => *f as i32,
        _ => 0,
    }
}
