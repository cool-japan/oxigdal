//! TimescaleDB continuous aggregates for real-time analytics.

use crate::error::{Error, Result};
use crate::sql::{escape_pg_literal, quote_pg_ident};
use crate::timescale::TimescaleConnector;
use tokio_postgres::types::ToSql;

/// Continuous aggregate manager.
pub struct ContinuousAggregateManager {
    connector: TimescaleConnector,
    view_name: String,
    source_table: String,
}

impl ContinuousAggregateManager {
    /// Create a new continuous aggregate manager.
    pub fn new(connector: TimescaleConnector, view_name: String, source_table: String) -> Self {
        Self {
            connector,
            view_name,
            source_table,
        }
    }

    /// Create a continuous aggregate for spatial-temporal data.
    ///
    /// # Security
    ///
    /// `time_column`, and the manager's view/source-table names, are validated
    /// and quoted as identifiers, and `bucket_interval` is escaped as a string
    /// literal. `aggregate_query` is a raw SQL fragment (the select-list of the
    /// aggregate, e.g. `count(*), avg(temp)`) spliced **verbatim** — it is
    /// **not** injection-safe and must be a trusted, developer-authored
    /// expression, never unsanitized user input. (A `CREATE MATERIALIZED VIEW`
    /// definition cannot use bind parameters, so these values are escaped
    /// rather than parameterized.)
    pub async fn create(
        &self,
        time_column: &str,
        bucket_interval: &str,
        aggregate_query: &str,
    ) -> Result<()> {
        let client = self.connector.get_conn().await?;

        let view = quote_pg_ident(&self.view_name)?;
        let source = quote_pg_ident(&self.source_table)?;
        let time_col = quote_pg_ident(time_column)?;
        let bucket = escape_pg_literal(bucket_interval)?;

        let sql = format!(
            "CREATE MATERIALIZED VIEW {view} WITH (timescaledb.continuous) AS SELECT time_bucket(INTERVAL '{bucket}', {time_col}) as bucket, {aggregate_query} FROM {source} GROUP BY bucket"
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Add a refresh policy to the continuous aggregate.
    pub async fn add_refresh_policy(
        &self,
        start_offset: &str,
        end_offset: &str,
        schedule_interval: &str,
    ) -> Result<()> {
        let client = self.connector.get_conn().await?;

        // Bind the view (as regclass) and each offset (text cast to interval)
        // as parameters, removing the injection surface.
        let sql = "SELECT add_continuous_aggregate_policy($1::regclass, start_offset => $2::interval, end_offset => $3::interval, schedule_interval => $4::interval)";
        let params: [&(dyn ToSql + Sync); 4] = [
            &self.view_name,
            &start_offset,
            &end_offset,
            &schedule_interval,
        ];

        client
            .execute(sql, &params)
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Refresh the continuous aggregate.
    ///
    /// When both `start_time` and `end_time` are supplied they are bound as
    /// `timestamptz` parameters; otherwise the whole range is refreshed
    /// (`NULL, NULL`).
    pub async fn refresh(&self, start_time: Option<&str>, end_time: Option<&str>) -> Result<()> {
        let client = self.connector.get_conn().await?;

        match (start_time, end_time) {
            (Some(start), Some(end)) => {
                let sql = "CALL refresh_continuous_aggregate($1::regclass, $2::timestamptz, $3::timestamptz)";
                let params: [&(dyn ToSql + Sync); 3] = [&self.view_name, &start, &end];
                client
                    .execute(sql, &params)
                    .await
                    .map_err(|e| Error::TimescaleDB(e.to_string()))?;
            }
            _ => {
                let sql = "CALL refresh_continuous_aggregate($1::regclass, NULL, NULL)";
                let params: [&(dyn ToSql + Sync); 1] = [&self.view_name];
                client
                    .execute(sql, &params)
                    .await
                    .map_err(|e| Error::TimescaleDB(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Drop the continuous aggregate.
    pub async fn drop(&self) -> Result<()> {
        let client = self.connector.get_conn().await?;

        // DDL cannot be parameterized; validate + quote the view identifier.
        let sql = format!(
            "DROP MATERIALIZED VIEW IF EXISTS {} CASCADE",
            quote_pg_ident(&self.view_name)?
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Query the continuous aggregate.
    ///
    /// # Security
    ///
    /// The view name is validated + quoted, but `where_clause` is a raw SQL
    /// fragment spliced verbatim after `WHERE`; it is **not** injection-safe.
    /// Pass only trusted, developer-authored fragments — never unsanitized user
    /// input.
    pub async fn query(&self, where_clause: Option<&str>) -> Result<Vec<tokio_postgres::Row>> {
        let client = self.connector.get_conn().await?;

        let view = quote_pg_ident(&self.view_name)?;
        let sql = if let Some(clause) = where_clause {
            format!("SELECT * FROM {view} WHERE {clause}")
        } else {
            format!("SELECT * FROM {view}")
        };

        let rows = client
            .query(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(rows)
    }
}
