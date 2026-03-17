//! Cassandra spatial data reader.

use crate::cassandra::{CassandraConnector, types::Point};
use crate::error::{Error, Result};
use geo_types::Geometry;
use scylla::value::CqlValue;
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

        let rows_result = result
            .into_rows_result()
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        let maybe_row = rows_result
            .maybe_first_row::<(uuid::Uuid, CqlValue, CqlValue)>()
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        match maybe_row {
            Some(row_tuple) => Ok(Some(self.tuple_to_feature(row_tuple)?)),
            None => Ok(None),
        }
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

        let rows_result = result
            .into_rows_result()
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        let mut features = Vec::new();

        // Deserialize rows as (Uuid, CqlValue) for id + location
        // Additional columns beyond the first two are not captured in this tuple
        let rows_iter = rows_result
            .rows::<(uuid::Uuid, CqlValue)>()
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        for row in rows_iter {
            let (id, location_val) = row.map_err(|e| Error::Cassandra(e.to_string()))?;
            if let Ok(feature) = self.cqlvalue_to_feature(id, &location_val) {
                features.push(feature);
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

        let rows_result = result
            .into_rows_result()
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        let mut features = Vec::new();

        let rows_iter = rows_result
            .rows::<(uuid::Uuid, CqlValue)>()
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        for row in rows_iter {
            let (id, location_val) = row.map_err(|e| Error::Cassandra(e.to_string()))?;
            if let Ok(feature) = self.cqlvalue_to_feature(id, &location_val) {
                features.push(feature);
            }
        }

        Ok(features)
    }

    /// Convert a 3-element tuple to a feature (for read_by_id with extra column).
    fn tuple_to_feature(
        &self,
        (id, location_val, _extra): (uuid::Uuid, CqlValue, CqlValue),
    ) -> Result<CassandraFeature> {
        self.cqlvalue_to_feature(id, &location_val)
    }

    /// Convert CqlValue location to a CassandraFeature.
    fn cqlvalue_to_feature(
        &self,
        id: uuid::Uuid,
        location_val: &CqlValue,
    ) -> Result<CassandraFeature> {
        // Extract location from UDT point
        let location = if let CqlValue::UserDefinedType { fields, .. } = location_val {
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

        let properties = HashMap::new();

        Ok(CassandraFeature {
            id,
            location,
            properties,
        })
    }
}
