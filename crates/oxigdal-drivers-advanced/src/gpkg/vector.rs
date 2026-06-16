//! GeoPackage vector feature tables.

use super::{connection::GpkgConnection, metadata::Extent};
use crate::error::{Error, Result};
use oxisql_core::ToSqlValue;
use std::str::FromStr;

/// Geometry type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryType {
    /// Point
    Point,
    /// LineString
    LineString,
    /// Polygon
    Polygon,
    /// MultiPoint
    MultiPoint,
    /// MultiLineString
    MultiLineString,
    /// MultiPolygon
    MultiPolygon,
    /// GeometryCollection
    GeometryCollection,
    /// Generic Geometry
    Geometry,
}

impl GeometryType {
    /// Get type as string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Point => "POINT",
            Self::LineString => "LINESTRING",
            Self::Polygon => "POLYGON",
            Self::MultiPoint => "MULTIPOINT",
            Self::MultiLineString => "MULTILINESTRING",
            Self::MultiPolygon => "MULTIPOLYGON",
            Self::GeometryCollection => "GEOMETRYCOLLECTION",
            Self::Geometry => "GEOMETRY",
        }
    }
}

impl FromStr for GeometryType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_str() {
            "POINT" => Ok(Self::Point),
            "LINESTRING" => Ok(Self::LineString),
            "POLYGON" => Ok(Self::Polygon),
            "MULTIPOINT" => Ok(Self::MultiPoint),
            "MULTILINESTRING" => Ok(Self::MultiLineString),
            "MULTIPOLYGON" => Ok(Self::MultiPolygon),
            "GEOMETRYCOLLECTION" => Ok(Self::GeometryCollection),
            "GEOMETRY" => Ok(Self::Geometry),
            _ => Err(Error::geopackage(format!("Unknown geometry type: {}", s))),
        }
    }
}

/// Feature with geometry and attributes.
#[derive(Debug, Clone)]
pub struct Feature {
    /// Feature ID
    pub fid: Option<i64>,
    /// Geometry (WKB format)
    pub geometry: Option<Vec<u8>>,
    /// Attributes (key-value pairs)
    pub attributes: Vec<(String, AttributeValue)>,
}

/// Attribute value types.
#[derive(Debug, Clone)]
pub enum AttributeValue {
    /// Text value
    Text(String),
    /// Integer value
    Integer(i64),
    /// Real (float) value
    Real(f64),
    /// Blob value
    Blob(Vec<u8>),
    /// NULL value
    Null,
}

/// Feature table representation.
pub struct FeatureTable {
    table_name: String,
    geometry_column: String,
    geometry_type: GeometryType,
    srs_id: i32,
}

impl FeatureTable {
    /// Open an existing feature table.
    pub fn open(conn: &GpkgConnection, table_name: &str) -> Result<Self> {
        // Query geometry columns table
        let table_name_owned = table_name.to_string();
        let rows = conn.connection().query(
            "SELECT column_name, geometry_type_name, srs_id FROM gpkg_geometry_columns WHERE table_name = $1",
            &[&table_name_owned as &dyn ToSqlValue],
        )?;
        let row = rows.into_iter().next().ok_or_else(|| {
            Error::geopackage(format!("no geometry columns for table '{table_name}'"))
        })?;
        let geometry_column: String = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;
        let geometry_type_str: String = row
            .try_get_by_index(1)
            .map_err(|e| Error::geopackage(format!("column 1: {e}")))?;
        let srs_id: i32 = row
            .try_get_by_index(2)
            .map_err(|e| Error::geopackage(format!("column 2: {e}")))?;

        let geometry_type = GeometryType::from_str(&geometry_type_str)?;

        Ok(Self {
            table_name: table_name.to_string(),
            geometry_column,
            geometry_type,
            srs_id,
        })
    }

    /// Create a new feature table.
    pub fn create(
        conn: &GpkgConnection,
        table_name: &str,
        geometry_type: GeometryType,
        srs_id: i32,
    ) -> Result<Self> {
        let geometry_column = "geom";

        // Create table
        let create_sql = format!(
            "CREATE TABLE {} (fid INTEGER PRIMARY KEY AUTOINCREMENT, {} BLOB)",
            table_name, geometry_column
        );
        conn.execute_batch(&create_sql)?;

        // Register in gpkg_contents
        let tname = table_name.to_string();
        let features_str = "features".to_string();
        let srs_id_i64: i64 = i64::from(srs_id);
        conn.execute(
            "INSERT INTO gpkg_contents (table_name, data_type, identifier, srs_id) VALUES ($1, $2, $3, $4)",
            &[
                &tname as &dyn ToSqlValue,
                &features_str,
                &tname,
                &srs_id_i64,
            ],
        )?;

        // Register in gpkg_geometry_columns
        let geom_col_str = geometry_column.to_string();
        let geom_type_str2 = geometry_type.as_str().to_string();
        conn.execute(
            "INSERT INTO gpkg_geometry_columns (table_name, column_name, geometry_type_name, srs_id, z, m) VALUES ($1, $2, $3, $4, 0, 0)",
            &[
                &tname as &dyn ToSqlValue,
                &geom_col_str,
                &geom_type_str2,
                &srs_id_i64,
            ],
        )?;

        Ok(Self {
            table_name: table_name.to_string(),
            geometry_column: geometry_column.to_string(),
            geometry_type,
            srs_id,
        })
    }

    /// Get table name.
    pub fn name(&self) -> &str {
        &self.table_name
    }

    /// Get geometry column name.
    pub fn geometry_column(&self) -> &str {
        &self.geometry_column
    }

    /// Get geometry type.
    pub fn geometry_type(&self) -> GeometryType {
        self.geometry_type
    }

    /// Get SRS ID.
    pub fn srs_id(&self) -> i32 {
        self.srs_id
    }

    /// Count features.
    pub fn count(&self, conn: &GpkgConnection) -> Result<i64> {
        let rows = conn
            .connection()
            .query(&format!("SELECT COUNT(*) FROM {}", self.table_name), &[])?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::geopackage("count: no row returned"))?;
        let count: i64 = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;
        Ok(count)
    }

    /// Get extent.
    pub fn extent(&self, conn: &GpkgConnection) -> Result<Option<Extent>> {
        let tname = self.table_name.clone();
        let rows = conn.connection().query(
            "SELECT min_x, min_y, max_x, max_y FROM gpkg_contents WHERE table_name = $1",
            &[&tname as &dyn ToSqlValue],
        )?;
        let result: Option<(Option<f64>, Option<f64>, Option<f64>, Option<f64>)> =
            if let Some(row) = rows.into_iter().next() {
                let min_x: Option<f64> = row
                    .try_get_by_index(0)
                    .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;
                let min_y: Option<f64> = row
                    .try_get_by_index(1)
                    .map_err(|e| Error::geopackage(format!("column 1: {e}")))?;
                let max_x: Option<f64> = row
                    .try_get_by_index(2)
                    .map_err(|e| Error::geopackage(format!("column 2: {e}")))?;
                let max_y: Option<f64> = row
                    .try_get_by_index(3)
                    .map_err(|e| Error::geopackage(format!("column 3: {e}")))?;
                Some((min_x, min_y, max_x, max_y))
            } else {
                None
            };

        match result {
            Some((Some(min_x), Some(min_y), Some(max_x), Some(max_y))) => {
                Ok(Some(Extent::new(min_x, min_y, max_x, max_y)))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpkg::schema;
    use tempfile::NamedTempFile;

    #[test]
    fn test_geometry_type() {
        assert_eq!(GeometryType::Point.as_str(), "POINT");
        let gt = GeometryType::from_str("POINT");
        assert!(gt.is_ok());
        if let Ok(g) = gt {
            assert_eq!(g, GeometryType::Point);
        }
        let gt = GeometryType::from_str("point");
        assert!(gt.is_ok());
        if let Ok(g) = gt {
            assert_eq!(g, GeometryType::Point);
        }
    }

    #[test]
    fn test_feature_table_creation() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        schema::initialize_schema(&conn)?;

        let table = FeatureTable::create(&conn, "test_features", GeometryType::Point, 4326)?;
        assert_eq!(table.name(), "test_features");
        assert_eq!(table.geometry_type(), GeometryType::Point);
        assert_eq!(table.srs_id(), 4326);

        Ok(())
    }

    #[test]
    fn test_feature_table_count() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        schema::initialize_schema(&conn)?;

        let table = FeatureTable::create(&conn, "test_features", GeometryType::Point, 4326)?;
        let count = table.count(&conn)?;
        assert_eq!(count, 0);

        Ok(())
    }
}
