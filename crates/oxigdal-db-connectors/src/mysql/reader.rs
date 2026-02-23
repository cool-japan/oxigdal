//! MySQL spatial data reader.

use crate::error::{Error, Result};
use crate::mysql::{MySqlConnector, wkt_to_geometry};
use geo_types::Geometry;
use mysql_async::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

/// Feature read from MySQL.
#[derive(Debug, Clone)]
pub struct MySqlFeature {
    /// Feature ID.
    pub id: i64,
    /// Geometry.
    pub geometry: Geometry<f64>,
    /// Properties.
    pub properties: HashMap<String, Value>,
}

/// MySQL spatial data reader.
pub struct MySqlReader {
    connector: MySqlConnector,
    table_name: String,
    geometry_column: String,
}

impl MySqlReader {
    /// Create a new MySQL reader.
    pub fn new(connector: MySqlConnector, table_name: String, geometry_column: String) -> Self {
        Self {
            connector,
            table_name,
            geometry_column,
        }
    }

    /// Read all features from the table.
    pub async fn read_all(&self) -> Result<Vec<MySqlFeature>> {
        let sql = format!(
            "SELECT *, ST_AsText({}) as geom_wkt FROM {}",
            self.geometry_column, self.table_name
        );

        self.read_with_query(&sql).await
    }

    /// Read features with a WHERE clause.
    pub async fn read_where(&self, where_clause: &str) -> Result<Vec<MySqlFeature>> {
        let sql = format!(
            "SELECT *, ST_AsText({}) as geom_wkt FROM {} WHERE {}",
            self.geometry_column, self.table_name, where_clause
        );

        self.read_with_query(&sql).await
    }

    /// Read features within a bounding box.
    pub async fn read_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<Vec<MySqlFeature>> {
        let bbox_wkt = format!(
            "POLYGON(({} {}, {} {}, {} {}, {} {}, {} {}))",
            min_x, min_y, max_x, min_y, max_x, max_y, min_x, max_y, min_x, min_y
        );

        let sql = format!(
            "SELECT *, ST_AsText({}) as geom_wkt FROM {} WHERE ST_Intersects({}, ST_GeomFromText(?))",
            self.geometry_column, self.table_name, self.geometry_column
        );

        let mut conn = self.connector.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(&sql, (&bbox_wkt,))
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        self.rows_to_features(rows)
    }

    /// Read features that intersect with a geometry.
    pub async fn read_intersects(&self, geometry_wkt: &str) -> Result<Vec<MySqlFeature>> {
        let sql = format!(
            "SELECT *, ST_AsText({}) as geom_wkt FROM {} WHERE ST_Intersects({}, ST_GeomFromText(?))",
            self.geometry_column, self.table_name, self.geometry_column
        );

        let mut conn = self.connector.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(&sql, (geometry_wkt,))
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        self.rows_to_features(rows)
    }

    /// Read features within a distance of a point.
    pub async fn read_within_distance(
        &self,
        x: f64,
        y: f64,
        distance: f64,
    ) -> Result<Vec<MySqlFeature>> {
        let point_wkt = format!("POINT({} {})", x, y);

        let sql = format!(
            "SELECT *, ST_AsText({}) as geom_wkt FROM {} WHERE ST_Distance({}, ST_GeomFromText(?)) <= ?",
            self.geometry_column, self.table_name, self.geometry_column
        );

        let mut conn = self.connector.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(&sql, (&point_wkt, distance))
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        self.rows_to_features(rows)
    }

    /// Count features in the table.
    pub async fn count(&self) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) FROM {}", self.table_name);

        let mut conn = self.connector.get_conn().await?;
        let count: Option<i64> = conn
            .query_first(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        count.ok_or_else(|| Error::Query("Failed to get count".to_string()))
    }

    /// Count features matching a WHERE clause.
    pub async fn count_where(&self, where_clause: &str) -> Result<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM {} WHERE {}",
            self.table_name, where_clause
        );

        let mut conn = self.connector.get_conn().await?;
        let count: Option<i64> = conn
            .query_first(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        count.ok_or_else(|| Error::Query("Failed to get count".to_string()))
    }

    /// Get the bounding box of all geometries.
    pub async fn extent(&self) -> Result<(f64, f64, f64, f64)> {
        let sql = format!(
            "SELECT ST_AsText(ST_Envelope(ST_Union({}))) FROM {}",
            self.geometry_column, self.table_name
        );

        let mut conn = self.connector.get_conn().await?;
        let extent_wkt: Option<String> = conn
            .query_first(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        let wkt = extent_wkt.ok_or_else(|| Error::Query("Failed to get extent".to_string()))?;
        let geom = wkt_to_geometry(&wkt)?;

        match geom {
            Geometry::Polygon(poly) => {
                let coords: Vec<_> = poly.exterior().coords().collect();
                if coords.len() >= 4 {
                    let min_x = coords[0].x;
                    let min_y = coords[0].y;
                    let max_x = coords[2].x;
                    let max_y = coords[2].y;
                    Ok((min_x, min_y, max_x, max_y))
                } else {
                    Err(Error::GeometryParsing("Invalid extent polygon".to_string()))
                }
            }
            _ => Err(Error::GeometryParsing(
                "Extent is not a polygon".to_string(),
            )),
        }
    }

    /// Read features with custom query.
    async fn read_with_query(&self, sql: &str) -> Result<Vec<MySqlFeature>> {
        let mut conn = self.connector.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .query(sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        self.rows_to_features(rows)
    }

    /// Convert rows to features.
    fn rows_to_features(&self, rows: Vec<mysql_async::Row>) -> Result<Vec<MySqlFeature>> {
        let mut features = Vec::with_capacity(rows.len());

        for row in rows {
            let id: i64 = row
                .get("id")
                .ok_or_else(|| Error::TypeConversion("Missing id column".to_string()))?;

            let geom_wkt: String = row
                .get("geom_wkt")
                .ok_or_else(|| Error::TypeConversion("Missing geom_wkt column".to_string()))?;

            let geometry = wkt_to_geometry(&geom_wkt)?;

            let mut properties = HashMap::new();
            let columns = row.columns();

            for (i, column) in columns.iter().enumerate() {
                let col_name = column.name_str();
                if col_name.as_ref() != "id"
                    && col_name.as_ref() != self.geometry_column.as_str()
                    && col_name.as_ref() != "geom_wkt"
                {
                    if let Some(value) = row.get_opt::<mysql_async::Value, _>(i) {
                        let value = value.map_err(|e| Error::TypeConversion(e.to_string()))?;
                        let json_value = mysql_value_to_json(value)?;
                        properties.insert(col_name.to_string(), json_value);
                    }
                }
            }

            features.push(MySqlFeature {
                id,
                geometry,
                properties,
            });
        }

        Ok(features)
    }
}

/// Convert MySQL value to JSON value.
fn mysql_value_to_json(value: mysql_async::Value) -> Result<Value> {
    match value {
        mysql_async::Value::NULL => Ok(Value::Null),
        mysql_async::Value::Bytes(bytes) => String::from_utf8(bytes)
            .map(Value::String)
            .map_err(|e| Error::TypeConversion(e.to_string())),
        mysql_async::Value::Int(i) => Ok(Value::Number(i.into())),
        mysql_async::Value::UInt(u) => Ok(Value::Number(u.into())),
        mysql_async::Value::Float(f) => serde_json::Number::from_f64(f as f64)
            .map(Value::Number)
            .ok_or_else(|| Error::TypeConversion("Invalid float value".to_string())),
        mysql_async::Value::Double(d) => serde_json::Number::from_f64(d)
            .map(Value::Number)
            .ok_or_else(|| Error::TypeConversion("Invalid double value".to_string())),
        mysql_async::Value::Date(year, month, day, hour, minute, second, _) => {
            let datetime = format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                year, month, day, hour, minute, second
            );
            Ok(Value::String(datetime))
        }
        mysql_async::Value::Time(..) => {
            Ok(Value::String("TIME_VALUE".to_string())) // Simplified
        }
    }
}
