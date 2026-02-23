//! AST Compiler and Executor
//!
//! This module compiles the AST into executable operations and provides
//! runtime execution with type checking and optimization.

use super::ast::{BinaryOp, Expr, Program, Statement, UnaryOp};
use super::functions::FunctionRegistry;
use super::variables::{BandContext, Environment, Value};
use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, string::String, vec::Vec};

/// Compiled program ready for execution
pub struct CompiledProgram {
    program: Program,
    func_registry: FunctionRegistry,
}

impl CompiledProgram {
    /// Creates a new compiled program from AST
    pub fn new(program: Program) -> Self {
        Self {
            program,
            func_registry: FunctionRegistry::new(),
        }
    }

    /// Executes the program with given bands
    pub fn execute(&self, bands: &[RasterBuffer]) -> Result<RasterBuffer> {
        if bands.is_empty() {
            return Err(AlgorithmError::EmptyInput {
                operation: "execute",
            });
        }

        let width = bands[0].width();
        let height = bands[0].height();

        // Check all bands have same dimensions
        for band in bands.iter().skip(1) {
            if band.width() != width || band.height() != height {
                return Err(AlgorithmError::InvalidDimensions {
                    message: "All bands must have same dimensions",
                    actual: band.width() as usize,
                    expected: width as usize,
                });
            }
        }

        let mut env = Environment::new();
        let band_ctx = BandContext::new(bands);
        let mut executor = Executor::new(&self.func_registry);

        // Execute all statements
        for stmt in &self.program.statements {
            executor.execute_statement(stmt, &mut env, &band_ctx)?;
        }

        // Get the last expression result or create a default raster
        if let Some(Statement::Expr(expr)) = self.program.statements.last() {
            let result = executor.evaluate_expr(expr, &env, &band_ctx)?;

            match result {
                Value::Raster(r) => Ok(*r),
                Value::Number(n) => {
                    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);
                    for y in 0..height {
                        for x in 0..width {
                            raster.set_pixel(x, y, n).map_err(AlgorithmError::Core)?;
                        }
                    }
                    Ok(raster)
                }
                Value::Bool(b) => {
                    let val = if b { 1.0 } else { 0.0 };
                    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);
                    for y in 0..height {
                        for x in 0..width {
                            raster.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
                        }
                    }
                    Ok(raster)
                }
                _ => Err(AlgorithmError::InvalidParameter {
                    parameter: "result",
                    message: "Program must return a raster or scalar".to_string(),
                }),
            }
        } else {
            Err(AlgorithmError::InvalidParameter {
                parameter: "program",
                message: "Program has no expression to evaluate".to_string(),
            })
        }
    }

    /// Executes a single expression with given bands
    pub fn execute_expr(&self, expr: &Expr, bands: &[RasterBuffer]) -> Result<RasterBuffer> {
        if bands.is_empty() {
            return Err(AlgorithmError::EmptyInput {
                operation: "execute_expr",
            });
        }

        let width = bands[0].width();
        let height = bands[0].height();

        let env = Environment::new();
        let band_ctx = BandContext::new(bands);
        let mut executor = Executor::new(&self.func_registry);

        let result = executor.evaluate_expr(expr, &env, &band_ctx)?;

        match result {
            Value::Raster(r) => Ok(*r),
            Value::Number(n) => {
                let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);
                for y in 0..height {
                    for x in 0..width {
                        raster.set_pixel(x, y, n).map_err(AlgorithmError::Core)?;
                    }
                }
                Ok(raster)
            }
            Value::Bool(b) => {
                let val = if b { 1.0 } else { 0.0 };
                let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);
                for y in 0..height {
                    for x in 0..width {
                        raster.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
                    }
                }
                Ok(raster)
            }
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "result",
                message: "Expression must return a raster or scalar".to_string(),
            }),
        }
    }
}

/// Runtime executor for expressions
struct Executor<'a> {
    func_registry: &'a FunctionRegistry,
}

impl<'a> Executor<'a> {
    fn new(func_registry: &'a FunctionRegistry) -> Self {
        Self { func_registry }
    }

    fn execute_statement(
        &mut self,
        stmt: &Statement,
        env: &mut Environment,
        band_ctx: &BandContext,
    ) -> Result<()> {
        match stmt {
            Statement::VariableDecl { name, value } => {
                let val = self.evaluate_expr(value, env, band_ctx)?;
                env.define(name.clone(), val);
                Ok(())
            }
            Statement::FunctionDecl { name, params, body } => {
                let func_val = Value::Function {
                    params: params.clone(),
                    body: body.clone(),
                    env: env.clone(),
                };
                env.define(name.clone(), func_val);
                Ok(())
            }
            Statement::Return(_) => Err(AlgorithmError::InvalidParameter {
                parameter: "return",
                message: "Return statements not supported in top-level".to_string(),
            }),
            Statement::Expr(expr) => {
                let _ = self.evaluate_expr(expr, env, band_ctx)?;
                Ok(())
            }
        }
    }

    fn evaluate_expr(
        &mut self,
        expr: &Expr,
        env: &Environment,
        band_ctx: &BandContext,
    ) -> Result<Value> {
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Band(b) => {
                let band = band_ctx.get_band(*b)?;
                Ok(Value::Raster(Box::new(band.clone())))
            }
            Expr::Variable(name) => env.lookup(name).cloned(),
            Expr::Binary {
                left, op, right, ..
            } => self.evaluate_binary(left, *op, right, env, band_ctx),
            Expr::Unary {
                op, expr: inner, ..
            } => self.evaluate_unary(*op, inner, env, band_ctx),
            Expr::Call { name, args, .. } => self.evaluate_call(name, args, env, band_ctx),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => self.evaluate_conditional(condition, then_expr, else_expr, env, band_ctx),
            Expr::Block {
                statements, result, ..
            } => self.evaluate_block(statements, result.as_deref(), env, band_ctx),
            Expr::ForLoop { .. } => Err(AlgorithmError::InvalidParameter {
                parameter: "for",
                message: "For loops not yet implemented".to_string(),
            }),
        }
    }

    fn evaluate_binary(
        &mut self,
        left: &Expr,
        op: BinaryOp,
        right: &Expr,
        env: &Environment,
        band_ctx: &BandContext,
    ) -> Result<Value> {
        let left_val = self.evaluate_expr(left, env, band_ctx)?;
        let right_val = self.evaluate_expr(right, env, band_ctx)?;

        match (left_val, right_val) {
            (Value::Number(l), Value::Number(r)) => {
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
                    BinaryOp::Modulo => l % r,
                    BinaryOp::Power => l.powf(r),
                    BinaryOp::Equal => return Ok(Value::Bool((l - r).abs() < f64::EPSILON)),
                    BinaryOp::NotEqual => return Ok(Value::Bool((l - r).abs() >= f64::EPSILON)),
                    BinaryOp::Less => return Ok(Value::Bool(l < r)),
                    BinaryOp::LessEqual => return Ok(Value::Bool(l <= r)),
                    BinaryOp::Greater => return Ok(Value::Bool(l > r)),
                    BinaryOp::GreaterEqual => return Ok(Value::Bool(l >= r)),
                    BinaryOp::And | BinaryOp::Or => {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "operator",
                            message: "Logical operators require boolean operands".to_string(),
                        });
                    }
                };
                Ok(Value::Number(result))
            }
            (Value::Bool(l), Value::Bool(r)) => {
                let result = match op {
                    BinaryOp::And => l && r,
                    BinaryOp::Or => l || r,
                    BinaryOp::Equal => l == r,
                    BinaryOp::NotEqual => l != r,
                    _ => {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "operator",
                            message: format!("Operator {:?} not supported for booleans", op),
                        });
                    }
                };
                Ok(Value::Bool(result))
            }
            (Value::Raster(l), Value::Raster(r)) => self.evaluate_raster_binary(&l, op, &r),
            (Value::Raster(l), Value::Number(r)) => {
                self.evaluate_raster_scalar_binary(&l, op, r, false)
            }
            (Value::Number(l), Value::Raster(r)) => {
                self.evaluate_raster_scalar_binary(&r, op, l, true)
            }
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "operands",
                message: "Incompatible operand types".to_string(),
            }),
        }
    }

    fn evaluate_raster_binary(
        &self,
        left: &RasterBuffer,
        op: BinaryOp,
        right: &RasterBuffer,
    ) -> Result<Value> {
        if left.width() != right.width() || left.height() != right.height() {
            return Err(AlgorithmError::InvalidDimensions {
                message: "Rasters must have same dimensions",
                actual: right.width() as usize,
                expected: left.width() as usize,
            });
        }

        let mut result = RasterBuffer::zeros(left.width(), left.height(), left.data_type());

        for y in 0..left.height() {
            for x in 0..left.width() {
                let l = left.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let r = right.get_pixel(x, y).map_err(AlgorithmError::Core)?;

                let val = match op {
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
                    BinaryOp::Modulo => l % r,
                    BinaryOp::Power => l.powf(r),
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
                    BinaryOp::Less => {
                        if l < r {
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
                    BinaryOp::Greater => {
                        if l > r {
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
                    BinaryOp::And => {
                        // Treat rasters as boolean rasters: 0 is false, non-zero is true
                        let l_bool = l.abs() > f64::EPSILON;
                        let r_bool = r.abs() > f64::EPSILON;
                        if l_bool && r_bool { 1.0 } else { 0.0 }
                    }
                    BinaryOp::Or => {
                        // Treat rasters as boolean rasters: 0 is false, non-zero is true
                        let l_bool = l.abs() > f64::EPSILON;
                        let r_bool = r.abs() > f64::EPSILON;
                        if l_bool || r_bool { 1.0 } else { 0.0 }
                    }
                };

                result.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
            }
        }

        Ok(Value::Raster(Box::new(result)))
    }

    fn evaluate_raster_scalar_binary(
        &self,
        raster: &RasterBuffer,
        op: BinaryOp,
        scalar: f64,
        scalar_left: bool,
    ) -> Result<Value> {
        let mut result = RasterBuffer::zeros(raster.width(), raster.height(), raster.data_type());

        for y in 0..raster.height() {
            for x in 0..raster.width() {
                let r = raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;

                let val = if scalar_left {
                    match op {
                        BinaryOp::Add => scalar + r,
                        BinaryOp::Subtract => scalar - r,
                        BinaryOp::Multiply => scalar * r,
                        BinaryOp::Divide => {
                            if r.abs() < f64::EPSILON {
                                f64::NAN
                            } else {
                                scalar / r
                            }
                        }
                        BinaryOp::Modulo => scalar % r,
                        BinaryOp::Power => scalar.powf(r),
                        BinaryOp::Equal => {
                            if (scalar - r).abs() < f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::NotEqual => {
                            if (scalar - r).abs() >= f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Less => {
                            if scalar < r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::LessEqual => {
                            if scalar <= r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Greater => {
                            if scalar > r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::GreaterEqual => {
                            if scalar >= r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::And | BinaryOp::Or => {
                            return Err(AlgorithmError::InvalidParameter {
                                parameter: "operator",
                                message: "Logical operators require boolean operands".to_string(),
                            });
                        }
                    }
                } else {
                    match op {
                        BinaryOp::Add => r + scalar,
                        BinaryOp::Subtract => r - scalar,
                        BinaryOp::Multiply => r * scalar,
                        BinaryOp::Divide => {
                            if scalar.abs() < f64::EPSILON {
                                f64::NAN
                            } else {
                                r / scalar
                            }
                        }
                        BinaryOp::Modulo => r % scalar,
                        BinaryOp::Power => r.powf(scalar),
                        BinaryOp::Equal => {
                            if (r - scalar).abs() < f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::NotEqual => {
                            if (r - scalar).abs() >= f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Less => {
                            if r < scalar {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::LessEqual => {
                            if r <= scalar {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Greater => {
                            if r > scalar {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::GreaterEqual => {
                            if r >= scalar {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::And | BinaryOp::Or => {
                            return Err(AlgorithmError::InvalidParameter {
                                parameter: "operator",
                                message: "Logical operators require boolean operands".to_string(),
                            });
                        }
                    }
                };

                result.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
            }
        }

        Ok(Value::Raster(Box::new(result)))
    }

    fn evaluate_unary(
        &mut self,
        op: UnaryOp,
        expr: &Expr,
        env: &Environment,
        band_ctx: &BandContext,
    ) -> Result<Value> {
        let val = self.evaluate_expr(expr, env, band_ctx)?;

        match val {
            Value::Number(n) => {
                let result = match op {
                    UnaryOp::Negate => -n,
                    UnaryOp::Plus => n,
                    UnaryOp::Not => {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "operator",
                            message: "Not operator requires boolean".to_string(),
                        });
                    }
                };
                Ok(Value::Number(result))
            }
            Value::Bool(b) => match op {
                UnaryOp::Not => Ok(Value::Bool(!b)),
                _ => Err(AlgorithmError::InvalidParameter {
                    parameter: "operator",
                    message: "Operator not supported for booleans".to_string(),
                }),
            },
            Value::Raster(raster) => {
                let mut result =
                    RasterBuffer::zeros(raster.width(), raster.height(), raster.data_type());

                for y in 0..raster.height() {
                    for x in 0..raster.width() {
                        let val = raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                        let new_val = match op {
                            UnaryOp::Negate => -val,
                            UnaryOp::Plus => val,
                            UnaryOp::Not => {
                                return Err(AlgorithmError::InvalidParameter {
                                    parameter: "operator",
                                    message: "Not operator requires boolean operands".to_string(),
                                });
                            }
                        };
                        result
                            .set_pixel(x, y, new_val)
                            .map_err(AlgorithmError::Core)?;
                    }
                }

                Ok(Value::Raster(Box::new(result)))
            }
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "operand",
                message: "Incompatible operand type for unary operator".to_string(),
            }),
        }
    }

    fn evaluate_call(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &Environment,
        band_ctx: &BandContext,
    ) -> Result<Value> {
        // Check if it's a user-defined function
        if let Ok(func_val) = env.lookup(name) {
            if let Value::Function {
                params,
                body,
                env: func_env,
            } = func_val
            {
                if params.len() != args.len() {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "arguments",
                        message: format!("Expected {} arguments, got {}", params.len(), args.len()),
                    });
                }

                // Create new environment with parameters bound
                let mut new_env = Environment::with_parent(func_env.clone());
                for (param, arg) in params.iter().zip(args.iter()) {
                    let arg_val = self.evaluate_expr(arg, env, band_ctx)?;
                    new_env.define(param.clone(), arg_val);
                }

                return self.evaluate_expr(body, &new_env, band_ctx);
            }
        }

        // Check if it's a built-in function
        if let Some((func, arity)) = self.func_registry.lookup(name) {
            if arity > 0 && args.len() != arity {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "arguments",
                    message: format!("Expected {arity} arguments, got {}", args.len()),
                });
            }

            let arg_vals: Result<Vec<Value>> = args
                .iter()
                .map(|arg| self.evaluate_expr(arg, env, band_ctx))
                .collect();

            func(&arg_vals?)
        } else {
            Err(AlgorithmError::InvalidParameter {
                parameter: "function",
                message: format!("Unknown function: {name}"),
            })
        }
    }

    fn evaluate_conditional(
        &mut self,
        condition: &Expr,
        then_expr: &Expr,
        else_expr: &Expr,
        env: &Environment,
        band_ctx: &BandContext,
    ) -> Result<Value> {
        let cond_val = self.evaluate_expr(condition, env, band_ctx)?;

        match cond_val {
            Value::Bool(b) => {
                if b {
                    self.evaluate_expr(then_expr, env, band_ctx)
                } else {
                    self.evaluate_expr(else_expr, env, band_ctx)
                }
            }
            Value::Number(n) => {
                if n.abs() > f64::EPSILON {
                    self.evaluate_expr(then_expr, env, band_ctx)
                } else {
                    self.evaluate_expr(else_expr, env, band_ctx)
                }
            }
            Value::Raster(cond_raster) => {
                // Pixel-wise conditional evaluation
                let then_val = self.evaluate_expr(then_expr, env, band_ctx)?;
                let else_val = self.evaluate_expr(else_expr, env, band_ctx)?;

                let width = cond_raster.width();
                let height = cond_raster.height();
                let mut result = RasterBuffer::zeros(width, height, cond_raster.data_type());

                for y in 0..height {
                    for x in 0..width {
                        let cond = cond_raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                        let is_true = cond.abs() > f64::EPSILON;

                        let val = if is_true {
                            match &then_val {
                                Value::Raster(r) => {
                                    r.get_pixel(x, y).map_err(AlgorithmError::Core)?
                                }
                                Value::Number(n) => *n,
                                Value::Bool(b) => {
                                    if *b {
                                        1.0
                                    } else {
                                        0.0
                                    }
                                }
                                _ => {
                                    return Err(AlgorithmError::InvalidParameter {
                                        parameter: "then_expr",
                                        message: "Then expression must be raster or scalar"
                                            .to_string(),
                                    });
                                }
                            }
                        } else {
                            match &else_val {
                                Value::Raster(r) => {
                                    r.get_pixel(x, y).map_err(AlgorithmError::Core)?
                                }
                                Value::Number(n) => *n,
                                Value::Bool(b) => {
                                    if *b {
                                        1.0
                                    } else {
                                        0.0
                                    }
                                }
                                _ => {
                                    return Err(AlgorithmError::InvalidParameter {
                                        parameter: "else_expr",
                                        message: "Else expression must be raster or scalar"
                                            .to_string(),
                                    });
                                }
                            }
                        };

                        result.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
                    }
                }

                Ok(Value::Raster(Box::new(result)))
            }
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "condition",
                message: "Condition must be boolean, number, or raster".to_string(),
            }),
        }
    }

    fn evaluate_block(
        &mut self,
        statements: &[Statement],
        result: Option<&Expr>,
        env: &Environment,
        band_ctx: &BandContext,
    ) -> Result<Value> {
        let mut block_env = Environment::with_parent(env.clone());

        for stmt in statements {
            self.execute_statement(stmt, &mut block_env, band_ctx)?;
        }

        if let Some(expr) = result {
            self.evaluate_expr(expr, &block_env, band_ctx)
        } else {
            Ok(Value::Number(0.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::parse_expression;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_compile_number() {
        let expr = parse_expression("42").expect("Should parse");
        let program = Program {
            statements: vec![Statement::Expr(Box::new(expr))],
        };
        let compiled = CompiledProgram::new(program);

        let bands = vec![RasterBuffer::zeros(10, 10, RasterDataType::Float32)];
        let result = compiled.execute(&bands);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_band() {
        let expr = parse_expression("B1").expect("Should parse");
        let program = Program {
            statements: vec![Statement::Expr(Box::new(expr))],
        };
        let compiled = CompiledProgram::new(program);

        let bands = vec![RasterBuffer::zeros(10, 10, RasterDataType::Float32)];
        let result = compiled.execute(&bands);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_arithmetic() {
        let expr = parse_expression("B1 + B2").expect("Should parse");
        let program = Program {
            statements: vec![Statement::Expr(Box::new(expr))],
        };
        let compiled = CompiledProgram::new(program);

        let bands = vec![
            RasterBuffer::zeros(10, 10, RasterDataType::Float32),
            RasterBuffer::zeros(10, 10, RasterDataType::Float32),
        ];
        let result = compiled.execute(&bands);
        assert!(result.is_ok());
    }
}
