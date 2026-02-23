//! Sort executor.

use crate::error::Result;
use crate::executor::scan::{ColumnData, RecordBatch};
use crate::parser::ast::{Expr, OrderByExpr};
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

        // Create index array for indirect sorting
        let mut indices: Vec<usize> = (0..batch.num_rows).collect();

        // Sort indices based on order by clauses
        indices.sort_by(|&a, &b| self.compare_rows(batch, a, b));

        // Reorder columns based on sorted indices
        let mut sorted_columns = Vec::new();
        for column in &batch.columns {
            sorted_columns.push(self.reorder_column(column, &indices));
        }

        RecordBatch::new(batch.schema.clone(), sorted_columns, batch.num_rows)
    }

    /// Compare two rows based on ORDER BY clauses.
    fn compare_rows(&self, batch: &RecordBatch, a: usize, b: usize) -> Ordering {
        for order in &self.order_by {
            let ordering = self.compare_values(batch, &order.expr, a, b);
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

    /// Compare values for two rows.
    fn compare_values(&self, batch: &RecordBatch, expr: &Expr, a: usize, b: usize) -> Ordering {
        // Simplified: only handle column references
        if let Expr::Column { name, .. } = expr {
            if let Some(column) = batch.column_by_name(name) {
                return self.compare_column_values(column, a, b);
            }
        }
        Ordering::Equal
    }

    /// Compare two values in a column.
    fn compare_column_values(&self, column: &ColumnData, a: usize, b: usize) -> Ordering {
        match column {
            ColumnData::Boolean(data) => Self::compare_optional(&data[a], &data[b]),
            ColumnData::Int32(data) => Self::compare_optional(&data[a], &data[b]),
            ColumnData::Int64(data) => Self::compare_optional(&data[a], &data[b]),
            ColumnData::Float32(data) => {
                let val_a = data[a];
                let val_b = data[b];
                match (val_a, val_b) {
                    (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                }
            }
            ColumnData::Float64(data) => {
                let val_a = data[a];
                let val_b = data[b];
                match (val_a, val_b) {
                    (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                }
            }
            ColumnData::String(data) => Self::compare_optional(&data[a], &data[b]),
            ColumnData::Binary(_) => Ordering::Equal,
        }
    }

    /// Compare optional values.
    fn compare_optional<T: Ord>(a: &Option<T>, b: &Option<T>) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
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

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::executor::scan::{DataType, Field, Schema};
    use std::sync::Arc;

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
}
