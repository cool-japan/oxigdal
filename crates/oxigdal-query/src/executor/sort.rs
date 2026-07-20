//! Sort executor.

use crate::error::{QueryError, Result};
use crate::executor::filter::{Value, evaluate_expr_for_row};
use crate::executor::scan::{ColumnData, RecordBatch};
use crate::parser::ast::OrderByExpr;
use std::cmp::Ordering;

/// Sort operator.
pub struct Sort {
    /// ORDER BY expressions.
    pub order_by: Vec<OrderByExpr>,
}

impl Sort {
    /// Create a new sort operator.
    pub fn new(order_by: Vec<OrderByExpr>) -> Self {
        Self { order_by }
    }

    /// Execute the sort.
    pub fn execute(&self, batch: &RecordBatch) -> Result<RecordBatch> {
        if self.order_by.is_empty() {
            return Ok(batch.clone());
        }

        // Pre-evaluate every ORDER BY key for every row. Unlike the previous
        // implementation, this routes *all* expressions (arithmetic, function
        // calls, ...) through the shared expression evaluator instead of only
        // handling bare column references and silently no-opping everything
        // else. Evaluation errors (e.g. unknown column) are propagated rather
        // than swallowed.
        let mut keys: Vec<Vec<Value>> = Vec::with_capacity(self.order_by.len());
        for order in &self.order_by {
            let mut column_keys = Vec::with_capacity(batch.num_rows);
            for row in 0..batch.num_rows {
                let value = evaluate_expr_for_row(&order.expr, batch, row)?;
                // Values with no defined total ordering (e.g. geometry) cannot
                // be sorted; surface an error instead of returning misordered
                // rows.
                if matches!(value, Value::Geometry(_)) {
                    return Err(QueryError::unsupported(
                        "ORDER BY expression produced a value with no defined ordering (geometry)",
                    ));
                }
                column_keys.push(value);
            }
            keys.push(column_keys);
        }

        // Create index array for indirect sorting.
        let mut indices: Vec<usize> = (0..batch.num_rows).collect();
        indices.sort_by(|&a, &b| self.compare_rows(&keys, a, b));

        // Reorder columns based on sorted indices.
        let mut sorted_columns = Vec::new();
        for column in &batch.columns {
            sorted_columns.push(self.reorder_column(column, &indices));
        }

        RecordBatch::new(batch.schema.clone(), sorted_columns, batch.num_rows)
    }

    /// Compare two rows based on the pre-evaluated ORDER BY keys.
    fn compare_rows(&self, keys: &[Vec<Value>], a: usize, b: usize) -> Ordering {
        for (idx, order) in self.order_by.iter().enumerate() {
            let ordering = compare_value(&keys[idx][a], &keys[idx][b], order.nulls_first);
            let ordering = if order.asc {
                ordering
            } else {
                ordering.reverse()
            };

            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    }

    /// Reorder a column based on indices.
    fn reorder_column(&self, column: &ColumnData, indices: &[usize]) -> ColumnData {
        match column {
            ColumnData::Boolean(data) => {
                let reordered = indices.iter().map(|&i| data[i]).collect();
                ColumnData::Boolean(reordered)
            }
            ColumnData::Int32(data) => {
                let reordered = indices.iter().map(|&i| data[i]).collect();
                ColumnData::Int32(reordered)
            }
            ColumnData::Int64(data) => {
                let reordered = indices.iter().map(|&i| data[i]).collect();
                ColumnData::Int64(reordered)
            }
            ColumnData::Float32(data) => {
                let reordered = indices.iter().map(|&i| data[i]).collect();
                ColumnData::Float32(reordered)
            }
            ColumnData::Float64(data) => {
                let reordered = indices.iter().map(|&i| data[i]).collect();
                ColumnData::Float64(reordered)
            }
            ColumnData::String(data) => {
                let reordered = indices.iter().map(|&i| data[i].clone()).collect();
                ColumnData::String(reordered)
            }
            ColumnData::Binary(data) => {
                let reordered = indices.iter().map(|&i| data[i].clone()).collect();
                ColumnData::Binary(reordered)
            }
        }
    }
}

/// Compare two runtime [`Value`]s for ORDER BY, honoring NULL placement.
///
/// `nulls_first` places NULLs before non-NULL values when `true` (SQL
/// `NULLS FIRST`) and after them when `false` (`NULLS LAST`). As in the
/// parallel sort implementation, the caller applies `.reverse()` afterwards for
/// descending order.
fn compare_value(a: &Value, b: &Value, nulls_first: bool) -> Ordering {
    use Value::*;

    // NULL handling first, so it is uniform across all value types.
    match (a, b) {
        (Null, Null) => return Ordering::Equal,
        (Null, _) => {
            return if nulls_first {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        (_, Null) => {
            return if nulls_first {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        _ => {}
    }

    match (a, b) {
        (Boolean(x), Boolean(y)) => x.cmp(y),
        (Int32(x), Int32(y)) => x.cmp(y),
        (Int64(x), Int64(y)) => x.cmp(y),
        (Int32(x), Int64(y)) => (*x as i64).cmp(y),
        (Int64(x), Int32(y)) => x.cmp(&(*y as i64)),
        (Float32(x), Float32(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Float64(x), Float64(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Float32(x), Float64(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Float64(x), Float32(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        (Int32(x), Float64(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Int64(x), Float64(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Float64(x), Int32(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        (Float64(x), Int64(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        (Int32(x), Float32(y)) => (*x as f32).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Int64(x), Float32(y)) => (*x as f64)
            .partial_cmp(&(*y as f64))
            .unwrap_or(Ordering::Equal),
        (Float32(x), Int32(y)) => x.partial_cmp(&(*y as f32)).unwrap_or(Ordering::Equal),
        (Float32(x), Int64(y)) => (*x as f64)
            .partial_cmp(&(*y as f64))
            .unwrap_or(Ordering::Equal),
        (String(x), String(y)) => x.cmp(y),
        // Incomparable / mismatched types: treat as equal so the sort remains
        // total without reordering rows arbitrarily.
        _ => Ordering::Equal,
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::executor::scan::{DataType, Field, Schema};
    use crate::parser::ast::{BinaryOperator, Expr};
    use std::sync::Arc;

    fn col(name: &str) -> Expr {
        Expr::Column {
            table: None,
            name: name.to_string(),
        }
    }

    #[test]
    fn test_sort_execution() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id".to_string(), DataType::Int64, false),
            Field::new("value".to_string(), DataType::Int64, false),
        ]));

        let columns = vec![
            ColumnData::Int64(vec![Some(3), Some(1), Some(4), Some(1), Some(5)]),
            ColumnData::Int64(vec![Some(30), Some(10), Some(40), Some(15), Some(50)]),
        ];

        let batch = RecordBatch::new(schema, columns, 5)?;

        // Sort by id ASC
        let order_by = vec![OrderByExpr {
            expr: Expr::Column {
                table: None,
                name: "id".to_string(),
            },
            asc: true,
            nulls_first: false,
        }];

        let sort = Sort::new(order_by);
        let sorted = sort.execute(&batch)?;

        // Verify sorted order
        let ColumnData::Int64(data) = &sorted.columns[0] else {
            panic!("Expected Int64 column");
        };
        assert_eq!(data[0], Some(1));
        assert_eq!(data[1], Some(1));
        assert_eq!(data[2], Some(3));
        assert_eq!(data[3], Some(4));
        assert_eq!(data[4], Some(5));

        Ok(())
    }

    #[test]
    fn test_sort_by_arithmetic_expression() -> Result<()> {
        // ORDER BY a * b should now actually sort (previously a silent no-op).
        let schema = Arc::new(Schema::new(vec![
            Field::new("a".to_string(), DataType::Int64, false),
            Field::new("b".to_string(), DataType::Int64, false),
        ]));
        let columns = vec![
            ColumnData::Int64(vec![Some(2), Some(1), Some(3)]),
            ColumnData::Int64(vec![Some(5), Some(4), Some(1)]),
        ];
        let batch = RecordBatch::new(schema, columns, 3)?;
        // products: row0=10, row1=4, row2=3  -> ascending order: row2, row1, row0
        let order_by = vec![OrderByExpr {
            expr: Expr::BinaryOp {
                left: Box::new(col("a")),
                op: BinaryOperator::Multiply,
                right: Box::new(col("b")),
            },
            asc: true,
            nulls_first: false,
        }];
        let sorted = Sort::new(order_by).execute(&batch)?;
        let ColumnData::Int64(a) = &sorted.columns[0] else {
            panic!("Expected Int64 column");
        };
        assert_eq!(a[0], Some(3)); // product 3
        assert_eq!(a[1], Some(1)); // product 4
        assert_eq!(a[2], Some(2)); // product 10
        Ok(())
    }

    #[test]
    fn test_sort_nulls_first() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "v".to_string(),
            DataType::Int64,
            true,
        )]));
        let columns = vec![ColumnData::Int64(vec![
            Some(3),
            None,
            Some(1),
            None,
            Some(2),
        ])];
        let batch = RecordBatch::new(schema, columns, 5)?;

        // NULLS FIRST, ascending
        let order_by = vec![OrderByExpr {
            expr: col("v"),
            asc: true,
            nulls_first: true,
        }];
        let sorted = Sort::new(order_by).execute(&batch)?;
        let ColumnData::Int64(v) = &sorted.columns[0] else {
            panic!("Expected Int64 column");
        };
        assert_eq!(v[0], None);
        assert_eq!(v[1], None);
        assert_eq!(v[2], Some(1));
        assert_eq!(v[3], Some(2));
        assert_eq!(v[4], Some(3));

        // NULLS LAST, ascending (default) puts NULLs at the end.
        let order_by = vec![OrderByExpr {
            expr: col("v"),
            asc: true,
            nulls_first: false,
        }];
        let sorted = Sort::new(order_by).execute(&batch)?;
        let ColumnData::Int64(v) = &sorted.columns[0] else {
            panic!("Expected Int64 column");
        };
        assert_eq!(v[0], Some(1));
        assert_eq!(v[1], Some(2));
        assert_eq!(v[2], Some(3));
        assert_eq!(v[3], None);
        assert_eq!(v[4], None);
        Ok(())
    }

    #[test]
    fn test_sort_unknown_column_errors() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "v".to_string(),
            DataType::Int64,
            false,
        )]));
        let columns = vec![ColumnData::Int64(vec![Some(1), Some(2)])];
        let batch = RecordBatch::new(schema, columns, 2)?;
        let order_by = vec![OrderByExpr {
            expr: col("does_not_exist"),
            asc: true,
            nulls_first: false,
        }];
        // Previously this silently returned rows in input order; now it errors.
        assert!(Sort::new(order_by).execute(&batch).is_err());
        Ok(())
    }
}
