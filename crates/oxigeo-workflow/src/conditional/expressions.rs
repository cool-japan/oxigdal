//! Conditional expression evaluation.

use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A conditional expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expression {
    /// Literal value.
    Literal(Value),
    /// Variable reference.
    Variable(String),
    /// Binary operation.
    Binary {
        /// Left operand.
        left: Box<Expression>,
        /// Binary operator.
        op: BinaryOperator,
        /// Right operand.
        right: Box<Expression>,
    },
    /// Unary operation.
    Unary {
        /// Unary operator.
        op: UnaryOperator,
        /// Expression to apply operator to.
        expr: Box<Expression>,
    },
    /// Function call.
    Function {
        /// Function name.
        name: String,
        /// Function arguments.
        args: Vec<Expression>,
    },
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOperator {
    /// Equality (==).
    Eq,
    /// Inequality (!=).
    Ne,
    /// Less than (<).
    Lt,
    /// Less than or equal (<=).
    Le,
    /// Greater than (>).
    Gt,
    /// Greater than or equal (>=).
    Ge,
    /// Logical AND (&&).
    And,
    /// Logical OR (||).
    Or,
    /// Addition (+).
    Add,
    /// Subtraction (-).
    Sub,
    /// Multiplication (*).
    Mul,
    /// Division (/).
    Div,
    /// Modulo (%).
    Mod,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOperator {
    /// Logical NOT (!).
    Not,
    /// Negation (-).
    Neg,
}

/// Expression context for variable lookup.
pub type ExpressionContext = HashMap<String, Value>;

impl Expression {
    /// Create a literal expression.
    pub fn literal(value: Value) -> Self {
        Self::Literal(value)
    }

    /// Create a variable reference expression.
    pub fn variable<S: Into<String>>(name: S) -> Self {
        Self::Variable(name.into())
    }

    /// Create a binary expression.
    pub fn binary(left: Expression, op: BinaryOperator, right: Expression) -> Self {
        Self::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        }
    }

    /// Create an equality expression.
    pub fn eq(left: Expression, right: Expression) -> Self {
        Self::binary(left, BinaryOperator::Eq, right)
    }

    /// Create a logical AND expression.
    pub fn and(left: Expression, right: Expression) -> Self {
        Self::binary(left, BinaryOperator::And, right)
    }

    /// Create a logical OR expression.
    pub fn or(left: Expression, right: Expression) -> Self {
        Self::binary(left, BinaryOperator::Or, right)
    }

    /// Create a NOT expression.
    pub fn logical_not(expr: Expression) -> Self {
        Self::Unary {
            op: UnaryOperator::Not,
            expr: Box::new(expr),
        }
    }

    /// Evaluate the expression.
    pub fn evaluate(&self, context: &ExpressionContext) -> Result<Value> {
        match self {
            Expression::Literal(value) => Ok(value.clone()),

            Expression::Variable(name) => context.get(name).cloned().ok_or_else(|| {
                WorkflowError::conditional(format!("Variable '{}' not found", name))
            }),

            Expression::Binary { left, op, right } => {
                let left_val = left.evaluate(context)?;
                let right_val = right.evaluate(context)?;
                self.evaluate_binary(*op, &left_val, &right_val)
            }

            Expression::Unary { op, expr } => {
                let val = expr.evaluate(context)?;
                self.evaluate_unary(*op, &val)
            }

            Expression::Function { name, args } => {
                let arg_vals: Result<Vec<_>> =
                    args.iter().map(|arg| arg.evaluate(context)).collect();
                let arg_vals = arg_vals?;
                self.evaluate_function(name, &arg_vals)
            }
        }
    }

    /// Evaluate a binary operation.
    fn evaluate_binary(&self, op: BinaryOperator, left: &Value, right: &Value) -> Result<Value> {
        match op {
            BinaryOperator::Eq => Ok(Value::Bool(left == right)),
            BinaryOperator::Ne => Ok(Value::Bool(left != right)),
            BinaryOperator::Lt => self.compare_values(left, right, |cmp| cmp.is_lt()),
            BinaryOperator::Le => self.compare_values(left, right, |cmp| cmp.is_le()),
            BinaryOperator::Gt => self.compare_values(left, right, |cmp| cmp.is_gt()),
            BinaryOperator::Ge => self.compare_values(left, right, |cmp| cmp.is_ge()),
            BinaryOperator::And => self.logical_and(left, right),
            BinaryOperator::Or => self.logical_or(left, right),
            BinaryOperator::Add => self.arithmetic_op(left, right, |a, b| a + b),
            BinaryOperator::Sub => self.arithmetic_op(left, right, |a, b| a - b),
            BinaryOperator::Mul => self.arithmetic_op(left, right, |a, b| a * b),
            BinaryOperator::Div => {
                self.arithmetic_op(left, right, |a, b| if b == 0.0 { f64::NAN } else { a / b })
            }
            BinaryOperator::Mod => self.arithmetic_op(left, right, |a, b| a % b),
        }
    }

    /// Compare two values.
    fn compare_values<F>(&self, left: &Value, right: &Value, pred: F) -> Result<Value>
    where
        F: FnOnce(std::cmp::Ordering) -> bool,
    {
        let cmp = match (left, right) {
            (Value::Number(l), Value::Number(r)) => {
                let l = l
                    .as_f64()
                    .ok_or_else(|| WorkflowError::conditional("Invalid number"))?;
                let r = r
                    .as_f64()
                    .ok_or_else(|| WorkflowError::conditional("Invalid number"))?;
                l.partial_cmp(&r)
                    .ok_or_else(|| WorkflowError::conditional("NaN comparison"))?
            }
            (Value::String(l), Value::String(r)) => l.cmp(r),
            _ => {
                return Err(WorkflowError::conditional("Cannot compare these types"));
            }
        };

        Ok(Value::Bool(pred(cmp)))
    }

    /// Logical AND operation.
    fn logical_and(&self, left: &Value, right: &Value) -> Result<Value> {
        let left_bool = left
            .as_bool()
            .ok_or_else(|| WorkflowError::conditional("Expected boolean"))?;
        let right_bool = right
            .as_bool()
            .ok_or_else(|| WorkflowError::conditional("Expected boolean"))?;
        Ok(Value::Bool(left_bool && right_bool))
    }

    /// Logical OR operation.
    fn logical_or(&self, left: &Value, right: &Value) -> Result<Value> {
        let left_bool = left
            .as_bool()
            .ok_or_else(|| WorkflowError::conditional("Expected boolean"))?;
        let right_bool = right
            .as_bool()
            .ok_or_else(|| WorkflowError::conditional("Expected boolean"))?;
        Ok(Value::Bool(left_bool || right_bool))
    }

    /// Arithmetic operation.
    fn arithmetic_op<F>(&self, left: &Value, right: &Value, op: F) -> Result<Value>
    where
        F: FnOnce(f64, f64) -> f64,
    {
        let left_num = left
            .as_f64()
            .ok_or_else(|| WorkflowError::conditional("Expected number"))?;
        let right_num = right
            .as_f64()
            .ok_or_else(|| WorkflowError::conditional("Expected number"))?;

        let result = op(left_num, right_num);
        Ok(serde_json::json!(result))
    }

    /// Evaluate a unary operation.
    fn evaluate_unary(&self, op: UnaryOperator, val: &Value) -> Result<Value> {
        match op {
            UnaryOperator::Not => {
                let bool_val = val
                    .as_bool()
                    .ok_or_else(|| WorkflowError::conditional("Expected boolean"))?;
                Ok(Value::Bool(!bool_val))
            }
            UnaryOperator::Neg => {
                let num_val = val
                    .as_f64()
                    .ok_or_else(|| WorkflowError::conditional("Expected number"))?;
                Ok(serde_json::json!(-num_val))
            }
        }
    }

    /// Evaluate a function call.
    fn evaluate_function(&self, name: &str, args: &[Value]) -> Result<Value> {
        match name {
            "len" => {
                if args.len() != 1 {
                    return Err(WorkflowError::conditional("len() expects 1 argument"));
                }
                match &args[0] {
                    Value::String(s) => Ok(Value::Number(s.len().into())),
                    Value::Array(a) => Ok(Value::Number(a.len().into())),
                    _ => Err(WorkflowError::conditional("len() expects string or array")),
                }
            }
            "upper" => {
                if args.len() != 1 {
                    return Err(WorkflowError::conditional("upper() expects 1 argument"));
                }
                match &args[0] {
                    Value::String(s) => Ok(Value::String(s.to_uppercase())),
                    _ => Err(WorkflowError::conditional("upper() expects string")),
                }
            }
            "lower" => {
                if args.len() != 1 {
                    return Err(WorkflowError::conditional("lower() expects 1 argument"));
                }
                match &args[0] {
                    Value::String(s) => Ok(Value::String(s.to_lowercase())),
                    _ => Err(WorkflowError::conditional("lower() expects string")),
                }
            }
            _ => Err(WorkflowError::conditional(format!(
                "Unknown function '{}'",
                name
            ))),
        }
    }
}

/// Parse a simple conditional expression from a string.
/// Format: "variable operator value" (e.g., "status == 'success'")
pub fn parse_simple_expression(expr: &str) -> Result<Expression> {
    let parts: Vec<&str> = expr.split_whitespace().collect();

    if parts.len() != 3 {
        return Err(WorkflowError::conditional(
            "Invalid expression format. Expected: 'variable operator value'",
        ));
    }

    let var = Expression::variable(parts[0]);
    let value = parse_value(parts[2])?;

    let op = match parts[1] {
        "==" => BinaryOperator::Eq,
        "!=" => BinaryOperator::Ne,
        "<" => BinaryOperator::Lt,
        "<=" => BinaryOperator::Le,
        ">" => BinaryOperator::Gt,
        ">=" => BinaryOperator::Ge,
        _ => {
            return Err(WorkflowError::conditional(format!(
                "Unknown operator '{}'",
                parts[1]
            )));
        }
    };

    Ok(Expression::binary(var, op, Expression::literal(value)))
}

/// Parse a value from a string.
fn parse_value(s: &str) -> Result<Value> {
    // Try to parse as number
    if let Ok(num) = s.parse::<i64>() {
        return Ok(Value::Number(num.into()));
    }
    if let Ok(num) = s.parse::<f64>() {
        return Ok(serde_json::json!(num));
    }

    // Try to parse as boolean
    if let Ok(b) = s.parse::<bool>() {
        return Ok(Value::Bool(b));
    }

    // Parse as string (remove quotes if present)
    let s = s.trim_matches('\'').trim_matches('"');
    Ok(Value::String(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal() {
        let expr = Expression::literal(Value::Bool(true));
        let result = expr.evaluate(&HashMap::new()).expect("Failed to evaluate");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_variable() {
        let mut ctx = HashMap::new();
        ctx.insert("x".to_string(), Value::Number(42.into()));

        let expr = Expression::variable("x");
        let result = expr.evaluate(&ctx).expect("Failed to evaluate");
        assert_eq!(result, Value::Number(42.into()));
    }

    #[test]
    fn test_equality() {
        let mut ctx = HashMap::new();
        ctx.insert("status".to_string(), Value::String("success".to_string()));

        let expr = Expression::eq(
            Expression::variable("status"),
            Expression::literal(Value::String("success".to_string())),
        );

        let result = expr.evaluate(&ctx).expect("Failed to evaluate");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_comparison() {
        let mut ctx = HashMap::new();
        ctx.insert("count".to_string(), Value::Number(10.into()));

        let expr = Expression::binary(
            Expression::variable("count"),
            BinaryOperator::Gt,
            Expression::literal(Value::Number(5.into())),
        );

        let result = expr.evaluate(&ctx).expect("Failed to evaluate");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_parse_simple_expression() {
        let expr = parse_simple_expression("status == 'success'").expect("Failed to parse");

        let mut ctx = HashMap::new();
        ctx.insert("status".to_string(), Value::String("success".to_string()));

        let result = expr.evaluate(&ctx).expect("Failed to evaluate");
        assert_eq!(result, Value::Bool(true));
    }
}
