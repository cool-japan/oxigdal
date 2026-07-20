//! Parallel query execution.

use crate::error::Result;
use crate::executor::scan::{ColumnData, RecordBatch};
use crate::parser::ast::OrderByExpr;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;

/// Parallel execution configuration.
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Number of threads to use.
    pub num_threads: usize,
    /// Minimum batch size for parallelization.
    pub min_batch_size: usize,
}

/// Cursor into a batch for k-way merge.
#[derive(Debug, Clone, Copy)]
struct BatchCursor {
    /// Index of the batch.
    batch_idx: usize,
    /// Current row index within the batch.
    row_idx: usize,
}

/// Entry in the merge heap.
struct MergeEntry<'a> {
    /// Current cursor position.
    cursor: BatchCursor,
    /// Reference to all batches.
    batches: &'a [RecordBatch],
    /// ORDER BY specification.
    order_by: &'a [OrderByExpr],
}

impl<'a> Eq for MergeEntry<'a> {}

impl<'a> PartialEq for MergeEntry<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<'a> Ord for MergeEntry<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap behavior (BinaryHeap is max-heap)
        compare_rows(
            self.batches,
            self.cursor.batch_idx,
            self.cursor.row_idx,
            other.cursor.batch_idx,
            other.cursor.row_idx,
            self.order_by,
        )
        .reverse()
    }
}

impl<'a> PartialOrd for MergeEntry<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Generic column value for intermediate storage during merge.
#[derive(Debug, Clone)]
enum ColumnValue {
    Boolean(Option<bool>),
    Int32(Option<i32>),
    Int64(Option<i64>),
    Float32(Option<f32>),
    Float64(Option<f64>),
    String(Option<String>),
    Binary(Option<bytes::Bytes>),
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            num_threads: num_cpus(),
            min_batch_size: 1000,
        }
    }
}

/// Parallel executor.
pub struct ParallelExecutor {
    /// Configuration.
    config: ParallelConfig,
}

impl ParallelExecutor {
    /// Create a new parallel executor.
    pub fn new(config: ParallelConfig) -> Self {
        Self { config }
    }

    /// Execute a function on batches in parallel.
    pub fn execute_parallel<F>(
        &self,
        batches: Vec<RecordBatch>,
        func: F,
    ) -> Result<Vec<RecordBatch>>
    where
        F: Fn(&RecordBatch) -> Result<RecordBatch> + Send + Sync,
    {
        if batches.len() < 2
            || batches
                .iter()
                .all(|b| b.num_rows < self.config.min_batch_size)
        {
            // Execute sequentially for small workloads
            return batches.into_iter().map(|batch| func(&batch)).collect();
        }

        // Execute in parallel
        let func = Arc::new(func);
        let results: Result<Vec<_>> = batches
            .par_iter()
            .map(|batch| {
                let f = func.clone();
                f(batch)
            })
            .collect();

        results
    }

    /// Partition batches for parallel processing.
    pub fn partition_batches(
        &self,
        batches: Vec<RecordBatch>,
        num_partitions: usize,
    ) -> Vec<Vec<RecordBatch>> {
        if num_partitions == 0 || batches.is_empty() {
            return vec![batches];
        }

        let total_rows: usize = batches.iter().map(|b| b.num_rows).sum();
        let rows_per_partition = total_rows.div_ceil(num_partitions);

        let mut partitions: Vec<Vec<RecordBatch>> = vec![Vec::new(); num_partitions];
        let mut current_partition = 0;
        let mut current_partition_rows = 0;

        for batch in batches {
            if current_partition_rows >= rows_per_partition
                && current_partition < num_partitions - 1
            {
                current_partition += 1;
                current_partition_rows = 0;
            }

            current_partition_rows += batch.num_rows;
            partitions[current_partition].push(batch);
        }

        partitions
    }

    /// Merge results from parallel execution.
    ///
    /// If `order_by` is provided, performs k-way merge assuming input batches are already sorted.
    /// Otherwise, simply concatenates the batches.
    pub fn merge_batches(
        &self,
        batches: Vec<RecordBatch>,
        order_by: Option<&[OrderByExpr]>,
    ) -> Result<Vec<RecordBatch>> {
        if batches.is_empty() {
            return Ok(vec![]);
        }

        // If no ordering specified, simple concatenation
        let Some(order_by) = order_by else {
            return Ok(batches);
        };

        // If only one batch, no merge needed
        if batches.len() == 1 {
            return Ok(batches);
        }

        // Perform k-way merge
        self.k_way_merge(batches, order_by)
    }

    /// Perform k-way merge of pre-sorted batches.
    fn k_way_merge(
        &self,
        batches: Vec<RecordBatch>,
        order_by: &[OrderByExpr],
    ) -> Result<Vec<RecordBatch>> {
        if batches.is_empty() {
            return Ok(vec![]);
        }

        let schema = batches[0].schema.clone();

        // Initialize cursors for each batch
        let cursors: Vec<BatchCursor> = batches
            .iter()
            .enumerate()
            .filter(|(_, batch)| batch.num_rows > 0)
            .map(|(idx, _)| BatchCursor {
                batch_idx: idx,
                row_idx: 0,
            })
            .collect();

        if cursors.is_empty() {
            return Ok(vec![]);
        }

        // Build a binary heap for k-way merge
        // Note: BinaryHeap is a max-heap, so we need to reverse the comparison
        let mut heap = BinaryHeap::new();
        for cursor in &cursors {
            heap.push(MergeEntry {
                cursor: *cursor,
                batches: &batches,
                order_by,
            });
        }

        // Calculate total rows
        let total_rows: usize = batches.iter().map(|b| b.num_rows).sum();

        // Pre-allocate output columns
        let num_columns = schema.fields.len();
        let mut output_columns: Vec<Vec<Option<ColumnValue>>> =
            vec![Vec::with_capacity(total_rows); num_columns];

        // Perform k-way merge
        while let Some(entry) = heap.pop() {
            let batch = &batches[entry.cursor.batch_idx];
            let row_idx = entry.cursor.row_idx;

            // Extract row values and append to output
            for (col_idx, column) in batch.columns.iter().enumerate() {
                let value = extract_value(column, row_idx);
                output_columns[col_idx].push(value);
            }

            // Advance cursor
            let next_row = row_idx + 1;
            if next_row < batch.num_rows {
                heap.push(MergeEntry {
                    cursor: BatchCursor {
                        batch_idx: entry.cursor.batch_idx,
                        row_idx: next_row,
                    },
                    batches: &batches,
                    order_by,
                });
            }
        }

        // Convert output columns to ColumnData
        let merged_columns: Vec<ColumnData> = output_columns
            .into_iter()
            .zip(schema.fields.iter())
            .map(|(values, field)| values_to_column_data(values, &field.data_type))
            .collect();

        // Create merged batch
        let merged_batch = RecordBatch::new(schema, merged_columns, total_rows)?;

        Ok(vec![merged_batch])
    }
}

/// Pipeline parallel execution.
pub struct Pipeline {
    /// Stages in the pipeline.
    stages: Vec<Box<dyn PipelineStage>>,
}

impl Pipeline {
    /// Create a new pipeline.
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Add a stage to the pipeline.
    pub fn add_stage<S: PipelineStage + 'static>(mut self, stage: S) -> Self {
        self.stages.push(Box::new(stage));
        self
    }

    /// Execute the pipeline.
    pub async fn execute(&self, input: Vec<RecordBatch>) -> Result<Vec<RecordBatch>> {
        let mut current = input;

        for stage in &self.stages {
            current = stage.execute(current).await?;
        }

        Ok(current)
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Pipeline stage trait.
#[async_trait::async_trait]
pub trait PipelineStage: Send + Sync {
    /// Execute the stage.
    async fn execute(&self, input: Vec<RecordBatch>) -> Result<Vec<RecordBatch>>;
}

/// Task scheduler for parallel execution.
pub struct TaskScheduler {
    /// Number of worker threads.
    num_workers: usize,
}

impl TaskScheduler {
    /// Create a new task scheduler.
    pub fn new(num_workers: usize) -> Self {
        Self { num_workers }
    }

    /// Schedule tasks for execution.
    pub fn schedule<F, T>(&self, tasks: Vec<F>) -> Vec<T>
    where
        F: Fn() -> T + Send,
        T: Send,
    {
        tasks.into_par_iter().map(|task| task()).collect()
    }

    /// Get number of workers.
    pub fn num_workers(&self) -> usize {
        self.num_workers
    }
}

/// Helper function to get optimal number of CPU threads.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Compare two rows from potentially different batches based on ORDER BY clauses.
fn compare_rows(
    batches: &[RecordBatch],
    batch_a: usize,
    row_a: usize,
    batch_b: usize,
    row_b: usize,
    order_by: &[OrderByExpr],
) -> Ordering {
    use crate::parser::ast::Expr;

    let batch_a = &batches[batch_a];
    let batch_b = &batches[batch_b];

    for order in order_by {
        // Extract column name from expression
        let column_name = match &order.expr {
            Expr::Column { name, .. } => name,
            _ => continue, // Skip non-column expressions
        };

        // Find column in both batches
        let col_idx_a = batch_a.schema.index_of(column_name);
        let col_idx_b = batch_b.schema.index_of(column_name);

        if let (Some(idx_a), Some(idx_b)) = (col_idx_a, col_idx_b) {
            let ordering = compare_column_values(
                &batch_a.columns[idx_a],
                row_a,
                &batch_b.columns[idx_b],
                row_b,
                order.nulls_first,
            );

            let ordering = if order.asc {
                ordering
            } else {
                ordering.reverse()
            };

            if ordering != Ordering::Equal {
                return ordering;
            }
        }
    }

    Ordering::Equal
}

/// Compare values from two columns at specified row indices.
fn compare_column_values(
    col_a: &ColumnData,
    row_a: usize,
    col_b: &ColumnData,
    row_b: usize,
    nulls_first: bool,
) -> Ordering {
    match (col_a, col_b) {
        (ColumnData::Boolean(data_a), ColumnData::Boolean(data_b)) => {
            compare_optional(&data_a[row_a], &data_b[row_b], nulls_first)
        }
        (ColumnData::Int32(data_a), ColumnData::Int32(data_b)) => {
            compare_optional(&data_a[row_a], &data_b[row_b], nulls_first)
        }
        (ColumnData::Int64(data_a), ColumnData::Int64(data_b)) => {
            compare_optional(&data_a[row_a], &data_b[row_b], nulls_first)
        }
        (ColumnData::Float32(data_a), ColumnData::Float32(data_b)) => {
            let val_a = &data_a[row_a];
            let val_b = &data_b[row_b];
            match (val_a, val_b) {
                (Some(a), Some(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
                (Some(_), None) => {
                    if nulls_first {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
                }
                (None, Some(_)) => {
                    if nulls_first {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                (None, None) => Ordering::Equal,
            }
        }
        (ColumnData::Float64(data_a), ColumnData::Float64(data_b)) => {
            let val_a = &data_a[row_a];
            let val_b = &data_b[row_b];
            match (val_a, val_b) {
                (Some(a), Some(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
                (Some(_), None) => {
                    if nulls_first {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
                }
                (None, Some(_)) => {
                    if nulls_first {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                (None, None) => Ordering::Equal,
            }
        }
        (ColumnData::String(data_a), ColumnData::String(data_b)) => {
            compare_optional(&data_a[row_a], &data_b[row_b], nulls_first)
        }
        (ColumnData::Binary(data_a), ColumnData::Binary(data_b)) => {
            compare_optional(&data_a[row_a], &data_b[row_b], nulls_first)
        }
        _ => Ordering::Equal, // Mismatched types
    }
}

/// Compare optional values with configurable NULL handling.
fn compare_optional<T: Ord>(a: &Option<T>, b: &Option<T>, nulls_first: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => {
            if nulls_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (None, Some(_)) => {
            if nulls_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (None, None) => Ordering::Equal,
    }
}

/// Extract a value from a column at the specified row index.
fn extract_value(column: &ColumnData, row_idx: usize) -> Option<ColumnValue> {
    match column {
        ColumnData::Boolean(data) => Some(ColumnValue::Boolean(data[row_idx])),
        ColumnData::Int32(data) => Some(ColumnValue::Int32(data[row_idx])),
        ColumnData::Int64(data) => Some(ColumnValue::Int64(data[row_idx])),
        ColumnData::Float32(data) => Some(ColumnValue::Float32(data[row_idx])),
        ColumnData::Float64(data) => Some(ColumnValue::Float64(data[row_idx])),
        ColumnData::String(data) => Some(ColumnValue::String(data[row_idx].clone())),
        ColumnData::Binary(data) => Some(ColumnValue::Binary(data[row_idx].clone())),
    }
}

/// Convert a vector of column values back to ColumnData.
fn values_to_column_data(
    values: Vec<Option<ColumnValue>>,
    data_type: &crate::executor::scan::DataType,
) -> ColumnData {
    use crate::executor::scan::DataType;

    match data_type {
        DataType::Boolean => {
            let data: Vec<Option<bool>> = values
                .into_iter()
                .map(|v| {
                    v.and_then(|val| {
                        if let ColumnValue::Boolean(b) = val {
                            b
                        } else {
                            None
                        }
                    })
                })
                .collect();
            ColumnData::Boolean(data)
        }
        DataType::Int32 => {
            let data: Vec<Option<i32>> = values
                .into_iter()
                .map(|v| {
                    v.and_then(|val| {
                        if let ColumnValue::Int32(i) = val {
                            i
                        } else {
                            None
                        }
                    })
                })
                .collect();
            ColumnData::Int32(data)
        }
        DataType::Int64 => {
            let data: Vec<Option<i64>> = values
                .into_iter()
                .map(|v| {
                    v.and_then(|val| {
                        if let ColumnValue::Int64(i) = val {
                            i
                        } else {
                            None
                        }
                    })
                })
                .collect();
            ColumnData::Int64(data)
        }
        DataType::Float32 => {
            let data: Vec<Option<f32>> = values
                .into_iter()
                .map(|v| {
                    v.and_then(|val| {
                        if let ColumnValue::Float32(f) = val {
                            f
                        } else {
                            None
                        }
                    })
                })
                .collect();
            ColumnData::Float32(data)
        }
        DataType::Float64 => {
            let data: Vec<Option<f64>> = values
                .into_iter()
                .map(|v| {
                    v.and_then(|val| {
                        if let ColumnValue::Float64(f) = val {
                            f
                        } else {
                            None
                        }
                    })
                })
                .collect();
            ColumnData::Float64(data)
        }
        DataType::String => {
            let data: Vec<Option<String>> = values
                .into_iter()
                .map(|v| {
                    v.and_then(|val| {
                        if let ColumnValue::String(s) = val {
                            s
                        } else {
                            None
                        }
                    })
                })
                .collect();
            ColumnData::String(data)
        }
        DataType::Binary => {
            let data: Vec<Option<bytes::Bytes>> = values
                .into_iter()
                .map(|v| {
                    v.and_then(|val| {
                        if let ColumnValue::Binary(b) = val {
                            b
                        } else {
                            None
                        }
                    })
                })
                .collect();
            ColumnData::Binary(data)
        }
        DataType::Geometry => {
            // Geometry not supported in ColumnData yet, return empty binary
            ColumnData::Binary(vec![None; values.len()])
        }
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::executor::scan::{ColumnData, DataType, Field, Schema};
    use std::sync::Arc;

    #[test]
    fn test_parallel_executor() -> Result<()> {
        let config = ParallelConfig::default();
        let executor = ParallelExecutor::new(config);

        let schema = Arc::new(Schema::new(vec![Field::new(
            "value".to_string(),
            DataType::Int64,
            false,
        )]));

        let mut batches = Vec::new();
        for i in 0..5 {
            let columns = vec![ColumnData::Int64(vec![Some(i), Some(i + 1)])];
            batches.push(RecordBatch::new(schema.clone(), columns, 2)?);
        }

        let results = executor.execute_parallel(batches, |batch| Ok(batch.clone()))?;

        assert_eq!(results.len(), 5);

        Ok(())
    }

    #[test]
    fn test_partition_batches() {
        let config = ParallelConfig::default();
        let executor = ParallelExecutor::new(config);

        let schema = Arc::new(Schema::new(vec![Field::new(
            "value".to_string(),
            DataType::Int64,
            false,
        )]));

        let mut batches = Vec::new();
        for i in 0..10 {
            let columns = vec![ColumnData::Int64(vec![Some(i)])];
            if let Ok(batch) = RecordBatch::new(schema.clone(), columns, 1) {
                batches.push(batch);
            }
        }

        let partitions = executor.partition_batches(batches, 3);
        assert_eq!(partitions.len(), 3);
    }

    #[test]
    fn test_merge_batches_no_order() -> Result<()> {
        let config = ParallelConfig::default();
        let executor = ParallelExecutor::new(config);

        let schema = Arc::new(Schema::new(vec![Field::new(
            "value".to_string(),
            DataType::Int64,
            false,
        )]));

        let mut batches = Vec::new();
        for i in 0..3 {
            let columns = vec![ColumnData::Int64(vec![Some(i), Some(i + 1)])];
            batches.push(RecordBatch::new(schema.clone(), columns, 2)?);
        }

        let merged = executor.merge_batches(batches, None)?;
        assert_eq!(merged.len(), 3); // No merge, just concatenation

        Ok(())
    }

    #[test]
    fn test_merge_batches_with_order() -> Result<()> {
        use crate::parser::ast::{Expr, OrderByExpr};

        let config = ParallelConfig::default();
        let executor = ParallelExecutor::new(config);

        let schema = Arc::new(Schema::new(vec![
            Field::new("id".to_string(), DataType::Int64, false),
            Field::new("value".to_string(), DataType::Int64, false),
        ]));

        // Create three pre-sorted batches
        let batch1 = RecordBatch::new(
            schema.clone(),
            vec![
                ColumnData::Int64(vec![Some(1), Some(4), Some(7)]),
                ColumnData::Int64(vec![Some(10), Some(40), Some(70)]),
            ],
            3,
        )?;

        let batch2 = RecordBatch::new(
            schema.clone(),
            vec![
                ColumnData::Int64(vec![Some(2), Some(5), Some(8)]),
                ColumnData::Int64(vec![Some(20), Some(50), Some(80)]),
            ],
            3,
        )?;

        let batch3 = RecordBatch::new(
            schema.clone(),
            vec![
                ColumnData::Int64(vec![Some(3), Some(6), Some(9)]),
                ColumnData::Int64(vec![Some(30), Some(60), Some(90)]),
            ],
            3,
        )?;

        let order_by = vec![OrderByExpr {
            expr: Expr::Column {
                table: None,
                name: "id".to_string(),
            },
            asc: true,
            nulls_first: false,
        }];

        let merged = executor.merge_batches(vec![batch1, batch2, batch3], Some(&order_by))?;

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].num_rows, 9);

        // Verify the merged result is sorted
        let ColumnData::Int64(data) = &merged[0].columns[0] else {
            panic!("Expected Int64 column");
        };
        for i in 0..9 {
            assert_eq!(data[i], Some((i + 1) as i64));
        }

        Ok(())
    }

    #[test]
    fn test_merge_batches_descending() -> Result<()> {
        use crate::parser::ast::{Expr, OrderByExpr};

        let config = ParallelConfig::default();
        let executor = ParallelExecutor::new(config);

        let schema = Arc::new(Schema::new(vec![Field::new(
            "score".to_string(),
            DataType::Float64,
            false,
        )]));

        // Create two pre-sorted batches (descending)
        let batch1 = RecordBatch::new(
            schema.clone(),
            vec![ColumnData::Float64(vec![Some(9.5), Some(7.5), Some(5.5)])],
            3,
        )?;

        let batch2 = RecordBatch::new(
            schema.clone(),
            vec![ColumnData::Float64(vec![Some(8.5), Some(6.5), Some(4.5)])],
            3,
        )?;

        let order_by = vec![OrderByExpr {
            expr: Expr::Column {
                table: None,
                name: "score".to_string(),
            },
            asc: false, // Descending
            nulls_first: false,
        }];

        let merged = executor.merge_batches(vec![batch1, batch2], Some(&order_by))?;

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].num_rows, 6);

        // Verify the merged result is sorted in descending order
        let ColumnData::Float64(data) = &merged[0].columns[0] else {
            panic!("Expected Float64 column");
        };
        let expected = [9.5, 8.5, 7.5, 6.5, 5.5, 4.5];
        for (i, &exp) in expected.iter().enumerate() {
            assert_eq!(data[i], Some(exp));
        }

        Ok(())
    }

    #[test]
    fn test_merge_batches_with_nulls() -> Result<()> {
        use crate::parser::ast::{Expr, OrderByExpr};

        let config = ParallelConfig::default();
        let executor = ParallelExecutor::new(config);

        let schema = Arc::new(Schema::new(vec![Field::new(
            "value".to_string(),
            DataType::Int32,
            true,
        )]));

        // Test NULLS LAST: batches must be pre-sorted with NULLS LAST
        let batch1_nulls_last = RecordBatch::new(
            schema.clone(),
            vec![ColumnData::Int32(vec![Some(1), Some(5), None])],
            3,
        )?;

        let batch2_nulls_last = RecordBatch::new(
            schema.clone(),
            vec![ColumnData::Int32(vec![Some(3), Some(7), None])], // Fixed: was [3, NULL, 7]
            3,
        )?;

        let order_by = vec![OrderByExpr {
            expr: Expr::Column {
                table: None,
                name: "value".to_string(),
            },
            asc: true,
            nulls_first: false,
        }];

        let merged =
            executor.merge_batches(vec![batch1_nulls_last, batch2_nulls_last], Some(&order_by))?;

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].num_rows, 6);

        let ColumnData::Int32(data) = &merged[0].columns[0] else {
            panic!("Expected Int32 column");
        };
        // Values should be: 1, 3, 5, 7, NULL, NULL
        assert_eq!(data[0], Some(1));
        assert_eq!(data[1], Some(3));
        assert_eq!(data[2], Some(5));
        assert_eq!(data[3], Some(7));
        assert_eq!(data[4], None);
        assert_eq!(data[5], None);

        // Test NULLS FIRST: batches must be pre-sorted with NULLS FIRST
        let batch1_nulls_first = RecordBatch::new(
            schema.clone(),
            vec![ColumnData::Int32(vec![None, Some(1), Some(5)])],
            3,
        )?;

        let batch2_nulls_first = RecordBatch::new(
            schema.clone(),
            vec![ColumnData::Int32(vec![None, Some(3), Some(7)])],
            3,
        )?;

        let order_by_nulls_first = vec![OrderByExpr {
            expr: Expr::Column {
                table: None,
                name: "value".to_string(),
            },
            asc: true,
            nulls_first: true,
        }];

        let merged2 = executor.merge_batches(
            vec![batch1_nulls_first, batch2_nulls_first],
            Some(&order_by_nulls_first),
        )?;

        let ColumnData::Int32(data) = &merged2[0].columns[0] else {
            panic!("Expected Int32 column");
        };
        // Values should be: NULL, NULL, 1, 3, 5, 7
        assert_eq!(data[0], None);
        assert_eq!(data[1], None);
        assert_eq!(data[2], Some(1));
        assert_eq!(data[3], Some(3));
        assert_eq!(data[4], Some(5));
        assert_eq!(data[5], Some(7));

        Ok(())
    }
}
