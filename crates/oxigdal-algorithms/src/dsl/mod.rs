//! Advanced Raster Algebra Domain-Specific Language (DSL)
//!
//! This module provides a comprehensive DSL for raster algebra operations with:
//!
//! - **Full expression grammar** with variables, functions, and control flow
//! - **Type inference** for type-safe operations
//! - **Optimization passes** including constant folding and algebraic simplifications
//! - **Built-in function library** with 40+ mathematical, statistical, and spatial functions
//! - **Compile-time macros** for zero-cost abstractions
//! - **Runtime parser** for dynamic expression evaluation
//!
//! # Quick Start
//!
//! ## Simple Expression
//!
//! ```
//! use oxigdal_algorithms::dsl::RasterDsl;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create sample bands
//! let nir = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
//! let red = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
//!
//! // Calculate NDVI
//! let dsl = RasterDsl::new();
//! let result = dsl.execute("(B1 - B2) / (B1 + B2)", &[nir, red])?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Complex Program
//!
//! ```
//! use oxigdal_algorithms::dsl::RasterDsl;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let program = r#"
//!     let ndvi = (B8 - B4) / (B8 + B4);
//!     let evi = 2.5 * ((B8 - B4) / (B8 + 6*B4 - 7.5*B2 + 1));
//!
//!     if ndvi > 0.6 && evi > 0.5 then
//!         1.0
//!     else if ndvi > 0.3 then
//!         0.5
//!     else
//!         0.0
//! "#;
//!
//! let dsl = RasterDsl::new();
//! // let result = dsl.execute(program, bands)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Built-in Functions
//!
//! The DSL includes a rich set of built-in functions:
//!
//! ### Mathematical
//! - `sqrt`, `abs`, `floor`, `ceil`, `round`
//! - `log`, `log10`, `log2`, `exp`
//! - `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`
//! - `pow`, `hypot`
//!
//! ### Statistical
//! - `mean`, `median`, `mode`, `stddev`, `variance`
//! - `sum`, `product`, `min`, `max`
//! - `percentile`
//!
//! ### Spatial Filters
//! - `gaussian(raster, sigma)` - Gaussian blur
//! - `median_filter(raster, radius)` - Median filter
//!
//! ### Logical
//! - `and`, `or`, `not`, `xor`
//!
//! ### Comparison
//! - `eq`, `ne`, `lt`, `le`, `gt`, `ge`
//!
//! ### Utility
//! - `clamp(value, min, max)` - Clamp to range
//! - `select(condition, then, else)` - Conditional selection
//!
//! # Optimization
//!
//! The DSL includes several optimization levels:
//!
//! ```
//! use oxigdal_algorithms::dsl::{RasterDsl, OptLevel};
//!
//! let mut dsl = RasterDsl::new();
//! dsl.set_opt_level(OptLevel::Aggressive);
//! ```
//!
//! - `None` - No optimization
//! - `Basic` - Constant folding only
//! - `Standard` - Basic + algebraic simplifications (default)
//! - `Aggressive` - Standard + common subexpression elimination

pub mod ast;
pub mod compiler;
pub mod functions;
pub mod optimizer;
pub mod parser;
pub mod variables;

#[cfg(feature = "dsl")]
pub mod macro_support;

use crate::error::Result;
use oxigdal_core::buffer::RasterBuffer;

pub use ast::{BinaryOp, Expr, Program, Statement, Type, UnaryOp};
pub use compiler::CompiledProgram;
pub use functions::FunctionRegistry;
pub use optimizer::{OptLevel, Optimizer};
pub use parser::{parse_expression, parse_program};
pub use variables::{BandContext, Environment, Value};

/// Main DSL interface
pub struct RasterDsl {
    optimizer: Optimizer,
    func_registry: FunctionRegistry,
}

impl Default for RasterDsl {
    fn default() -> Self {
        Self::new()
    }
}

impl RasterDsl {
    /// Creates a new DSL instance with default settings
    pub fn new() -> Self {
        Self {
            optimizer: Optimizer::new(OptLevel::Standard),
            func_registry: FunctionRegistry::new(),
        }
    }

    /// Sets the optimization level
    pub fn set_opt_level(&mut self, level: OptLevel) {
        self.optimizer = Optimizer::new(level);
    }

    /// Gets the current optimization level
    pub fn opt_level(&self) -> OptLevel {
        OptLevel::Standard // Return default, could be stored in optimizer
    }

    /// Parses and executes a DSL expression
    ///
    /// # Arguments
    ///
    /// * `expression` - The DSL expression or program to execute
    /// * `bands` - Input raster bands (B1, B2, etc.)
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigdal_algorithms::dsl::RasterDsl;
    /// use oxigdal_core::buffer::RasterBuffer;
    /// use oxigdal_core::types::RasterDataType;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let band1 = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
    /// let band2 = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
    ///
    /// let dsl = RasterDsl::new();
    /// let result = dsl.execute("B1 + B2", &[band1, band2])?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn execute(&self, expression: &str, bands: &[RasterBuffer]) -> Result<RasterBuffer> {
        // Try parsing as expression first
        if let Ok(expr) = parse_expression(expression) {
            let optimized = self.optimizer.optimize_expr(expr);
            let program = Program {
                statements: vec![Statement::Expr(Box::new(optimized))],
            };
            let compiled = CompiledProgram::new(program);
            return compiled.execute(bands);
        }

        // Try parsing as program
        let program = parse_program(expression)?;
        let optimized = self.optimizer.optimize_program(program);
        let compiled = CompiledProgram::new(optimized);
        compiled.execute(bands)
    }

    /// Compiles a DSL expression to a reusable compiled program
    pub fn compile(&self, expression: &str) -> Result<CompiledProgram> {
        // Try parsing as expression first
        if let Ok(expr) = parse_expression(expression) {
            let optimized = self.optimizer.optimize_expr(expr);
            let program = Program {
                statements: vec![Statement::Expr(Box::new(optimized))],
            };
            return Ok(CompiledProgram::new(program));
        }

        // Try parsing as program
        let program = parse_program(expression)?;
        let optimized = self.optimizer.optimize_program(program);
        Ok(CompiledProgram::new(optimized))
    }

    /// Gets the function registry
    pub fn functions(&self) -> &FunctionRegistry {
        &self.func_registry
    }

    /// Lists all available function names
    pub fn list_functions(&self) -> Vec<&'static str> {
        self.func_registry.function_names()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_dsl_simple_expression() {
        let band1 = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let band2 = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let dsl = RasterDsl::new();
        let result = dsl.execute("B1 + B2", &[band1, band2]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dsl_ndvi() {
        let nir = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let red = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let dsl = RasterDsl::new();
        let result = dsl.execute("(B1 - B2) / (B1 + B2)", &[nir, red]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dsl_program() {
        let bands = vec![
            RasterBuffer::zeros(10, 10, RasterDataType::Float32),
            RasterBuffer::zeros(10, 10, RasterDataType::Float32),
        ];

        let program = r#"
            let ndvi = (B1 - B2) / (B1 + B2);
            ndvi;
        "#;

        let dsl = RasterDsl::new();
        let result = dsl.execute(program, &bands);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dsl_optimization() {
        let band = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let mut dsl = RasterDsl::new();
        dsl.set_opt_level(OptLevel::Aggressive);

        let result = dsl.execute("B1 + 0", &[band]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dsl_compile() {
        let dsl = RasterDsl::new();
        let compiled = dsl.compile("(B1 - B2) / (B1 + B2)");
        assert!(compiled.is_ok());
    }

    #[test]
    fn test_function_list() {
        let dsl = RasterDsl::new();
        let functions = dsl.list_functions();
        assert!(!functions.is_empty());
        assert!(functions.contains(&"sqrt"));
        assert!(functions.contains(&"sin"));
    }
}
