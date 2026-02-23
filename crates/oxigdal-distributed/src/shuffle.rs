//! Data shuffle operations for distributed processing.
//!
//! This module provides shuffle operations for redistributing data across
//! worker nodes, supporting operations like group-by, sort, and join.

use crate::error::{DistributedError, Result};
use crate::partition::{HashPartitioner, RangePartitioner};
use crate::task::PartitionId;
use arrow::array::{Array, ArrayRef, AsArray};
use arrow::compute;
use arrow::datatypes::*;
use arrow::record_batch::RecordBatch;
use std::collections::HashMap;

/// Type of shuffle operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuffleType {
    /// Hash-based shuffle for group-by operations.
    Hash,
    /// Range-based shuffle for sorting.
    Range,
    /// Broadcast shuffle (send same data to all workers).
    Broadcast,
    /// Custom shuffle with user-defined logic.
    Custom,
}

/// Shuffle key for determining partition assignment.
#[derive(Debug, Clone)]
pub enum ShuffleKey {
    /// Shuffle by a single column.
    Column(String),
    /// Shuffle by multiple columns.
    Columns(Vec<String>),
    /// Shuffle by a computed expression.
    Expression(String),
}

/// Configuration for shuffle operations.
#[derive(Debug, Clone)]
pub struct ShuffleConfig {
    /// Type of shuffle.
    pub shuffle_type: ShuffleType,
    /// Key to shuffle by.
    pub key: ShuffleKey,
    /// Number of target partitions.
    pub num_partitions: usize,
    /// Buffer size for shuffle writes.
    pub buffer_size: usize,
}

impl ShuffleConfig {
    /// Create a new shuffle configuration.
    pub fn new(shuffle_type: ShuffleType, key: ShuffleKey, num_partitions: usize) -> Result<Self> {
        if num_partitions == 0 {
            return Err(DistributedError::shuffle(
                "Number of partitions must be greater than zero",
            ));
        }

        Ok(Self {
            shuffle_type,
            key,
            num_partitions,
            buffer_size: 1024 * 1024, // 1 MB default
        })
    }

    /// Set the buffer size.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

/// Result of a shuffle operation.
pub struct ShuffleResult {
    /// Partitioned data, keyed by partition ID.
    pub partitions: HashMap<PartitionId, Vec<RecordBatch>>,
    /// Statistics about the shuffle.
    pub stats: ShuffleStats,
}

/// Statistics about a shuffle operation.
#[derive(Debug, Clone, Default)]
pub struct ShuffleStats {
    /// Total number of rows shuffled.
    pub total_rows: u64,
    /// Total bytes shuffled.
    pub total_bytes: u64,
    /// Number of output partitions.
    pub num_partitions: usize,
    /// Time taken in milliseconds.
    pub duration_ms: u64,
}

/// Hash-based shuffle implementation.
pub struct HashShuffle {
    /// Partitioner for hash-based distribution.
    partitioner: HashPartitioner,
    /// Column name to hash.
    column_name: String,
}

impl HashShuffle {
    /// Create a new hash shuffle.
    pub fn new(column_name: String, num_partitions: usize) -> Result<Self> {
        let partitioner = HashPartitioner::new(num_partitions)?;
        Ok(Self {
            partitioner,
            column_name,
        })
    }

    /// Shuffle a record batch.
    pub fn shuffle(&self, batch: &RecordBatch) -> Result<HashMap<PartitionId, RecordBatch>> {
        let schema = batch.schema();

        // Find the column to hash
        let column_index = schema
            .column_with_name(&self.column_name)
            .map(|(idx, _)| idx)
            .ok_or_else(|| {
                DistributedError::shuffle(format!("Column {} not found", self.column_name))
            })?;

        let column = batch.column(column_index);

        // Compute partition for each row
        let partitions = self.compute_partitions(column)?;

        // Group rows by partition
        let mut partition_indices: HashMap<PartitionId, Vec<usize>> = HashMap::new();
        for (row_idx, &partition_id) in partitions.iter().enumerate() {
            partition_indices
                .entry(partition_id)
                .or_default()
                .push(row_idx);
        }

        // Create a record batch for each partition
        let mut result = HashMap::new();
        for (partition_id, indices) in partition_indices {
            let partition_batch = self.create_partition_batch(batch, &indices)?;
            result.insert(partition_id, partition_batch);
        }

        Ok(result)
    }

    /// Compute partition for each value in a column.
    fn compute_partitions(&self, column: &ArrayRef) -> Result<Vec<PartitionId>> {
        let mut partitions = Vec::with_capacity(column.len());

        match column.data_type() {
            DataType::Int32 => {
                let array = column.as_primitive::<Int32Type>();
                for i in 0..array.len() {
                    if array.is_null(i) {
                        partitions.push(PartitionId(0));
                    } else {
                        let value = array.value(i);
                        let key = value.to_le_bytes();
                        partitions.push(self.partitioner.partition_for_key(&key));
                    }
                }
            }
            DataType::Int64 => {
                let array = column.as_primitive::<Int64Type>();
                for i in 0..array.len() {
                    if array.is_null(i) {
                        partitions.push(PartitionId(0));
                    } else {
                        let value = array.value(i);
                        let key = value.to_le_bytes();
                        partitions.push(self.partitioner.partition_for_key(&key));
                    }
                }
            }
            DataType::Utf8 => {
                let array = column.as_string::<i32>();
                for i in 0..array.len() {
                    if array.is_null(i) {
                        partitions.push(PartitionId(0));
                    } else {
                        let value = array.value(i);
                        let key = value.as_bytes();
                        partitions.push(self.partitioner.partition_for_key(key));
                    }
                }
            }
            DataType::Float64 => {
                let array = column.as_primitive::<Float64Type>();
                for i in 0..array.len() {
                    if array.is_null(i) {
                        partitions.push(PartitionId(0));
                    } else {
                        let value = array.value(i);
                        let key = value.to_le_bytes();
                        partitions.push(self.partitioner.partition_for_key(&key));
                    }
                }
            }
            _ => {
                return Err(DistributedError::shuffle(format!(
                    "Unsupported column type for hash shuffle: {:?}",
                    column.data_type()
                )));
            }
        }

        Ok(partitions)
    }

    /// Create a record batch from selected indices.
    fn create_partition_batch(
        &self,
        batch: &RecordBatch,
        indices: &[usize],
    ) -> Result<RecordBatch> {
        // Convert indices to Int32Array for use with take kernel
        let indices_array =
            arrow::array::Int32Array::from(indices.iter().map(|&i| i as i32).collect::<Vec<_>>());

        // Use Arrow's take kernel to extract rows
        let columns: Result<Vec<ArrayRef>> = batch
            .columns()
            .iter()
            .map(|col| {
                compute::take(col.as_ref(), &indices_array, None)
                    .map_err(|e| DistributedError::arrow(e.to_string()))
            })
            .collect();

        RecordBatch::try_new(batch.schema(), columns?)
            .map_err(|e| DistributedError::arrow(e.to_string()))
    }
}

/// Range-based shuffle implementation for sorting.
pub struct RangeShuffle {
    /// Partitioner for range-based distribution.
    partitioner: RangePartitioner,
    /// Column name to partition by.
    column_name: String,
}

impl RangeShuffle {
    /// Create a new range shuffle.
    pub fn new(column_name: String, boundaries: Vec<f64>) -> Result<Self> {
        let partitioner = RangePartitioner::new(boundaries)?;
        Ok(Self {
            partitioner,
            column_name,
        })
    }

    /// Shuffle a record batch.
    pub fn shuffle(&self, batch: &RecordBatch) -> Result<HashMap<PartitionId, RecordBatch>> {
        let schema = batch.schema();

        // Find the column
        let column_index = schema
            .column_with_name(&self.column_name)
            .map(|(idx, _)| idx)
            .ok_or_else(|| {
                DistributedError::shuffle(format!("Column {} not found", self.column_name))
            })?;

        let column = batch.column(column_index);

        // Compute partition for each row
        let partitions = self.compute_partitions(column)?;

        // Group rows by partition
        let mut partition_indices: HashMap<PartitionId, Vec<usize>> = HashMap::new();
        for (row_idx, &partition_id) in partitions.iter().enumerate() {
            partition_indices
                .entry(partition_id)
                .or_default()
                .push(row_idx);
        }

        // Create a record batch for each partition
        let mut result = HashMap::new();
        for (partition_id, indices) in partition_indices {
            let partition_batch = self.create_partition_batch(batch, &indices)?;
            result.insert(partition_id, partition_batch);
        }

        Ok(result)
    }

    /// Compute partition for each value in a column.
    fn compute_partitions(&self, column: &ArrayRef) -> Result<Vec<PartitionId>> {
        let mut partitions = Vec::with_capacity(column.len());

        match column.data_type() {
            DataType::Float64 => {
                let array = column.as_primitive::<Float64Type>();
                for i in 0..array.len() {
                    if array.is_null(i) {
                        partitions.push(PartitionId(0));
                    } else {
                        let value = array.value(i);
                        partitions.push(self.partitioner.partition_for_value(value));
                    }
                }
            }
            DataType::Int32 => {
                let array = column.as_primitive::<Int32Type>();
                for i in 0..array.len() {
                    if array.is_null(i) {
                        partitions.push(PartitionId(0));
                    } else {
                        let value = f64::from(array.value(i));
                        partitions.push(self.partitioner.partition_for_value(value));
                    }
                }
            }
            DataType::Int64 => {
                let array = column.as_primitive::<Int64Type>();
                for i in 0..array.len() {
                    if array.is_null(i) {
                        partitions.push(PartitionId(0));
                    } else {
                        let value = array.value(i) as f64;
                        partitions.push(self.partitioner.partition_for_value(value));
                    }
                }
            }
            _ => {
                return Err(DistributedError::shuffle(format!(
                    "Unsupported column type for range shuffle: {:?}",
                    column.data_type()
                )));
            }
        }

        Ok(partitions)
    }

    /// Create a record batch from selected indices.
    fn create_partition_batch(
        &self,
        batch: &RecordBatch,
        indices: &[usize],
    ) -> Result<RecordBatch> {
        let indices_array =
            arrow::array::Int32Array::from(indices.iter().map(|&i| i as i32).collect::<Vec<_>>());

        let columns: Result<Vec<ArrayRef>> = batch
            .columns()
            .iter()
            .map(|col| {
                compute::take(col.as_ref(), &indices_array, None)
                    .map_err(|e| DistributedError::arrow(e.to_string()))
            })
            .collect();

        RecordBatch::try_new(batch.schema(), columns?)
            .map_err(|e| DistributedError::arrow(e.to_string()))
    }
}

/// Broadcast shuffle that replicates data to all partitions.
pub struct BroadcastShuffle {
    /// Number of target partitions.
    num_partitions: usize,
}

impl BroadcastShuffle {
    /// Create a new broadcast shuffle.
    pub fn new(num_partitions: usize) -> Result<Self> {
        if num_partitions == 0 {
            return Err(DistributedError::shuffle(
                "Number of partitions must be greater than zero",
            ));
        }
        Ok(Self { num_partitions })
    }

    /// Shuffle a record batch (broadcast to all partitions).
    pub fn shuffle(&self, batch: &RecordBatch) -> HashMap<PartitionId, RecordBatch> {
        let mut result = HashMap::new();
        for i in 0..self.num_partitions {
            result.insert(PartitionId(i as u64), batch.clone());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Float64Array, Int32Array, StringArray};
    use arrow::datatypes::{Field, Schema};
    use std::sync::Arc;

    fn create_test_batch() -> std::result::Result<RecordBatch, Box<dyn std::error::Error>> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("value", DataType::Float64, false),
            Field::new("name", DataType::Utf8, false),
        ]));

        let id_array = Int32Array::from(vec![1, 2, 3, 4, 5]);
        let value_array = Float64Array::from(vec![10.0, 20.0, 30.0, 40.0, 50.0]);
        let name_array = StringArray::from(vec!["a", "b", "c", "d", "e"]);

        Ok(RecordBatch::try_new(
            schema,
            vec![
                Arc::new(id_array),
                Arc::new(value_array),
                Arc::new(name_array),
            ],
        )?)
    }

    #[test]
    fn test_hash_shuffle() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let batch = create_test_batch()?;
        let shuffle = HashShuffle::new("id".to_string(), 2)?;

        let result = shuffle.shuffle(&batch)?;

        // Should have at most 2 partitions
        assert!(result.len() <= 2);

        // Total rows should match
        let total_rows: usize = result.values().map(|b| b.num_rows()).sum();
        assert_eq!(total_rows, batch.num_rows());
        Ok(())
    }

    #[test]
    fn test_range_shuffle() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let batch = create_test_batch()?;
        let boundaries = vec![2.5];
        let shuffle = RangeShuffle::new("id".to_string(), boundaries)?;

        let result = shuffle.shuffle(&batch)?;

        // Should have at most 2 partitions
        assert!(result.len() <= 2);

        // Total rows should match
        let total_rows: usize = result.values().map(|b| b.num_rows()).sum();
        assert_eq!(total_rows, batch.num_rows());
        Ok(())
    }

    #[test]
    fn test_broadcast_shuffle() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let batch = create_test_batch()?;
        let shuffle = BroadcastShuffle::new(3)?;

        let result = shuffle.shuffle(&batch);

        // Should have exactly 3 partitions
        assert_eq!(result.len(), 3);

        // Each partition should have all rows
        for partition_batch in result.values() {
            assert_eq!(partition_batch.num_rows(), batch.num_rows());
        }
        Ok(())
    }

    #[test]
    fn test_shuffle_config() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let config =
            ShuffleConfig::new(ShuffleType::Hash, ShuffleKey::Column("id".to_string()), 4)?;

        assert_eq!(config.shuffle_type, ShuffleType::Hash);
        assert_eq!(config.num_partitions, 4);
        assert_eq!(config.buffer_size, 1024 * 1024);
        Ok(())
    }
}
