//! AWS Glue data catalog integration.

use crate::error::{CloudEnhancedError, Result};
use aws_sdk_glue::Client as AwsGlueClient;
use aws_sdk_glue::types::{Column, DatabaseInput, StorageDescriptor, TableInput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Glue data catalog client.
#[derive(Debug, Clone)]
pub struct GlueClient {
    client: Arc<AwsGlueClient>,
}

impl GlueClient {
    /// Creates a new Glue client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AwsConfig) -> Result<Self> {
        let client = AwsGlueClient::new(config.sdk_config());
        Ok(Self {
            client: Arc::new(client),
        })
    }

    /// Creates a database in the Glue catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be created.
    pub async fn create_database(
        &self,
        name: &str,
        description: Option<&str>,
        location_uri: Option<&str>,
    ) -> Result<()> {
        let mut db_input = DatabaseInput::builder().name(name);

        if let Some(desc) = description {
            db_input = db_input.description(desc);
        }

        if let Some(loc) = location_uri {
            db_input = db_input.location_uri(loc);
        }

        self.client
            .create_database()
            .database_input(db_input.build().map_err(|e| {
                CloudEnhancedError::data_catalog(format!("Failed to build database input: {}", e))
            })?)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to create database: {}", e))
            })?;

        Ok(())
    }

    /// Gets a database from the Glue catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be retrieved.
    pub async fn get_database(&self, name: &str) -> Result<DatabaseMetadata> {
        let response = self
            .client
            .get_database()
            .name(name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to get database: {}", e))
            })?;

        let database = response
            .database
            .ok_or_else(|| CloudEnhancedError::not_found(format!("Database {} not found", name)))?;

        Ok(DatabaseMetadata {
            name: database.name.clone(),
            description: database.description.clone(),
            location_uri: database.location_uri.clone(),
            parameters: database.parameters.clone().unwrap_or_default(),
            create_time: database.create_time,
        })
    }

    /// Lists databases in the Glue catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the databases cannot be listed.
    pub async fn list_databases(&self, max_results: Option<i32>) -> Result<Vec<String>> {
        let mut request = self.client.get_databases();

        if let Some(max) = max_results {
            request = request.max_results(max);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to list databases: {}", e))
        })?;

        Ok(response
            .database_list()
            .iter()
            .map(|db| db.name.clone())
            .collect())
    }

    /// Deletes a database from the Glue catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be deleted.
    pub async fn delete_database(&self, name: &str) -> Result<()> {
        self.client
            .delete_database()
            .name(name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to delete database: {}", e))
            })?;

        Ok(())
    }

    /// Creates a table in the Glue catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the table cannot be created.
    pub async fn create_table(&self, database: &str, table: TableDefinition) -> Result<()> {
        let columns: Result<Vec<Column>> = table
            .columns
            .into_iter()
            .map(|col| {
                Column::builder()
                    .name(col.name)
                    .r#type(col.data_type)
                    .set_comment(col.comment)
                    .build()
                    .map_err(|e| {
                        CloudEnhancedError::aws_service(format!("Failed to build column: {}", e))
                    })
            })
            .collect();
        let columns = columns?;

        let storage_descriptor = StorageDescriptor::builder()
            .set_columns(Some(columns))
            .set_location(table.location)
            .set_input_format(table.input_format)
            .set_output_format(table.output_format)
            .set_serde_info(table.serde_info.map(|info| {
                aws_sdk_glue::types::SerDeInfo::builder()
                    .set_name(info.name)
                    .set_serialization_library(info.serialization_library)
                    .set_parameters(if info.parameters.is_empty() {
                        None
                    } else {
                        Some(info.parameters)
                    })
                    .build()
            }))
            .build();

        let mut table_input = TableInput::builder()
            .name(table.name)
            .storage_descriptor(storage_descriptor);

        if let Some(desc) = table.description {
            table_input = table_input.description(desc);
        }

        if !table.parameters.is_empty() {
            table_input = table_input.set_parameters(Some(table.parameters));
        }

        self.client
            .create_table()
            .database_name(database)
            .table_input(table_input.build().map_err(|e| {
                CloudEnhancedError::data_catalog(format!("Failed to build table input: {}", e))
            })?)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to create table: {}", e))
            })?;

        Ok(())
    }

    /// Gets a table from the Glue catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the table cannot be retrieved.
    pub async fn get_table(&self, database: &str, name: &str) -> Result<TableMetadata> {
        let response = self
            .client
            .get_table()
            .database_name(database)
            .name(name)
            .send()
            .await
            .map_err(|e| CloudEnhancedError::aws_service(format!("Failed to get table: {}", e)))?;

        let table = response.table.ok_or_else(|| {
            CloudEnhancedError::not_found(format!("Table {}.{} not found", database, name))
        })?;

        let storage_desc = table.storage_descriptor;
        let columns = storage_desc
            .as_ref()
            .and_then(|sd| sd.columns.clone())
            .unwrap_or_default();

        Ok(TableMetadata {
            name: table.name.clone(),
            database_name: table.database_name.clone().unwrap_or_default(),
            description: table.description.clone(),
            location: storage_desc.as_ref().and_then(|sd| sd.location.clone()),
            parameters: table.parameters.clone().unwrap_or_default(),
            create_time: table.create_time,
            update_time: table.update_time,
            columns: columns
                .into_iter()
                .map(|col| ColumnMetadata {
                    name: col.name.clone(),
                    data_type: col.r#type.clone().unwrap_or_default(),
                    comment: col.comment,
                })
                .collect(),
        })
    }

    /// Lists tables in a database.
    ///
    /// # Errors
    ///
    /// Returns an error if the tables cannot be listed.
    pub async fn list_tables(
        &self,
        database: &str,
        max_results: Option<i32>,
    ) -> Result<Vec<String>> {
        let mut request = self.client.get_tables().database_name(database);

        if let Some(max) = max_results {
            request = request.max_results(max);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to list tables: {}", e))
        })?;

        Ok(response
            .table_list
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.name.clone())
            .collect())
    }

    /// Deletes a table from the Glue catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the table cannot be deleted.
    pub async fn delete_table(&self, database: &str, name: &str) -> Result<()> {
        self.client
            .delete_table()
            .database_name(database)
            .name(name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to delete table: {}", e))
            })?;

        Ok(())
    }

    /// Updates table partitions.
    ///
    /// # Errors
    ///
    /// Returns an error if the partitions cannot be updated.
    pub async fn update_partitions(&self, database: &str, table: &str) -> Result<()> {
        // This would typically involve calling batch_create_partition or similar
        // For now, we'll just demonstrate the pattern
        tracing::info!("Updating partitions for table {}.{}", database, table);
        Ok(())
    }
}

/// Database metadata from Glue catalog.
#[derive(Debug, Clone)]
pub struct DatabaseMetadata {
    /// Database name
    pub name: String,
    /// Database description
    pub description: Option<String>,
    /// Location URI
    pub location_uri: Option<String>,
    /// Parameters
    pub parameters: HashMap<String, String>,
    /// Create time
    pub create_time: Option<aws_smithy_types::DateTime>,
}

/// Table definition for creating a table.
#[derive(Debug, Clone)]
pub struct TableDefinition {
    /// Table name
    pub name: String,
    /// Table description
    pub description: Option<String>,
    /// Column definitions
    pub columns: Vec<ColumnDefinition>,
    /// Storage location
    pub location: Option<String>,
    /// Input format
    pub input_format: Option<String>,
    /// Output format
    pub output_format: Option<String>,
    /// SerDe information
    pub serde_info: Option<SerDeInfo>,
    /// Table parameters
    pub parameters: HashMap<String, String>,
}

/// Column definition.
#[derive(Debug, Clone)]
pub struct ColumnDefinition {
    /// Column name
    pub name: String,
    /// Data type
    pub data_type: String,
    /// Comment
    pub comment: Option<String>,
}

/// SerDe information.
#[derive(Debug, Clone)]
pub struct SerDeInfo {
    /// SerDe name
    pub name: Option<String>,
    /// Serialization library
    pub serialization_library: Option<String>,
    /// Parameters
    pub parameters: HashMap<String, String>,
}

/// Table metadata from Glue catalog.
#[derive(Debug, Clone)]
pub struct TableMetadata {
    /// Table name
    pub name: String,
    /// Database name
    pub database_name: String,
    /// Table description
    pub description: Option<String>,
    /// Storage location
    pub location: Option<String>,
    /// Parameters
    pub parameters: HashMap<String, String>,
    /// Create time
    pub create_time: Option<aws_smithy_types::DateTime>,
    /// Update time
    pub update_time: Option<aws_smithy_types::DateTime>,
    /// Columns
    pub columns: Vec<ColumnMetadata>,
}

/// Column metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMetadata {
    /// Column name
    pub name: String,
    /// Data type
    pub data_type: String,
    /// Comment
    pub comment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_definition() {
        let col = ColumnDefinition {
            name: "id".to_string(),
            data_type: "bigint".to_string(),
            comment: Some("Primary key".to_string()),
        };

        assert_eq!(col.name, "id");
        assert_eq!(col.data_type, "bigint");
    }

    #[test]
    fn test_table_definition() {
        let table = TableDefinition {
            name: "test_table".to_string(),
            description: Some("Test table".to_string()),
            columns: vec![],
            location: Some("s3://bucket/path".to_string()),
            input_format: Some("org.apache.hadoop.mapred.TextInputFormat".to_string()),
            output_format: Some(
                "org.apache.hadoop.hive.ql.io.HiveIgnoreKeyTextOutputFormat".to_string(),
            ),
            serde_info: None,
            parameters: HashMap::new(),
        };

        assert_eq!(table.name, "test_table");
        assert!(table.location.is_some());
    }
}
