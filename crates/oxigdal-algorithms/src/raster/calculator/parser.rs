//! Recursive-descent parser for the raster expression language

use super::ast::{BinaryOp, Expr, Token, UnaryOp};
use crate::error::{AlgorithmError, Result};
use crate::expr_depth::guard_depth;

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// Parser for raster expressions
pub(super) struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    /// Parses the token stream into an [`Expr`].
    ///
    /// # Errors
    ///
    /// Returns [`AlgorithmError::NestingTooDeep`] when the expression nests
    /// more deeply than [`crate::MAX_EXPRESSION_DEPTH`]. The bound is explicit
    /// rather than discovered by overflowing: this is a recursive-descent
    /// parser, so without it a deeply nested expression exhausts the thread
    /// stack and aborts the process.
    pub(super) fn parse(&mut self) -> Result<Expr> {
        self.parse_conditional(0)
    }

    fn parse_conditional(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        if matches!(self.current(), Some(Token::If)) {
            self.advance();
            let condition = Box::new(self.parse_or(depth)?);

            if !matches!(self.current(), Some(Token::Then)) {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "expression",
                    message: "Expected 'then' after if condition".to_string(),
                });
            }
            self.advance();

            let then_expr = Box::new(self.parse_or(depth)?);

            if !matches!(self.current(), Some(Token::Else)) {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "expression",
                    message: "Expected 'else' in conditional".to_string(),
                });
            }
            self.advance();

            let else_expr = Box::new(self.parse_or(depth)?);

            Ok(Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            })
        } else {
            self.parse_or(depth)
        }
    }

    fn parse_or(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        let mut left = self.parse_and(depth)?;

        while matches!(self.current(), Some(Token::Or)) {
            self.advance();
            let right = self.parse_and(depth)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        let mut left = self.parse_comparison(depth)?;

        while matches!(self.current(), Some(Token::And)) {
            self.advance();
            let right = self.parse_comparison(depth)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_comparison(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        let mut left = self.parse_additive(depth)?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Greater => BinaryOp::Greater,
                Token::Less => BinaryOp::Less,
                Token::GreaterEqual => BinaryOp::GreaterEqual,
                Token::LessEqual => BinaryOp::LessEqual,
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                _ => break,
            };

            self.advance();
            let right = self.parse_additive(depth)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        let mut left = self.parse_multiplicative(depth)?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Subtract,
                _ => break,
            };

            self.advance();
            let right = self.parse_multiplicative(depth)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        let mut left = self.parse_power(depth)?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Multiply => BinaryOp::Multiply,
                Token::Divide => BinaryOp::Divide,
                _ => break,
            };

            self.advance();
            let right = self.parse_power(depth)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_power(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        let mut left = self.parse_unary(depth)?;

        while matches!(self.current(), Some(Token::Power)) {
            self.advance();
            let right = self.parse_unary(depth)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Power,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        if matches!(self.current(), Some(Token::Minus)) {
            self.advance();
            // A `-` prefix chain (`---B1`) recurses once per sign.
            let expr = self.parse_unary(depth + 1)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Negate,
                expr: Box::new(expr),
            })
        } else {
            self.parse_primary(depth)
        }
    }

    fn parse_primary(&mut self, depth: usize) -> Result<Expr> {
        guard_depth(depth, "expression")?;

        match self.current() {
            Some(Token::Number(n)) => {
                let val = *n;
                self.advance();
                Ok(Expr::Number(val))
            }
            Some(Token::Band(b)) => {
                let band = *b;
                self.advance();
                Ok(Expr::Band(band))
            }
            Some(Token::Ident(name)) => {
                let func_name = name.clone();
                self.advance();

                if matches!(self.current(), Some(Token::LeftParen)) {
                    self.advance();
                    let mut args = Vec::new();

                    if !matches!(self.current(), Some(Token::RightParen)) {
                        loop {
                            // Each call argument re-enters the expression grammar.
                            args.push(self.parse_conditional(depth + 1)?);

                            if matches!(self.current(), Some(Token::Comma)) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }

                    if !matches!(self.current(), Some(Token::RightParen)) {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "expression",
                            message: "Expected ')' after function arguments".to_string(),
                        });
                    }
                    self.advance();

                    Ok(Expr::Function {
                        name: func_name,
                        args,
                    })
                } else {
                    Err(AlgorithmError::InvalidParameter {
                        parameter: "expression",
                        message: format!("Unknown identifier: {func_name}"),
                    })
                }
            }
            Some(Token::LeftParen) => {
                self.advance();
                // A parenthesised group is one more level of nesting.
                let expr = self.parse_conditional(depth + 1)?;

                if !matches!(self.current(), Some(Token::RightParen)) {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "expression",
                        message: "Expected ')'".to_string(),
                    });
                }
                self.advance();

                Ok(expr)
            }
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "expression",
                message: "Unexpected token in expression".to_string(),
            }),
        }
    }
}
