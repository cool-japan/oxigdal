//! Cassandra spatial data writer.

use crate::cassandra::CassandraConnector;
use crate::error::{Error, Result};
use geo_types::Geometry;

/// Cassandra spatial data writer.
pub struct CassandraWriter {
    connector: CassandraConnector,
    table_name: String,
}

impl CassandraWriter {
    /// Create a new Cassandra writer.
    pub fn new(connector: CassandraConnector, table_name: String) -> Self {
        Self {
            connector,
            table_name,
        }
    }

    /// Insert a feature with a point location.
    pub async fn insert(&self, id: uuid::Uuid, x: f64, y: f64) -> Result<()> {
        let cql = format!(
            "INSERT INTO {} (id, location) VALUES (?, {{x: ?, y: ?}})",
            self.table_name
        );

        self.connector
            .session()
            .query_unpaged(cql, (id, x, y))
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }

    /// Insert a feature with geometry (only Point supported).
    pub async fn insert_geometry(&self, id: uuid::Uuid, geometry: &Geometry<f64>) -> Result<()> {
        match geometry {
            Geometry::Point(p) => self.insert(id, p.x(), p.y()).await,
            _ => Err(Error::Cassandra(
                "Only Point geometries are supported".to_string(),
            )),
        }
    }

    /// Insert multiple features in batch.
    pub async fn insert_batch(&self, features: &[(uuid::Uuid, f64, f64)]) -> Result<()> {
        if features.is_empty() {
            return Ok(());
        }

        // Prepare statement for better performance
        let cql = format!(
            "INSERT INTO {} (id, location) VALUES (?, {{x: ?, y: ?}})",
            self.table_name
        );

        let prepared = self
            .connector
            .session()
            .prepare(cql)
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        for (id, x, y) in features {
            self.connector
                .session()
                .execute_unpaged(&prepared, (*id, *x, *y))
                .await
                .map_err(|e| Error::Cassandra(e.to_string()))?;
        }

        Ok(())
    }

    /// Update a feature location.
    pub async fn update(&self, id: uuid::Uuid, x: f64, y: f64) -> Result<()> {
        let cql = format!(
            "UPDATE {} SET location = {{x: ?, y: ?}} WHERE id = ?",
            self.table_name
        );

        self.connector
            .session()
            .query_unpaged(cql, (x, y, id))
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }

    /// Delete a feature by partition key.
    pub async fn delete(&self, id: uuid::Uuid) -> Result<()> {
        let cql = format!("DELETE FROM {} WHERE id = ?", self.table_name);

        self.connector
            .session()
            .query_unpaged(cql, (id,))
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }

    /// Truncate the table (delete all data).
    pub async fn truncate(&self) -> Result<()> {
        let cql = format!("TRUNCATE {}", self.table_name);

        self.connector
            .session()
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }
}
