//! Expression optimizer: constant folding and algebraic simplifications

use super::ast::{BinaryOp, Expr, UnaryOp};

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Expression optimizer for constant folding and algebraic simplifications
pub(super) struct Optimizer;

impl Optimizer {
    /// Optimize an expression tree
    pub(super) fn optimize(expr: Expr) -> Expr {
        let expr = Self::constant_fold(expr);
        let expr = Self::algebraic_simplify(expr);
        Self::eliminate_common_subexpressions(expr)
    }

    /// Constant folding: evaluate constant expressions at compile time
    fn constant_fold(expr: Expr) -> Expr {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                let left = Self::constant_fold(*left);
                let right = Self::constant_fold(*right);

                // If both sides are constants, evaluate
                if let (Expr::Number(l), Expr::Number(r)) = (&left, &right) {
                    let result = match op {
                        BinaryOp::Add => l + r,
                        BinaryOp::Subtract => l - r,
                        BinaryOp::Multiply => l * r,
                        BinaryOp::Divide => {
                            if r.abs() < f64::EPSILON {
                                f64::NAN
                            } else {
                                l / r
                            }
                        }
                        BinaryOp::Power => l.powf(*r),
                        BinaryOp::Greater => {
                            if l > r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Less => {
                            if l < r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::GreaterEqual => {
                            if l >= r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::LessEqual => {
                            if l <= r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Equal => {
                            if (l - r).abs() < f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::NotEqual => {
                            if (l - r).abs() >= f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::And => {
                            if *l != 0.0 && *r != 0.0 {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Or => {
                            if *l != 0.0 || *r != 0.0 {
                                1.0
                            } else {
                                0.0
                            }
                        }
                    };
                    return Expr::Number(result);
                }

                Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                }
            }
            Expr::UnaryOp { op, expr } => {
                let expr = Self::constant_fold(*expr);
                if let Expr::Number(n) = expr {
                    let result = match op {
                        UnaryOp::Negate => -n,
                    };
                    return Expr::Number(result);
                }
                Expr::UnaryOp {
                    op,
                    expr: Box::new(expr),
                }
            }
            Expr::Function { name, args } => {
                let args: Vec<Expr> = args.into_iter().map(Self::constant_fold).collect();

                // If all args are constants, evaluate the function
                let all_const = args.iter().all(|arg| matches!(arg, Expr::Number(_)));
                if all_const {
                    let arg_vals: Vec<f64> = args
                        .iter()
                        .filter_map(|arg| {
                            if let Expr::Number(n) = arg {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect();

                    let result = match name.as_str() {
                        "sqrt" if arg_vals.len() == 1 => Some(arg_vals[0].sqrt()),
                        "abs" if arg_vals.len() == 1 => Some(arg_vals[0].abs()),
                        "log" if arg_vals.len() == 1 => Some(arg_vals[0].ln()),
                        "log10" if arg_vals.len() == 1 => Some(arg_vals[0].log10()),
                        "exp" if arg_vals.len() == 1 => Some(arg_vals[0].exp()),
                        "sin" if arg_vals.len() == 1 => Some(arg_vals[0].sin()),
                        "cos" if arg_vals.len() == 1 => Some(arg_vals[0].cos()),
                        "tan" if arg_vals.len() == 1 => Some(arg_vals[0].tan()),
                        "floor" if arg_vals.len() == 1 => Some(arg_vals[0].floor()),
                        "ceil" if arg_vals.len() == 1 => Some(arg_vals[0].ceil()),
                        "round" if arg_vals.len() == 1 => Some(arg_vals[0].round()),
                        "min" if !arg_vals.is_empty() => {
                            Some(arg_vals.iter().copied().fold(f64::INFINITY, f64::min))
                        }
                        "max" if !arg_vals.is_empty() => {
                            Some(arg_vals.iter().copied().fold(f64::NEG_INFINITY, f64::max))
                        }
                        _ => None,
                    };

                    if let Some(val) = result {
                        return Expr::Number(val);
                    }
                }

                Expr::Function { name, args }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let condition = Self::constant_fold(*condition);
                let then_expr = Self::constant_fold(*then_expr);
                let else_expr = Self::constant_fold(*else_expr);

                // If condition is constant, choose branch
                if let Expr::Number(cond) = condition {
                    if cond != 0.0 {
                        return then_expr;
                    } else {
                        return else_expr;
                    }
                }

                Expr::Conditional {
                    condition: Box::new(condition),
                    then_expr: Box::new(then_expr),
                    else_expr: Box::new(else_expr),
                }
            }
            other => other,
        }
    }

    /// Algebraic simplifications: x + 0 -> x, x * 1 -> x, x * 0 -> 0, etc.
    fn algebraic_simplify(expr: Expr) -> Expr {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                let left = Self::algebraic_simplify(*left);
                let right = Self::algebraic_simplify(*right);

                match (&left, op, &right) {
                    // x + 0 = x, 0 + x = x
                    (_, BinaryOp::Add, Expr::Number(n)) if n.abs() < f64::EPSILON => left,
                    (Expr::Number(n), BinaryOp::Add, _) if n.abs() < f64::EPSILON => right,

                    // x - 0 = x
                    (_, BinaryOp::Subtract, Expr::Number(n)) if n.abs() < f64::EPSILON => left,

                    // x * 0 = 0, 0 * x = 0
                    (_, BinaryOp::Multiply, Expr::Number(n))
                    | (Expr::Number(n), BinaryOp::Multiply, _)
                        if n.abs() < f64::EPSILON =>
                    {
                        Expr::Number(0.0)
                    }

                    // x * 1 = x, 1 * x = x
                    (_, BinaryOp::Multiply, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => {
                        left
                    }
                    (Expr::Number(n), BinaryOp::Multiply, _) if (n - 1.0).abs() < f64::EPSILON => {
                        right
                    }

                    // x / 1 = x
                    (_, BinaryOp::Divide, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => {
                        left
                    }

                    // x ^ 0 = 1
                    (_, BinaryOp::Power, Expr::Number(n)) if n.abs() < f64::EPSILON => {
                        Expr::Number(1.0)
                    }

                    // x ^ 1 = x
                    (_, BinaryOp::Power, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => left,

                    _ => Expr::BinaryOp {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    },
                }
            }
            Expr::UnaryOp { op, expr } => {
                let expr = Self::algebraic_simplify(*expr);
                Expr::UnaryOp {
                    op,
                    expr: Box::new(expr),
                }
            }
            Expr::Function { name, args } => {
                let args = args.into_iter().map(Self::algebraic_simplify).collect();
                Expr::Function { name, args }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => Expr::Conditional {
                condition: Box::new(Self::algebraic_simplify(*condition)),
                then_expr: Box::new(Self::algebraic_simplify(*then_expr)),
                else_expr: Box::new(Self::algebraic_simplify(*else_expr)),
            },
            other => other,
        }
    }

    /// Common Subexpression Elimination (simplified version)
    /// In a full implementation, this would detect and cache repeated subexpressions
    fn eliminate_common_subexpressions(expr: Expr) -> Expr {
        // For now, this is a placeholder for a more sophisticated CSE pass
        // A full implementation would:
        // 1. Build a hash map of expression -> cache variable
        // 2. Replace repeated expressions with cache lookups
        // 3. Require changing the evaluation model to support cached values
        expr
    }
}
