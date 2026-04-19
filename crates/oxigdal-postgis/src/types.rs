//! Type conversions between PostgreSQL/PostGIS and OxiGDAL types
//!
//! This module provides conversion traits for geometries and attributes.

use crate::error::Result;
use crate::wkb::{WkbDecoder, WkbEncoder};
use bytes::BytesMut;
use oxigdal_core::vector::feature::{Feature, FeatureId, FieldValue};
use oxigdal_core::vector::geometry::Geometry;
use postgres_types::{FromSql, IsNull, ToSql, Type};
use std::error::Error;

/// PostGIS geometry wrapper for PostgreSQL type conversion
#[derive(Debug, Clone)]
pub struct PostGisGeometry {
    /// The OxiGDAL geometry
    pub geometry: Geometry,
    /// SRID (Spatial Reference System Identifier)
    pub srid: Option<i32>,
}

impl PostGisGeometry {
    /// Creates a new PostGIS geometry
    pub const fn new(geometry: Geometry) -> Self {
        Self {
            geometry,
            srid: None,
        }
    }

    /// Creates a new PostGIS geometry with SRID
    pub const fn with_srid(geometry: Geometry, srid: i32) -> Self {
        Self {
            geometry,
            srid: Some(srid),
        }
    }

    /// Returns the SRID
    pub const fn srid(&self) -> Option<i32> {
        self.srid
    }

    /// Returns a reference to the geometry
    pub const fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// Consumes self and returns the geometry
    pub fn into_geometry(self) -> Geometry {
        self.geometry
    }
}

impl From<Geometry> for PostGisGeometry {
    fn from(geometry: Geometry) -> Self {
        Self::new(geometry)
    }
}

impl ToSql for PostGisGeometry {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut BytesMut,
    ) -> std::result::Result<IsNull, Box<dyn Error + Sync + Send>> {
        let mut encoder = if let Some(srid) = self.srid {
            WkbEncoder::with_srid(srid)
        } else {
            WkbEncoder::new()
        };

        let wkb = encoder
            .encode(&self.geometry)
            .map_err(|e| Box::new(e) as Box<dyn Error + Sync + Send>)?;

        out.extend_from_slice(&wkb);
        Ok(IsNull::No)
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    postgres_types::to_sql_checked!();
}

impl<'a> FromSql<'a> for PostGisGeometry {
    fn from_sql(
        _ty: &Type,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn Error + Sync + Send>> {
        let mut decoder = WkbDecoder::new();
        let geometry = decoder
            .decode(raw)
            .map_err(|e| Box::new(e) as Box<dyn Error + Sync + Send>)?;
        let srid = decoder.srid();

        Ok(Self { geometry, srid })
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

/// Converts FieldValue to PostgreSQL-compatible JSON value
pub fn property_to_sql(value: &FieldValue) -> serde_json::Value {
    value.to_json()
}

/// Converts PostgreSQL column value to FieldValue
pub fn property_from_sql(value: &serde_json::Value) -> FieldValue {
    FieldValue::from_json(value)
}

/// Converts a row to a Feature
pub struct FeatureBuilder {
    feature: Feature,
    geometry_column: Option<String>,
    id_column: Option<String>,
}

impl FeatureBuilder {
    /// Creates a new feature builder
    pub fn new() -> Self {
        Self {
            feature: Feature::new_attribute_only(),
            geometry_column: Some("geom".to_string()),
            id_column: Some("id".to_string()),
        }
    }

    /// Sets the geometry column name
    pub fn geometry_column(mut self, name: impl Into<String>) -> Self {
        self.geometry_column = Some(name.into());
        self
    }

    /// Sets the ID column name
    pub fn id_column(mut self, name: impl Into<String>) -> Self {
        self.id_column = Some(name.into());
        self
    }

    /// Disables geometry column
    pub fn no_geometry(mut self) -> Self {
        self.geometry_column = None;
        self
    }

    /// Disables ID column
    pub fn no_id(mut self) -> Self {
        self.id_column = None;
        self
    }

    /// Builds a feature from a PostgreSQL row
    pub fn build_from_row(mut self, row: &tokio_postgres::Row) -> Result<Feature> {
        // Extract geometry if column is specified
        if let Some(geom_col) = &self.geometry_column {
            if let Ok(Some(postgis_geom)) =
                row.try_get::<_, Option<PostGisGeometry>>(geom_col.as_str())
            {
                self.feature.geometry = Some(postgis_geom.geometry);
            }
        }

        // Extract ID if column is specified
        if let Some(id_col) = &self.id_column {
            // Try different ID types
            if let Ok(Some(id)) = row.try_get::<_, Option<i64>>(id_col.as_str()) {
                self.feature.id = Some(FeatureId::Integer(id));
            } else if let Ok(Some(id)) = row.try_get::<_, Option<i32>>(id_col.as_str()) {
                self.feature.id = Some(FeatureId::Integer(i64::from(id)));
            } else if let Ok(Some(id)) = row.try_get::<_, Option<String>>(id_col.as_str()) {
                self.feature.id = Some(FeatureId::String(id));
            }
        }

        // Extract all columns as properties
        for (idx, column) in row.columns().iter().enumerate() {
            let col_name = column.name();

            // Skip geometry and ID columns
            if let Some(ref geom_col) = self.geometry_column {
                if col_name == geom_col {
                    continue;
                }
            }
            if let Some(ref id_col) = self.id_column {
                if col_name == id_col {
                    continue;
                }
            }

            // Try to extract value as different types
            let value = if let Ok(Some(v)) = row.try_get::<_, Option<bool>>(idx) {
                FieldValue::Bool(v)
            } else if let Ok(Some(v)) = row.try_get::<_, Option<i16>>(idx) {
                FieldValue::Integer(i64::from(v))
            } else if let Ok(Some(v)) = row.try_get::<_, Option<i32>>(idx) {
                FieldValue::Integer(i64::from(v))
            } else if let Ok(Some(v)) = row.try_get::<_, Option<i64>>(idx) {
                FieldValue::Integer(v)
            } else if let Ok(Some(v)) = row.try_get::<_, Option<f32>>(idx) {
                FieldValue::Float(f64::from(v))
            } else if let Ok(Some(v)) = row.try_get::<_, Option<f64>>(idx) {
                FieldValue::Float(v)
            } else if let Ok(Some(v)) = row.try_get::<_, Option<String>>(idx) {
                FieldValue::String(v)
            } else if let Ok(Some(v)) = row.try_get::<_, Option<serde_json::Value>>(idx) {
                property_from_sql(&v)
            } else {
                FieldValue::Null
            };

            self.feature.set_property(col_name, value);
        }

        Ok(self.feature)
    }
}

impl Default for FeatureBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// SRID constants for common spatial reference systems
pub mod srid {
    /// WGS 84 (GPS coordinates)
    pub const WGS84: i32 = 4326;

    /// WGS 84 / Pseudo-Mercator (Web Mercator)
    pub const WEB_MERCATOR: i32 = 3857;

    /// NAD83 (North American Datum 1983)
    pub const NAD83: i32 = 4269;

    /// ETRS89 (European Terrestrial Reference System 1989)
    pub const ETRS89: i32 = 4258;
}

/// Converts an OxiGDAL geometry to PostGIS geometry with SRID
pub fn to_postgis(geometry: Geometry, srid: Option<i32>) -> PostGisGeometry {
    if let Some(srid) = srid {
        PostGisGeometry::with_srid(geometry, srid)
    } else {
        PostGisGeometry::new(geometry)
    }
}

/// Converts a PostGIS geometry to OxiGDAL geometry
pub fn from_postgis(postgis_geom: PostGisGeometry) -> (Geometry, Option<i32>) {
    (postgis_geom.geometry, postgis_geom.srid)
}

/// FieldValue conversion helpers (standalone functions)
/// Converts FieldValue to PostgreSQL boolean
pub fn property_to_sql_bool(value: &FieldValue) -> Option<bool> {
    value.as_bool()
}

/// Converts FieldValue to PostgreSQL integer
pub fn property_to_sql_int(value: &FieldValue) -> Option<i64> {
    value.as_i64()
}

/// Converts FieldValue to PostgreSQL float
pub fn property_to_sql_float(value: &FieldValue) -> Option<f64> {
    value.as_f64()
}

/// Converts FieldValue to PostgreSQL text
pub fn property_to_sql_text(value: &FieldValue) -> Option<String> {
    match value {
        FieldValue::String(s) => Some(s.clone()),
        FieldValue::Integer(i) => Some(i.to_string()),
        FieldValue::Float(f) => Some(f.to_string()),
        FieldValue::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::approx_constant)]
mod tests {
    use super::*;
    use oxigdal_core::vector::geometry::Point;

    #[test]
    fn test_postgis_geometry_creation() {
        let point = Point::new(1.0, 2.0);
        let geom = Geometry::Point(point);
        let postgis = PostGisGeometry::new(geom.clone());

        assert_eq!(postgis.srid(), None);
    }

    #[test]
    fn test_postgis_geometry_with_srid() {
        let point = Point::new(1.0, 2.0);
        let geom = Geometry::Point(point);
        let postgis = PostGisGeometry::with_srid(geom.clone(), 4326);

        assert_eq!(postgis.srid(), Some(4326));
    }

    #[test]
    fn test_property_value_conversions() {
        let pv = FieldValue::Bool(true);
        assert!(matches!(pv, FieldValue::Bool(true)));

        let pv = FieldValue::Integer(42);
        assert!(matches!(pv, FieldValue::Integer(42)));

        let pv = FieldValue::Float(3.14);
        if let FieldValue::Float(f) = pv {
            assert!((f - 3.14).abs() < f64::EPSILON);
        } else {
            panic!("Expected Float variant");
        }

        let pv = FieldValue::String("test".to_string());
        assert!(matches!(pv, FieldValue::String(ref s) if s == "test"));
    }

    #[test]
    fn test_to_postgis() {
        let point = Point::new(1.0, 2.0);
        let geom = Geometry::Point(point);
        let postgis = to_postgis(geom, Some(4326));

        assert_eq!(postgis.srid(), Some(4326));
    }

    #[test]
    fn test_from_postgis() {
        let point = Point::new(1.0, 2.0);
        let geom = Geometry::Point(point);
        let postgis = PostGisGeometry::with_srid(geom, 4326);

        let (_, srid) = from_postgis(postgis);
        assert_eq!(srid, Some(4326));
    }

    #[test]
    fn test_feature_builder_defaults() {
        let builder = FeatureBuilder::new();
        assert!(builder.geometry_column.is_some());
        assert!(builder.id_column.is_some());
    }

    #[test]
    fn test_feature_builder_no_geometry() {
        let builder = FeatureBuilder::new().no_geometry();
        assert!(builder.geometry_column.is_none());
    }

    #[test]
    fn test_feature_builder_no_id() {
        let builder = FeatureBuilder::new().no_id();
        assert!(builder.id_column.is_none());
    }

    #[test]
    fn test_srid_constants() {
        assert_eq!(srid::WGS84, 4326);
        assert_eq!(srid::WEB_MERCATOR, 3857);
        assert_eq!(srid::NAD83, 4269);
        assert_eq!(srid::ETRS89, 4258);
    }
}
