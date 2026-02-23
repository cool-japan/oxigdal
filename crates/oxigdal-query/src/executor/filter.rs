//! Filter executor.

use crate::error::{QueryError, Result};
use crate::executor::scan::{ColumnData, RecordBatch};
use crate::parser::ast::{BinaryOperator, Expr, Literal, UnaryOperator};
use oxigdal_core::error::OxiGdalError;

/// Filter operator.
pub struct Filter {
    /// Filter predicate.
    pub predicate: Expr,
}

impl Filter {
    /// Create a new filter.
    pub fn new(predicate: Expr) -> Self {
        Self { predicate }
    }

    /// Execute the filter on a record batch.
    pub fn execute(&self, batch: &RecordBatch) -> Result<RecordBatch> {
        let mut selection = vec![false; batch.num_rows];

        // Evaluate predicate for each row
        for (row_idx, sel) in selection.iter_mut().enumerate().take(batch.num_rows) {
            let result = self.evaluate_expr(&self.predicate, batch, row_idx)?;
            if let Value::Boolean(b) = result {
                *sel = b;
            } else {
                return Err(QueryError::execution(
                    OxiGdalError::invalid_operation_builder(
                        "Filter predicate must evaluate to boolean type",
                    )
                    .with_operation("filter_evaluation")
                    .with_parameter("row_index", row_idx.to_string())
                    .with_parameter("actual_type", format!("{:?}", result))
                    .with_suggestion("Ensure WHERE clause uses comparison or boolean operators")
                    .build()
                    .to_string(),
                ));
            }
        }

        // Filter columns based on selection
        let mut filtered_columns = Vec::new();
        for column in &batch.columns {
            filtered_columns.push(self.filter_column(column, &selection));
        }

        let filtered_rows = selection.iter().filter(|&&b| b).count();

        RecordBatch::new(batch.schema.clone(), filtered_columns, filtered_rows)
    }

    /// Filter a column based on selection.
    fn filter_column(&self, column: &ColumnData, selection: &[bool]) -> ColumnData {
        match column {
            ColumnData::Boolean(data) => {
                let filtered: Vec<Option<bool>> = data
                    .iter()
                    .zip(selection)
                    .filter_map(|(v, &sel)| if sel { Some(*v) } else { None })
                    .collect();
                ColumnData::Boolean(filtered)
            }
            ColumnData::Int32(data) => {
                let filtered: Vec<Option<i32>> = data
                    .iter()
                    .zip(selection)
                    .filter_map(|(v, &sel)| if sel { Some(*v) } else { None })
                    .collect();
                ColumnData::Int32(filtered)
            }
            ColumnData::Int64(data) => {
                let filtered: Vec<Option<i64>> = data
                    .iter()
                    .zip(selection)
                    .filter_map(|(v, &sel)| if sel { Some(*v) } else { None })
                    .collect();
                ColumnData::Int64(filtered)
            }
            ColumnData::Float32(data) => {
                let filtered: Vec<Option<f32>> = data
                    .iter()
                    .zip(selection)
                    .filter_map(|(v, &sel)| if sel { Some(*v) } else { None })
                    .collect();
                ColumnData::Float32(filtered)
            }
            ColumnData::Float64(data) => {
                let filtered: Vec<Option<f64>> = data
                    .iter()
                    .zip(selection)
                    .filter_map(|(v, &sel)| if sel { Some(*v) } else { None })
                    .collect();
                ColumnData::Float64(filtered)
            }
            ColumnData::String(data) => {
                let filtered: Vec<Option<String>> = data
                    .iter()
                    .zip(selection)
                    .filter_map(|(v, &sel)| if sel { Some(v.clone()) } else { None })
                    .collect();
                ColumnData::String(filtered)
            }
            ColumnData::Binary(data) => {
                let filtered = data
                    .iter()
                    .zip(selection)
                    .filter_map(|(v, &sel)| if sel { Some(v.clone()) } else { None })
                    .collect();
                ColumnData::Binary(filtered)
            }
        }
    }

    /// Evaluate an expression for a specific row.
    fn evaluate_expr(&self, expr: &Expr, batch: &RecordBatch, row_idx: usize) -> Result<Value> {
        match expr {
            Expr::Column { table: _, name } => {
                let column = batch
                    .column_by_name(name)
                    .ok_or_else(|| QueryError::ColumnNotFound(name.clone()))?;
                self.get_column_value(column, row_idx)
            }
            Expr::Literal(lit) => Ok(Value::from_literal(lit)),
            Expr::BinaryOp { left, op, right } => {
                let left_val = self.evaluate_expr(left, batch, row_idx)?;
                let right_val = self.evaluate_expr(right, batch, row_idx)?;
                self.evaluate_binary_op(&left_val, *op, &right_val)
            }
            Expr::UnaryOp { op, expr } => {
                let val = self.evaluate_expr(expr, batch, row_idx)?;
                self.evaluate_unary_op(*op, &val)
            }
            Expr::IsNull(expr) => {
                let val = self.evaluate_expr(expr, batch, row_idx)?;
                Ok(Value::Boolean(matches!(val, Value::Null)))
            }
            Expr::IsNotNull(expr) => {
                let val = self.evaluate_expr(expr, batch, row_idx)?;
                Ok(Value::Boolean(!matches!(val, Value::Null)))
            }
            _ => Err(QueryError::unsupported(
                OxiGdalError::not_supported_builder("Unsupported expression type in filter")
                    .with_operation("filter_evaluation")
                    .with_parameter("expression_type", format!("{:?}", expr))
                    .with_suggestion(
                        "Use simpler expressions: columns, literals, binary/unary operators, IS [NOT] NULL",
                    )
                    .build()
                    .to_string(),
            )),
        }
    }

    /// Get value from column at row index.
    fn get_column_value(&self, column: &ColumnData, row_idx: usize) -> Result<Value> {
        match column {
            ColumnData::Boolean(data) => Ok(data
                .get(row_idx)
                .and_then(|v| v.as_ref())
                .map(|&v| Value::Boolean(v))
                .unwrap_or(Value::Null)),
            ColumnData::Int32(data) => Ok(data
                .get(row_idx)
                .and_then(|v| v.as_ref())
                .map(|&v| Value::Int32(v))
                .unwrap_or(Value::Null)),
            ColumnData::Int64(data) => Ok(data
                .get(row_idx)
                .and_then(|v| v.as_ref())
                .map(|&v| Value::Int64(v))
                .unwrap_or(Value::Null)),
            ColumnData::Float32(data) => Ok(data
                .get(row_idx)
                .and_then(|v| v.as_ref())
                .map(|&v| Value::Float32(v))
                .unwrap_or(Value::Null)),
            ColumnData::Float64(data) => Ok(data
                .get(row_idx)
                .and_then(|v| v.as_ref())
                .map(|&v| Value::Float64(v))
                .unwrap_or(Value::Null)),
            ColumnData::String(data) => Ok(data
                .get(row_idx)
                .and_then(|v| v.as_ref())
                .map(|v| Value::String(v.clone()))
                .unwrap_or(Value::Null)),
            ColumnData::Binary(_) => Err(QueryError::unsupported(
                OxiGdalError::not_supported_builder(
                    "Binary column type not supported in filter predicates",
                )
                .with_operation("column_value_extraction")
                .with_parameter("row_index", row_idx.to_string())
                .with_suggestion(
                    "Cast binary columns to supported types or filter at a different stage",
                )
                .build()
                .to_string(),
            )),
        }
    }

    /// Evaluate a binary operation.
    fn evaluate_binary_op(&self, left: &Value, op: BinaryOperator, right: &Value) -> Result<Value> {
        match (left, right) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            // Type coercion: Int32 with Int64
            (Value::Int32(l), Value::Int64(r)) => {
                self.evaluate_binary_op(&Value::Int64(*l as i64), op, &Value::Int64(*r))
            }
            (Value::Int64(l), Value::Int32(r)) => {
                self.evaluate_binary_op(&Value::Int64(*l), op, &Value::Int64(*r as i64))
            }
            (Value::Int32(l), Value::Int32(r)) => match op {
                BinaryOperator::Plus => Ok(Value::Int32(l + r)),
                BinaryOperator::Minus => Ok(Value::Int32(l - r)),
                BinaryOperator::Multiply => Ok(Value::Int32(l * r)),
                BinaryOperator::Divide => {
                    if *r == 0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Int32(l / r))
                    }
                }
                BinaryOperator::Modulo => {
                    if *r == 0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Int32(l % r))
                    }
                }
                BinaryOperator::Eq => Ok(Value::Boolean(l == r)),
                BinaryOperator::NotEq => Ok(Value::Boolean(l != r)),
                BinaryOperator::Lt => Ok(Value::Boolean(l < r)),
                BinaryOperator::LtEq => Ok(Value::Boolean(l <= r)),
                BinaryOperator::Gt => Ok(Value::Boolean(l > r)),
                BinaryOperator::GtEq => Ok(Value::Boolean(l >= r)),
                _ => Err(QueryError::unsupported("Unsupported operator for integers")),
            },
            (Value::Int64(l), Value::Int64(r)) => match op {
                BinaryOperator::Plus => Ok(Value::Int64(l + r)),
                BinaryOperator::Minus => Ok(Value::Int64(l - r)),
                BinaryOperator::Multiply => Ok(Value::Int64(l * r)),
                BinaryOperator::Divide => {
                    if *r == 0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Int64(l / r))
                    }
                }
                BinaryOperator::Modulo => {
                    if *r == 0 {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Int64(l % r))
                    }
                }
                BinaryOperator::Eq => Ok(Value::Boolean(l == r)),
                BinaryOperator::NotEq => Ok(Value::Boolean(l != r)),
                BinaryOperator::Lt => Ok(Value::Boolean(l < r)),
                BinaryOperator::LtEq => Ok(Value::Boolean(l <= r)),
                BinaryOperator::Gt => Ok(Value::Boolean(l > r)),
                BinaryOperator::GtEq => Ok(Value::Boolean(l >= r)),
                _ => Err(QueryError::unsupported("Unsupported operator for integers")),
            },
            // Type coercion: Float32 with Float64
            (Value::Float32(l), Value::Float64(r)) => {
                self.evaluate_binary_op(&Value::Float64(*l as f64), op, &Value::Float64(*r))
            }
            (Value::Float64(l), Value::Float32(r)) => {
                self.evaluate_binary_op(&Value::Float64(*l), op, &Value::Float64(*r as f64))
            }
            (Value::Float32(l), Value::Float32(r)) => match op {
                BinaryOperator::Plus => Ok(Value::Float32(l + r)),
                BinaryOperator::Minus => Ok(Value::Float32(l - r)),
                BinaryOperator::Multiply => Ok(Value::Float32(l * r)),
                BinaryOperator::Divide => Ok(Value::Float32(l / r)),
                BinaryOperator::Eq => Ok(Value::Boolean((l - r).abs() < f32::EPSILON)),
                BinaryOperator::NotEq => Ok(Value::Boolean((l - r).abs() >= f32::EPSILON)),
                BinaryOperator::Lt => Ok(Value::Boolean(l < r)),
                BinaryOperator::LtEq => Ok(Value::Boolean(l <= r)),
                BinaryOperator::Gt => Ok(Value::Boolean(l > r)),
                BinaryOperator::GtEq => Ok(Value::Boolean(l >= r)),
                _ => Err(QueryError::unsupported("Unsupported operator for floats")),
            },
            (Value::Float64(l), Value::Float64(r)) => match op {
                BinaryOperator::Plus => Ok(Value::Float64(l + r)),
                BinaryOperator::Minus => Ok(Value::Float64(l - r)),
                BinaryOperator::Multiply => Ok(Value::Float64(l * r)),
                BinaryOperator::Divide => Ok(Value::Float64(l / r)),
                BinaryOperator::Eq => Ok(Value::Boolean((l - r).abs() < f64::EPSILON)),
                BinaryOperator::NotEq => Ok(Value::Boolean((l - r).abs() >= f64::EPSILON)),
                BinaryOperator::Lt => Ok(Value::Boolean(l < r)),
                BinaryOperator::LtEq => Ok(Value::Boolean(l <= r)),
                BinaryOperator::Gt => Ok(Value::Boolean(l > r)),
                BinaryOperator::GtEq => Ok(Value::Boolean(l >= r)),
                _ => Err(QueryError::unsupported("Unsupported operator for floats")),
            },
            // Type coercion: Int with Float
            (Value::Int32(l), Value::Float64(r)) => {
                self.evaluate_binary_op(&Value::Float64(*l as f64), op, &Value::Float64(*r))
            }
            (Value::Int64(l), Value::Float64(r)) => {
                self.evaluate_binary_op(&Value::Float64(*l as f64), op, &Value::Float64(*r))
            }
            (Value::Float64(l), Value::Int32(r)) => {
                self.evaluate_binary_op(&Value::Float64(*l), op, &Value::Float64(*r as f64))
            }
            (Value::Float64(l), Value::Int64(r)) => {
                self.evaluate_binary_op(&Value::Float64(*l), op, &Value::Float64(*r as f64))
            }
            (Value::Boolean(l), Value::Boolean(r)) => match op {
                BinaryOperator::And => Ok(Value::Boolean(*l && *r)),
                BinaryOperator::Or => Ok(Value::Boolean(*l || *r)),
                BinaryOperator::Eq => Ok(Value::Boolean(l == r)),
                BinaryOperator::NotEq => Ok(Value::Boolean(l != r)),
                _ => Err(QueryError::unsupported("Unsupported operator for booleans")),
            },
            (Value::String(l), Value::String(r)) => match op {
                BinaryOperator::Eq => Ok(Value::Boolean(l == r)),
                BinaryOperator::NotEq => Ok(Value::Boolean(l != r)),
                BinaryOperator::Concat => Ok(Value::String(format!("{}{}", l, r))),
                _ => Err(QueryError::unsupported("Unsupported operator for strings")),
            },
            _ => Err(QueryError::execution(
                OxiGdalError::invalid_operation_builder("Type mismatch in binary operation")
                    .with_operation("binary_operator_evaluation")
                    .with_parameter("left_type", format!("{:?}", left))
                    .with_parameter("right_type", format!("{:?}", right))
                    .with_parameter("operator", format!("{:?}", op))
                    .with_suggestion(
                        "Ensure both operands have compatible types or use explicit type casts",
                    )
                    .build()
                    .to_string(),
            )),
        }
    }

    /// Evaluate a unary operation.
    fn evaluate_unary_op(&self, op: UnaryOperator, val: &Value) -> Result<Value> {
        match (op, val) {
            (UnaryOperator::Minus, Value::Int64(i)) => Ok(Value::Int64(-i)),
            (UnaryOperator::Minus, Value::Float64(f)) => Ok(Value::Float64(-f)),
            (UnaryOperator::Not, Value::Boolean(b)) => Ok(Value::Boolean(!b)),
            (_, Value::Null) => Ok(Value::Null),
            _ => Err(QueryError::unsupported("Unsupported unary operation")),
        }
    }
}

/// Runtime value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Null value.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// 32-bit integer value.
    Int32(i32),
    /// 64-bit integer value.
    Int64(i64),
    /// 32-bit float value.
    Float32(f32),
    /// 64-bit float value.
    Float64(f64),
    /// String value.
    String(String),
}

impl Value {
    /// Convert from a literal.
    fn from_literal(lit: &Literal) -> Self {
        match lit {
            Literal::Null => Value::Null,
            Literal::Boolean(b) => Value::Boolean(*b),
            Literal::Integer(i) => Value::Int64(*i),
            Literal::Float(f) => Value::Float64(*f),
            Literal::String(s) => Value::String(s.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::scan::{Field, Schema};
    use std::sync::Arc;

    #[test]
    fn test_filter_execution() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "id".to_string(),
                crate::executor::scan::DataType::Int64,
                false,
            ),
            Field::new(
                "value".to_string(),
                crate::executor::scan::DataType::Int64,
                false,
            ),
        ]));

        let columns = vec![
            ColumnData::Int64(vec![Some(1), Some(2), Some(3), Some(4), Some(5)]),
            ColumnData::Int64(vec![Some(10), Some(20), Some(30), Some(40), Some(50)]),
        ];

        let batch = RecordBatch::new(schema, columns, 5)?;

        // Filter: value > 25
        let predicate = Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: None,
                name: "value".to_string(),
            }),
            op: BinaryOperator::Gt,
            right: Box::new(Expr::Literal(Literal::Integer(25))),
        };

        let filter = Filter::new(predicate);
        let filtered = filter.execute(&batch)?;

        assert_eq!(filtered.num_rows, 3); // 30, 40, 50 are > 25

        Ok(())
    }
}
