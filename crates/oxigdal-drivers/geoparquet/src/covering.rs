//! GeoParquet 1.1 covering.bbox column detection and row-group statistics extraction.
//!
//! GeoParquet 1.1 defines two layouts for covering bbox columns:
//!
//! * **Struct shape**: a struct column `<geomcol>_bbox` (or `geometry_bbox`) with
//!   subfields `xmin`, `ymin`, `xmax`, `ymax`.
//! * **Flat shape**: four top-level columns named `<geomcol>_bbox_xmin`,
//!   `<geomcol>_bbox_ymin`, `<geomcol>_bbox_xmax`, `<geomcol>_bbox_ymax`.
//!
//! [`BboxColumns`] provides detection and row-group bbox extraction from
//! Parquet column statistics.

use parquet::file::metadata::RowGroupMetaData;
use parquet::file::statistics::Statistics;
use parquet::schema::types::SchemaDescriptor;

/// Indices of the four covering bbox leaf columns within a [`SchemaDescriptor`].
///
/// Indices are **leaf** indices as returned by `SchemaDescriptor::columns()`,
/// matching what `ProjectionMask::leaves` expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BboxColumns {
    /// Leaf index of `xmin` (or equivalent).
    pub xmin_col: usize,
    /// Leaf index of `ymin` (or equivalent).
    pub ymin_col: usize,
    /// Leaf index of `xmax` (or equivalent).
    pub xmax_col: usize,
    /// Leaf index of `ymax` (or equivalent).
    pub ymax_col: usize,
}

impl BboxColumns {
    /// Attempt to detect covering.bbox columns in `schema` for geometry column
    /// `geom_col`.
    ///
    /// Tries the struct shape first, then the flat shape.  Returns `None` if
    /// neither is present.
    ///
    /// # Struct shape
    ///
    /// Looks for four leaf columns whose path matches either:
    /// * `<geomcol>_bbox.xmin` / `.ymin` / `.xmax` / `.ymax`
    /// * `geometry_bbox.xmin` / `.ymin` / `.xmax` / `.ymax` (common default)
    ///
    /// # Flat shape
    ///
    /// Looks for four leaf columns named:
    /// * `<geomcol>_bbox_xmin`, `<geomcol>_bbox_ymin`,
    ///   `<geomcol>_bbox_xmax`, `<geomcol>_bbox_ymax`
    pub fn detect(schema: &SchemaDescriptor, geom_col: &str) -> Option<Self> {
        Self::detect_struct_shape(schema, geom_col)
            .or_else(|| Self::detect_flat_shape(schema, geom_col))
    }

    /// Returns `true` (this struct being present already implies availability,
    /// but the method is provided for convenience / API symmetry).
    pub fn is_available(&self) -> bool {
        true
    }

    /// Extract the covering bbox for a row group from Parquet column statistics.
    ///
    /// Returns `(xmin, ymin, xmax, ymax)` representing the union of all geometry
    /// bounding boxes within that row group, or `None` if statistics are absent
    /// for any of the four columns.
    ///
    /// The returned tuple uses the column statistics' own semantics:
    /// * `xmin` column → we want its **minimum** value across the row group.
    /// * `ymin` column → minimum.
    /// * `xmax` column → we want its **maximum** value across the row group.
    /// * `ymax` column → maximum.
    pub fn row_group_bbox(&self, rg: &RowGroupMetaData) -> Option<(f64, f64, f64, f64)> {
        let xmin = stat_min(rg, self.xmin_col)?;
        let ymin = stat_min(rg, self.ymin_col)?;
        let xmax = stat_max(rg, self.xmax_col)?;
        let ymax = stat_max(rg, self.ymax_col)?;
        Some((xmin, ymin, xmax, ymax))
    }

    // ── private helpers ─────────────────────────────────────────────────────────

    fn detect_struct_shape(schema: &SchemaDescriptor, geom_col: &str) -> Option<Self> {
        let struct_name = format!("{geom_col}_bbox");
        let (mut xmin, mut ymin, mut xmax, mut ymax) = (None, None, None, None);

        for (idx, col) in schema.columns().iter().enumerate() {
            let parts = col.path().parts();
            // We need a two-element path: [struct_name, field_name]
            if parts.len() < 2 {
                continue;
            }
            // Accept both "<geomcol>_bbox" and "geometry_bbox" as the struct root.
            let is_matching_struct =
                parts[0] == struct_name.as_str() || parts[0] == "geometry_bbox";
            if !is_matching_struct {
                continue;
            }
            match parts[1].as_str() {
                "xmin" => xmin = Some(idx),
                "ymin" => ymin = Some(idx),
                "xmax" => xmax = Some(idx),
                "ymax" => ymax = Some(idx),
                _ => {}
            }
        }

        match (xmin, ymin, xmax, ymax) {
            (Some(a), Some(b), Some(c), Some(d)) => Some(Self {
                xmin_col: a,
                ymin_col: b,
                xmax_col: c,
                ymax_col: d,
            }),
            _ => None,
        }
    }

    fn detect_flat_shape(schema: &SchemaDescriptor, geom_col: &str) -> Option<Self> {
        let xmin_name = format!("{geom_col}_bbox_xmin");
        let ymin_name = format!("{geom_col}_bbox_ymin");
        let xmax_name = format!("{geom_col}_bbox_xmax");
        let ymax_name = format!("{geom_col}_bbox_ymax");

        let (mut xmin, mut ymin, mut xmax, mut ymax) = (None, None, None, None);

        for (idx, col) in schema.columns().iter().enumerate() {
            let parts = col.path().parts();
            // Flat columns have exactly one path element.
            if parts.len() != 1 {
                continue;
            }
            let name = parts[0].as_str();
            if name == xmin_name.as_str() {
                xmin = Some(idx);
            } else if name == ymin_name.as_str() {
                ymin = Some(idx);
            } else if name == xmax_name.as_str() {
                xmax = Some(idx);
            } else if name == ymax_name.as_str() {
                ymax = Some(idx);
            }
        }

        match (xmin, ymin, xmax, ymax) {
            (Some(a), Some(b), Some(c), Some(d)) => Some(Self {
                xmin_col: a,
                ymin_col: b,
                xmax_col: c,
                ymax_col: d,
            }),
            _ => None,
        }
    }
}

// ── Statistics helpers ──────────────────────────────────────────────────────────

/// Extracts the minimum f64 value from column statistics for column `col_idx`.
///
/// Supports `Float` (f32) and `Double` (f64) physical types; other types are
/// not valid for bbox columns and return `None`.
fn stat_min(rg: &RowGroupMetaData, col_idx: usize) -> Option<f64> {
    let stats = rg.column(col_idx).statistics()?;
    match stats {
        Statistics::Double(typed) => typed.min_opt().copied(),
        Statistics::Float(typed) => typed.min_opt().copied().map(f64::from),
        _ => None,
    }
}

/// Extracts the maximum f64 value from column statistics for column `col_idx`.
fn stat_max(rg: &RowGroupMetaData, col_idx: usize) -> Option<f64> {
    let stats = rg.column(col_idx).statistics()?;
    match stats {
        Statistics::Double(typed) => typed.max_opt().copied(),
        Statistics::Float(typed) => typed.max_opt().copied().map(f64::from),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::basic::{Repetition, Type as PhysicalType};
    use parquet::schema::types::{SchemaDescriptor, Type};
    use std::sync::Arc;

    fn make_flat_schema(geom_col: &str) -> SchemaDescriptor {
        let xmin_name = format!("{geom_col}_bbox_xmin");
        let ymin_name = format!("{geom_col}_bbox_ymin");
        let xmax_name = format!("{geom_col}_bbox_xmax");
        let ymax_name = format!("{geom_col}_bbox_ymax");

        let schema = Type::group_type_builder("schema")
            .with_fields(vec![
                Arc::new(
                    Type::primitive_type_builder(geom_col, PhysicalType::BYTE_ARRAY)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(
                    Type::primitive_type_builder(&xmin_name, PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(
                    Type::primitive_type_builder(&ymin_name, PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(
                    Type::primitive_type_builder(&xmax_name, PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(
                    Type::primitive_type_builder(&ymax_name, PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
            ])
            .build()
            .expect("schema");

        SchemaDescriptor::new(Arc::new(schema))
    }

    fn make_struct_schema(geom_col: &str) -> SchemaDescriptor {
        let struct_name = format!("{geom_col}_bbox");

        let bbox_struct = Type::group_type_builder(&struct_name)
            .with_repetition(Repetition::OPTIONAL)
            .with_fields(vec![
                Arc::new(
                    Type::primitive_type_builder("xmin", PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(
                    Type::primitive_type_builder("ymin", PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(
                    Type::primitive_type_builder("xmax", PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(
                    Type::primitive_type_builder("ymax", PhysicalType::DOUBLE)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
            ])
            .build()
            .expect("struct");

        let schema = Type::group_type_builder("schema")
            .with_fields(vec![
                Arc::new(
                    Type::primitive_type_builder(geom_col, PhysicalType::BYTE_ARRAY)
                        .with_repetition(Repetition::OPTIONAL)
                        .build()
                        .expect("prim"),
                ),
                Arc::new(bbox_struct),
            ])
            .build()
            .expect("schema");

        SchemaDescriptor::new(Arc::new(schema))
    }

    #[test]
    fn test_detect_flat_shape() {
        let schema = make_flat_schema("geometry");
        let bbox = BboxColumns::detect(&schema, "geometry");
        assert!(bbox.is_some(), "should detect flat bbox columns");
        let bbox = bbox.expect("present");
        assert!(bbox.is_available());
        // geometry is leaf 0; _bbox_xmin=1, _bbox_ymin=2, _bbox_xmax=3, _bbox_ymax=4
        assert_eq!(bbox.xmin_col, 1);
        assert_eq!(bbox.ymin_col, 2);
        assert_eq!(bbox.xmax_col, 3);
        assert_eq!(bbox.ymax_col, 4);
    }

    #[test]
    fn test_detect_struct_shape() {
        let schema = make_struct_schema("geometry");
        let bbox = BboxColumns::detect(&schema, "geometry");
        assert!(bbox.is_some(), "should detect struct bbox columns");
        let bbox = bbox.expect("present");
        // geometry is leaf 0; struct expands to leaves 1,2,3,4
        assert_eq!(bbox.xmin_col, 1);
        assert_eq!(bbox.ymin_col, 2);
        assert_eq!(bbox.xmax_col, 3);
        assert_eq!(bbox.ymax_col, 4);
    }

    #[test]
    fn test_detect_no_bbox_columns() {
        let schema_type = Type::group_type_builder("schema")
            .with_fields(vec![Arc::new(
                Type::primitive_type_builder("geometry", PhysicalType::BYTE_ARRAY)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .expect("prim"),
            )])
            .build()
            .expect("schema");
        let schema = SchemaDescriptor::new(Arc::new(schema_type));
        let bbox = BboxColumns::detect(&schema, "geometry");
        assert!(bbox.is_none(), "no bbox columns should return None");
    }
}
