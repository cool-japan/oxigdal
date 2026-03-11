//! Recursive-descent parser for the raster expression language

use super::ast::{BinaryOp, Expr, Token, UnaryOp};
use crate::error::{AlgorithmError, Result};

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

    pub(super) fn parse(&mut self) -> Result<Expr> {
        self.parse_conditional()
    }

    fn parse_conditional(&mut self) -> Result<Expr> {
        if matches!(self.current(), Some(Token::If)) {
            self.advance();
            let condition = Box::new(self.parse_or()?);

            if !matches!(self.current(), Some(Token::Then)) {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "expression",
                    message: "Expected 'then' after if condition".to_string(),
                });
            }
            self.advance();

            let then_expr = Box::new(self.parse_or()?);

            if !matches!(self.current(), Some(Token::Else)) {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "expression",
                    message: "Expected 'else' in conditional".to_string(),
                });
            }
            self.advance();

            let else_expr = Box::new(self.parse_or()?);

            Ok(Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            })
        } else {
            self.parse_or()
        }
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;

        while matches!(self.current(), Some(Token::Or)) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;

        while matches!(self.current(), Some(Token::And)) {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;

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
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Subtract,
                _ => break,
            };

            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_power()?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Multiply => BinaryOp::Multiply,
                Token::Divide => BinaryOp::Divide,
                _ => break,
            };

            self.advance();
            let right = self.parse_power()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;

        while matches!(self.current(), Some(Token::Power)) {
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Power,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.current(), Some(Token::Minus)) {
            self.advance();
            let expr = self.parse_unary()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Negate,
                expr: Box::new(expr),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
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
                            args.push(self.parse_conditional()?);

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
                let expr = self.parse_conditional()?;

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
