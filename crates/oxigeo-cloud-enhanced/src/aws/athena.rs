//! AWS Athena integration for SQL queries on S3 data.

use crate::error::{CloudEnhancedError, Result};
use aws_sdk_athena::Client as AwsAthenaClient;
use aws_sdk_athena::types::{QueryExecutionState, ResultConfiguration};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Athena client for executing SQL queries on S3 data.
#[derive(Debug, Clone)]
pub struct AthenaClient {
    client: Arc<AwsAthenaClient>,
}

impl AthenaClient {
    /// Creates a new Athena client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AwsConfig) -> Result<Self> {
        let client = AwsAthenaClient::new(config.sdk_config());
        Ok(Self {
            client: Arc::new(client),
        })
    }

    /// Executes a SQL query and returns the execution ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the query cannot be started.
    pub async fn start_query(
        &self,
        query: &str,
        database: Option<&str>,
        output_location: &str,
        workgroup: Option<&str>,
    ) -> Result<String> {
        let result_config = ResultConfiguration::builder()
            .output_location(output_location)
            .build();

        let mut request = self
            .client
            .start_query_execution()
            .query_string(query)
            .result_configuration(result_config);

        if let Some(db) = database {
            request = request.query_execution_context(
                aws_sdk_athena::types::QueryExecutionContext::builder()
                    .database(db)
                    .build(),
            );
        }

        if let Some(wg) = workgroup {
            request = request.work_group(wg);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to start Athena query: {}", e))
        })?;

        response
            .query_execution_id()
            .map(String::from)
            .ok_or_else(|| {
                CloudEnhancedError::aws_service("No query execution ID returned".to_string())
            })
    }

    /// Waits for a query to complete and returns the final state.
    ///
    /// # Errors
    ///
    /// Returns an error if the query status cannot be retrieved or the query fails.
    pub async fn wait_for_query(
        &self,
        execution_id: &str,
        poll_interval: Duration,
        timeout: Duration,
    ) -> Result<QueryExecutionState> {
        let start = std::time::Instant::now();

        loop {
            let status = self.get_query_status(execution_id).await?;

            match status {
                QueryExecutionState::Succeeded => return Ok(status),
                QueryExecutionState::Failed | QueryExecutionState::Cancelled => {
                    return Err(CloudEnhancedError::query_execution(format!(
                        "Query {} with state: {:?}",
                        execution_id, status
                    )));
                }
                QueryExecutionState::Queued | QueryExecutionState::Running => {
                    if start.elapsed() > timeout {
                        return Err(CloudEnhancedError::timeout(format!(
                            "Query {} timed out after {:?}",
                            execution_id, timeout
                        )));
                    }
                    sleep(poll_interval).await;
                }
                _ => {
                    return Err(CloudEnhancedError::aws_service(format!(
                        "Unknown query state: {:?}",
                        status
                    )));
                }
            }
        }
    }

    /// Gets the status of a query execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_query_status(&self, execution_id: &str) -> Result<QueryExecutionState> {
        let response = self
            .client
            .get_query_execution()
            .query_execution_id(execution_id)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to get query status: {}", e))
            })?;

        let execution = response.query_execution.ok_or_else(|| {
            CloudEnhancedError::aws_service("No query execution returned".to_string())
        })?;

        let status = execution.status.ok_or_else(|| {
            CloudEnhancedError::aws_service("No status in query execution".to_string())
        })?;

        status
            .state
            .ok_or_else(|| CloudEnhancedError::aws_service("No state in query status".to_string()))
    }

    /// Executes a query and waits for completion.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or times out.
    pub async fn execute_query(
        &self,
        query: &str,
        database: Option<&str>,
        output_location: &str,
        workgroup: Option<&str>,
    ) -> Result<String> {
        let execution_id = self
            .start_query(query, database, output_location, workgroup)
            .await?;

        self.wait_for_query(
            &execution_id,
            Duration::from_secs(2),
            Duration::from_secs(300),
        )
        .await?;

        Ok(execution_id)
    }

    /// Gets query results.
    ///
    /// # Errors
    ///
    /// Returns an error if the results cannot be retrieved.
    pub async fn get_query_results(&self, execution_id: &str) -> Result<Vec<Vec<String>>> {
        let response = self
            .client
            .get_query_results()
            .query_execution_id(execution_id)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to get query results: {}", e))
            })?;

        let result_set = response
            .result_set
            .ok_or_else(|| CloudEnhancedError::aws_service("No result set returned".to_string()))?;

        let rows = result_set.rows.unwrap_or_default();
        let mut results = Vec::new();

        for row in rows {
            let data = row.data.unwrap_or_default();
            let row_data: Vec<String> = data
                .into_iter()
                .map(|datum| datum.var_char_value.unwrap_or_default())
                .collect();
            results.push(row_data);
        }

        Ok(results)
    }

    /// Stops a running query execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the query cannot be stopped.
    pub async fn stop_query(&self, execution_id: &str) -> Result<()> {
        self.client
            .stop_query_execution()
            .query_execution_id(execution_id)
            .send()
            .await
            .map_err(|e| CloudEnhancedError::aws_service(format!("Failed to stop query: {}", e)))?;

        Ok(())
    }

    /// Lists query executions in a workgroup.
    ///
    /// # Errors
    ///
    /// Returns an error if the list cannot be retrieved.
    pub async fn list_query_executions(
        &self,
        workgroup: Option<&str>,
        max_results: Option<i32>,
    ) -> Result<Vec<String>> {
        let mut request = self.client.list_query_executions();

        if let Some(wg) = workgroup {
            request = request.work_group(wg);
        }

        if let Some(max) = max_results {
            request = request.max_results(max);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to list query executions: {}", e))
        })?;

        Ok(response.query_execution_ids.unwrap_or_default())
    }

    /// Gets statistics for a query execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the statistics cannot be retrieved.
    pub async fn get_query_statistics(&self, execution_id: &str) -> Result<QueryStatistics> {
        let response = self
            .client
            .get_query_execution()
            .query_execution_id(execution_id)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to get query statistics: {}", e))
            })?;

        let execution = response.query_execution.ok_or_else(|| {
            CloudEnhancedError::aws_service("No query execution returned".to_string())
        })?;

        let statistics = execution.statistics.ok_or_else(|| {
            CloudEnhancedError::aws_service("No statistics in query execution".to_string())
        })?;

        Ok(QueryStatistics {
            engine_execution_time_ms: statistics.engine_execution_time_in_millis,
            data_scanned_bytes: statistics.data_scanned_in_bytes,
            data_manifest_location: statistics.data_manifest_location,
            total_execution_time_ms: statistics.total_execution_time_in_millis,
            query_queue_time_ms: statistics.query_queue_time_in_millis,
            query_planning_time_ms: statistics.query_planning_time_in_millis,
            service_processing_time_ms: statistics.service_processing_time_in_millis,
        })
    }
}

/// Query execution statistics.
#[derive(Debug, Clone)]
pub struct QueryStatistics {
    /// Engine execution time in milliseconds
    pub engine_execution_time_ms: Option<i64>,
    /// Data scanned in bytes
    pub data_scanned_bytes: Option<i64>,
    /// Data manifest location
    pub data_manifest_location: Option<String>,
    /// Total execution time in milliseconds
    pub total_execution_time_ms: Option<i64>,
    /// Query queue time in milliseconds
    pub query_queue_time_ms: Option<i64>,
    /// Query planning time in milliseconds
    pub query_planning_time_ms: Option<i64>,
    /// Service processing time in milliseconds
    pub service_processing_time_ms: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_statistics() {
        let stats = QueryStatistics {
            engine_execution_time_ms: Some(1000),
            data_scanned_bytes: Some(1024),
            data_manifest_location: None,
            total_execution_time_ms: Some(1500),
            query_queue_time_ms: Some(100),
            query_planning_time_ms: Some(200),
            service_processing_time_ms: Some(200),
        };

        assert_eq!(stats.engine_execution_time_ms, Some(1000));
        assert_eq!(stats.data_scanned_bytes, Some(1024));
    }
}
