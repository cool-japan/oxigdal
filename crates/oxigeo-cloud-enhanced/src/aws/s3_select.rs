//! S3 Select integration for in-place queries on S3 data.

use crate::error::{CloudEnhancedError, Result};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::types::{
    CompressionType, CsvInput, CsvOutput, ExpressionType, FileHeaderInfo, InputSerialization,
    JsonInput, JsonOutput, JsonType, OutputSerialization, ParquetInput,
    SelectObjectContentEventStream,
};
use std::sync::Arc;

/// S3 Select client for executing SQL queries on S3 data.
#[derive(Debug, Clone)]
pub struct S3SelectClient {
    client: Arc<S3Client>,
}

impl S3SelectClient {
    /// Creates a new S3 Select client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AwsConfig) -> Result<Self> {
        let client = S3Client::new(config.sdk_config());
        Ok(Self {
            client: Arc::new(client),
        })
    }

    /// Executes a SQL query on CSV data in S3.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or the data cannot be retrieved.
    pub async fn query_csv(
        &self,
        bucket: &str,
        key: &str,
        query: &str,
        options: CsvSelectOptions,
    ) -> Result<Vec<u8>> {
        let csv_input = CsvInput::builder()
            .set_field_delimiter(options.field_delimiter)
            .set_record_delimiter(options.record_delimiter)
            .set_quote_character(options.quote_character)
            .set_quote_escape_character(options.quote_escape_character)
            .set_comments(options.comments)
            .set_file_header_info(options.file_header_info)
            .build();

        let input_serialization = InputSerialization::builder()
            .csv(csv_input)
            .set_compression_type(options.compression_type)
            .build();

        let csv_output = CsvOutput::builder()
            .set_field_delimiter(options.output_field_delimiter)
            .set_record_delimiter(options.output_record_delimiter)
            .set_quote_character(options.output_quote_character)
            .set_quote_escape_character(options.output_quote_escape_character)
            .build();

        let output_serialization = OutputSerialization::builder().csv(csv_output).build();

        self.execute_select(
            bucket,
            key,
            query,
            input_serialization,
            output_serialization,
        )
        .await
    }

    /// Executes a SQL query on JSON data in S3.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or the data cannot be retrieved.
    pub async fn query_json(
        &self,
        bucket: &str,
        key: &str,
        query: &str,
        options: JsonSelectOptions,
    ) -> Result<Vec<u8>> {
        let json_input = JsonInput::builder().set_type(options.json_type).build();

        let input_serialization = InputSerialization::builder()
            .json(json_input)
            .set_compression_type(options.compression_type)
            .build();

        let json_output = JsonOutput::builder()
            .set_record_delimiter(options.output_record_delimiter)
            .build();

        let output_serialization = OutputSerialization::builder().json(json_output).build();

        self.execute_select(
            bucket,
            key,
            query,
            input_serialization,
            output_serialization,
        )
        .await
    }

    /// Executes a SQL query on Parquet data in S3.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or the data cannot be retrieved.
    pub async fn query_parquet(&self, bucket: &str, key: &str, query: &str) -> Result<Vec<u8>> {
        let parquet_input = ParquetInput::builder().build();

        let input_serialization = InputSerialization::builder().parquet(parquet_input).build();

        let json_output = JsonOutput::builder().build();

        let output_serialization = OutputSerialization::builder().json(json_output).build();

        self.execute_select(
            bucket,
            key,
            query,
            input_serialization,
            output_serialization,
        )
        .await
    }

    /// Executes a select query on S3 data.
    async fn execute_select(
        &self,
        bucket: &str,
        key: &str,
        query: &str,
        input_serialization: InputSerialization,
        output_serialization: OutputSerialization,
    ) -> Result<Vec<u8>> {
        let response = self
            .client
            .select_object_content()
            .bucket(bucket)
            .key(key)
            .expression(query)
            .expression_type(ExpressionType::Sql)
            .input_serialization(input_serialization)
            .output_serialization(output_serialization)
            .send()
            .await
            .map_err(|e| CloudEnhancedError::aws_service(format!("S3 Select failed: {}", e)))?;

        let mut result = Vec::new();
        let mut stream = response.payload;

        loop {
            match stream.recv().await {
                Ok(Some(event)) => match event {
                    SelectObjectContentEventStream::Records(records) => {
                        if let Some(payload) = records.payload {
                            result.extend_from_slice(payload.as_ref());
                        }
                    }
                    SelectObjectContentEventStream::Stats(stats) => {
                        tracing::debug!("S3 Select stats: {:?}", stats.details);
                    }
                    SelectObjectContentEventStream::Progress(progress) => {
                        tracing::debug!("S3 Select progress: {:?}", progress.details);
                    }
                    SelectObjectContentEventStream::End(_) => {
                        tracing::debug!("S3 Select completed");
                        break;
                    }
                    _ => {}
                },
                Ok(None) => break,
                Err(e) => {
                    return Err(CloudEnhancedError::aws_service(format!(
                        "Error reading S3 Select stream: {}",
                        e
                    )));
                }
            }
        }

        Ok(result)
    }

    /// Counts rows in a CSV file using S3 Select.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn count_csv_rows(
        &self,
        bucket: &str,
        key: &str,
        options: CsvSelectOptions,
    ) -> Result<u64> {
        let result = self
            .query_csv(bucket, key, "SELECT COUNT(*) FROM S3Object", options)
            .await?;

        let count_str = String::from_utf8(result)
            .map_err(|e| CloudEnhancedError::serialization(format!("Invalid UTF-8: {}", e)))?;

        count_str
            .trim()
            .parse::<u64>()
            .map_err(|e| CloudEnhancedError::serialization(format!("Invalid count: {}", e)))
    }
}

/// Options for CSV S3 Select queries.
#[derive(Debug, Clone)]
pub struct CsvSelectOptions {
    /// Field delimiter (default: ",")
    pub field_delimiter: Option<String>,
    /// Record delimiter (default: "\n")
    pub record_delimiter: Option<String>,
    /// Quote character (default: "\"")
    pub quote_character: Option<String>,
    /// Quote escape character
    pub quote_escape_character: Option<String>,
    /// Comments character
    pub comments: Option<String>,
    /// File header info
    pub file_header_info: Option<FileHeaderInfo>,
    /// Compression type
    pub compression_type: Option<CompressionType>,
    /// Output field delimiter
    pub output_field_delimiter: Option<String>,
    /// Output record delimiter
    pub output_record_delimiter: Option<String>,
    /// Output quote character
    pub output_quote_character: Option<String>,
    /// Output quote escape character
    pub output_quote_escape_character: Option<String>,
}

impl Default for CsvSelectOptions {
    fn default() -> Self {
        Self {
            field_delimiter: Some(",".to_string()),
            record_delimiter: Some("\n".to_string()),
            quote_character: Some("\"".to_string()),
            quote_escape_character: None,
            comments: None,
            file_header_info: Some(FileHeaderInfo::Use),
            compression_type: None,
            output_field_delimiter: Some(",".to_string()),
            output_record_delimiter: Some("\n".to_string()),
            output_quote_character: Some("\"".to_string()),
            output_quote_escape_character: None,
        }
    }
}

/// Options for JSON S3 Select queries.
#[derive(Debug, Clone)]
pub struct JsonSelectOptions {
    /// JSON type (DOCUMENT or LINES)
    pub json_type: Option<JsonType>,
    /// Compression type
    pub compression_type: Option<CompressionType>,
    /// Output record delimiter
    pub output_record_delimiter: Option<String>,
}

impl Default for JsonSelectOptions {
    fn default() -> Self {
        Self {
            json_type: Some(JsonType::Lines),
            compression_type: None,
            output_record_delimiter: Some("\n".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_select_options_default() {
        let options = CsvSelectOptions::default();
        assert_eq!(options.field_delimiter, Some(",".to_string()));
        assert_eq!(options.record_delimiter, Some("\n".to_string()));
    }

    #[test]
    fn test_json_select_options_default() {
        let options = JsonSelectOptions::default();
        assert_eq!(options.json_type, Some(JsonType::Lines));
        assert_eq!(options.output_record_delimiter, Some("\n".to_string()));
    }
}
