//! Parser for Raster Algebra DSL
//!
//! This module uses Pest to parse DSL text into an AST.

// Parser derive macro generates items that don't have documentation
#![allow(missing_docs)]

use super::ast::{BinaryOp, Expr, Program, Statement, Type, UnaryOp};
use crate::error::{AlgorithmError, Result};
use crate::expr_depth::{check_source_nesting_depth, guard_depth};
use pest::Parser;
use pest_derive::Parser;

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, string::String, vec::Vec};

#[derive(Parser)]
#[grammar = "dsl/grammar.pest"]
struct RasterParser;

/// Parses a DSL program from text
///
/// # Errors
///
/// Returns [`AlgorithmError::NestingTooDeep`] when `input` nests more deeply
/// than [`crate::MAX_EXPRESSION_DEPTH`]. The check runs before the generated
/// parser is invoked, because Pest builds its own recursive descent that no
/// depth counter can be threaded through; without it, deeply nested input
/// overflows the thread stack and aborts the process.
pub fn parse_program(input: &str) -> Result<Program> {
    check_source_nesting_depth(input, "dsl")?;

    let pairs = RasterParser::parse(Rule::program, input).map_err(|e| {
        AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: format!("Parse error: {e}"),
        }
    })?;

    let mut statements = Vec::new();

    for pair in pairs {
        match pair.as_rule() {
            Rule::program => {
                for inner in pair.into_inner() {
                    if inner.as_rule() == Rule::statement {
                        statements.push(parse_statement(inner, 0)?);
                    }
                }
            }
            Rule::EOI => {}
            _ => {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: format!("Unexpected rule: {:?}", pair.as_rule()),
                });
            }
        }
    }

    Ok(Program { statements })
}

/// Parses a single expression from text
///
/// # Errors
///
/// Returns [`AlgorithmError::NestingTooDeep`] when `input` nests more deeply
/// than [`crate::MAX_EXPRESSION_DEPTH`]. See [`parse_program`] for why the
/// check has to happen before the generated parser runs.
pub fn parse_expression(input: &str) -> Result<Expr> {
    check_source_nesting_depth(input, "dsl")?;

    let mut full_input = String::from(input);
    if !full_input.ends_with(';') {
        full_input.push(';');
    }

    let pairs = RasterParser::parse(Rule::program, &full_input).map_err(|e| {
        AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: format!("Parse error: {e}"),
        }
    })?;

    for pair in pairs {
        if pair.as_rule() == Rule::program {
            for inner in pair.into_inner() {
                if inner.as_rule() == Rule::statement {
                    // Get the inner rule from statement (expr_stmt, variable_decl, etc.)
                    let stmt_inner = inner.into_inner().next().ok_or_else(|| {
                        AlgorithmError::InvalidParameter {
                            parameter: "dsl",
                            message: "Empty statement".to_string(),
                        }
                    })?;

                    // If it's an expr_stmt, extract the expression
                    if stmt_inner.as_rule() == Rule::expr_stmt {
                        return parse_expr_stmt(stmt_inner, 0);
                    }
                }
            }
        }
    }

    Err(AlgorithmError::InvalidParameter {
        parameter: "dsl",
        message: "No expression found".to_string(),
    })
}

fn parse_statement(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Statement> {
    guard_depth(depth, "dsl")?;

    let inner = pair
        .into_inner()
        .next()
        .ok_or_else(|| AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: "Empty statement".to_string(),
        })?;

    match inner.as_rule() {
        Rule::variable_decl => {
            let mut parts = inner.into_inner();
            let name = parts
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing variable name".to_string(),
                })?
                .as_str()
                .to_string();

            let value = parts
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing variable value".to_string(),
                })?;

            Ok(Statement::VariableDecl {
                name,
                value: Box::new(parse_expr(value, depth)?),
            })
        }
        Rule::function_decl => {
            let mut parts = inner.into_inner();
            let name = parts
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing function name".to_string(),
                })?
                .as_str()
                .to_string();

            let mut params = Vec::new();
            let mut body_pair = None;

            for part in parts {
                match part.as_rule() {
                    Rule::param_list => {
                        for param in part.into_inner() {
                            params.push(param.as_str().to_string());
                        }
                    }
                    Rule::expression => {
                        body_pair = Some(part);
                    }
                    _ => {}
                }
            }

            let body = body_pair.ok_or_else(|| AlgorithmError::InvalidParameter {
                parameter: "dsl",
                message: "Missing function body".to_string(),
            })?;

            Ok(Statement::FunctionDecl {
                name,
                params,
                body: Box::new(parse_expr(body, depth)?),
            })
        }
        Rule::return_stmt => {
            let expr =
                inner
                    .into_inner()
                    .next()
                    .ok_or_else(|| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: "Missing return expression".to_string(),
                    })?;

            Ok(Statement::Return(Box::new(parse_expr(expr, depth)?)))
        }
        Rule::expr_stmt => parse_expr_stmt(inner, depth).map(|e| Statement::Expr(Box::new(e))),
        _ => Err(AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: format!("Unexpected statement: {:?}", inner.as_rule()),
        }),
    }
}

fn parse_expr_stmt(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let expr = pair
        .into_inner()
        .next()
        .ok_or_else(|| AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: "Empty expression statement".to_string(),
        })?;

    parse_expr(expr, depth)
}

/// Builds an [`Expr`] from a Pest pair.
///
/// `depth` counts *source* nesting levels, matching what
/// [`crate::expr_depth::source_nesting_depth`] estimates: it is incremented
/// only where the grammar re-enters `expression` (bracketing, call arguments,
/// conditional branches, blocks, and unary sign chains), not for each step down
/// the precedence ladder, which stays at a fixed source level.
fn parse_expr(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    match pair.as_rule() {
        Rule::expression => parse_expr(
            pair.into_inner()
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Empty expression".to_string(),
                })?,
            depth,
        ),
        Rule::logical_or => parse_binary_op(pair, BinaryOp::Or, depth),
        Rule::logical_and => parse_binary_op(pair, BinaryOp::And, depth),
        Rule::logical_not => {
            let mut inner = pair.into_inner();
            let mut not_count = 0;

            // Count NOT operators
            while let Some(next) = inner.peek() {
                if matches!(next.as_rule(), Rule::not_op) {
                    not_count += 1;
                    inner.next();
                } else {
                    break;
                }
            }

            let mut expr = parse_expr(
                inner
                    .next()
                    .ok_or_else(|| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: "Missing expression after not".to_string(),
                    })?,
                depth,
            )?;

            // Apply NOT operators
            for _ in 0..not_count {
                expr = Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                    ty: Type::Unknown,
                };
            }

            Ok(expr)
        }
        Rule::comparison => parse_comparison(pair, depth),
        Rule::additive => parse_additive(pair, depth),
        Rule::multiplicative => parse_multiplicative(pair, depth),
        Rule::power => parse_power(pair, depth),
        Rule::unary => parse_unary(pair, depth),
        Rule::primary => parse_primary(pair, depth),
        _ => Err(AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: format!("Unexpected expression rule: {:?}", pair.as_rule()),
        }),
    }
}

fn parse_binary_op(
    pair: pest::iterators::Pair<Rule>,
    default_op: BinaryOp,
    depth: usize,
) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let mut inner = pair.into_inner();
    let mut left = parse_expr(
        inner
            .next()
            .ok_or_else(|| AlgorithmError::InvalidParameter {
                parameter: "dsl",
                message: "Missing left operand".to_string(),
            })?,
        depth,
    )?;

    while let Some(next) = inner.next() {
        let op = match next.as_rule() {
            Rule::or_op => BinaryOp::Or,
            Rule::and_op => BinaryOp::And,
            _ => {
                let right = parse_expr(next, depth)?;
                left = Expr::Binary {
                    left: Box::new(left),
                    op: default_op,
                    right: Box::new(right),
                    ty: Type::Unknown,
                };
                continue;
            }
        };

        let right = parse_expr(
            inner
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing right operand".to_string(),
                })?,
            depth,
        )?;

        left = Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
            ty: Type::Unknown,
        };
    }

    Ok(left)
}

fn parse_comparison(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let mut inner = pair.into_inner();
    let left = parse_expr(
        inner
            .next()
            .ok_or_else(|| AlgorithmError::InvalidParameter {
                parameter: "dsl",
                message: "Missing left operand".to_string(),
            })?,
        depth,
    )?;

    if let Some(op_pair) = inner.next() {
        let op = match op_pair.as_rule() {
            Rule::eq_op => BinaryOp::Equal,
            Rule::ne_op => BinaryOp::NotEqual,
            Rule::lt_op => BinaryOp::Less,
            Rule::le_op => BinaryOp::LessEqual,
            Rule::gt_op => BinaryOp::Greater,
            Rule::ge_op => BinaryOp::GreaterEqual,
            _ => {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: format!("Unknown comparison operator: {:?}", op_pair.as_rule()),
                });
            }
        };

        let right = parse_expr(
            inner
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing right operand".to_string(),
                })?,
            depth,
        )?;

        Ok(Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
            ty: Type::Unknown,
        })
    } else {
        Ok(left)
    }
}

fn parse_additive(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let mut inner = pair.into_inner();
    let mut left = parse_expr(
        inner
            .next()
            .ok_or_else(|| AlgorithmError::InvalidParameter {
                parameter: "dsl",
                message: "Missing left operand".to_string(),
            })?,
        depth,
    )?;

    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_rule() {
            Rule::add_op => BinaryOp::Add,
            Rule::sub_op => BinaryOp::Subtract,
            _ => {
                let right = parse_expr(op_pair, depth)?;
                left = Expr::Binary {
                    left: Box::new(left),
                    op: BinaryOp::Add,
                    right: Box::new(right),
                    ty: Type::Unknown,
                };
                continue;
            }
        };

        let right = parse_expr(
            inner
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing right operand".to_string(),
                })?,
            depth,
        )?;

        left = Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
            ty: Type::Unknown,
        };
    }

    Ok(left)
}

fn parse_multiplicative(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let mut inner = pair.into_inner();
    let mut left = parse_expr(
        inner
            .next()
            .ok_or_else(|| AlgorithmError::InvalidParameter {
                parameter: "dsl",
                message: "Missing left operand".to_string(),
            })?,
        depth,
    )?;

    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_rule() {
            Rule::mul_op => BinaryOp::Multiply,
            Rule::div_op => BinaryOp::Divide,
            Rule::mod_op => BinaryOp::Modulo,
            _ => {
                let right = parse_expr(op_pair, depth)?;
                left = Expr::Binary {
                    left: Box::new(left),
                    op: BinaryOp::Multiply,
                    right: Box::new(right),
                    ty: Type::Unknown,
                };
                continue;
            }
        };

        let right = parse_expr(
            inner
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing right operand".to_string(),
                })?,
            depth,
        )?;

        left = Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
            ty: Type::Unknown,
        };
    }

    Ok(left)
}

fn parse_power(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let mut inner = pair.into_inner();
    let mut left = parse_expr(
        inner
            .next()
            .ok_or_else(|| AlgorithmError::InvalidParameter {
                parameter: "dsl",
                message: "Missing left operand".to_string(),
            })?,
        depth,
    )?;

    while let Some(op_pair) = inner.next() {
        if matches!(op_pair.as_rule(), Rule::pow_op) {
            let right = parse_expr(
                inner
                    .next()
                    .ok_or_else(|| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: "Missing right operand".to_string(),
                    })?,
                depth,
            )?;

            left = Expr::Binary {
                left: Box::new(left),
                op: BinaryOp::Power,
                right: Box::new(right),
                ty: Type::Unknown,
            };
        } else {
            let right = parse_expr(op_pair, depth)?;
            left = Expr::Binary {
                left: Box::new(left),
                op: BinaryOp::Power,
                right: Box::new(right),
                ty: Type::Unknown,
            };
        }
    }

    Ok(left)
}

fn parse_unary(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let mut inner = pair.into_inner();
    let first = inner
        .next()
        .ok_or_else(|| AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: "Empty unary expression".to_string(),
        })?;

    match first.as_rule() {
        Rule::sub_op => {
            // Next element will be another unary expression
            let expr = inner
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing expression after -".to_string(),
                })?;

            Ok(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(parse_expr(expr, depth + 1)?),
                ty: Type::Unknown,
            })
        }
        Rule::add_op => {
            // Next element will be another unary expression
            let expr = inner
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing expression after +".to_string(),
                })?;

            Ok(Expr::Unary {
                op: UnaryOp::Plus,
                expr: Box::new(parse_expr(expr, depth + 1)?),
                ty: Type::Unknown,
            })
        }
        _ => parse_expr(first, depth),
    }
}

fn parse_primary(pair: pest::iterators::Pair<Rule>, depth: usize) -> Result<Expr> {
    guard_depth(depth, "dsl")?;

    let inner = pair
        .into_inner()
        .next()
        .ok_or_else(|| AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: "Empty primary expression".to_string(),
        })?;

    match inner.as_rule() {
        Rule::number => {
            let num =
                inner
                    .as_str()
                    .parse::<f64>()
                    .map_err(|_| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: format!("Invalid number: {}", inner.as_str()),
                    })?;
            Ok(Expr::Number(num))
        }
        Rule::band_ref => {
            let band_str = inner.as_str();
            let band_num =
                band_str[1..]
                    .parse::<usize>()
                    .map_err(|_| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: format!("Invalid band reference: {band_str}"),
                    })?;
            Ok(Expr::Band(band_num))
        }
        Rule::function_call => {
            let mut parts = inner.into_inner();
            let name = parts
                .next()
                .ok_or_else(|| AlgorithmError::InvalidParameter {
                    parameter: "dsl",
                    message: "Missing function name".to_string(),
                })?
                .as_str()
                .to_string();

            let mut args = Vec::new();
            if let Some(arg_list) = parts.next() {
                for arg in arg_list.into_inner() {
                    args.push(parse_expr(arg, depth + 1)?);
                }
            }

            Ok(Expr::Call {
                name,
                args,
                ty: Type::Unknown,
            })
        }
        Rule::variable_ref => Ok(Expr::Variable(inner.as_str().to_string())),
        Rule::conditional => {
            let mut parts = inner.into_inner();
            let condition = parse_expr(
                parts
                    .next()
                    .ok_or_else(|| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: "Missing condition".to_string(),
                    })?,
                depth + 1,
            )?;

            let then_expr = parse_expr(
                parts
                    .next()
                    .ok_or_else(|| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: "Missing then expression".to_string(),
                    })?,
                depth + 1,
            )?;

            let else_expr = parse_expr(
                parts
                    .next()
                    .ok_or_else(|| AlgorithmError::InvalidParameter {
                        parameter: "dsl",
                        message: "Missing else expression".to_string(),
                    })?,
                depth + 1,
            )?;

            Ok(Expr::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
                ty: Type::Unknown,
            })
        }
        Rule::block => {
            let mut statements = Vec::new();
            let mut result = None;

            for part in inner.into_inner() {
                match part.as_rule() {
                    Rule::statement => statements.push(parse_statement(part, depth + 1)?),
                    Rule::expression => result = Some(Box::new(parse_expr(part, depth + 1)?)),
                    _ => {}
                }
            }

            Ok(Expr::Block {
                statements,
                result,
                ty: Type::Unknown,
            })
        }
        Rule::expression => parse_expr(inner, depth + 1),
        _ => Err(AlgorithmError::InvalidParameter {
            parameter: "dsl",
            message: format!("Unexpected primary: {:?}", inner.as_rule()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number() {
        let expr = parse_expression("42.5").expect("Should parse");
        assert!(matches!(expr, Expr::Number(n) if (n - 42.5).abs() < 1e-10));
    }

    #[test]
    fn test_parse_band() {
        let expr = parse_expression("B1").expect("Should parse");
        assert!(matches!(expr, Expr::Band(1)));
    }

    #[test]
    fn test_parse_arithmetic() {
        let result = parse_expression("1 + 2 * 3");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_ndvi() {
        let result = parse_expression("(B1 - B2) / (B1 + B2)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_conditional() {
        let result = parse_expression("if B1 > 0.5 then 1 else 0");
        if let Err(e) = &result {
            eprintln!("Parse error: {:?}", e);
        }
        assert!(result.is_ok(), "Parse failed: {:?}", result);
    }

    #[test]
    fn test_parse_function_call() {
        let result = parse_expression("sqrt(B1 * B1 + B2 * B2)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_program() {
        let program = r#"
            let ndvi = (B8 - B4) / (B8 + B4);
            let result = if ndvi > 0.5 then 1 else 0;
        "#;
        let result = parse_program(program);
        assert!(result.is_ok());
    }

    // ===================================================================
    // Nesting-depth limit
    //
    // Every case below used to abort the process with a stack overflow
    // (SIGABRT), which is unrecoverable — so these are regression tests for
    // a denial-of-service bug, not merely for an error message.
    // ===================================================================

    use crate::expr_depth::MAX_EXPRESSION_DEPTH;

    fn nested_parens(depth: usize) -> String {
        let mut expr = String::from("B1");
        for _ in 0..depth {
            expr = format!("({expr} + 1)");
        }
        expr
    }

    #[test]
    fn test_parse_accepts_expression_at_the_depth_limit() {
        let expr = nested_parens(MAX_EXPRESSION_DEPTH);
        assert!(
            parse_expression(&expr).is_ok(),
            "an expression exactly at the limit must still parse"
        );
    }

    #[test]
    fn test_parse_rejects_one_level_past_the_depth_limit() {
        let expr = nested_parens(MAX_EXPRESSION_DEPTH + 1);
        let err = parse_expression(&expr).expect_err("one level past the limit must be rejected");
        assert!(
            matches!(err, AlgorithmError::NestingTooDeep { max, .. } if max == MAX_EXPRESSION_DEPTH),
            "expected NestingTooDeep, got {err:?}"
        );
    }

    #[test]
    fn test_parse_rejects_deeply_nested_parentheses_without_overflowing() {
        let err =
            parse_expression(&nested_parens(1000)).expect_err("deep nesting must be rejected");
        assert!(matches!(err, AlgorithmError::NestingTooDeep { .. }));
        // The limit has to be discoverable from the message.
        assert!(err.to_string().contains(&MAX_EXPRESSION_DEPTH.to_string()));
    }

    #[test]
    fn test_parse_rejects_deep_conditional_chain_without_overflowing() {
        // `conditional` recurses through its `else` branch, so this nests just
        // as deeply as parentheses do while containing no brackets at all.
        let mut expr = String::new();
        for _ in 0..1000 {
            expr.push_str("if B1 > 0 then 1 else ");
        }
        expr.push('0');

        let err = parse_expression(&expr).expect_err("deep if-chain must be rejected");
        assert!(matches!(err, AlgorithmError::NestingTooDeep { .. }));
    }

    #[test]
    fn test_parse_rejects_deep_unary_chain_without_overflowing() {
        // `unary = (sub_op | add_op) ~ unary` recurses once per sign.
        let mut expr = "-".repeat(5000);
        expr.push_str("B1");

        let err = parse_expression(&expr).expect_err("deep unary chain must be rejected");
        assert!(matches!(err, AlgorithmError::NestingTooDeep { .. }));
    }

    #[test]
    fn test_parse_program_rejects_deep_nesting_without_overflowing() {
        let program = format!("let x = {};", nested_parens(1000));
        let err = parse_program(&program).expect_err("deep nesting must be rejected");
        assert!(matches!(err, AlgorithmError::NestingTooDeep { .. }));
    }

    #[test]
    fn test_depth_limit_does_not_reject_realistic_expressions() {
        // Guard against the pre-scan being over-eager: none of these are deep.
        let cases = [
            "(B1 - B2) / (B1 + B2)",
            "sqrt(B1 * B1 + B2 * B2)",
            "if B1 > 0.6 then 1.0 else if B1 > 0.3 then 0.5 else 0.0",
            "2.5 * ((B8 - B4) / (B8 + 6 * B4 - 7.5 * B2 + 1))",
            "clamp(pow(abs(B1 - B2), 2), 0, 1)",
            "-B1 + -B2",
            "1e-3 * B1 + 2.5e+4",
        ];
        for case in cases {
            assert!(parse_expression(case).is_ok(), "should parse: {case}");
        }

        // A long *flat* sum is depth 0 and must not be mistaken for nesting.
        let flat = (0..500)
            .map(|i| format!("B{}", i % 8 + 1))
            .collect::<Vec<_>>()
            .join(" + ");
        assert!(parse_expression(&flat).is_ok(), "flat sums must parse");

        // Many sibling statements must not accumulate depth against each other.
        let program = "let a = if B1 > 0 then 1 else 0;\n".repeat(200);
        assert!(parse_program(&program).is_ok(), "sibling statements");
    }
}
