//! Table scan executor.

use crate::error::{QueryError, Result};
use async_trait::async_trait;
use bytes::Bytes;
use oxigdal_core::error::OxiGdalError;
use std::sync::Arc;

/// A record batch of data.
#[derive(Debug, Clone)]
pub struct RecordBatch {
    /// Schema of the batch.
    pub schema: Arc<Schema>,
    /// Column data.
    pub columns: Vec<ColumnData>,
    /// Number of rows.
    pub num_rows: usize,
}

impl RecordBatch {
    /// Create a new record batch.
    pub fn new(schema: Arc<Schema>, columns: Vec<ColumnData>, num_rows: usize) -> Result<Self> {
        if columns.len() != schema.fields.len() {
            return Err(QueryError::execution(
                OxiGdalError::invalid_state_builder("Column count does not match schema")
                    .with_operation("record_batch_creation")
                    .with_parameter("schema_fields", schema.fields.len().to_string())
                    .with_parameter("actual_columns", columns.len().to_string())
                    .with_suggestion("Ensure all schema fields have corresponding column data")
                    .build()
                    .to_string(),
            ));
        }

        for (idx, column) in columns.iter().enumerate() {
            if column.len() != num_rows {
                return Err(QueryError::execution(
                    OxiGdalError::invalid_state_builder("Column length mismatch in batch")
                        .with_operation("record_batch_creation")
                        .with_parameter("expected_rows", num_rows.to_string())
                        .with_parameter("actual_rows", column.len().to_string())
                        .with_parameter("column_index", idx.to_string())
                        .with_suggestion("Ensure all columns have the same number of rows")
                        .build()
                        .to_string(),
                ));
            }
        }

        Ok(Self {
            schema,
            columns,
            num_rows,
        })
    }

    /// Get a column by index.
    pub fn column(&self, index: usize) -> Option<&ColumnData> {
        self.columns.get(index)
    }

    /// Get a column by name.
    pub fn column_by_name(&self, name: &str) -> Option<&ColumnData> {
        self.schema
            .fields
            .iter()
            .position(|f| f.name == name)
            .and_then(|idx| self.columns.get(idx))
    }
}

/// Schema definition.
#[derive(Debug, Clone)]
pub struct Schema {
    /// Fields in the schema.
    pub fields: Vec<Field>,
}

impl Schema {
    /// Create a new schema.
    pub fn new(fields: Vec<Field>) -> Self {
        Self { fields }
    }

    /// Find field by name.
    pub fn field_with_name(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Get field index by name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

/// Field definition.
#[derive(Debug, Clone)]
pub struct Field {
    /// Field name.
    pub name: String,
    /// Data type.
    pub data_type: DataType,
    /// Nullable.
    pub nullable: bool,
}

impl Field {
    /// Create a new field.
    pub fn new(name: String, data_type: DataType, nullable: bool) -> Self {
        Self {
            name,
            data_type,
            nullable,
        }
    }
}

/// Data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    /// Boolean.
    Boolean,
    /// 32-bit integer.
    Int32,
    /// 64-bit integer.
    Int64,
    /// 32-bit float.
    Float32,
    /// 64-bit float.
    Float64,
    /// UTF-8 string.
    String,
    /// Binary data.
    Binary,
    /// Geometry.
    Geometry,
}

/// Column data.
#[derive(Debug, Clone)]
pub enum ColumnData {
    /// Boolean column.
    Boolean(Vec<Option<bool>>),
    /// 32-bit integer column.
    Int32(Vec<Option<i32>>),
    /// 64-bit integer column.
    Int64(Vec<Option<i64>>),
    /// 32-bit float column.
    Float32(Vec<Option<f32>>),
    /// 64-bit float column.
    Float64(Vec<Option<f64>>),
    /// String column.
    String(Vec<Option<String>>),
    /// Binary column.
    Binary(Vec<Option<Bytes>>),
}

impl ColumnData {
    /// Get the length of the column.
    pub fn len(&self) -> usize {
        match self {
            ColumnData::Boolean(v) => v.len(),
            ColumnData::Int32(v) => v.len(),
            ColumnData::Int64(v) => v.len(),
            ColumnData::Float32(v) => v.len(),
            ColumnData::Float64(v) => v.len(),
            ColumnData::String(v) => v.len(),
            ColumnData::Binary(v) => v.len(),
        }
    }

    /// Check if the column is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Data source trait.
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Get the schema of the data source.
    async fn schema(&self) -> Result<Arc<Schema>>;

    /// Scan the data source.
    async fn scan(&self) -> Result<Vec<RecordBatch>>;
}

/// Table scan operator.
pub struct TableScan {
    /// Table name.
    pub table_name: String,
    /// Data source.
    pub source: Arc<dyn DataSource>,
    /// Projected columns (None means all columns).
    pub projection: Option<Vec<usize>>,
}

impl TableScan {
    /// Create a new table scan.
    pub fn new(table_name: String, source: Arc<dyn DataSource>) -> Self {
        Self {
            table_name,
            source,
            projection: None,
        }
    }

    /// Set projection.
    pub fn with_projection(mut self, projection: Vec<usize>) -> Self {
        self.projection = Some(projection);
        self
    }

    /// Execute the scan.
    pub async fn execute(&self) -> Result<Vec<RecordBatch>> {
        let batches = self.source.scan().await?;

        if let Some(ref projection) = self.projection {
            // Apply projection
            let mut projected_batches = Vec::new();
            for batch in batches {
                projected_batches.push(self.project_batch(batch, projection)?);
            }
            Ok(projected_batches)
        } else {
            Ok(batches)
        }
    }

    /// Project a record batch.
    fn project_batch(&self, batch: RecordBatch, projection: &[usize]) -> Result<RecordBatch> {
        let mut projected_columns = Vec::new();
        let mut projected_fields = Vec::new();

        for &idx in projection {
            if idx >= batch.columns.len() {
                return Err(QueryError::execution(format!(
                    "Column index {} out of bounds",
                    idx
                )));
            }
            projected_columns.push(batch.columns[idx].clone());
            projected_fields.push(batch.schema.fields[idx].clone());
        }

        let projected_schema = Arc::new(Schema::new(projected_fields));
        RecordBatch::new(projected_schema, projected_columns, batch.num_rows)
    }
}

/// In-memory data source for testing.
pub struct MemoryDataSource {
    /// Schema.
    schema: Arc<Schema>,
    /// Batches.
    batches: Vec<RecordBatch>,
}

impl MemoryDataSource {
    /// Create a new memory data source.
    pub fn new(schema: Arc<Schema>, batches: Vec<RecordBatch>) -> Self {
        Self { schema, batches }
    }
}

#[async_trait]
impl DataSource for MemoryDataSource {
    async fn schema(&self) -> Result<Arc<Schema>> {
        Ok(self.schema.clone())
    }

    async fn scan(&self) -> Result<Vec<RecordBatch>> {
        Ok(self.batches.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creation() {
        let schema = Schema::new(vec![
            Field::new("id".to_string(), DataType::Int64, false),
            Field::new("name".to_string(), DataType::String, true),
        ]);

        assert_eq!(schema.fields.len(), 2);
        assert_eq!(schema.index_of("id"), Some(0));
        assert_eq!(schema.index_of("name"), Some(1));
    }

    #[test]
    fn test_record_batch_creation() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id".to_string(), DataType::Int64, false),
            Field::new("value".to_string(), DataType::Float64, true),
        ]));

        let columns = vec![
            ColumnData::Int64(vec![Some(1), Some(2), Some(3)]),
            ColumnData::Float64(vec![Some(1.0), Some(2.0), Some(3.0)]),
        ];

        let batch = RecordBatch::new(schema, columns, 3)?;
        assert_eq!(batch.num_rows, 3);
        assert_eq!(batch.columns.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_memory_data_source() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "id".to_string(),
            DataType::Int64,
            false,
        )]));

        let columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
        let batch = RecordBatch::new(schema.clone(), columns, 3)?;

        let source = MemoryDataSource::new(schema, vec![batch]);
        let result_schema = source.schema().await?;
        assert_eq!(result_schema.fields.len(), 1);

        let batches = source.scan().await?;
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows, 3);

        Ok(())
    }
}
