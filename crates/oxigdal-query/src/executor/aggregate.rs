//! Aggregation executor.

use crate::error::{QueryError, Result};
use crate::executor::scan::{ColumnData, Field, RecordBatch, Schema};
use crate::parser::ast::Expr;
use std::sync::Arc;

/// Aggregate operator.
pub struct Aggregate {
    /// GROUP BY expressions.
    pub group_by: Vec<Expr>,
    /// Aggregate functions.
    pub aggregates: Vec<AggregateFunction>,
}

impl Aggregate {
    /// Create a new aggregate operator.
    pub fn new(group_by: Vec<Expr>, aggregates: Vec<AggregateFunction>) -> Self {
        Self {
            group_by,
            aggregates,
        }
    }

    /// Execute aggregation.
    pub fn execute(&self, batch: &RecordBatch) -> Result<RecordBatch> {
        if self.group_by.is_empty() {
            // Global aggregation
            self.execute_global_aggregate(batch)
        } else {
            // Grouped aggregation
            self.execute_grouped_aggregate(batch)
        }
    }

    /// Execute global aggregation (no GROUP BY).
    fn execute_global_aggregate(&self, batch: &RecordBatch) -> Result<RecordBatch> {
        let mut result_fields = Vec::new();
        let mut result_columns = Vec::new();

        for agg in &self.aggregates {
            let value = if agg.column == "*" {
                // COUNT(*) - count all rows regardless of NULL values
                if matches!(agg.func, AggregateFunc::Count) {
                    Some(batch.num_rows as f64)
                } else {
                    return Err(QueryError::semantic(
                        "Wildcard (*) can only be used with COUNT function",
                    ));
                }
            } else {
                let column = batch
                    .column_by_name(&agg.column)
                    .ok_or_else(|| QueryError::ColumnNotFound(agg.column.clone()))?;
                self.compute_aggregate(agg.func, column)?
            };

            result_fields.push(Field::new(
                agg.alias.clone().unwrap_or_else(|| {
                    if agg.column == "*" {
                        "count".to_string()
                    } else {
                        agg.column.clone()
                    }
                }),
                crate::executor::scan::DataType::Float64,
                true,
            ));
            result_columns.push(ColumnData::Float64(vec![value]));
        }

        let schema = Arc::new(Schema::new(result_fields));
        RecordBatch::new(schema, result_columns, 1)
    }

    /// Execute grouped aggregation.
    fn execute_grouped_aggregate(&self, _batch: &RecordBatch) -> Result<RecordBatch> {
        // Simplified implementation
        Err(QueryError::unsupported(
            "Grouped aggregation not implemented",
        ))
    }

    /// Compute aggregate function.
    fn compute_aggregate(&self, func: AggregateFunc, column: &ColumnData) -> Result<Option<f64>> {
        match func {
            AggregateFunc::Count => Ok(Some(self.count(column))),
            AggregateFunc::Sum => self.sum(column),
            AggregateFunc::Avg => self.avg(column),
            AggregateFunc::Min => self.min(column),
            AggregateFunc::Max => self.max(column),
        }
    }

    /// Count aggregate.
    fn count(&self, column: &ColumnData) -> f64 {
        let non_null_count = match column {
            ColumnData::Boolean(data) => data.iter().filter(|v| v.is_some()).count(),
            ColumnData::Int32(data) => data.iter().filter(|v| v.is_some()).count(),
            ColumnData::Int64(data) => data.iter().filter(|v| v.is_some()).count(),
            ColumnData::Float32(data) => data.iter().filter(|v| v.is_some()).count(),
            ColumnData::Float64(data) => data.iter().filter(|v| v.is_some()).count(),
            ColumnData::String(data) => data.iter().filter(|v| v.is_some()).count(),
            ColumnData::Binary(data) => data.iter().filter(|v| v.is_some()).count(),
        };
        non_null_count as f64
    }

    /// Sum aggregate.
    fn sum(&self, column: &ColumnData) -> Result<Option<f64>> {
        match column {
            ColumnData::Int32(data) => {
                let sum: i64 = data.iter().filter_map(|v| v.map(|i| i as i64)).sum();
                Ok(Some(sum as f64))
            }
            ColumnData::Int64(data) => {
                let sum: i64 = data.iter().filter_map(|v| *v).sum();
                Ok(Some(sum as f64))
            }
            ColumnData::Float32(data) => {
                let sum: f32 = data.iter().filter_map(|v| *v).sum();
                Ok(Some(sum as f64))
            }
            ColumnData::Float64(data) => {
                let sum: f64 = data.iter().filter_map(|v| *v).sum();
                Ok(Some(sum))
            }
            _ => Err(QueryError::type_mismatch("numeric", "non-numeric")),
        }
    }

    /// Average aggregate.
    fn avg(&self, column: &ColumnData) -> Result<Option<f64>> {
        let sum = self.sum(column)?;
        let count = self.count(column);
        if count > 0.0 {
            Ok(sum.map(|s| s / count))
        } else {
            Ok(None)
        }
    }

    /// Minimum aggregate.
    fn min(&self, column: &ColumnData) -> Result<Option<f64>> {
        match column {
            ColumnData::Int32(data) => {
                let min = data.iter().filter_map(|v| *v).min();
                Ok(min.map(|m| m as f64))
            }
            ColumnData::Int64(data) => {
                let min = data.iter().filter_map(|v| *v).min();
                Ok(min.map(|m| m as f64))
            }
            ColumnData::Float32(data) => {
                let min = data
                    .iter()
                    .filter_map(|v| *v)
                    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                Ok(min.map(|m| m as f64))
            }
            ColumnData::Float64(data) => {
                let min = data
                    .iter()
                    .filter_map(|v| *v)
                    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                Ok(min)
            }
            _ => Err(QueryError::type_mismatch("numeric", "non-numeric")),
        }
    }

    /// Maximum aggregate.
    fn max(&self, column: &ColumnData) -> Result<Option<f64>> {
        match column {
            ColumnData::Int32(data) => {
                let max = data.iter().filter_map(|v| *v).max();
                Ok(max.map(|m| m as f64))
            }
            ColumnData::Int64(data) => {
                let max = data.iter().filter_map(|v| *v).max();
                Ok(max.map(|m| m as f64))
            }
            ColumnData::Float32(data) => {
                let max = data
                    .iter()
                    .filter_map(|v| *v)
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                Ok(max.map(|m| m as f64))
            }
            ColumnData::Float64(data) => {
                let max = data
                    .iter()
                    .filter_map(|v| *v)
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                Ok(max)
            }
            _ => Err(QueryError::type_mismatch("numeric", "non-numeric")),
        }
    }
}

/// Aggregate function.
#[derive(Debug, Clone)]
pub struct AggregateFunction {
    /// Function type.
    pub func: AggregateFunc,
    /// Column to aggregate.
    pub column: String,
    /// Output alias.
    pub alias: Option<String>,
}

/// Aggregate function type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateFunc {
    /// COUNT function.
    Count,
    /// SUM function.
    Sum,
    /// AVG function.
    Avg,
    /// MIN function.
    Min,
    /// MAX function.
    Max,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::scan::DataType;

    #[test]
    fn test_global_aggregate() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value".to_string(),
            DataType::Int64,
            false,
        )]));

        let columns = vec![ColumnData::Int64(vec![
            Some(10),
            Some(20),
            Some(30),
            Some(40),
            Some(50),
        ])];

        let batch = RecordBatch::new(schema, columns, 5)?;

        let agg = Aggregate::new(
            vec![],
            vec![
                AggregateFunction {
                    func: AggregateFunc::Sum,
                    column: "value".to_string(),
                    alias: Some("sum".to_string()),
                },
                AggregateFunction {
                    func: AggregateFunc::Avg,
                    column: "value".to_string(),
                    alias: Some("avg".to_string()),
                },
            ],
        );

        let result = agg.execute(&batch)?;
        assert_eq!(result.num_rows, 1);
        assert_eq!(result.columns.len(), 2);

        Ok(())
    }
}
