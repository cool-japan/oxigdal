//! Tiled Gridded Coverage support for GeoPackage (OGC GeoPackage Extension §F.7).
//!
//! Implements reading and decoding of the two extension tables:
//! - `gpkg_2d_gridded_coverage_ancillary` — per-table coverage metadata
//! - `gpkg_2d_gridded_tile_ancillary` — per-tile scale/offset/statistics
//!
//! These tables are present only when the
//! `gpkg_2d_gridded_coverage` extension is in use (elevation/DEM data).  All
//! functions return `Ok(vec![])` or `Ok(…)` gracefully when the optional tables
//! are absent.

use std::str::FromStr;

use crate::error::GpkgError;
use crate::gpkg::{GeoPackage, cell_to_i64};

// ─────────────────────────────────────────────────────────────────────────────
// CoverageDatatype
// ─────────────────────────────────────────────────────────────────────────────

/// The data type stored in each tile of a gridded coverage.
///
/// Corresponds to the `datatype` column of `gpkg_2d_gridded_coverage_ancillary`.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverageDatatype {
    /// 16-bit (or wider) integer samples — typical for elevation data.
    Integer,
    /// 32-bit IEEE-754 float samples.
    Float,
}

impl CoverageDatatype {
    /// Return the canonical string stored in `gpkg_2d_gridded_coverage_ancillary`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
        }
    }
}

impl FromStr for CoverageDatatype {
    type Err = GpkgError;

    /// Parse the `datatype` column string from `gpkg_2d_gridded_coverage_ancillary`.
    ///
    /// # Errors
    /// Returns [`GpkgError::InvalidCoverageDatatype`] for any unrecognised string.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "integer" => Ok(Self::Integer),
            "float" => Ok(Self::Float),
            other => Err(GpkgError::InvalidCoverageDatatype(other.to_string())),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GridCellEncoding
// ─────────────────────────────────────────────────────────────────────────────

/// Interpretation of how each grid sample relates to the cell boundary.
///
/// Corresponds to the `grid_cell_encoding` column of
/// `gpkg_2d_gridded_coverage_ancillary` (OGC §F.7).
#[derive(Debug, Clone, PartialEq)]
pub enum GridCellEncoding {
    /// `"grid-value-is-center"` — the value represents the cell centre.
    Grid,
    /// `"grid-value-is-area"` — the value is an average over the entire cell.
    PixelIsArea,
    /// `"grid-value-is-corner"` — the value is located at the cell corner.
    PixelIsPoint,
}

impl GridCellEncoding {
    /// Return the canonical encoding string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Grid => "grid-value-is-center",
            Self::PixelIsArea => "grid-value-is-area",
            Self::PixelIsPoint => "grid-value-is-corner",
        }
    }
}

impl FromStr for GridCellEncoding {
    /// The parse is infallible: unrecognised strings map to the default
    /// ([`GridCellEncoding::Grid`]) per OGC §F.7.
    type Err = std::convert::Infallible;

    /// Parse the `grid_cell_encoding` column string.
    ///
    /// Per OGC §F.7 the encoding is lenient: any unrecognised string maps to
    /// [`GridCellEncoding::Grid`] (the most common default).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "grid-value-is-center" => Self::Grid,
            "grid-value-is-area" => Self::PixelIsArea,
            "grid-value-is-corner" => Self::PixelIsPoint,
            // Lenient: unknown strings default to Grid per §F.7 note.
            _ => Self::Grid,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GriddedCoverage
// ─────────────────────────────────────────────────────────────────────────────

/// A parsed row from `gpkg_2d_gridded_coverage_ancillary`.
///
/// Column layout (0-indexed):
///
/// | # | Column                 | SQLite type |
/// |---|------------------------|-------------|
/// | 0 | `id`                   | INTEGER PK  |
/// | 1 | `tile_matrix_set_name` | TEXT        |
/// | 2 | `datatype`             | TEXT        |
/// | 3 | `scale`                | REAL        |
/// | 4 | `offset`               | REAL        |
/// | 5 | `precision`            | REAL        |
/// | 6 | `data_null`            | REAL NULL   |
/// | 7 | `grid_cell_encoding`   | TEXT        |
/// | 8 | `uom`                  | TEXT NULL   |
/// | 9 | `field_name`           | TEXT        |
/// |10 | `quantity_definition`  | TEXT NULL   |
#[derive(Debug, Clone)]
pub struct GriddedCoverage {
    /// Name of the tile matrix set / user data table this row describes.
    pub table_name: String,
    /// Sample data type (`integer` or `float`).
    pub datatype: CoverageDatatype,
    /// Scale factor applied to raw integer values: `phys = raw * scale + offset`.
    pub scale: f64,
    /// Offset added after scaling: `phys = raw * scale + offset`.
    pub offset: f64,
    /// Minimum meaningful difference between adjacent physical values.
    pub precision: f64,
    /// Raw value that represents a missing/void sample; `None` if not specified.
    pub data_null: Option<f64>,
    /// Interpretation of the grid cell sample position.
    pub grid_cell_encoding: GridCellEncoding,
    /// Unit of measure (e.g. `"metre"`), if present.
    pub uom: Option<String>,
    /// Short name identifying the measured field (e.g. `"Height"`).
    pub field_name: String,
    /// Human-readable description of the physical quantity, if present.
    pub quantity_definition: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// TileGriddedAncillary
// ─────────────────────────────────────────────────────────────────────────────

/// A parsed row from `gpkg_2d_gridded_tile_ancillary`.
///
/// Column layout (0-indexed):
///
/// | # | Column       | SQLite type |
/// |---|--------------|-------------|
/// | 0 | `id`         | INTEGER PK  |
/// | 1 | `tpudt_name` | TEXT        |
/// | 2 | `tpudt_id`   | INTEGER     |
/// | 3 | `scale`      | REAL        |
/// | 4 | `offset`     | REAL        |
/// | 5 | `min`        | REAL NULL   |
/// | 6 | `max`        | REAL NULL   |
/// | 7 | `mean`       | REAL NULL   |
/// | 8 | `std_dev`    | REAL NULL   |
///
/// `tpudt_name` is the user tile pyramid table name; `tpudt_id` is the `id`
/// of the corresponding row in that tile table.
#[derive(Debug, Clone)]
pub struct TileGriddedAncillary {
    /// Primary key of this row.
    pub id: i64,
    /// Foreign key into the tile pyramid table (`id` column of that table).
    pub tpudt_id: i64,
    /// Per-tile scale override.  Typically `1.0`.
    pub scale: f64,
    /// Per-tile offset override.  Typically `0.0`.
    pub offset: f64,
    /// Minimum physical value in this tile (optional).
    pub min: Option<f64>,
    /// Maximum physical value in this tile (optional).
    pub max: Option<f64>,
    /// Mean physical value in this tile (optional).
    pub mean: Option<f64>,
    /// Standard deviation of physical values in this tile (optional).
    pub std_dev: Option<f64>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Cell-value coercions (module-local helpers)
// ─────────────────────────────────────────────────────────────────────────────

use crate::btree::CellValue;

/// Coerce a [`CellValue`] to `f64`, returning `0.0` for non-numeric types.
fn cell_to_f64(v: &CellValue) -> f64 {
    match v {
        CellValue::Float(f) => *f,
        CellValue::Integer(i) => *i as f64,
        _ => 0.0,
    }
}

/// Coerce a [`CellValue`] to `Option<f64>`, returning `None` for SQL NULL.
fn cell_to_optional_f64(v: &CellValue) -> Option<f64> {
    match v {
        CellValue::Null => None,
        CellValue::Float(f) => Some(*f),
        CellValue::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// Coerce a [`CellValue`] to a `String`, returning an empty string for NULL.
fn cell_to_string(v: &CellValue) -> String {
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

/// Coerce a [`CellValue`] to `Option<String>`, returning `None` for SQL NULL.
fn cell_to_optional_string(v: &CellValue) -> Option<String> {
    match v {
        CellValue::Null => None,
        CellValue::Text(s) if s.is_empty() => None,
        other => Some(cell_to_string(other)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// load_gridded_coverages
// ─────────────────────────────────────────────────────────────────────────────

/// Read all rows from `gpkg_2d_gridded_coverage_ancillary`.
///
/// Returns `Ok(vec![])` when the table is not present in the GeoPackage
/// (the extension is optional).  Returns a descriptive error for malformed
/// B-tree data.
///
/// # Errors
/// Propagates any B-tree parse error from the underlying SQLite reader.
pub fn load_gridded_coverages(reader: &GeoPackage) -> Result<Vec<GriddedCoverage>, GpkgError> {
    let table_name = "gpkg_2d_gridded_coverage_ancillary";
    let rows = match reader.scan_table_by_name(table_name)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut out = Vec::with_capacity(rows.len());
    for (_rowid, values) in rows {
        // Minimum 11 columns expected (id + 10 data columns).
        if values.len() < 11 {
            continue;
        }

        // col 0: id (INTEGER) — skipped (not in the public struct)
        // col 1: tile_matrix_set_name (TEXT)
        let tbl = cell_to_string(&values[1]);
        // col 2: datatype (TEXT)
        let datatype_str = cell_to_string(&values[2]);
        let datatype = datatype_str.parse::<CoverageDatatype>()?;
        // col 3: scale (REAL)
        let scale = cell_to_f64(&values[3]);
        // col 4: offset (REAL)
        let offset = cell_to_f64(&values[4]);
        // col 5: precision (REAL)
        let precision = cell_to_f64(&values[5]);
        // col 6: data_null (REAL NULL)
        let data_null = cell_to_optional_f64(&values[6]);
        // col 7: grid_cell_encoding (TEXT)
        let encoding_str = cell_to_string(&values[7]);
        // GridCellEncoding::FromStr is infallible (returns Grid for unknown)
        let grid_cell_encoding = encoding_str
            .parse::<GridCellEncoding>()
            .unwrap_or(GridCellEncoding::Grid);
        // col 8: uom (TEXT NULL)
        let uom = cell_to_optional_string(&values[8]);
        // col 9: field_name (TEXT)
        let field_name = cell_to_string(&values[9]);
        // col 10: quantity_definition (TEXT NULL)
        let quantity_definition = cell_to_optional_string(&values[10]);

        out.push(GriddedCoverage {
            table_name: tbl,
            datatype,
            scale,
            offset,
            precision,
            data_null,
            grid_cell_encoding,
            uom,
            field_name,
            quantity_definition,
        });
    }

    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// load_gridded_tile_ancillary
// ─────────────────────────────────────────────────────────────────────────────

/// Read rows from `gpkg_2d_gridded_tile_ancillary` for a specific tile table.
///
/// Only rows where `tpudt_name == table_name` are returned; all other rows
/// are filtered out.  Returns `Ok(vec![])` when the ancillary table is absent.
///
/// # Errors
/// Propagates any B-tree parse error from the underlying SQLite reader.
pub fn load_gridded_tile_ancillary(
    reader: &GeoPackage,
    table_name: &str,
) -> Result<Vec<TileGriddedAncillary>, GpkgError> {
    let sys_table = "gpkg_2d_gridded_tile_ancillary";
    let rows = match reader.scan_table_by_name(sys_table)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut out = Vec::new();
    for (_rowid, values) in rows {
        // Minimum 9 columns expected:
        // id, tpudt_name, tpudt_id, scale, offset, min, max, mean, std_dev
        if values.len() < 9 {
            continue;
        }

        // col 1: tpudt_name — filter by table_name
        let tpudt_name = cell_to_string(&values[1]);
        if tpudt_name != table_name {
            continue;
        }

        // col 0: id (INTEGER PK)
        let id = cell_to_i64(&values[0]);
        // col 2: tpudt_id (INTEGER FK into the tile table)
        let tpudt_id = cell_to_i64(&values[2]);
        // col 3: scale (REAL)
        let scale = cell_to_f64(&values[3]);
        // col 4: offset (REAL)
        let offset = cell_to_f64(&values[4]);
        // col 5: min (REAL NULL)
        let min = cell_to_optional_f64(&values[5]);
        // col 6: max (REAL NULL)
        let max = cell_to_optional_f64(&values[6]);
        // col 7: mean (REAL NULL)
        let mean = cell_to_optional_f64(&values[7]);
        // col 8: std_dev (REAL NULL)
        let std_dev = cell_to_optional_f64(&values[8]);

        out.push(TileGriddedAncillary {
            id,
            tpudt_id,
            scale,
            offset,
            min,
            max,
            mean,
            std_dev,
        });
    }

    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// unscale_value
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a raw sample value to its physical value using scale and offset.
///
/// The conversion formula is:
/// `phys = raw * effective_scale + effective_offset`
///
/// Scale/offset priority:
/// - If `tile_ancillary` is `Some`, use its `scale` and `offset`.
/// - Otherwise use `coverage.scale` and `coverage.offset`.
///
/// Null-data handling: if `coverage.data_null` is `Some(n)` and `raw == n`,
/// the function returns [`f64::NAN`] (the sample is void/missing).
pub fn unscale_value(
    raw: f64,
    coverage: &GriddedCoverage,
    tile_ancillary: Option<&TileGriddedAncillary>,
) -> f64 {
    // Null-data check before scaling — compare with coverage data_null.
    if let Some(null_val) = coverage.data_null {
        // IEEE equality is intentional: the raw value must exactly match the
        // sentinel defined in the coverage metadata.
        #[allow(clippy::float_cmp)]
        if raw == null_val {
            return f64::NAN;
        }
    }

    // Determine effective scale/offset: tile ancillary wins when present.
    let (effective_scale, effective_offset) = match tile_ancillary {
        Some(ta) => (ta.scale, ta.offset),
        None => (coverage.scale, coverage.offset),
    };

    raw * effective_scale + effective_offset
}

// ─────────────────────────────────────────────────────────────────────────────
// unscale_tile_buffer_u16
// ─────────────────────────────────────────────────────────────────────────────

/// Apply `unscale_value` to every sample in a u16 tile buffer.
///
/// Typical GeoPackage elevation tiles store 16-bit unsigned integers packed in
/// little-endian byte order (after PNG/TIFF decoding); this function accepts the
/// already-decoded integer slice and returns the physical float values.
///
/// The returned `Vec<f64>` has the same length as `raw_buf`.
pub fn unscale_tile_buffer_u16(
    raw_buf: &[u16],
    coverage: &GriddedCoverage,
    tile_ancillary: Option<&TileGriddedAncillary>,
) -> Vec<f64> {
    raw_buf
        .iter()
        .map(|&sample| unscale_value(sample as f64, coverage, tile_ancillary))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// unscale_tile_buffer_i16
// ─────────────────────────────────────────────────────────────────────────────

/// Apply `unscale_value` to every sample in an i16 tile buffer.
///
/// Signed 16-bit elevation data is common in DTM datasets where terrain can
/// dip below the reference datum (negative elevations).  The conversion is
/// identical to [`unscale_tile_buffer_u16`] except the input type is `i16`.
///
/// The returned `Vec<f64>` has the same length as `raw_buf`.
pub fn unscale_tile_buffer_i16(
    raw_buf: &[i16],
    coverage: &GriddedCoverage,
    tile_ancillary: Option<&TileGriddedAncillary>,
) -> Vec<f64> {
    raw_buf
        .iter()
        .map(|&sample| unscale_value(sample as f64, coverage, tile_ancillary))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn make_coverage(scale: f64, offset: f64, data_null: Option<f64>) -> GriddedCoverage {
        GriddedCoverage {
            table_name: "dem".to_string(),
            datatype: CoverageDatatype::Integer,
            scale,
            offset,
            precision: 1.0,
            data_null,
            grid_cell_encoding: GridCellEncoding::Grid,
            uom: None,
            field_name: "Height".to_string(),
            quantity_definition: None,
        }
    }

    fn make_tile_ancillary(scale: f64, offset: f64) -> TileGriddedAncillary {
        TileGriddedAncillary {
            id: 1,
            tpudt_id: 42,
            scale,
            offset,
            min: None,
            max: None,
            mean: None,
            std_dev: None,
        }
    }

    // ── CoverageDatatype ─────────────────────────────────────────────────────

    #[test]
    fn datatype_from_str_integer() {
        assert_eq!(
            "integer".parse::<CoverageDatatype>().unwrap(),
            CoverageDatatype::Integer
        );
    }

    #[test]
    fn datatype_from_str_float() {
        assert_eq!(
            "float".parse::<CoverageDatatype>().unwrap(),
            CoverageDatatype::Float
        );
    }

    #[test]
    fn datatype_from_str_invalid() {
        let err = "raster".parse::<CoverageDatatype>().unwrap_err();
        assert!(
            matches!(err, GpkgError::InvalidCoverageDatatype(ref s) if s == "raster"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn datatype_as_str_roundtrip() {
        assert_eq!(CoverageDatatype::Integer.as_str(), "integer");
        assert_eq!(CoverageDatatype::Float.as_str(), "float");
    }

    // ── GridCellEncoding ─────────────────────────────────────────────────────

    #[test]
    fn grid_cell_encoding_parses_all_three() {
        assert_eq!(
            "grid-value-is-center".parse::<GridCellEncoding>().unwrap(),
            GridCellEncoding::Grid
        );
        assert_eq!(
            "grid-value-is-area".parse::<GridCellEncoding>().unwrap(),
            GridCellEncoding::PixelIsArea
        );
        assert_eq!(
            "grid-value-is-corner".parse::<GridCellEncoding>().unwrap(),
            GridCellEncoding::PixelIsPoint
        );
    }

    #[test]
    fn grid_cell_encoding_unknown_defaults_to_grid() {
        // OGC §F.7: lenient parse — unknown strings → Grid
        assert_eq!(
            "unknown-encoding".parse::<GridCellEncoding>().unwrap(),
            GridCellEncoding::Grid
        );
    }

    // ── unscale_value ────────────────────────────────────────────────────────

    #[test]
    fn unscale_value_identity_passthrough() {
        let cov = make_coverage(1.0, 0.0, None);
        assert_eq!(unscale_value(42.0, &cov, None), 42.0);
    }

    #[test]
    fn unscale_value_scale_and_offset_applied() {
        // scale=0.1, offset=-100.0: raw=1000.0 → phys = 1000*0.1 + (-100) = 0.0
        let cov = make_coverage(0.1, -100.0, None);
        let phys = unscale_value(1000.0, &cov, None);
        assert!((phys - 0.0).abs() < 1e-10, "expected 0.0, got {phys}");
    }

    #[test]
    fn unscale_value_tile_ancillary_overrides_coverage() {
        // coverage scale=1.0 offset=0.0, tile scale=2.0 offset=5.0
        // raw=10.0 → tile wins → phys = 10*2 + 5 = 25.0
        let cov = make_coverage(1.0, 0.0, None);
        let ta = make_tile_ancillary(2.0, 5.0);
        let phys = unscale_value(10.0, &cov, Some(&ta));
        assert!((phys - 25.0).abs() < 1e-10, "expected 25.0, got {phys}");
    }

    #[test]
    fn unscale_value_data_null_returns_nan() {
        let cov = make_coverage(1.0, 0.0, Some(0.0));
        let result = unscale_value(0.0, &cov, None);
        assert!(result.is_nan(), "expected NAN, got {result}");
    }

    #[test]
    fn unscale_value_non_null_not_nan() {
        let cov = make_coverage(1.0, 0.0, Some(0.0));
        let result = unscale_value(1.0, &cov, None);
        assert!(!result.is_nan(), "should not be NAN for non-null value");
        assert!((result - 1.0).abs() < 1e-10);
    }

    // ── unscale_tile_buffer_u16 ──────────────────────────────────────────────

    #[test]
    fn unscale_buffer_u16_identity() {
        let cov = make_coverage(1.0, 0.0, None);
        let raw = [0u16, 1, 65535];
        let out = unscale_tile_buffer_u16(&raw, &cov, None);
        assert_eq!(out.len(), 3);
        assert!((out[0] - 0.0).abs() < 1e-10);
        assert!((out[1] - 1.0).abs() < 1e-10);
        assert!((out[2] - 65535.0).abs() < 1e-10);
    }

    #[test]
    fn unscale_buffer_u16_with_scale() {
        // scale=0.01, offset=0.0: [100, 200] → [1.0, 2.0]
        let cov = make_coverage(0.01, 0.0, None);
        let raw = [100u16, 200u16];
        let out = unscale_tile_buffer_u16(&raw, &cov, None);
        assert!((out[0] - 1.0).abs() < 1e-10, "got {}", out[0]);
        assert!((out[1] - 2.0).abs() < 1e-10, "got {}", out[1]);
    }

    // ── unscale_tile_buffer_i16 ──────────────────────────────────────────────

    #[test]
    fn unscale_buffer_i16_negative_elevations() {
        let cov = make_coverage(1.0, 0.0, None);
        let raw = [-100i16, 0i16, 100i16];
        let out = unscale_tile_buffer_i16(&raw, &cov, None);
        assert_eq!(out.len(), 3);
        assert!((out[0] - (-100.0)).abs() < 1e-10, "got {}", out[0]);
        assert!((out[1] - 0.0).abs() < 1e-10, "got {}", out[1]);
        assert!((out[2] - 100.0).abs() < 1e-10, "got {}", out[2]);
    }

    #[test]
    fn unscale_buffer_i16_with_scale_and_offset() {
        // scale=0.5, offset=10.0: [-2, 0, 4] → [9.0, 10.0, 12.0]
        let cov = make_coverage(0.5, 10.0, None);
        let raw = [-2i16, 0i16, 4i16];
        let out = unscale_tile_buffer_i16(&raw, &cov, None);
        assert!((out[0] - 9.0).abs() < 1e-10, "got {}", out[0]);
        assert!((out[1] - 10.0).abs() < 1e-10, "got {}", out[1]);
        assert!((out[2] - 12.0).abs() < 1e-10, "got {}", out[2]);
    }
}
