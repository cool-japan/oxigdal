//! Cassandra spatial data reader.

use crate::cassandra::{CassandraConnector, types::Point};
use crate::error::{Error, Result};
use geo_types::Geometry;
use scylla::frame::response::result::CqlValue;
use std::collections::HashMap;

/// Feature read from Cassandra.
#[derive(Debug, Clone)]
pub struct CassandraFeature {
    /// Partition key value.
    pub id: uuid::Uuid,
    /// Location point.
    pub location: Point,
    /// Additional properties.
    pub properties: HashMap<String, CqlValue>,
}

impl CassandraFeature {
    /// Convert location to geo-types geometry.
    pub fn to_geometry(&self) -> Geometry<f64> {
        Geometry::Point(self.location.into())
    }
}

/// Cassandra spatial data reader.
pub struct CassandraReader {
    connector: CassandraConnector,
    table_name: String,
}

impl CassandraReader {
    /// Create a new Cassandra reader.
    pub fn new(connector: CassandraConnector, table_name: String) -> Self {
        Self {
            connector,
            table_name,
        }
    }

    /// Read a feature by partition key.
    pub async fn read_by_id(&self, id: uuid::Uuid) -> Result<Option<CassandraFeature>> {
        let cql = format!("SELECT * FROM {} WHERE id = ?", self.table_name);

        let result = self
            .connector
            .session()
            .query_unpaged(cql, (id,))
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        if let Some(rows) = result.rows {
            if let Some(row) = rows.first() {
                return Ok(Some(self.row_to_feature(row)?));
            }
        }

        Ok(None)
    }

    /// Scan all features (use with caution - may be expensive).
    pub async fn scan_all(&self) -> Result<Vec<CassandraFeature>> {
        let cql = format!("SELECT * FROM {}", self.table_name);

        let result = self
            .connector
            .session()
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        let mut features = Vec::new();

        if let Some(rows) = result.rows {
            for row in rows {
                if let Ok(feature) = self.row_to_feature(&row) {
                    features.push(feature);
                }
            }
        }

        Ok(features)
    }

    /// Read features with a custom CQL query.
    pub async fn query(&self, cql: &str) -> Result<Vec<CassandraFeature>> {
        let result = self
            .connector
            .session()
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        let mut features = Vec::new();

        if let Some(rows) = result.rows {
            for row in rows {
                if let Ok(feature) = self.row_to_feature(&row) {
                    features.push(feature);
                }
            }
        }

        Ok(features)
    }

    /// Convert row to feature.
    fn row_to_feature(
        &self,
        row: &scylla::frame::response::result::Row,
    ) -> Result<CassandraFeature> {
        // Extract id (assuming first column is UUID)
        let id = if let Some(Some(CqlValue::Uuid(uuid))) = row.columns.first() {
            *uuid
        } else {
            return Err(Error::Cassandra("Missing or invalid id column".to_string()));
        };

        // Extract location (assuming second column is UDT point)
        let location =
            if let Some(Some(CqlValue::UserDefinedType { fields, .. })) = row.columns.get(1) {
                let x = if let Some((_, Some(CqlValue::Double(x_val)))) = fields.first() {
                    *x_val
                } else {
                    return Err(Error::Cassandra("Invalid point x coordinate".to_string()));
                };

                let y = if let Some((_, Some(CqlValue::Double(y_val)))) = fields.get(1) {
                    *y_val
                } else {
                    return Err(Error::Cassandra("Invalid point y coordinate".to_string()));
                };

                Point::new(x, y)
            } else {
                return Err(Error::Cassandra(
                    "Missing or invalid location column".to_string(),
                ));
            };

        let mut properties = HashMap::new();

        // Collect remaining columns as properties
        for (i, value) in row.columns.iter().enumerate().skip(2) {
            if let Some(val) = value {
                properties.insert(format!("col_{}", i), val.clone());
            }
        }

        Ok(CassandraFeature {
            id,
            location,
            properties,
        })
    }
}
