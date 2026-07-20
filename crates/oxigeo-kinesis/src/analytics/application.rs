//! Analytics application management

use crate::error::{KinesisError, Result};
use aws_sdk_kinesisanalyticsv2::Client as AnalyticsClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationConfig {
    /// Application name
    pub application_name: String,
    /// Runtime environment
    pub runtime_environment: RuntimeEnvironment,
    /// Service execution role ARN
    pub service_execution_role: String,
    /// Input configurations
    pub inputs: Vec<InputConfig>,
    /// Output configurations
    pub outputs: Vec<OutputConfig>,
    /// SQL code (for SQL applications)
    pub sql_code: Option<String>,
    /// Application properties
    pub properties: Option<Vec<PropertyGroup>>,
}

impl ApplicationConfig {
    /// Creates a new application configuration
    pub fn new(
        application_name: impl Into<String>,
        runtime_environment: RuntimeEnvironment,
        service_execution_role: impl Into<String>,
    ) -> Self {
        Self {
            application_name: application_name.into(),
            runtime_environment,
            service_execution_role: service_execution_role.into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            sql_code: None,
            properties: None,
        }
    }

    /// Adds an input
    pub fn add_input(mut self, input: InputConfig) -> Self {
        self.inputs.push(input);
        self
    }

    /// Adds an output
    pub fn add_output(mut self, output: OutputConfig) -> Self {
        self.outputs.push(output);
        self
    }

    /// Sets the SQL code
    pub fn with_sql_code(mut self, sql: impl Into<String>) -> Self {
        self.sql_code = Some(sql.into());
        self
    }

    /// Adds a property group
    pub fn add_property_group(mut self, group: PropertyGroup) -> Self {
        self.properties.get_or_insert_with(Vec::new).push(group);
        self
    }
}

/// Runtime environment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeEnvironment {
    /// SQL 1.0
    Sql10,
    /// Flink 1.6
    Flink16,
    /// Flink 1.8
    Flink18,
    /// Flink 1.11
    Flink111,
    /// Flink 1.13
    Flink113,
    /// Flink 1.15
    Flink115,
}

impl RuntimeEnvironment {
    /// Converts to AWS SDK runtime environment
    pub fn to_aws_runtime(&self) -> &str {
        match self {
            Self::Sql10 => "SQL-1_0",
            Self::Flink16 => "FLINK-1_6",
            Self::Flink18 => "FLINK-1_8",
            Self::Flink111 => "FLINK-1_11",
            Self::Flink113 => "FLINK-1_13",
            Self::Flink115 => "FLINK-1_15",
        }
    }
}

/// Input configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// Input name prefix
    pub name_prefix: String,
    /// Kinesis stream ARN
    pub kinesis_stream_arn: String,
    /// Input schema
    pub schema: InputSchema,
    /// Input parallelism
    pub parallelism: Option<i32>,
}

impl InputConfig {
    /// Creates a new input configuration
    pub fn new(
        name_prefix: impl Into<String>,
        kinesis_stream_arn: impl Into<String>,
        schema: InputSchema,
    ) -> Self {
        Self {
            name_prefix: name_prefix.into(),
            kinesis_stream_arn: kinesis_stream_arn.into(),
            schema,
            parallelism: None,
        }
    }

    /// Sets the input parallelism
    pub fn with_parallelism(mut self, parallelism: i32) -> Self {
        self.parallelism = Some(parallelism);
        self
    }
}

/// Input schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSchema {
    /// Record format
    pub record_format: RecordFormat,
    /// Record encoding
    pub record_encoding: Option<String>,
    /// Record columns
    pub columns: Vec<RecordColumn>,
}

impl InputSchema {
    /// Creates a new input schema
    pub fn new(record_format: RecordFormat) -> Self {
        Self {
            record_format,
            record_encoding: Some("UTF-8".to_string()),
            columns: Vec::new(),
        }
    }

    /// Adds a column
    pub fn add_column(mut self, column: RecordColumn) -> Self {
        self.columns.push(column);
        self
    }
}

/// Record format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordFormat {
    /// JSON format
    Json {
        /// JSON mapping path
        record_row_path: Option<String>,
    },
    /// CSV format
    Csv {
        /// Record row delimiter
        record_row_delimiter: String,
        /// Record column delimiter
        record_column_delimiter: String,
    },
}

/// Record column
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordColumn {
    /// Column name
    pub name: String,
    /// SQL type
    pub sql_type: String,
    /// Mapping (for nested JSON)
    pub mapping: Option<String>,
}

impl RecordColumn {
    /// Creates a new record column
    pub fn new(name: impl Into<String>, sql_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sql_type: sql_type.into(),
            mapping: None,
        }
    }

    /// Sets the JSON mapping
    pub fn with_mapping(mut self, mapping: impl Into<String>) -> Self {
        self.mapping = Some(mapping.into());
        self
    }
}

/// Output configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output name
    pub name: String,
    /// Kinesis stream ARN (for stream output)
    pub kinesis_stream_arn: Option<String>,
    /// Kinesis Firehose ARN (for Firehose output)
    pub kinesis_firehose_arn: Option<String>,
    /// Lambda ARN (for Lambda output)
    pub lambda_arn: Option<String>,
    /// Destination schema
    pub destination_schema: DestinationSchema,
}

impl OutputConfig {
    /// Creates a new output configuration for Kinesis stream
    pub fn kinesis_stream(
        name: impl Into<String>,
        stream_arn: impl Into<String>,
        schema: DestinationSchema,
    ) -> Self {
        Self {
            name: name.into(),
            kinesis_stream_arn: Some(stream_arn.into()),
            kinesis_firehose_arn: None,
            lambda_arn: None,
            destination_schema: schema,
        }
    }

    /// Creates a new output configuration for Kinesis Firehose
    pub fn kinesis_firehose(
        name: impl Into<String>,
        firehose_arn: impl Into<String>,
        schema: DestinationSchema,
    ) -> Self {
        Self {
            name: name.into(),
            kinesis_stream_arn: None,
            kinesis_firehose_arn: Some(firehose_arn.into()),
            lambda_arn: None,
            destination_schema: schema,
        }
    }

    /// Creates a new output configuration for Lambda
    pub fn lambda(
        name: impl Into<String>,
        lambda_arn: impl Into<String>,
        schema: DestinationSchema,
    ) -> Self {
        Self {
            name: name.into(),
            kinesis_stream_arn: None,
            kinesis_firehose_arn: None,
            lambda_arn: Some(lambda_arn.into()),
            destination_schema: schema,
        }
    }
}

/// Destination schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestinationSchema {
    /// Record format type
    pub record_format_type: String,
}

impl DestinationSchema {
    /// Creates a JSON destination schema
    pub fn json() -> Self {
        Self {
            record_format_type: "JSON".to_string(),
        }
    }

    /// Creates a CSV destination schema
    pub fn csv() -> Self {
        Self {
            record_format_type: "CSV".to_string(),
        }
    }
}

/// Property group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyGroup {
    /// Property group ID
    pub property_group_id: String,
    /// Property map
    pub property_map: std::collections::HashMap<String, String>,
}

impl PropertyGroup {
    /// Creates a new property group
    pub fn new(property_group_id: impl Into<String>) -> Self {
        Self {
            property_group_id: property_group_id.into(),
            property_map: std::collections::HashMap::new(),
        }
    }

    /// Adds a property
    pub fn add_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.property_map.insert(key.into(), value.into());
        self
    }
}

/// Application status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplicationStatus {
    /// Deleting
    Deleting,
    /// Starting
    Starting,
    /// Stopping
    Stopping,
    /// Ready
    Ready,
    /// Running
    Running,
    /// Updating
    Updating,
    /// Autoscaling
    Autoscaling,
    /// Force stopping
    ForceStopping,
}

/// Analytics application
pub struct AnalyticsApplication {
    client: Arc<AnalyticsClient>,
    config: ApplicationConfig,
}

impl AnalyticsApplication {
    /// Creates a new analytics application
    pub fn new(client: AnalyticsClient, config: ApplicationConfig) -> Self {
        Self {
            client: Arc::new(client),
            config,
        }
    }

    /// Creates the application in AWS
    pub async fn create(&self) -> Result<String> {
        info!(
            "Creating analytics application: {}",
            self.config.application_name
        );

        // This is a simplified version - full implementation would build all configurations
        let response = self
            .client
            .create_application()
            .application_name(&self.config.application_name)
            .runtime_environment(aws_sdk_kinesisanalyticsv2::types::RuntimeEnvironment::from(
                self.config.runtime_environment.to_aws_runtime(),
            ))
            .service_execution_role(&self.config.service_execution_role)
            .send()
            .await
            .map_err(|e| KinesisError::Analytics {
                message: e.to_string(),
            })?;

        let detail = response
            .application_detail()
            .ok_or_else(|| KinesisError::Analytics {
                message: "Application detail not returned".to_string(),
            })?;

        let arn = detail.application_arn().to_string();
        info!("Analytics application created: {}", arn);

        Ok(arn)
    }

    /// Gets the application configuration
    pub fn config(&self) -> &ApplicationConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_application_config() {
        let config = ApplicationConfig::new(
            "test-app",
            RuntimeEnvironment::Sql10,
            "arn:aws:iam::123456789012:role/service-role",
        );

        assert_eq!(config.application_name, "test-app");
        assert_eq!(config.runtime_environment, RuntimeEnvironment::Sql10);
    }

    #[test]
    fn test_runtime_environment_conversion() {
        assert_eq!(RuntimeEnvironment::Sql10.to_aws_runtime(), "SQL-1_0");
        assert_eq!(RuntimeEnvironment::Flink16.to_aws_runtime(), "FLINK-1_6");
        assert_eq!(RuntimeEnvironment::Flink115.to_aws_runtime(), "FLINK-1_15");
    }

    #[test]
    fn test_input_config() {
        let schema = InputSchema::new(RecordFormat::Json {
            record_row_path: Some("$".to_string()),
        })
        .add_column(RecordColumn::new("timestamp", "BIGINT"))
        .add_column(RecordColumn::new("value", "DOUBLE"));

        let input = InputConfig::new(
            "SOURCE_SQL_STREAM",
            "arn:aws:kinesis:us-east-1:123456789012:stream/input-stream",
            schema,
        )
        .with_parallelism(1);

        assert_eq!(input.name_prefix, "SOURCE_SQL_STREAM");
        assert_eq!(input.parallelism, Some(1));
    }

    #[test]
    fn test_output_config_kinesis_stream() {
        let output = OutputConfig::kinesis_stream(
            "DESTINATION_SQL_STREAM",
            "arn:aws:kinesis:us-east-1:123456789012:stream/output-stream",
            DestinationSchema::json(),
        );

        assert_eq!(output.name, "DESTINATION_SQL_STREAM");
        assert!(output.kinesis_stream_arn.is_some());
        assert!(output.kinesis_firehose_arn.is_none());
    }

    #[test]
    fn test_output_config_firehose() {
        let output = OutputConfig::kinesis_firehose(
            "DESTINATION_SQL_STREAM",
            "arn:aws:firehose:us-east-1:123456789012:deliverystream/output-stream",
            DestinationSchema::json(),
        );

        assert_eq!(output.name, "DESTINATION_SQL_STREAM");
        assert!(output.kinesis_stream_arn.is_none());
        assert!(output.kinesis_firehose_arn.is_some());
    }

    #[test]
    fn test_property_group() {
        let group = PropertyGroup::new("ConsumerConfigProperties")
            .add_property("flink.stream.initpos", "LATEST")
            .add_property("aws.region", "us-east-1");

        assert_eq!(group.property_group_id, "ConsumerConfigProperties");
        assert_eq!(group.property_map.len(), 2);
    }

    #[test]
    fn test_record_column() {
        let column = RecordColumn::new("userId", "VARCHAR(32)").with_mapping("$.user.id");

        assert_eq!(column.name, "userId");
        assert_eq!(column.sql_type, "VARCHAR(32)");
        assert_eq!(column.mapping, Some("$.user.id".to_string()));
    }
}
