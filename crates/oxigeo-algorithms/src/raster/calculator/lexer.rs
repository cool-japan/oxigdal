//! Lexer (tokenizer) for the raster expression language

use super::ast::Token;
use crate::error::{AlgorithmError, Result};

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// Tokenizer for raster expressions
pub(super) struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub(super) fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn current(&self) -> Option<char> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_number(&mut self) -> Result<f64> {
        let mut num_str = String::new();
        let mut has_dot = false;

        while let Some(c) = self.current() {
            if c.is_ascii_digit() {
                num_str.push(c);
                self.advance();
            } else if c == '.' && !has_dot {
                has_dot = true;
                num_str.push(c);
                self.advance();
            } else {
                break;
            }
        }

        num_str
            .parse::<f64>()
            .map_err(|_| AlgorithmError::InvalidParameter {
                parameter: "expression",
                message: format!("Invalid number: {num_str}"),
            })
    }

    fn read_ident(&mut self) -> String {
        let mut ident = String::new();

        while let Some(c) = self.current() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(c);
                self.advance();
            } else {
                break;
            }
        }

        ident
    }

    fn next_token(&mut self) -> Result<Option<Token>> {
        self.skip_whitespace();

        let c = match self.current() {
            Some(c) => c,
            None => return Ok(None),
        };

        let token = if c.is_ascii_digit() {
            Token::Number(self.read_number()?)
        } else if c.is_alphabetic() || c == 'B' {
            let ident = self.read_ident();

            // Check for band reference (B1, B2, etc.)
            if ident.starts_with('B') && ident.len() > 1 {
                let band_num = ident[1..].parse::<usize>().ok();
                if let Some(num) = band_num {
                    return Ok(Some(Token::Band(num)));
                }
            }

            // Check for keywords
            match ident.as_str() {
                "if" | "IF" => Token::If,
                "then" | "THEN" => Token::Then,
                "else" | "ELSE" => Token::Else,
                "and" | "AND" => Token::And,
                "or" | "OR" => Token::Or,
                _ => Token::Ident(ident),
            }
        } else {
            self.advance();
            match c {
                '+' => Token::Plus,
                '-' => Token::Minus,
                '*' => Token::Multiply,
                '/' => Token::Divide,
                '^' => Token::Power,
                '(' => Token::LeftParen,
                ')' => Token::RightParen,
                ',' => Token::Comma,
                '>' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::GreaterEqual
                    } else {
                        Token::Greater
                    }
                }
                '<' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::LessEqual
                    } else {
                        Token::Less
                    }
                }
                '=' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::Equal
                    } else {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "expression",
                            message: "Expected '==' for equality comparison".to_string(),
                        });
                    }
                }
                '!' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::NotEqual
                    } else {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "expression",
                            message: "Expected '!=' for inequality comparison".to_string(),
                        });
                    }
                }
                _ => {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "expression",
                        message: format!("Unexpected character: {c}"),
                    });
                }
            }
        };

        Ok(Some(token))
    }

    pub(super) fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();

        while let Some(token) = self.next_token()? {
            tokens.push(token);
        }

        Ok(tokens)
    }
}
