//! Expression evaluation for join conditions

use super::{Join, JoinContext, JoinValue};
use crate::error::{QueryError, Result};
use crate::executor::scan::{ColumnData, RecordBatch};
use crate::parser::ast::{BinaryOperator, Expr, Literal, UnaryOperator};

impl Join {
    pub(super) fn evaluate_join_condition(
        &self,
        left: &RecordBatch,
        right: &RecordBatch,
        left_row: usize,
        right_row: usize,
    ) -> Result<bool> {
        match &self.on_condition {
            Some(expr) => {
                let ctx = JoinContext {
                    left,
                    right,
                    left_row,
                    right_row,
                    left_alias: self.left_alias.as_deref(),
                    right_alias: self.right_alias.as_deref(),
                };
                let result = self.evaluate_expr(expr, &ctx)?;
                Ok(result.to_bool().unwrap_or(false))
            }
            None => Ok(true), // No condition means always true (cross join behavior)
        }
    }

    /// Evaluate an expression in the join context.
    pub(super) fn evaluate_expr(&self, expr: &Expr, ctx: &JoinContext) -> Result<JoinValue> {
        match expr {
            Expr::Column { table, name } => self.resolve_column_value(table.as_deref(), name, ctx),

            Expr::Literal(lit) => Ok(self.literal_to_value(lit)),

            Expr::BinaryOp { left, op, right } => {
                let left_val = self.evaluate_expr(left, ctx)?;
                let right_val = self.evaluate_expr(right, ctx)?;
                self.evaluate_binary_op(&left_val, op, &right_val)
            }

            Expr::UnaryOp { op, expr } => {
                let val = self.evaluate_expr(expr, ctx)?;
                self.evaluate_unary_op(op, &val)
            }

            Expr::IsNull(inner) => {
                let val = self.evaluate_expr(inner, ctx)?;
                Ok(JoinValue::Boolean(val.is_null()))
            }

            Expr::IsNotNull(inner) => {
                let val = self.evaluate_expr(inner, ctx)?;
                Ok(JoinValue::Boolean(!val.is_null()))
            }

            Expr::Between {
                expr,
                low,
                high,
                negated,
            } => {
                let val = self.evaluate_expr(expr, ctx)?;
                let low_val = self.evaluate_expr(low, ctx)?;
                let high_val = self.evaluate_expr(high, ctx)?;

                let result = match (val.compare(&low_val), val.compare(&high_val)) {
                    (Some(cmp_low), Some(cmp_high)) => {
                        let in_range = cmp_low != std::cmp::Ordering::Less
                            && cmp_high != std::cmp::Ordering::Greater;
                        if *negated { !in_range } else { in_range }
                    }
                    _ => return Ok(JoinValue::Null),
                };
                Ok(JoinValue::Boolean(result))
            }

            Expr::InList {
                expr,
                list,
                negated,
            } => {
                let val = self.evaluate_expr(expr, ctx)?;
                if val.is_null() {
                    return Ok(JoinValue::Null);
                }

                let mut found = false;
                for item in list {
                    let item_val = self.evaluate_expr(item, ctx)?;
                    if let Some(true) = val.equals(&item_val) {
                        found = true;
                        break;
                    }
                }

                let result = if *negated { !found } else { found };
                Ok(JoinValue::Boolean(result))
            }

            Expr::Case {
                operand,
                when_then,
                else_result,
            } => {
                let operand_val = match operand {
                    Some(op) => Some(self.evaluate_expr(op, ctx)?),
                    None => None,
                };

                for (when_expr, then_expr) in when_then {
                    let when_val = self.evaluate_expr(when_expr, ctx)?;
                    let condition_met = match &operand_val {
                        Some(op) => op.equals(&when_val).unwrap_or(false),
                        None => when_val.to_bool().unwrap_or(false),
                    };

                    if condition_met {
                        return self.evaluate_expr(then_expr, ctx);
                    }
                }

                match else_result {
                    Some(else_expr) => self.evaluate_expr(else_expr, ctx),
                    None => Ok(JoinValue::Null),
                }
            }

            Expr::Function { name, args } => self.evaluate_function(name, args, ctx),

            Expr::Cast { expr, data_type: _ } => {
                // Simple cast: just return the value (type coercion happens in comparisons)
                self.evaluate_expr(expr, ctx)
            }

            _ => Err(QueryError::unsupported(format!(
                "Expression type not supported in join condition: {:?}",
                expr
            ))),
        }
    }

    /// Resolve a column reference to a value.
    pub(super) fn resolve_column_value(
        &self,
        table: Option<&str>,
        name: &str,
        ctx: &JoinContext,
    ) -> Result<JoinValue> {
        // Try left table first
        if let Some(idx) = self.find_column_index(ctx.left, table, name, ctx.left_alias) {
            return self.get_column_value(&ctx.left.columns[idx], ctx.left_row);
        }

        // Try right table
        if let Some(idx) = self.find_column_index(ctx.right, table, name, ctx.right_alias) {
            return self.get_column_value(&ctx.right.columns[idx], ctx.right_row);
        }

        Err(QueryError::ColumnNotFound(format!(
            "{}{}",
            table.map(|t| format!("{}.", t)).unwrap_or_default(),
            name
        )))
    }

    /// Convert a literal to a JoinValue.
    pub(super) fn literal_to_value(&self, lit: &Literal) -> JoinValue {
        match lit {
            Literal::Null => JoinValue::Null,
            Literal::Boolean(b) => JoinValue::Boolean(*b),
            Literal::Integer(i) => JoinValue::Integer(*i),
            Literal::Float(f) => JoinValue::Float(*f),
            Literal::String(s) => JoinValue::String(s.clone()),
        }
    }

    /// Evaluate a binary operation.
    pub(super) fn evaluate_binary_op(
        &self,
        left: &JoinValue,
        op: &BinaryOperator,
        right: &JoinValue,
    ) -> Result<JoinValue> {
        match op {
            BinaryOperator::Eq => Ok(match left.equals(right) {
                Some(b) => JoinValue::Boolean(b),
                None => JoinValue::Null,
            }),

            BinaryOperator::NotEq => Ok(match left.equals(right) {
                Some(b) => JoinValue::Boolean(!b),
                None => JoinValue::Null,
            }),

            BinaryOperator::Lt => Ok(match left.compare(right) {
                Some(std::cmp::Ordering::Less) => JoinValue::Boolean(true),
                Some(_) => JoinValue::Boolean(false),
                None => JoinValue::Null,
            }),

            BinaryOperator::LtEq => Ok(match left.compare(right) {
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal) => {
                    JoinValue::Boolean(true)
                }
                Some(_) => JoinValue::Boolean(false),
                None => JoinValue::Null,
            }),

            BinaryOperator::Gt => Ok(match left.compare(right) {
                Some(std::cmp::Ordering::Greater) => JoinValue::Boolean(true),
                Some(_) => JoinValue::Boolean(false),
                None => JoinValue::Null,
            }),

            BinaryOperator::GtEq => Ok(match left.compare(right) {
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal) => {
                    JoinValue::Boolean(true)
                }
                Some(_) => JoinValue::Boolean(false),
                None => JoinValue::Null,
            }),

            BinaryOperator::And => {
                // Three-valued logic for AND
                match (left.to_bool(), right.to_bool()) {
                    (Some(true), Some(true)) => Ok(JoinValue::Boolean(true)),
                    (Some(false), _) | (_, Some(false)) => Ok(JoinValue::Boolean(false)),
                    _ => Ok(JoinValue::Null),
                }
            }

            BinaryOperator::Or => {
                // Three-valued logic for OR
                match (left.to_bool(), right.to_bool()) {
                    (Some(true), _) | (_, Some(true)) => Ok(JoinValue::Boolean(true)),
                    (Some(false), Some(false)) => Ok(JoinValue::Boolean(false)),
                    _ => Ok(JoinValue::Null),
                }
            }

            BinaryOperator::Plus => left
                .add(right)
                .ok_or_else(|| QueryError::execution("Cannot add values of incompatible types")),

            BinaryOperator::Minus => left.subtract(right).ok_or_else(|| {
                QueryError::execution("Cannot subtract values of incompatible types")
            }),

            BinaryOperator::Multiply => left.multiply(right).ok_or_else(|| {
                QueryError::execution("Cannot multiply values of incompatible types")
            }),

            BinaryOperator::Divide => left
                .divide(right)
                .ok_or_else(|| QueryError::execution("Cannot divide values or division by zero")),

            BinaryOperator::Modulo => left
                .modulo(right)
                .ok_or_else(|| QueryError::execution("Cannot compute modulo or modulo by zero")),

            BinaryOperator::Concat => match (left, right) {
                (JoinValue::String(a), JoinValue::String(b)) => {
                    Ok(JoinValue::String(format!("{}{}", a, b)))
                }
                (JoinValue::Null, _) | (_, JoinValue::Null) => Ok(JoinValue::Null),
                _ => Ok(JoinValue::String(format!(
                    "{}{}",
                    self.value_to_string(left),
                    self.value_to_string(right)
                ))),
            },

            BinaryOperator::Like => Ok(match left.matches_like(right) {
                Some(b) => JoinValue::Boolean(b),
                None => JoinValue::Null,
            }),

            BinaryOperator::NotLike => Ok(match left.matches_like(right) {
                Some(b) => JoinValue::Boolean(!b),
                None => JoinValue::Null,
            }),
        }
    }

    /// Convert a value to string representation.
    pub(super) fn value_to_string(&self, val: &JoinValue) -> String {
        match val {
            JoinValue::Null => "NULL".to_string(),
            JoinValue::Boolean(b) => b.to_string(),
            JoinValue::Integer(i) => i.to_string(),
            JoinValue::Float(f) => f.to_string(),
            JoinValue::String(s) => s.clone(),
        }
    }

    /// Evaluate a unary operation.
    pub(super) fn evaluate_unary_op(
        &self,
        op: &UnaryOperator,
        val: &JoinValue,
    ) -> Result<JoinValue> {
        match op {
            UnaryOperator::Minus => val
                .negate()
                .ok_or_else(|| QueryError::execution("Cannot negate this value type")),
            UnaryOperator::Not => match val.to_bool() {
                Some(b) => Ok(JoinValue::Boolean(!b)),
                None => Ok(JoinValue::Null),
            },
        }
    }

    /// Evaluate a function call.
    pub(super) fn evaluate_function(
        &self,
        name: &str,
        args: &[Expr],
        ctx: &JoinContext,
    ) -> Result<JoinValue> {
        let upper_name = name.to_uppercase();

        match upper_name.as_str() {
            "COALESCE" => {
                for arg in args {
                    let val = self.evaluate_expr(arg, ctx)?;
                    if !val.is_null() {
                        return Ok(val);
                    }
                }
                Ok(JoinValue::Null)
            }

            "NULLIF" => {
                if args.len() != 2 {
                    return Err(QueryError::InvalidArgument(
                        "NULLIF requires exactly 2 arguments".to_string(),
                    ));
                }
                let val1 = self.evaluate_expr(&args[0], ctx)?;
                let val2 = self.evaluate_expr(&args[1], ctx)?;

                if val1.equals(&val2).unwrap_or(false) {
                    Ok(JoinValue::Null)
                } else {
                    Ok(val1)
                }
            }

            "ABS" => {
                if args.len() != 1 {
                    return Err(QueryError::InvalidArgument(
                        "ABS requires exactly 1 argument".to_string(),
                    ));
                }
                let val = self.evaluate_expr(&args[0], ctx)?;
                match val {
                    JoinValue::Integer(i) => Ok(JoinValue::Integer(i.abs())),
                    JoinValue::Float(f) => Ok(JoinValue::Float(f.abs())),
                    JoinValue::Null => Ok(JoinValue::Null),
                    _ => Err(QueryError::execution("ABS requires a numeric argument")),
                }
            }

            "UPPER" => {
                if args.len() != 1 {
                    return Err(QueryError::InvalidArgument(
                        "UPPER requires exactly 1 argument".to_string(),
                    ));
                }
                let val = self.evaluate_expr(&args[0], ctx)?;
                match val {
                    JoinValue::String(s) => Ok(JoinValue::String(s.to_uppercase())),
                    JoinValue::Null => Ok(JoinValue::Null),
                    _ => Err(QueryError::execution("UPPER requires a string argument")),
                }
            }

            "LOWER" => {
                if args.len() != 1 {
                    return Err(QueryError::InvalidArgument(
                        "LOWER requires exactly 1 argument".to_string(),
                    ));
                }
                let val = self.evaluate_expr(&args[0], ctx)?;
                match val {
                    JoinValue::String(s) => Ok(JoinValue::String(s.to_lowercase())),
                    JoinValue::Null => Ok(JoinValue::Null),
                    _ => Err(QueryError::execution("LOWER requires a string argument")),
                }
            }

            "LENGTH" => {
                if args.len() != 1 {
                    return Err(QueryError::InvalidArgument(
                        "LENGTH requires exactly 1 argument".to_string(),
                    ));
                }
                let val = self.evaluate_expr(&args[0], ctx)?;
                match val {
                    JoinValue::String(s) => Ok(JoinValue::Integer(s.len() as i64)),
                    JoinValue::Null => Ok(JoinValue::Null),
                    _ => Err(QueryError::execution("LENGTH requires a string argument")),
                }
            }

            _ => Err(QueryError::FunctionNotFound(name.to_string())),
        }
    }

    /// Get a value from a column at a specific row.
    pub(super) fn get_column_value(&self, column: &ColumnData, row: usize) -> Result<JoinValue> {
        match column {
            ColumnData::Boolean(data) => match data.get(row) {
                Some(Some(v)) => Ok(JoinValue::Boolean(*v)),
                Some(None) => Ok(JoinValue::Null),
                None => Err(QueryError::execution("Row index out of bounds")),
            },
            ColumnData::Int32(data) => match data.get(row) {
                Some(Some(v)) => Ok(JoinValue::Integer(*v as i64)),
                Some(None) => Ok(JoinValue::Null),
                None => Err(QueryError::execution("Row index out of bounds")),
            },
            ColumnData::Int64(data) => match data.get(row) {
                Some(Some(v)) => Ok(JoinValue::Integer(*v)),
                Some(None) => Ok(JoinValue::Null),
                None => Err(QueryError::execution("Row index out of bounds")),
            },
            ColumnData::Float32(data) => match data.get(row) {
                Some(Some(v)) => Ok(JoinValue::Float(*v as f64)),
                Some(None) => Ok(JoinValue::Null),
                None => Err(QueryError::execution("Row index out of bounds")),
            },
            ColumnData::Float64(data) => match data.get(row) {
                Some(Some(v)) => Ok(JoinValue::Float(*v)),
                Some(None) => Ok(JoinValue::Null),
                None => Err(QueryError::execution("Row index out of bounds")),
            },
            ColumnData::String(data) => match data.get(row) {
                Some(Some(v)) => Ok(JoinValue::String(v.clone())),
                Some(None) => Ok(JoinValue::Null),
                None => Err(QueryError::execution("Row index out of bounds")),
            },
            ColumnData::Binary(_) => {
                // Binary data cannot be compared directly
                Err(QueryError::unsupported(
                    "Binary column comparison not supported in join conditions",
                ))
            }
        }
    }

    /// Append a joined row.
    pub(super) fn append_row(
        &self,
        result_columns: &mut [Vec<Option<String>>],
        left: &RecordBatch,
        right: &RecordBatch,
        left_row: usize,
        right_row: usize,
    ) -> Result<()> {
        let mut col_idx = 0;

        // Append left columns
        for left_col_idx in 0..left.columns.len() {
            let value = self.get_value_as_string(&left.columns[left_col_idx], left_row);
            result_columns[col_idx].push(value);
            col_idx += 1;
        }

        // Append right columns
        for right_col_idx in 0..right.columns.len() {
            let value = self.get_value_as_string(&right.columns[right_col_idx], right_row);
            result_columns[col_idx].push(value);
            col_idx += 1;
        }

        Ok(())
    }

    /// Append left row with nulls for right side.
    pub(super) fn append_left_with_nulls(
        &self,
        result_columns: &mut [Vec<Option<String>>],
        left: &RecordBatch,
        right: &RecordBatch,
        left_row: usize,
    ) -> Result<()> {
        let mut col_idx = 0;

        // Append left columns
        for left_col_idx in 0..left.columns.len() {
            let value = self.get_value_as_string(&left.columns[left_col_idx], left_row);
            result_columns[col_idx].push(value);
            col_idx += 1;
        }

        // Append nulls for right columns
        for _ in 0..right.columns.len() {
            result_columns[col_idx].push(None);
            col_idx += 1;
        }

        Ok(())
    }

    /// Append nulls for left side with right row.
    pub(super) fn append_right_with_nulls(
        &self,
        result_columns: &mut [Vec<Option<String>>],
        left: &RecordBatch,
        right: &RecordBatch,
        right_row: usize,
    ) -> Result<()> {
        let mut col_idx = 0;

        // Append nulls for left columns
        for _ in 0..left.columns.len() {
            result_columns[col_idx].push(None);
            col_idx += 1;
        }

        // Append right columns
        for right_col_idx in 0..right.columns.len() {
            let value = self.get_value_as_string(&right.columns[right_col_idx], right_row);
            result_columns[col_idx].push(value);
            col_idx += 1;
        }

        Ok(())
    }

    /// Get value as string.
    pub(super) fn get_value_as_string(&self, column: &ColumnData, row: usize) -> Option<String> {
        match column {
            ColumnData::Boolean(data) => data.get(row).and_then(|v| v.map(|b| b.to_string())),
            ColumnData::Int32(data) => data.get(row).and_then(|v| v.map(|i| i.to_string())),
            ColumnData::Int64(data) => data.get(row).and_then(|v| v.map(|i| i.to_string())),
            ColumnData::Float32(data) => data.get(row).and_then(|v| v.map(|f| f.to_string())),
            ColumnData::Float64(data) => data.get(row).and_then(|v| v.map(|f| f.to_string())),
            ColumnData::String(data) => data.get(row).and_then(|v| v.clone()),
            ColumnData::Binary(_) => None,
        }
    }

    /// Convert intermediate columns to ColumnData.
    pub(super) fn convert_columns(&self, columns: Vec<Vec<Option<String>>>) -> Vec<ColumnData> {
        columns.into_iter().map(ColumnData::String).collect()
    }
}
