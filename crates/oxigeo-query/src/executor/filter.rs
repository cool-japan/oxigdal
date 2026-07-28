//! Filter executor.

use crate::error::{QueryError, Result};
use crate::executor::scan::{ColumnData, RecordBatch};
use crate::parser::ast::{BinaryOperator, Expr, Literal, UnaryOperator};
use oxigeo_core::error::OxiGeoError;

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
                    OxiGeoError::invalid_operation_builder(
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
    pub(crate) fn evaluate_expr(
        &self,
        expr: &Expr,
        batch: &RecordBatch,
        row_idx: usize,
    ) -> Result<Value> {
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
            Expr::Function { name, args } => {
                // Evaluate each argument first.
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|a| self.evaluate_expr(a, batch, row_idx))
                    .collect::<Result<Vec<_>>>()?;
                // Dispatch to the spatial-function evaluator. The coordinate
                // dimension is 2-D for the current row-based interpreter.
                crate::executor::spatial_funcs::evaluate_spatial_function(name, &arg_values, 2)
            }
            Expr::Between {
                expr: inner,
                low,
                high,
                negated,
            } => {
                let val = self.evaluate_expr(inner, batch, row_idx)?;
                let low_val = self.evaluate_expr(low, batch, row_idx)?;
                let high_val = self.evaluate_expr(high, batch, row_idx)?;

                // `val BETWEEN low AND high` == `val >= low AND val <= high`,
                // reusing the existing three-valued comparison semantics (NULL
                // operands yield NULL). `NOT BETWEEN` negates a definite result
                // but leaves NULL as NULL.
                let ge_low = self.evaluate_binary_op(&val, BinaryOperator::GtEq, &low_val)?;
                let le_high = self.evaluate_binary_op(&val, BinaryOperator::LtEq, &high_val)?;
                let in_range = match (ge_low, le_high) {
                    (Value::Boolean(a), Value::Boolean(b)) => Value::Boolean(a && b),
                    (Value::Null, _) | (_, Value::Null) => Value::Null,
                    (a, b) => {
                        return Err(QueryError::execution(format!(
                            "BETWEEN bounds did not evaluate to comparable values: {:?}, {:?}",
                            a, b
                        )));
                    }
                };
                Ok(match in_range {
                    Value::Boolean(b) => Value::Boolean(if *negated { !b } else { b }),
                    other => other, // NULL stays NULL
                })
            }
            Expr::InList {
                expr: inner,
                list,
                negated,
            } => {
                let val = self.evaluate_expr(inner, batch, row_idx)?;
                if matches!(val, Value::Null) {
                    return Ok(Value::Null);
                }
                let mut found = false;
                for item in list {
                    let item_val = self.evaluate_expr(item, batch, row_idx)?;
                    if let Value::Boolean(true) =
                        self.evaluate_binary_op(&val, BinaryOperator::Eq, &item_val)?
                    {
                        found = true;
                        break;
                    }
                }
                Ok(Value::Boolean(if *negated { !found } else { found }))
            }
            Expr::Case {
                operand,
                when_then,
                else_result,
            } => {
                let operand_val = match operand {
                    Some(op) => Some(self.evaluate_expr(op, batch, row_idx)?),
                    None => None,
                };
                for (when_expr, then_expr) in when_then {
                    let when_val = self.evaluate_expr(when_expr, batch, row_idx)?;
                    let condition_met = match &operand_val {
                        // Searched vs. simple CASE: with an operand, compare it
                        // to each WHEN value; without, the WHEN must be boolean.
                        Some(op_val) => matches!(
                            self.evaluate_binary_op(op_val, BinaryOperator::Eq, &when_val)?,
                            Value::Boolean(true)
                        ),
                        None => matches!(when_val, Value::Boolean(true)),
                    };
                    if condition_met {
                        return self.evaluate_expr(then_expr, batch, row_idx);
                    }
                }
                match else_result {
                    Some(else_expr) => self.evaluate_expr(else_expr, batch, row_idx),
                    None => Ok(Value::Null),
                }
            }
            Expr::Cast {
                expr: inner,
                data_type,
            } => {
                let val = self.evaluate_expr(inner, batch, row_idx)?;
                Self::cast_value(val, *data_type)
            }
            _ => Err(QueryError::unsupported(
                OxiGeoError::not_supported_builder("Unsupported expression type in filter")
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
                OxiGeoError::not_supported_builder(
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
                // Checked arithmetic: overflow yields NULL rather than panicking
                // (debug) or silently wrapping (release), mirroring the
                // divide/modulo-by-zero-returns-NULL convention below.
                BinaryOperator::Plus => {
                    Ok(l.checked_add(*r).map(Value::Int32).unwrap_or(Value::Null))
                }
                BinaryOperator::Minus => {
                    Ok(l.checked_sub(*r).map(Value::Int32).unwrap_or(Value::Null))
                }
                BinaryOperator::Multiply => {
                    Ok(l.checked_mul(*r).map(Value::Int32).unwrap_or(Value::Null))
                }
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
                // Checked arithmetic: overflow yields NULL rather than panicking
                // (debug) or silently wrapping (release).
                BinaryOperator::Plus => {
                    Ok(l.checked_add(*r).map(Value::Int64).unwrap_or(Value::Null))
                }
                BinaryOperator::Minus => {
                    Ok(l.checked_sub(*r).map(Value::Int64).unwrap_or(Value::Null))
                }
                BinaryOperator::Multiply => {
                    Ok(l.checked_mul(*r).map(Value::Int64).unwrap_or(Value::Null))
                }
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
                // Lexicographic ordering.
                BinaryOperator::Lt => Ok(Value::Boolean(l < r)),
                BinaryOperator::LtEq => Ok(Value::Boolean(l <= r)),
                BinaryOperator::Gt => Ok(Value::Boolean(l > r)),
                BinaryOperator::GtEq => Ok(Value::Boolean(l >= r)),
                // Pattern matching (`l` is the value, `r` is the pattern).
                BinaryOperator::Like => Ok(Value::Boolean(crate::executor::like::like_match(
                    l, r, false,
                ))),
                BinaryOperator::NotLike => Ok(Value::Boolean(!crate::executor::like::like_match(
                    l, r, false,
                ))),
                BinaryOperator::ILike => Ok(Value::Boolean(crate::executor::like::like_match(
                    l, r, true,
                ))),
                BinaryOperator::NotILike => Ok(Value::Boolean(!crate::executor::like::like_match(
                    l, r, true,
                ))),
                _ => Err(QueryError::unsupported("Unsupported operator for strings")),
            },
            _ => Err(QueryError::execution(
                OxiGeoError::invalid_operation_builder("Type mismatch in binary operation")
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

    /// Evaluate a SQL `CAST(expr AS type)`.
    ///
    /// Performs a real type conversion (not a pass-through). NULL casts to NULL.
    /// Integer target types collapse to `Int32` (8/16/32-bit and their unsigned
    /// variants) or `Int64` (64-bit); float targets to `Float32`/`Float64`;
    /// `String` stringifies the value; `Boolean` uses numeric-nonzero / textual
    /// truthiness. Unsupported target types or unparseable strings surface an
    /// honest execution error rather than silently succeeding.
    fn cast_value(val: Value, target: crate::parser::ast::DataType) -> Result<Value> {
        use crate::parser::ast::DataType as Dt;

        if matches!(val, Value::Null) {
            return Ok(Value::Null);
        }

        // Helper: interpret the value as an i64 for integer/boolean targets.
        let as_i64 = |v: &Value| -> Result<i64> {
            match v {
                Value::Boolean(b) => Ok(if *b { 1 } else { 0 }),
                Value::Int32(i) => Ok(*i as i64),
                Value::Int64(i) => Ok(*i),
                Value::Float32(f) => Ok(*f as i64),
                Value::Float64(f) => Ok(*f as i64),
                Value::String(s) => s.trim().parse::<i64>().map_err(|_| {
                    QueryError::execution(format!("cannot cast string {:?} to integer", s))
                }),
                Value::Geometry(_) => Err(QueryError::unsupported(
                    "cannot cast a geometry to an integer",
                )),
                Value::Null => Ok(0),
            }
        };

        let as_f64 = |v: &Value| -> Result<f64> {
            match v {
                Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
                Value::Int32(i) => Ok(*i as f64),
                Value::Int64(i) => Ok(*i as f64),
                Value::Float32(f) => Ok(*f as f64),
                Value::Float64(f) => Ok(*f),
                Value::String(s) => s.trim().parse::<f64>().map_err(|_| {
                    QueryError::execution(format!("cannot cast string {:?} to float", s))
                }),
                Value::Geometry(_) => {
                    Err(QueryError::unsupported("cannot cast a geometry to a float"))
                }
                Value::Null => Ok(0.0),
            }
        };

        match target {
            Dt::Boolean => match &val {
                Value::Boolean(b) => Ok(Value::Boolean(*b)),
                Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                    "true" | "t" | "1" | "yes" | "y" => Ok(Value::Boolean(true)),
                    "false" | "f" | "0" | "no" | "n" => Ok(Value::Boolean(false)),
                    _ => Err(QueryError::execution(format!(
                        "cannot cast string {:?} to boolean",
                        s
                    ))),
                },
                _ => Ok(Value::Boolean(as_i64(&val)? != 0)),
            },
            Dt::Int8 | Dt::Int16 | Dt::Int32 | Dt::UInt8 | Dt::UInt16 | Dt::UInt32 => {
                Ok(Value::Int32(as_i64(&val)? as i32))
            }
            Dt::Int64 | Dt::UInt64 => Ok(Value::Int64(as_i64(&val)?)),
            Dt::Float32 => Ok(Value::Float32(as_f64(&val)? as f32)),
            Dt::Float64 => Ok(Value::Float64(as_f64(&val)?)),
            Dt::String => Ok(Value::String(Self::value_to_display_string(&val)?)),
            Dt::Binary | Dt::Timestamp | Dt::Date | Dt::Geometry => {
                Err(QueryError::unsupported(format!(
                    "CAST to {:?} is not supported in filter expressions",
                    target
                )))
            }
        }
    }

    /// Render a scalar [`Value`] as its textual representation for `CAST … AS
    /// String`.
    fn value_to_display_string(val: &Value) -> Result<String> {
        Ok(match val {
            Value::Null => "NULL".to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Int32(i) => i.to_string(),
            Value::Int64(i) => i.to_string(),
            Value::Float32(f) => f.to_string(),
            Value::Float64(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Geometry(_) => {
                return Err(QueryError::unsupported(
                    "cannot cast a geometry to a string in filter expressions",
                ));
            }
        })
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

/// Evaluate an expression for a single row without a pre-built [`Filter`].
///
/// Shared by the Sort and Aggregate operators so `ORDER BY` / `GROUP BY` can
/// evaluate arbitrary scalar expressions (columns, arithmetic, functions)
/// through the exact same evaluator used for WHERE-clause predicates. The
/// dummy predicate is never inspected by [`Filter::evaluate_expr`].
pub(crate) fn evaluate_expr_for_row(
    expr: &Expr,
    batch: &RecordBatch,
    row_idx: usize,
) -> Result<Value> {
    Filter::new(Expr::Wildcard).evaluate_expr(expr, batch, row_idx)
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
    /// Geometry value (constructed by spatial functions or parsed from WKT).
    Geometry(geo::Geometry<f64>),
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

    fn string_batch() -> Result<RecordBatch> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "name".to_string(),
            crate::executor::scan::DataType::String,
            true,
        )]));
        let columns = vec![ColumnData::String(vec![
            Some("Apple".to_string()),
            Some("banana".to_string()),
            Some("Cherry".to_string()),
            Some("foobar".to_string()),
        ])];
        RecordBatch::new(schema, columns, 4)
    }

    fn name_col(expr_name: &str) -> Expr {
        Expr::Column {
            table: None,
            name: expr_name.to_string(),
        }
    }

    #[test]
    fn test_where_like_case_sensitive() -> Result<()> {
        let batch = string_batch()?;
        // WHERE name LIKE '%oo%' should match only "foobar".
        let predicate = Expr::BinaryOp {
            left: Box::new(name_col("name")),
            op: BinaryOperator::Like,
            right: Box::new(Expr::Literal(Literal::String("%oo%".to_string()))),
        };
        let filtered = Filter::new(predicate).execute(&batch)?;
        assert_eq!(filtered.num_rows, 1);

        // LIKE is case-sensitive: uppercase pattern matches nothing here.
        let predicate = Expr::BinaryOp {
            left: Box::new(name_col("name")),
            op: BinaryOperator::Like,
            right: Box::new(Expr::Literal(Literal::String("A%".to_string()))),
        };
        let filtered = Filter::new(predicate).execute(&batch)?;
        assert_eq!(filtered.num_rows, 1); // only "Apple"

        let predicate = Expr::BinaryOp {
            left: Box::new(name_col("name")),
            op: BinaryOperator::Like,
            right: Box::new(Expr::Literal(Literal::String("a%".to_string()))),
        };
        let filtered = Filter::new(predicate).execute(&batch)?;
        assert_eq!(filtered.num_rows, 0); // "Apple" is capital A, none start lowercase 'a'
        Ok(())
    }

    #[test]
    fn test_where_not_like() -> Result<()> {
        let batch = string_batch()?;
        let predicate = Expr::BinaryOp {
            left: Box::new(name_col("name")),
            op: BinaryOperator::NotLike,
            right: Box::new(Expr::Literal(Literal::String("%oo%".to_string()))),
        };
        let filtered = Filter::new(predicate).execute(&batch)?;
        assert_eq!(filtered.num_rows, 3); // everything except "foobar"
        Ok(())
    }

    #[test]
    fn test_where_ilike_case_insensitive() -> Result<()> {
        let batch = string_batch()?;
        let predicate = Expr::BinaryOp {
            left: Box::new(name_col("name")),
            op: BinaryOperator::ILike,
            right: Box::new(Expr::Literal(Literal::String("a%".to_string()))),
        };
        let filtered = Filter::new(predicate).execute(&batch)?;
        assert_eq!(filtered.num_rows, 1); // "Apple" matches case-insensitively
        Ok(())
    }

    #[test]
    fn test_where_string_ordering() -> Result<()> {
        let batch = string_batch()?;
        // WHERE name > 'C' -> lexicographic: "Cherry", "banana", "foobar"
        // ASCII: uppercase letters sort before lowercase, so "Cherry" > "C",
        // "banana"/"foobar" (lowercase) > "C" too; "Apple" < "C".
        let predicate = Expr::BinaryOp {
            left: Box::new(name_col("name")),
            op: BinaryOperator::Gt,
            right: Box::new(Expr::Literal(Literal::String("C".to_string()))),
        };
        let filtered = Filter::new(predicate).execute(&batch)?;
        assert_eq!(filtered.num_rows, 3);
        Ok(())
    }

    #[test]
    fn test_where_int_overflow_returns_null_not_panic() -> Result<()> {
        // i32::MAX + 1 must not panic; the arithmetic yields NULL, so the
        // comparison against it is NULL (not selected) rather than crashing.
        let schema = Arc::new(Schema::new(vec![Field::new(
            "v".to_string(),
            crate::executor::scan::DataType::Int32,
            false,
        )]));
        let columns = vec![ColumnData::Int32(vec![Some(i32::MAX), Some(1)])];
        let batch = RecordBatch::new(schema, columns, 2)?;

        // WHERE v + 1 > 0
        let predicate = Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(name_col("v")),
                op: BinaryOperator::Plus,
                right: Box::new(Expr::Literal(Literal::Integer(1))),
            }),
            op: BinaryOperator::Gt,
            right: Box::new(Expr::Literal(Literal::Integer(0))),
        };
        // Literal::Integer maps to Int64, so v(Int32) + 1(Int64) coerces to
        // i64 and does not overflow. Test the pure-i32 overflow path directly.
        let ovf = Filter::new(Expr::Wildcard).evaluate_binary_op(
            &Value::Int32(i32::MAX),
            BinaryOperator::Plus,
            &Value::Int32(1),
        )?;
        assert_eq!(ovf, Value::Null);

        let ovf64 = Filter::new(Expr::Wildcard).evaluate_binary_op(
            &Value::Int64(i64::MAX),
            BinaryOperator::Multiply,
            &Value::Int64(2),
        )?;
        assert_eq!(ovf64, Value::Null);

        // Sanity: the end-to-end predicate still executes without panicking.
        let _ = Filter::new(predicate).execute(&batch)?;
        Ok(())
    }

    fn int_batch() -> Result<RecordBatch> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "v".to_string(),
            crate::executor::scan::DataType::Int64,
            false,
        )]));
        let columns = vec![ColumnData::Int64(vec![
            Some(1),
            Some(5),
            Some(10),
            Some(18),
            Some(65),
        ])];
        RecordBatch::new(schema, columns, 5)
    }

    fn v_col() -> Expr {
        Expr::Column {
            table: None,
            name: "v".to_string(),
        }
    }

    #[test]
    fn test_where_between() -> Result<()> {
        let batch = int_batch()?;
        // WHERE v BETWEEN 5 AND 18 -> {5, 10, 18}
        let predicate = Expr::Between {
            expr: Box::new(v_col()),
            low: Box::new(Expr::Literal(Literal::Integer(5))),
            high: Box::new(Expr::Literal(Literal::Integer(18))),
            negated: false,
        };
        assert_eq!(Filter::new(predicate).execute(&batch)?.num_rows, 3);

        // WHERE v NOT BETWEEN 5 AND 18 -> {1, 65}
        let predicate = Expr::Between {
            expr: Box::new(v_col()),
            low: Box::new(Expr::Literal(Literal::Integer(5))),
            high: Box::new(Expr::Literal(Literal::Integer(18))),
            negated: true,
        };
        assert_eq!(Filter::new(predicate).execute(&batch)?.num_rows, 2);
        Ok(())
    }

    #[test]
    fn test_where_in_list() -> Result<()> {
        let batch = int_batch()?;
        // WHERE v IN (5, 65, 999) -> {5, 65}
        let predicate = Expr::InList {
            expr: Box::new(v_col()),
            list: vec![
                Expr::Literal(Literal::Integer(5)),
                Expr::Literal(Literal::Integer(65)),
                Expr::Literal(Literal::Integer(999)),
            ],
            negated: false,
        };
        assert_eq!(Filter::new(predicate).execute(&batch)?.num_rows, 2);

        // WHERE v NOT IN (5, 65) -> {1, 10, 18}
        let predicate = Expr::InList {
            expr: Box::new(v_col()),
            list: vec![
                Expr::Literal(Literal::Integer(5)),
                Expr::Literal(Literal::Integer(65)),
            ],
            negated: true,
        };
        assert_eq!(Filter::new(predicate).execute(&batch)?.num_rows, 3);
        Ok(())
    }

    #[test]
    fn test_where_case() -> Result<()> {
        let batch = int_batch()?;
        // WHERE (CASE WHEN v >= 18 THEN true ELSE false END) -> {18, 65}
        let predicate = Expr::Case {
            operand: None,
            when_then: vec![(
                Expr::BinaryOp {
                    left: Box::new(v_col()),
                    op: BinaryOperator::GtEq,
                    right: Box::new(Expr::Literal(Literal::Integer(18))),
                },
                Expr::Literal(Literal::Boolean(true)),
            )],
            else_result: Some(Box::new(Expr::Literal(Literal::Boolean(false)))),
        };
        assert_eq!(Filter::new(predicate).execute(&batch)?.num_rows, 2);
        Ok(())
    }

    #[test]
    fn test_where_cast() -> Result<()> {
        // Cast a string column to integer and compare.
        let schema = Arc::new(Schema::new(vec![Field::new(
            "s".to_string(),
            crate::executor::scan::DataType::String,
            false,
        )]));
        let columns = vec![ColumnData::String(vec![
            Some("1".to_string()),
            Some("42".to_string()),
            Some("100".to_string()),
        ])];
        let batch = RecordBatch::new(schema, columns, 3)?;

        // WHERE CAST(s AS Int64) > 40 -> {42, 100}
        let predicate = Expr::BinaryOp {
            left: Box::new(Expr::Cast {
                expr: Box::new(Expr::Column {
                    table: None,
                    name: "s".to_string(),
                }),
                data_type: crate::parser::ast::DataType::Int64,
            }),
            op: BinaryOperator::Gt,
            right: Box::new(Expr::Literal(Literal::Integer(40))),
        };
        assert_eq!(Filter::new(predicate).execute(&batch)?.num_rows, 2);
        Ok(())
    }
}
