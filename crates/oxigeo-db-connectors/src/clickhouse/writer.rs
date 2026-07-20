//! ClickHouse spatial data writer.

use crate::clickhouse::ClickHouseConnector;
use crate::error::{Error, Result};
use geo_types::Geometry;

/// ClickHouse spatial data writer.
pub struct ClickHouseWriter {
    connector: ClickHouseConnector,
    table_name: String,
    batch_size: usize,
}

impl ClickHouseWriter {
    /// Create a new ClickHouse writer.
    pub fn new(connector: ClickHouseConnector, table_name: String) -> Self {
        Self {
            connector,
            table_name,
            batch_size: 10000,
        }
    }

    /// Set batch size for bulk inserts.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Insert a single point.
    pub async fn insert_point(&self, id: u64, x: f64, y: f64) -> Result<()> {
        let sql = format!(
            "INSERT INTO {} (id, point) VALUES (?, (?, ?))",
            self.table_name
        );

        self.connector
            .client()
            .query(&sql)
            .bind(id)
            .bind(x)
            .bind(y)
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }

    /// Insert multiple points in batch.
    pub async fn insert_points(&self, points: &[(u64, f64, f64)]) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }

        let mut inserter: clickhouse::insert::Insert<(u64, f64, f64)> = self
            .connector
            .client()
            .insert(&self.table_name)
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        for (id, x, y) in points {
            inserter
                .write(&(*id, *x, *y))
                .await
                .map_err(|e| Error::ClickHouse(e.to_string()))?;
        }

        inserter
            .end()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }

    /// Insert geometries (only points supported for now).
    pub async fn insert_geometries(&self, features: &[(u64, Geometry<f64>)]) -> Result<()> {
        if features.is_empty() {
            return Ok(());
        }

        let points: Vec<(u64, f64, f64)> = features
            .iter()
            .filter_map(|(id, geom)| {
                if let Geometry::Point(p) = geom {
                    Some((*id, p.x(), p.y()))
                } else {
                    None
                }
            })
            .collect();

        self.insert_points(&points).await
    }

    /// Delete rows (Note: ClickHouse doesn't support DELETE in all table engines).
    pub async fn delete_where(&self, where_clause: &str) -> Result<()> {
        let sql = format!(
            "ALTER TABLE {} DELETE WHERE {}",
            self.table_name, where_clause
        );

        self.connector
            .client()
            .query(&sql)
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }

    /// Truncate the table.
    pub async fn truncate(&self) -> Result<()> {
        let sql = format!("TRUNCATE TABLE {}", self.table_name);

        self.connector
            .client()
            .query(&sql)
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }

    /// Optimize table (merge parts).
    pub async fn optimize(&self) -> Result<()> {
        let sql = format!("OPTIMIZE TABLE {} FINAL", self.table_name);

        self.connector
            .client()
            .query(&sql)
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }
}
