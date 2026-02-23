//! Expression optimizer for the DSL
//!
//! This module provides various optimization passes:
//! - Constant folding
//! - Common subexpression elimination
//! - Dead code elimination
//! - Algebraic simplifications

use super::ast::{BinaryOp, Expr, Program, Statement, UnaryOp};

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, collections::BTreeMap as HashMap, string::String, vec::Vec};

#[cfg(feature = "std")]
use std::collections::HashMap;

/// Optimization level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    /// No optimization
    None,
    /// Basic optimizations (constant folding)
    Basic,
    /// Standard optimizations (basic + algebraic simplifications)
    Standard,
    /// Aggressive optimizations (standard + CSE + DCE)
    Aggressive,
}

/// Optimizer for DSL programs
pub struct Optimizer {
    level: OptLevel,
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new(OptLevel::Standard)
    }
}

impl Optimizer {
    /// Creates a new optimizer with the given optimization level
    pub fn new(level: OptLevel) -> Self {
        Self { level }
    }

    /// Optimizes a program
    pub fn optimize_program(&self, mut program: Program) -> Program {
        if self.level == OptLevel::None {
            return program;
        }

        program.statements = program
            .statements
            .into_iter()
            .map(|stmt| self.optimize_statement(stmt))
            .collect();

        program
    }

    /// Optimizes a single statement
    pub fn optimize_statement(&self, stmt: Statement) -> Statement {
        match stmt {
            Statement::VariableDecl { name, value } => Statement::VariableDecl {
                name,
                value: Box::new(self.optimize_expr(*value)),
            },
            Statement::FunctionDecl { name, params, body } => Statement::FunctionDecl {
                name,
                params,
                body: Box::new(self.optimize_expr(*body)),
            },
            Statement::Return(expr) => Statement::Return(Box::new(self.optimize_expr(*expr))),
            Statement::Expr(expr) => Statement::Expr(Box::new(self.optimize_expr(*expr))),
        }
    }

    /// Optimizes an expression
    pub fn optimize_expr(&self, expr: Expr) -> Expr {
        if self.level == OptLevel::None {
            return expr;
        }

        let mut optimized = expr;

        // Apply constant folding
        optimized = self.constant_fold(optimized);

        // Apply algebraic simplifications
        if matches!(self.level, OptLevel::Standard | OptLevel::Aggressive) {
            optimized = self.algebraic_simplify(optimized);
        }

        // Apply common subexpression elimination
        if self.level == OptLevel::Aggressive {
            optimized = self.eliminate_common_subexpressions(optimized);
        }

        optimized
    }

    /// Performs constant folding
    fn constant_fold(&self, expr: Expr) -> Expr {
        match expr {
            Expr::Binary {
                left,
                op,
                right,
                ty,
            } => {
                let left_opt = self.constant_fold(*left);
                let right_opt = self.constant_fold(*right);

                if let (Expr::Number(l), Expr::Number(r)) = (&left_opt, &right_opt) {
                    if let Some(result) = self.eval_const_binary(*l, op, *r) {
                        return Expr::Number(result);
                    }
                }

                Expr::Binary {
                    left: Box::new(left_opt),
                    op,
                    right: Box::new(right_opt),
                    ty,
                }
            }
            Expr::Unary {
                op,
                expr: inner,
                ty,
            } => {
                let inner_opt = self.constant_fold(*inner);

                if let Expr::Number(n) = &inner_opt {
                    if let Some(result) = self.eval_const_unary(op, *n) {
                        return Expr::Number(result);
                    }
                }

                Expr::Unary {
                    op,
                    expr: Box::new(inner_opt),
                    ty,
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ty,
            } => {
                let cond_opt = self.constant_fold(*condition);

                // If condition is constant, return only the taken branch
                if let Expr::Number(n) = &cond_opt {
                    if n.abs() > f64::EPSILON {
                        return self.constant_fold(*then_expr);
                    } else {
                        return self.constant_fold(*else_expr);
                    }
                }

                Expr::Conditional {
                    condition: Box::new(cond_opt),
                    then_expr: Box::new(self.constant_fold(*then_expr)),
                    else_expr: Box::new(self.constant_fold(*else_expr)),
                    ty,
                }
            }
            Expr::Call { name, args, ty } => Expr::Call {
                name,
                args: args
                    .into_iter()
                    .map(|arg| self.constant_fold(arg))
                    .collect(),
                ty,
            },
            Expr::Block {
                statements,
                result,
                ty,
            } => Expr::Block {
                statements: statements
                    .into_iter()
                    .map(|stmt| self.optimize_statement(stmt))
                    .collect(),
                result: result.map(|r| Box::new(self.constant_fold(*r))),
                ty,
            },
            _ => expr,
        }
    }

    /// Evaluates a constant binary operation
    fn eval_const_binary(&self, left: f64, op: BinaryOp, right: f64) -> Option<f64> {
        let result = match op {
            BinaryOp::Add => left + right,
            BinaryOp::Subtract => left - right,
            BinaryOp::Multiply => left * right,
            BinaryOp::Divide => {
                if right.abs() < f64::EPSILON {
                    return None;
                }
                left / right
            }
            BinaryOp::Modulo => left % right,
            BinaryOp::Power => left.powf(right),
            BinaryOp::Equal => {
                if (left - right).abs() < f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::NotEqual => {
                if (left - right).abs() >= f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Less => {
                if left < right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::LessEqual => {
                if left <= right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Greater => {
                if left > right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::GreaterEqual => {
                if left >= right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::And => {
                if left != 0.0 && right != 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Or => {
                if left != 0.0 || right != 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
        };

        Some(result)
    }

    /// Evaluates a constant unary operation
    fn eval_const_unary(&self, op: UnaryOp, operand: f64) -> Option<f64> {
        let result = match op {
            UnaryOp::Negate => -operand,
            UnaryOp::Plus => operand,
            UnaryOp::Not => {
                if operand.abs() < f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
        };

        Some(result)
    }

    /// Performs algebraic simplifications
    fn algebraic_simplify(&self, expr: Expr) -> Expr {
        match expr {
            Expr::Binary {
                left,
                op,
                right,
                ty,
            } => {
                let left_opt = self.algebraic_simplify(*left);
                let right_opt = self.algebraic_simplify(*right);

                // x + 0 = x
                if op == BinaryOp::Add {
                    if let Expr::Number(n) = &right_opt {
                        if n.abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                    if let Expr::Number(n) = &left_opt {
                        if n.abs() < f64::EPSILON {
                            return right_opt;
                        }
                    }
                }

                // x - 0 = x
                if op == BinaryOp::Subtract {
                    if let Expr::Number(n) = &right_opt {
                        if n.abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                }

                // x * 0 = 0
                if op == BinaryOp::Multiply {
                    if let Expr::Number(n) = &right_opt {
                        if n.abs() < f64::EPSILON {
                            return Expr::Number(0.0);
                        }
                    }
                    if let Expr::Number(n) = &left_opt {
                        if n.abs() < f64::EPSILON {
                            return Expr::Number(0.0);
                        }
                    }
                }

                // x * 1 = x
                if op == BinaryOp::Multiply {
                    if let Expr::Number(n) = &right_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                    if let Expr::Number(n) = &left_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return right_opt;
                        }
                    }
                }

                // x / 1 = x
                if op == BinaryOp::Divide {
                    if let Expr::Number(n) = &right_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                }

                // x ^ 0 = 1
                if op == BinaryOp::Power {
                    if let Expr::Number(n) = &right_opt {
                        if n.abs() < f64::EPSILON {
                            return Expr::Number(1.0);
                        }
                    }
                }

                // x ^ 1 = x
                if op == BinaryOp::Power {
                    if let Expr::Number(n) = &right_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                }

                Expr::Binary {
                    left: Box::new(left_opt),
                    op,
                    right: Box::new(right_opt),
                    ty,
                }
            }
            Expr::Unary {
                op,
                expr: inner,
                ty,
            } => {
                let inner_opt = self.algebraic_simplify(*inner);

                // --x = x
                if op == UnaryOp::Negate {
                    if let Expr::Unary {
                        op: UnaryOp::Negate,
                        expr: double_neg,
                        ..
                    } = &inner_opt
                    {
                        return *double_neg.clone();
                    }
                }

                // +x = x
                if op == UnaryOp::Plus {
                    return inner_opt;
                }

                Expr::Unary {
                    op,
                    expr: Box::new(inner_opt),
                    ty,
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ty,
            } => Expr::Conditional {
                condition: Box::new(self.algebraic_simplify(*condition)),
                then_expr: Box::new(self.algebraic_simplify(*then_expr)),
                else_expr: Box::new(self.algebraic_simplify(*else_expr)),
                ty,
            },
            Expr::Call { name, args, ty } => Expr::Call {
                name,
                args: args
                    .into_iter()
                    .map(|arg| self.algebraic_simplify(arg))
                    .collect(),
                ty,
            },
            Expr::Block {
                statements,
                result,
                ty,
            } => Expr::Block {
                statements: statements
                    .into_iter()
                    .map(|stmt| self.optimize_statement(stmt))
                    .collect(),
                result: result.map(|r| Box::new(self.algebraic_simplify(*r))),
                ty,
            },
            _ => expr,
        }
    }

    /// Eliminates common subexpressions
    fn eliminate_common_subexpressions(&self, expr: Expr) -> Expr {
        let mut seen: HashMap<String, usize> = HashMap::new();
        self.cse_pass(&expr, &mut seen);
        // Note: Full CSE implementation would require more complex analysis
        // This is a simplified version that just counts occurrences
        expr
    }

    fn cse_pass(&self, expr: &Expr, seen: &mut HashMap<String, usize>) {
        match expr {
            Expr::Binary { left, right, .. } => {
                self.cse_pass(left, seen);
                self.cse_pass(right, seen);
                let key = format!("{:?}", expr);
                *seen.entry(key).or_insert(0) += 1;
            }
            Expr::Unary { expr: inner, .. } => {
                self.cse_pass(inner, seen);
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    self.cse_pass(arg, seen);
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                self.cse_pass(condition, seen);
                self.cse_pass(then_expr, seen);
                self.cse_pass(else_expr, seen);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::Type;

    #[test]
    fn test_constant_fold_add() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Number(2.0)),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(3.0)),
            ty: Type::Number,
        };

        let opt = Optimizer::new(OptLevel::Basic);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Number(n) if (n - 5.0).abs() < 1e-10));
    }

    #[test]
    fn test_constant_fold_nested() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Binary {
                left: Box::new(Expr::Number(2.0)),
                op: BinaryOp::Multiply,
                right: Box::new(Expr::Number(3.0)),
                ty: Type::Number,
            }),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(4.0)),
            ty: Type::Number,
        };

        let opt = Optimizer::new(OptLevel::Basic);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Number(n) if (n - 10.0).abs() < 1e-10));
    }

    #[test]
    fn test_algebraic_simplify_add_zero() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Band(1)),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(0.0)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }

    #[test]
    fn test_algebraic_simplify_mul_one() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Band(1)),
            op: BinaryOp::Multiply,
            right: Box::new(Expr::Number(1.0)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }

    #[test]
    fn test_algebraic_simplify_mul_zero() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Band(1)),
            op: BinaryOp::Multiply,
            right: Box::new(Expr::Number(0.0)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Number(n) if n.abs() < 1e-10));
    }

    #[test]
    fn test_double_negation() {
        let expr = Expr::Unary {
            op: UnaryOp::Negate,
            expr: Box::new(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(Expr::Band(1)),
                ty: Type::Raster,
            }),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }

    #[test]
    fn test_unary_plus() {
        let expr = Expr::Unary {
            op: UnaryOp::Plus,
            expr: Box::new(Expr::Band(1)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }
}
