//! Aggregation executor.

use crate::error::{QueryError, Result};
use crate::executor::filter::{Value, evaluate_expr_for_row};
use crate::executor::scan::{ColumnData, DataType, Field, RecordBatch, Schema};
use crate::parser::ast::Expr;
use std::collections::HashMap;
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

    /// Execute grouped aggregation (`GROUP BY`).
    ///
    /// Rows are partitioned into groups by evaluating every `GROUP BY`
    /// expression per row and hashing the resulting scalar values (NULLs group
    /// together, matching SQL semantics; composite keys are supported). For
    /// each group the aggregate functions are computed over the member rows,
    /// producing one output row per group. Output columns are the `GROUP BY`
    /// keys (preserving their original types) followed by the aggregate results
    /// (as `Float64`, matching the global-aggregation path).
    fn execute_grouped_aggregate(&self, batch: &RecordBatch) -> Result<RecordBatch> {
        let num_rows = batch.num_rows;

        // Evaluate each GROUP BY expression for every row.
        let mut group_values: Vec<Vec<Value>> = Vec::with_capacity(self.group_by.len());
        for expr in &self.group_by {
            let mut vals = Vec::with_capacity(num_rows);
            for row in 0..num_rows {
                let value = evaluate_expr_for_row(expr, batch, row)?;
                if matches!(value, Value::Geometry(_)) {
                    return Err(QueryError::unsupported(
                        "GROUP BY on a geometry value is not supported",
                    ));
                }
                vals.push(value);
            }
            group_values.push(vals);
        }

        // Partition rows into groups, preserving first-appearance order.
        let mut key_to_group: HashMap<Vec<String>, usize> = HashMap::new();
        let mut group_rows: Vec<Vec<usize>> = Vec::new();
        let mut group_repr: Vec<Vec<Value>> = Vec::new();

        for row in 0..num_rows {
            let key: Vec<String> = group_values
                .iter()
                .map(|gv| group_key_component(&gv[row]))
                .collect();
            let gid = match key_to_group.get(&key) {
                Some(&g) => g,
                None => {
                    let g = group_rows.len();
                    key_to_group.insert(key, g);
                    group_rows.push(Vec::new());
                    group_repr.push(group_values.iter().map(|gv| gv[row].clone()).collect());
                    g
                }
            };
            group_rows[gid].push(row);
        }

        let num_groups = group_rows.len();

        let mut result_fields = Vec::new();
        let mut result_columns = Vec::new();

        // GROUP BY key columns.
        for (gi, expr) in self.group_by.iter().enumerate() {
            let repr_vals: Vec<Value> =
                (0..num_groups).map(|g| group_repr[g][gi].clone()).collect();
            let column = values_to_column(&repr_vals);
            let data_type = column_data_type(&column);
            let name = match expr {
                Expr::Column { name, .. } => name.clone(),
                _ => format!("group_{}", gi),
            };
            result_fields.push(Field::new(name, data_type, true));
            result_columns.push(column);
        }

        // Aggregate columns.
        for agg in &self.aggregates {
            let mut agg_vals: Vec<Option<f64>> = Vec::with_capacity(num_groups);
            for rows in group_rows.iter() {
                let value = if agg.column == "*" {
                    if matches!(agg.func, AggregateFunc::Count) {
                        Some(rows.len() as f64)
                    } else {
                        return Err(QueryError::semantic(
                            "Wildcard (*) can only be used with COUNT function",
                        ));
                    }
                } else {
                    let column = batch
                        .column_by_name(&agg.column)
                        .ok_or_else(|| QueryError::ColumnNotFound(agg.column.clone()))?;
                    let sub = gather_column(column, rows);
                    self.compute_aggregate(agg.func, &sub)?
                };
                agg_vals.push(value);
            }
            result_fields.push(Field::new(
                agg.alias.clone().unwrap_or_else(|| {
                    if agg.column == "*" {
                        "count".to_string()
                    } else {
                        agg.column.clone()
                    }
                }),
                DataType::Float64,
                true,
            ));
            result_columns.push(ColumnData::Float64(agg_vals));
        }

        let schema = Arc::new(Schema::new(result_fields));
        RecordBatch::new(schema, result_columns, num_groups)
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

/// Build a hashable/equatable key component for a group-by value.
///
/// NULLs map to a single sentinel so they group together. Floats use their bit
/// representation so that `NaN` values group deterministically.
fn group_key_component(value: &Value) -> String {
    match value {
        Value::Null => "N".to_string(),
        Value::Boolean(b) => format!("b{}", b),
        Value::Int32(i) => format!("i{}", i),
        Value::Int64(i) => format!("l{}", i),
        Value::Float32(f) => format!("f{}", f.to_bits()),
        Value::Float64(f) => format!("d{}", f.to_bits()),
        Value::String(s) => format!("s{}", s),
        // Geometry is rejected before this point.
        Value::Geometry(_) => "g".to_string(),
    }
}

/// Convert per-group representative [`Value`]s into a typed [`ColumnData`].
///
/// The column type is inferred from the first non-NULL value; an all-NULL group
/// key defaults to an `Int64` column of NULLs.
fn values_to_column(values: &[Value]) -> ColumnData {
    let first_non_null = values.iter().find(|v| !matches!(v, Value::Null));
    match first_non_null {
        Some(Value::Boolean(_)) => ColumnData::Boolean(
            values
                .iter()
                .map(|v| match v {
                    Value::Boolean(b) => Some(*b),
                    _ => None,
                })
                .collect(),
        ),
        Some(Value::Int32(_)) => ColumnData::Int32(
            values
                .iter()
                .map(|v| match v {
                    Value::Int32(i) => Some(*i),
                    _ => None,
                })
                .collect(),
        ),
        Some(Value::Int64(_)) => ColumnData::Int64(
            values
                .iter()
                .map(|v| match v {
                    Value::Int64(i) => Some(*i),
                    _ => None,
                })
                .collect(),
        ),
        Some(Value::Float32(_)) => ColumnData::Float32(
            values
                .iter()
                .map(|v| match v {
                    Value::Float32(f) => Some(*f),
                    _ => None,
                })
                .collect(),
        ),
        Some(Value::Float64(_)) => ColumnData::Float64(
            values
                .iter()
                .map(|v| match v {
                    Value::Float64(f) => Some(*f),
                    _ => None,
                })
                .collect(),
        ),
        Some(Value::String(_)) => ColumnData::String(
            values
                .iter()
                .map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
        ),
        // All NULL (or geometry, which is rejected earlier): default to Int64 NULLs.
        _ => ColumnData::Int64(values.iter().map(|_| None).collect()),
    }
}

/// Map a [`ColumnData`] variant to its [`DataType`].
fn column_data_type(column: &ColumnData) -> DataType {
    match column {
        ColumnData::Boolean(_) => DataType::Boolean,
        ColumnData::Int32(_) => DataType::Int32,
        ColumnData::Int64(_) => DataType::Int64,
        ColumnData::Float32(_) => DataType::Float32,
        ColumnData::Float64(_) => DataType::Float64,
        ColumnData::String(_) => DataType::String,
        ColumnData::Binary(_) => DataType::Binary,
    }
}

/// Gather a subset of a column's rows (by index) into a new column.
fn gather_column(column: &ColumnData, indices: &[usize]) -> ColumnData {
    match column {
        ColumnData::Boolean(d) => {
            ColumnData::Boolean(indices.iter().filter_map(|&i| d.get(i).copied()).collect())
        }
        ColumnData::Int32(d) => {
            ColumnData::Int32(indices.iter().filter_map(|&i| d.get(i).copied()).collect())
        }
        ColumnData::Int64(d) => {
            ColumnData::Int64(indices.iter().filter_map(|&i| d.get(i).copied()).collect())
        }
        ColumnData::Float32(d) => {
            ColumnData::Float32(indices.iter().filter_map(|&i| d.get(i).copied()).collect())
        }
        ColumnData::Float64(d) => {
            ColumnData::Float64(indices.iter().filter_map(|&i| d.get(i).copied()).collect())
        }
        ColumnData::String(d) => {
            ColumnData::String(indices.iter().filter_map(|&i| d.get(i).cloned()).collect())
        }
        ColumnData::Binary(d) => {
            ColumnData::Binary(indices.iter().filter_map(|&i| d.get(i).cloned()).collect())
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[allow(clippy::panic)]
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

    #[test]
    fn test_grouped_aggregate() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("k".to_string(), DataType::Int64, false),
            Field::new("v".to_string(), DataType::Int64, false),
        ]));
        // groups: k=1 -> [10, 30] (count 2, sum 40); k=2 -> [20] (count 1, sum 20)
        let columns = vec![
            ColumnData::Int64(vec![Some(1), Some(2), Some(1)]),
            ColumnData::Int64(vec![Some(10), Some(20), Some(30)]),
        ];
        let batch = RecordBatch::new(schema, columns, 3)?;

        let agg = Aggregate::new(
            vec![Expr::Column {
                table: None,
                name: "k".to_string(),
            }],
            vec![
                AggregateFunction {
                    func: AggregateFunc::Sum,
                    column: "v".to_string(),
                    alias: Some("sum_v".to_string()),
                },
                AggregateFunction {
                    func: AggregateFunc::Count,
                    column: "*".to_string(),
                    alias: Some("cnt".to_string()),
                },
            ],
        );

        let result = agg.execute(&batch)?;
        assert_eq!(result.num_rows, 2); // two groups
        assert_eq!(result.columns.len(), 3); // k, sum_v, cnt

        // Group key column preserves the Int64 type and first-appearance order.
        let ColumnData::Int64(k) = &result.columns[0] else {
            panic!("expected int64 group key");
        };
        assert_eq!(k[0], Some(1));
        assert_eq!(k[1], Some(2));

        let ColumnData::Float64(sum_v) = &result.columns[1] else {
            panic!("expected float64 sum");
        };
        assert_eq!(sum_v[0], Some(40.0));
        assert_eq!(sum_v[1], Some(20.0));

        let ColumnData::Float64(cnt) = &result.columns[2] else {
            panic!("expected float64 count");
        };
        assert_eq!(cnt[0], Some(2.0));
        assert_eq!(cnt[1], Some(1.0));

        Ok(())
    }

    #[test]
    fn test_grouped_aggregate_null_key_groups_together() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("k".to_string(), DataType::Int64, true),
            Field::new("v".to_string(), DataType::Int64, false),
        ]));
        let columns = vec![
            ColumnData::Int64(vec![None, Some(1), None]),
            ColumnData::Int64(vec![Some(5), Some(7), Some(9)]),
        ];
        let batch = RecordBatch::new(schema, columns, 3)?;
        let agg = Aggregate::new(
            vec![Expr::Column {
                table: None,
                name: "k".to_string(),
            }],
            vec![AggregateFunction {
                func: AggregateFunc::Count,
                column: "*".to_string(),
                alias: Some("cnt".to_string()),
            }],
        );
        let result = agg.execute(&batch)?;
        // NULL group (2 rows) + k=1 group (1 row) => 2 groups.
        assert_eq!(result.num_rows, 2);
        let ColumnData::Float64(cnt) = &result.columns[1] else {
            panic!("expected float64 count");
        };
        assert_eq!(cnt[0], Some(2.0)); // NULL group first (rows 0 and 2)
        assert_eq!(cnt[1], Some(1.0));
        Ok(())
    }
}
