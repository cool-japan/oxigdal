//! Band-math expression compiler — string parser + WGSL codegen.
//!
//! This module converts algebraic strings such as `"(B4-B3)/(B4+B3)"` into a
//! [`crate::algebra::BandExpression`] AST and, separately, lowers that AST
//! into an executable WGSL compute shader.
//!
//! # Quick example
//!
//! ```
//! use oxigeo_gpu::{parse_band_expression, band_expression_to_wgsl};
//!
//! let expr = parse_band_expression("(B4-B3)/(B4+B3)").expect("parse");
//! let shader = band_expression_to_wgsl(&expr, &[0, 1]);
//! assert!(shader.contains("@compute"));
//! ```
//!
//! # Grammar
//!
//! ```text
//! expr    = term  (('+' | '-') term)*
//! term    = factor (('*' | '/') factor)*
//! factor  = power
//! power   = unary ('^' unary)?         // right-associative
//! unary   = '-' unary | primary
//! primary = Number | Band | Func '(' args ')' | '(' expr ')'
//! args    = expr (',' expr)*
//! ```
//!
//! # Supported functions
//!
//! - `log(x)` / `ln(x)` — natural logarithm
//! - `exp(x)` — exponential
//! - `sqrt(x)` — square root (zero-clamped)
//! - `abs(x)` — absolute value
//! - `min(a, b)` / `max(a, b)` — element-wise min/max
//! - `pow(a, b)` — power (same as `a ^ b`)
//! - `clamp(x, lo, hi)` — clamp `x` to `[lo, hi]`

mod codegen;
mod parser;

pub use codegen::{band_expression_to_wgsl, constant_fold};
pub use parser::{BandMathError, parse_band_expression};
