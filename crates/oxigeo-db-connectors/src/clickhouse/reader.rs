//! ClickHouse spatial data reader.

use crate::clickhouse::ClickHouseConnector;
use crate::error::{Error, Result};
use clickhouse::Row;
use geo_types::Geometry;
use serde::Deserialize;

/// Basic feature row with point geometry.
#[derive(Debug, Row, Deserialize)]
pub struct PointFeature {
    /// Feature ID.
    pub id: u64,
    /// Point coordinates as (x, y) tuple.
    pub point: (f64, f64),
}

impl PointFeature {
    /// Convert to geo-types geometry.
    pub fn to_geometry(&self) -> Geometry<f64> {
        Geometry::Point(geo_types::Point::new(self.point.0, self.point.1))
    }
}

/// ClickHouse spatial data reader.
pub struct ClickHouseReader {
    connector: ClickHouseConnector,
    table_name: String,
}

impl ClickHouseReader {
    /// Create a new ClickHouse reader.
    pub fn new(connector: ClickHouseConnector, table_name: String) -> Self {
        Self {
            connector,
            table_name,
        }
    }

    /// Read all point features from the table.
    pub async fn read_all_points(&self) -> Result<Vec<PointFeature>> {
        let sql = format!("SELECT id, point FROM {}", self.table_name);

        self.connector
            .client()
            .query(&sql)
            .fetch_all()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))
    }

    /// Read points within a bounding box.
    pub async fn read_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<Vec<PointFeature>> {
        let sql = format!(
            "SELECT id, point FROM {} WHERE tupleElement(point, 1) >= ? AND tupleElement(point, 1) <= ? AND tupleElement(point, 2) >= ? AND tupleElement(point, 2) <= ?",
            self.table_name
        );

        self.connector
            .client()
            .query(&sql)
            .bind(min_x)
            .bind(max_x)
            .bind(min_y)
            .bind(max_y)
            .fetch_all()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))
    }

    /// Read points within a circular region.
    pub async fn read_within_distance(
        &self,
        center_x: f64,
        center_y: f64,
        radius: f64,
    ) -> Result<Vec<PointFeature>> {
        let sql = format!(
            "SELECT id, point FROM {} WHERE sqrt(pow(tupleElement(point, 1) - ?, 2) + pow(tupleElement(point, 2) - ?, 2)) <= ?",
            self.table_name
        );

        self.connector
            .client()
            .query(&sql)
            .bind(center_x)
            .bind(center_y)
            .bind(radius)
            .fetch_all()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))
    }

    /// Count features in the table.
    pub async fn count(&self) -> Result<u64> {
        self.connector.count_table(&self.table_name).await
    }

    /// Count features within a bounding box.
    pub async fn count_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<u64> {
        let sql = format!(
            "SELECT count() FROM {} WHERE tupleElement(point, 1) >= ? AND tupleElement(point, 1) <= ? AND tupleElement(point, 2) >= ? AND tupleElement(point, 2) <= ?",
            self.table_name
        );

        self.connector
            .client()
            .query(&sql)
            .bind(min_x)
            .bind(max_x)
            .bind(min_y)
            .bind(max_y)
            .fetch_one()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))
    }

    /// Execute a custom query returning point features.
    pub async fn query_points(&self, sql: &str) -> Result<Vec<PointFeature>> {
        self.connector
            .client()
            .query(sql)
            .fetch_all()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))
    }
}
