//! PostGIS reader for streaming features from database
//!
//! This module provides functionality to read features from PostGIS tables.

use crate::connection::ConnectionPool;
use crate::error::{QueryError, Result};
use crate::query::SpatialQuery;
use crate::types::FeatureBuilder;
use futures::stream::{Stream, StreamExt};
use oxigdal_core::vector::feature::Feature;
use std::pin::Pin;
use tracing::debug;

/// PostGIS feature reader
pub struct PostGisReader {
    pool: ConnectionPool,
    table_name: String,
    geometry_column: String,
    id_column: Option<String>,
    batch_size: usize,
    buffer: Vec<Feature>,
}

impl PostGisReader {
    /// Creates a new PostGIS reader
    pub fn new(pool: ConnectionPool, table_name: impl Into<String>) -> Self {
        Self {
            pool,
            table_name: table_name.into(),
            geometry_column: "geom".to_string(),
            id_column: Some("id".to_string()),
            batch_size: 1000,
            buffer: Vec::new(),
        }
    }

    /// Sets the geometry column name
    pub fn geometry_column(mut self, column: impl Into<String>) -> Self {
        self.geometry_column = column.into();
        self
    }

    /// Sets the ID column name
    pub fn id_column(mut self, column: impl Into<String>) -> Self {
        self.id_column = Some(column.into());
        self
    }

    /// Sets the batch size for streaming reads
    pub const fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Reads all features from the table
    pub async fn read_all(&mut self) -> Result<Vec<Feature>> {
        let query = SpatialQuery::new(&self.table_name)?.geometry_column(&self.geometry_column);

        query.execute(&self.pool).await
    }

    /// Reads features with a custom query
    pub async fn read_with_query(&mut self, query: SpatialQuery) -> Result<Vec<Feature>> {
        query.execute(&self.pool).await
    }

    /// Streams features from the table
    pub async fn stream(&self) -> Result<Pin<Box<dyn Stream<Item = Result<Feature>> + Send + '_>>> {
        let client = self.pool.get().await?;

        let query = format!("SELECT * FROM \"{}\"", self.table_name);

        debug!("Streaming features with query: {query}");

        let row_stream = client
            .query_raw(&query, std::iter::empty::<i32>())
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let geometry_column = self.geometry_column.clone();
        let feature_stream = row_stream.map(move |result| {
            let row = result.map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

            FeatureBuilder::new()
                .geometry_column(&geometry_column)
                .build_from_row(&row)
        });

        Ok(Box::pin(feature_stream))
    }

    /// Counts the total number of features in the table
    pub async fn count(&self) -> Result<i64> {
        let query = SpatialQuery::new(&self.table_name)?;
        query.count(&self.pool).await
    }

    /// Gets the spatial extent of the table
    pub async fn extent(&self) -> Result<Option<(f64, f64, f64, f64)>> {
        let client = self.pool.get().await?;

        let query = format!(
            "SELECT ST_Extent(\"{}\")::text FROM \"{}\"",
            self.geometry_column, self.table_name
        );

        let row = client
            .query_one(&query, &[])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let extent_str: Option<String> = row.get(0);

        if let Some(extent) = extent_str {
            // Parse the extent string: "BOX(minx miny,maxx maxy)"
            let extent = extent.trim_start_matches("BOX(").trim_end_matches(')');
            let parts: Vec<&str> = extent.split(',').collect();

            if parts.len() == 2 {
                let min_parts: Vec<f64> = parts[0]
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                let max_parts: Vec<f64> = parts[1]
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();

                if min_parts.len() == 2 && max_parts.len() == 2 {
                    return Ok(Some((
                        min_parts[0],
                        min_parts[1],
                        max_parts[0],
                        max_parts[1],
                    )));
                }
            }
        }

        Ok(None)
    }

    /// Gets the SRID of the geometry column
    pub async fn srid(&self) -> Result<Option<i32>> {
        let client = self.pool.get().await?;

        let query = format!(
            "SELECT Find_SRID('public', '{}', '{}')",
            self.table_name, self.geometry_column
        );

        let row = client
            .query_one(&query, &[])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let srid: i32 = row.get(0);
        if srid == 0 { Ok(None) } else { Ok(Some(srid)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ConnectionConfig;

    #[test]
    fn test_reader_creation() {
        let config = ConnectionConfig::default();
        let pool = ConnectionPool::new(config).ok();
        assert!(pool.is_some());

        let pool = pool.expect("pool creation failed");
        let reader = PostGisReader::new(pool, "test_table");

        assert_eq!(reader.table_name, "test_table");
        assert_eq!(reader.geometry_column, "geom");
        assert_eq!(reader.batch_size, 1000);
    }

    #[test]
    fn test_reader_configuration() {
        let config = ConnectionConfig::default();
        let pool = ConnectionPool::new(config).ok();
        assert!(pool.is_some());

        let pool = pool.expect("pool creation failed");
        let reader = PostGisReader::new(pool, "test_table")
            .geometry_column("the_geom")
            .id_column("fid")
            .batch_size(500);

        assert_eq!(reader.geometry_column, "the_geom");
        assert_eq!(reader.id_column, Some("fid".to_string()));
        assert_eq!(reader.batch_size, 500);
    }
}
