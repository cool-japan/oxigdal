//! FlatGeoBuf export for GeoPackage feature tables.
//!
//! Provides [`FlatGeoBufExporter`] which converts in-memory [`FeatureTable`]
//! instances into the FlatGeobuf binary format.  All code in this module is
//! compiled only when the `flatgeobuf-export` Cargo feature is enabled.

#![cfg(feature = "flatgeobuf-export")]

use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Seek, Write};
use std::path::Path;

use oxigdal_core::vector::geometry::{
    Coordinate, LineString as CoreLineString, MultiLineString as CoreMultiLineString,
    MultiPoint as CoreMultiPoint, MultiPolygon as CoreMultiPolygon, Point as CorePoint,
    Polygon as CorePolygon,
};
use oxigdal_core::vector::{Feature, FieldValue as CoreFieldValue, Geometry as CoreGeometry};
use oxigdal_flatgeobuf::header::{Column, ColumnType, GeometryType, Header};
use oxigdal_flatgeobuf::writer::FlatGeobufWriter;

use crate::error::GpkgError;
use crate::vector::{FeatureRow, FeatureTable, FieldType, FieldValue, GpkgGeometry};

// ─────────────────────────────────────────────────────────────────────────────
// FlatGeoBufExporter
// ─────────────────────────────────────────────────────────────────────────────

/// Exports GeoPackage [`FeatureTable`] instances to the FlatGeobuf format.
///
/// The exporter works entirely on in-memory [`FeatureTable`] values; no live
/// database connection is required.
///
/// # Example
/// ```ignore
/// use std::io::Cursor;
/// use oxigdal_gpkg::flatgeobuf_export::FlatGeoBufExporter;
///
/// let exporter = FlatGeoBufExporter::new();
/// let mut output = Cursor::new(Vec::new());
/// let count = exporter.export_table(&my_feature_table, &mut output).unwrap();
/// ```
pub struct FlatGeoBufExporter;

impl FlatGeoBufExporter {
    /// Create a new exporter instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Export a single [`FeatureTable`] to a writer.
    ///
    /// Returns the number of feature rows successfully written.
    ///
    /// # Errors
    ///
    /// Returns [`GpkgError::FlatGeoBufExportError`] when the FlatGeobuf writer
    /// encounters an irrecoverable error (e.g. an I/O failure on the underlying
    /// writer).
    pub fn export_table<W: Write + Seek>(
        &self,
        table: &FeatureTable,
        writer: W,
    ) -> Result<usize, GpkgError> {
        // ── Build FlatGeobuf column schema from feature-table schema ──────────
        let fgb_columns: Vec<Column> = table
            .schema
            .iter()
            .map(|fd| Column::new(fd.name.clone(), field_type_to_column_type(fd.field_type)))
            .collect();

        // ── Determine geometry type from first non-empty feature ──────────────
        let (geom_type, has_z, has_m) = detect_geometry_meta(&table.features);

        // ── Assemble FlatGeobuf header ────────────────────────────────────────
        let mut header = Header::new(geom_type);
        header.has_z = has_z;
        header.has_m = has_m;
        header.has_index = false;
        for col in fgb_columns {
            header.add_column(col);
        }

        // ── Create writer ─────────────────────────────────────────────────────
        let mut fgb_writer = FlatGeobufWriter::new(writer, header)
            .map_err(|e| GpkgError::FlatGeoBufExportError(e.to_string()))?;

        // ── Write each feature ────────────────────────────────────────────────
        let mut written = 0usize;
        for row in &table.features {
            let core_feature = gpkg_row_to_core_feature(row);
            fgb_writer
                .add_feature(&core_feature)
                .map_err(|e| GpkgError::FlatGeoBufExportError(e.to_string()))?;
            written += 1;
        }

        // ── Finalise ──────────────────────────────────────────────────────────
        fgb_writer
            .finish()
            .map_err(|e| GpkgError::FlatGeoBufExportError(e.to_string()))?;

        Ok(written)
    }

    /// Export all tables in `tables` into separate `.fgb` files inside
    /// `output_dir`.
    ///
    /// Each file is named `<table_name>.fgb`.  The directory must already exist.
    ///
    /// Returns a map from table name to the number of features written.
    ///
    /// # Errors
    ///
    /// Returns [`GpkgError::FlatGeoBufExportError`] when a file cannot be
    /// created or the FlatGeobuf writer fails for any table.
    pub fn export_tables_to_dir<P: AsRef<Path>>(
        &self,
        tables: &[FeatureTable],
        output_dir: P,
    ) -> Result<HashMap<String, usize>, GpkgError> {
        let dir = output_dir.as_ref();
        let mut results = HashMap::new();

        for table in tables {
            let file_name = format!("{}.fgb", table.name);
            let file_path = dir.join(&file_name);

            let file = fs::File::create(&file_path).map_err(|e| {
                GpkgError::FlatGeoBufExportError(format!(
                    "cannot create {}: {e}",
                    file_path.display()
                ))
            })?;

            let count = self.export_table(table, file)?;
            results.insert(table.name.clone(), count);
        }

        Ok(results)
    }

    /// Export a single [`FeatureTable`] to an in-memory buffer.
    ///
    /// Convenience wrapper around [`export_table`](Self::export_table) for
    /// callers that only need the bytes without managing their own `Cursor`.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`export_table`](Self::export_table).
    pub fn export_table_to_bytes(&self, table: &FeatureTable) -> Result<Vec<u8>, GpkgError> {
        let mut cursor = Cursor::new(Vec::new());
        self.export_table(table, &mut cursor)?;
        Ok(cursor.into_inner())
    }
}

impl Default for FlatGeoBufExporter {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Geometry detection helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Scan the first non-empty geometry in `features` and return
/// `(geometry_type, has_z, has_m)` for the FlatGeobuf header.
///
/// Returns `(GeometryType::Unknown, false, false)` when there are no features
/// or no non-null geometry.
fn detect_geometry_meta(features: &[FeatureRow]) -> (GeometryType, bool, bool) {
    for row in features {
        if let Some(ref geom) = row.geometry {
            let (gt, has_z, has_m) = gpkg_geometry_type(geom);
            return (gt, has_z, has_m);
        }
    }
    (GeometryType::Unknown, false, false)
}

/// Derive a FlatGeobuf [`GeometryType`] and Z/M flags from a [`GpkgGeometry`].
fn gpkg_geometry_type(geom: &GpkgGeometry) -> (GeometryType, bool, bool) {
    match geom {
        GpkgGeometry::Point { .. } => (GeometryType::Point, false, false),
        GpkgGeometry::LineString { .. } => (GeometryType::LineString, false, false),
        GpkgGeometry::Polygon { .. } => (GeometryType::Polygon, false, false),
        GpkgGeometry::MultiPoint { .. } => (GeometryType::MultiPoint, false, false),
        GpkgGeometry::MultiLineString { .. } => (GeometryType::MultiLineString, false, false),
        GpkgGeometry::MultiPolygon { .. } => (GeometryType::MultiPolygon, false, false),
        GpkgGeometry::GeometryCollection(_) => (GeometryType::GeometryCollection, false, false),
        GpkgGeometry::PointZ { .. } => (GeometryType::Point, true, false),
        GpkgGeometry::LineStringZ { .. } => (GeometryType::LineString, true, false),
        GpkgGeometry::PolygonZ { .. } => (GeometryType::Polygon, true, false),
        GpkgGeometry::MultiPointZ { .. } => (GeometryType::MultiPoint, true, false),
        GpkgGeometry::MultiLineStringZ { .. } => (GeometryType::MultiLineString, true, false),
        GpkgGeometry::MultiPolygonZ { .. } => (GeometryType::MultiPolygon, true, false),
        GpkgGeometry::GeometryCollectionZ(_) => (GeometryType::GeometryCollection, true, false),
        GpkgGeometry::PointM { .. } => (GeometryType::Point, false, true),
        GpkgGeometry::LineStringM { .. } => (GeometryType::LineString, false, true),
        GpkgGeometry::PolygonM { .. } => (GeometryType::Polygon, false, true),
        GpkgGeometry::MultiPointM { .. } => (GeometryType::MultiPoint, false, true),
        GpkgGeometry::MultiLineStringM { .. } => (GeometryType::MultiLineString, false, true),
        GpkgGeometry::MultiPolygonM { .. } => (GeometryType::MultiPolygon, false, true),
        GpkgGeometry::GeometryCollectionM(_) => (GeometryType::GeometryCollection, false, true),
        GpkgGeometry::PointZM(_) => (GeometryType::Point, true, true),
        GpkgGeometry::LineStringZM { .. } => (GeometryType::LineString, true, true),
        GpkgGeometry::PolygonZM { .. } => (GeometryType::Polygon, true, true),
        GpkgGeometry::MultiPointZM { .. } => (GeometryType::MultiPoint, true, true),
        GpkgGeometry::MultiLineStringZM { .. } => (GeometryType::MultiLineString, true, true),
        GpkgGeometry::MultiPolygonZM { .. } => (GeometryType::MultiPolygon, true, true),
        GpkgGeometry::GeometryCollectionZM(_) => (GeometryType::GeometryCollection, true, true),
        GpkgGeometry::Empty => (GeometryType::Unknown, false, false),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Field-type mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Map a GeoPackage [`FieldType`] to the corresponding FlatGeobuf [`ColumnType`].
fn field_type_to_column_type(t: FieldType) -> ColumnType {
    match t {
        FieldType::Integer => ColumnType::Long,
        FieldType::Real => ColumnType::Double,
        FieldType::Text => ColumnType::String,
        FieldType::Blob => ColumnType::Binary,
        FieldType::Boolean => ColumnType::Bool,
        FieldType::Date => ColumnType::DateTime,
        FieldType::DateTime => ColumnType::DateTime,
        FieldType::Null => ColumnType::String,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a [`FeatureRow`] to an `oxigdal-core` [`Feature`].
fn gpkg_row_to_core_feature(row: &FeatureRow) -> Feature {
    let geometry = row.geometry.as_ref().and_then(gpkg_geometry_to_core);

    let mut feature = match geometry {
        Some(g) => Feature::new(g),
        None => Feature::new_attribute_only(),
    };

    // Copy all attribute fields
    for (name, val) in &row.fields {
        let core_val = gpkg_value_to_core_field(val);
        feature.set_property(name.clone(), core_val);
    }

    feature
}

/// Convert a [`GpkgGeometry`] to an `oxigdal-core` [`CoreGeometry`].
///
/// Returns `None` for [`GpkgGeometry::Empty`] and unsupported variants.
fn gpkg_geometry_to_core(geom: &GpkgGeometry) -> Option<CoreGeometry> {
    match geom {
        // ── 2-D ──────────────────────────────────────────────────────────────
        GpkgGeometry::Point { x, y } => Some(CoreGeometry::Point(CorePoint::new(*x, *y))),

        GpkgGeometry::LineString { coords } => {
            let core_coords: Vec<Coordinate> = coords
                .iter()
                .map(|(x, y)| Coordinate::new_2d(*x, *y))
                .collect();
            if core_coords.len() < 2 {
                return None;
            }
            CoreLineString::new(core_coords)
                .ok()
                .map(CoreGeometry::LineString)
        }

        GpkgGeometry::Polygon { rings } => build_core_polygon_2d(rings),

        GpkgGeometry::MultiPoint { points } => {
            let core_pts: Vec<CorePoint> =
                points.iter().map(|(x, y)| CorePoint::new(*x, *y)).collect();
            Some(CoreGeometry::MultiPoint(CoreMultiPoint::new(core_pts)))
        }

        GpkgGeometry::MultiLineString { lines } => {
            let mut core_lines = Vec::with_capacity(lines.len());
            for line in lines {
                let coords: Vec<Coordinate> = line
                    .iter()
                    .map(|(x, y)| Coordinate::new_2d(*x, *y))
                    .collect();
                if coords.len() < 2 {
                    continue;
                }
                if let Ok(ls) = CoreLineString::new(coords) {
                    core_lines.push(ls);
                }
            }
            Some(CoreGeometry::MultiLineString(CoreMultiLineString::new(
                core_lines,
            )))
        }

        GpkgGeometry::MultiPolygon { polygons } => {
            let mut core_polys = Vec::with_capacity(polygons.len());
            for rings in polygons {
                if let Some(CoreGeometry::Polygon(p)) = build_core_polygon_2d(rings) {
                    core_polys.push(p);
                }
            }
            Some(CoreGeometry::MultiPolygon(CoreMultiPolygon::new(
                core_polys,
            )))
        }

        // ── Z variants ───────────────────────────────────────────────────────
        GpkgGeometry::PointZ { x, y, z } => {
            Some(CoreGeometry::Point(CorePoint::new_3d(*x, *y, *z)))
        }

        GpkgGeometry::LineStringZ { coords } => {
            let core_coords: Vec<Coordinate> = coords
                .iter()
                .map(|(x, y, z)| Coordinate::new_3d(*x, *y, *z))
                .collect();
            if core_coords.len() < 2 {
                return None;
            }
            CoreLineString::new(core_coords)
                .ok()
                .map(CoreGeometry::LineString)
        }

        GpkgGeometry::PolygonZ { rings } => build_core_polygon_3d(rings),

        GpkgGeometry::MultiPointZ { points } => {
            let core_pts: Vec<CorePoint> = points
                .iter()
                .map(|(x, y, z)| CorePoint::new_3d(*x, *y, *z))
                .collect();
            Some(CoreGeometry::MultiPoint(CoreMultiPoint::new(core_pts)))
        }

        GpkgGeometry::MultiLineStringZ { lines } => {
            let mut core_lines = Vec::with_capacity(lines.len());
            for line in lines {
                let coords: Vec<Coordinate> = line
                    .iter()
                    .map(|(x, y, z)| Coordinate::new_3d(*x, *y, *z))
                    .collect();
                if coords.len() < 2 {
                    continue;
                }
                if let Ok(ls) = CoreLineString::new(coords) {
                    core_lines.push(ls);
                }
            }
            Some(CoreGeometry::MultiLineString(CoreMultiLineString::new(
                core_lines,
            )))
        }

        GpkgGeometry::MultiPolygonZ { polygons } => {
            let mut core_polys = Vec::with_capacity(polygons.len());
            for rings in polygons {
                if let Some(CoreGeometry::Polygon(p)) = build_core_polygon_3d(rings) {
                    core_polys.push(p);
                }
            }
            Some(CoreGeometry::MultiPolygon(CoreMultiPolygon::new(
                core_polys,
            )))
        }

        // ── M variants (M is discarded; GeoJSON / FGB has no M dimension) ────
        GpkgGeometry::PointM { x, y, .. } => Some(CoreGeometry::Point(CorePoint::new(*x, *y))),

        GpkgGeometry::LineStringM { coords } => {
            let core_coords: Vec<Coordinate> = coords
                .iter()
                .map(|(x, y, _m)| Coordinate::new_2d(*x, *y))
                .collect();
            if core_coords.len() < 2 {
                return None;
            }
            CoreLineString::new(core_coords)
                .ok()
                .map(CoreGeometry::LineString)
        }

        GpkgGeometry::PolygonM { rings } => {
            let rings_2d: Vec<Vec<(f64, f64)>> = rings
                .iter()
                .map(|r| r.iter().map(|(x, y, _m)| (*x, *y)).collect())
                .collect();
            build_core_polygon_2d(&rings_2d)
        }

        GpkgGeometry::MultiPointM { points } => {
            let core_pts: Vec<CorePoint> = points
                .iter()
                .map(|(x, y, _m)| CorePoint::new(*x, *y))
                .collect();
            Some(CoreGeometry::MultiPoint(CoreMultiPoint::new(core_pts)))
        }

        GpkgGeometry::MultiLineStringM { lines } => {
            let mut core_lines = Vec::with_capacity(lines.len());
            for line in lines {
                let coords: Vec<Coordinate> = line
                    .iter()
                    .map(|(x, y, _m)| Coordinate::new_2d(*x, *y))
                    .collect();
                if coords.len() < 2 {
                    continue;
                }
                if let Ok(ls) = CoreLineString::new(coords) {
                    core_lines.push(ls);
                }
            }
            Some(CoreGeometry::MultiLineString(CoreMultiLineString::new(
                core_lines,
            )))
        }

        GpkgGeometry::MultiPolygonM { polygons } => {
            let mut core_polys = Vec::with_capacity(polygons.len());
            for rings in polygons {
                let rings_2d: Vec<Vec<(f64, f64)>> = rings
                    .iter()
                    .map(|r| r.iter().map(|(x, y, _m)| (*x, *y)).collect())
                    .collect();
                if let Some(CoreGeometry::Polygon(p)) = build_core_polygon_2d(&rings_2d) {
                    core_polys.push(p);
                }
            }
            Some(CoreGeometry::MultiPolygon(CoreMultiPolygon::new(
                core_polys,
            )))
        }

        // ── ZM variants (M discarded) ─────────────────────────────────────────
        GpkgGeometry::PointZM(p4d) => Some(CoreGeometry::Point(CorePoint::new_3d(
            p4d.x,
            p4d.y,
            p4d.z.unwrap_or(0.0),
        ))),

        GpkgGeometry::LineStringZM { coords } => {
            let core_coords: Vec<Coordinate> = coords
                .iter()
                .map(|p| Coordinate::new_3d(p.x, p.y, p.z.unwrap_or(0.0)))
                .collect();
            if core_coords.len() < 2 {
                return None;
            }
            CoreLineString::new(core_coords)
                .ok()
                .map(CoreGeometry::LineString)
        }

        GpkgGeometry::PolygonZM { rings } => {
            let rings_3d: Vec<Vec<(f64, f64, f64)>> = rings
                .iter()
                .map(|r| r.iter().map(|p| (p.x, p.y, p.z.unwrap_or(0.0))).collect())
                .collect();
            build_core_polygon_3d(&rings_3d)
        }

        GpkgGeometry::MultiPointZM { points } => {
            let core_pts: Vec<CorePoint> = points
                .iter()
                .map(|p| CorePoint::new_3d(p.x, p.y, p.z.unwrap_or(0.0)))
                .collect();
            Some(CoreGeometry::MultiPoint(CoreMultiPoint::new(core_pts)))
        }

        GpkgGeometry::MultiLineStringZM { lines } => {
            let mut core_lines = Vec::with_capacity(lines.len());
            for line in lines {
                let coords: Vec<Coordinate> = line
                    .iter()
                    .map(|p| Coordinate::new_3d(p.x, p.y, p.z.unwrap_or(0.0)))
                    .collect();
                if coords.len() < 2 {
                    continue;
                }
                if let Ok(ls) = CoreLineString::new(coords) {
                    core_lines.push(ls);
                }
            }
            Some(CoreGeometry::MultiLineString(CoreMultiLineString::new(
                core_lines,
            )))
        }

        GpkgGeometry::MultiPolygonZM { polygons } => {
            let mut core_polys = Vec::with_capacity(polygons.len());
            for rings in polygons {
                let rings_3d: Vec<Vec<(f64, f64, f64)>> = rings
                    .iter()
                    .map(|r| r.iter().map(|p| (p.x, p.y, p.z.unwrap_or(0.0))).collect())
                    .collect();
                if let Some(CoreGeometry::Polygon(p)) = build_core_polygon_3d(&rings_3d) {
                    core_polys.push(p);
                }
            }
            Some(CoreGeometry::MultiPolygon(CoreMultiPolygon::new(
                core_polys,
            )))
        }

        // ── GeometryCollection variants: flatten to Unknown ───────────────────
        GpkgGeometry::GeometryCollection(_)
        | GpkgGeometry::GeometryCollectionZ(_)
        | GpkgGeometry::GeometryCollectionM(_)
        | GpkgGeometry::GeometryCollectionZM(_) => {
            // GeometryCollection has no direct FlatGeobuf encoding path in this
            // implementation; emit as absent geometry.
            None
        }

        // ── Empty geometry ────────────────────────────────────────────────────
        GpkgGeometry::Empty => None,
    }
}

/// Build a 2-D [`CoreGeometry::Polygon`] from a ring list of `(x, y)` pairs.
///
/// Returns `None` when the exterior ring has fewer than 4 coordinates or when
/// the ring is not closed (first == last coordinate).
fn build_core_polygon_2d(rings: &[Vec<(f64, f64)>]) -> Option<CoreGeometry> {
    if rings.is_empty() {
        return None;
    }

    let exterior_coords: Vec<Coordinate> = rings[0]
        .iter()
        .map(|(x, y)| Coordinate::new_2d(*x, *y))
        .collect();

    if exterior_coords.len() < 4 {
        return None;
    }

    let exterior = CoreLineString::new(exterior_coords).ok()?;

    let interiors: Vec<CoreLineString> = rings[1..]
        .iter()
        .filter_map(|ring| {
            if ring.len() < 4 {
                return None;
            }
            let coords: Vec<Coordinate> = ring
                .iter()
                .map(|(x, y)| Coordinate::new_2d(*x, *y))
                .collect();
            CoreLineString::new(coords).ok()
        })
        .collect();

    CorePolygon::new(exterior, interiors)
        .ok()
        .map(CoreGeometry::Polygon)
}

/// Build a 3-D [`CoreGeometry::Polygon`] from a ring list of `(x, y, z)` triples.
fn build_core_polygon_3d(rings: &[Vec<(f64, f64, f64)>]) -> Option<CoreGeometry> {
    if rings.is_empty() {
        return None;
    }

    let exterior_coords: Vec<Coordinate> = rings[0]
        .iter()
        .map(|(x, y, z)| Coordinate::new_3d(*x, *y, *z))
        .collect();

    if exterior_coords.len() < 4 {
        return None;
    }

    let exterior = CoreLineString::new(exterior_coords).ok()?;

    let interiors: Vec<CoreLineString> = rings[1..]
        .iter()
        .filter_map(|ring| {
            if ring.len() < 4 {
                return None;
            }
            let coords: Vec<Coordinate> = ring
                .iter()
                .map(|(x, y, z)| Coordinate::new_3d(*x, *y, *z))
                .collect();
            CoreLineString::new(coords).ok()
        })
        .collect();

    CorePolygon::new(exterior, interiors)
        .ok()
        .map(CoreGeometry::Polygon)
}

// ─────────────────────────────────────────────────────────────────────────────
// Field-value conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a GeoPackage [`FieldValue`] to an `oxigdal-core` [`CoreFieldValue`].
fn gpkg_value_to_core_field(val: &FieldValue) -> CoreFieldValue {
    match val {
        FieldValue::Integer(i) => CoreFieldValue::Integer(*i),
        FieldValue::Real(f) => CoreFieldValue::Float(*f),
        FieldValue::Text(s) => CoreFieldValue::String(s.clone()),
        FieldValue::Blob(b) => CoreFieldValue::Blob(b.clone()),
        FieldValue::Boolean(b) => CoreFieldValue::Bool(*b),
        FieldValue::Null => CoreFieldValue::Null,
    }
}
