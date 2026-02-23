//! Macro support for compile-time DSL
//!
//! This module provides the `raster!` macro for compile-time DSL evaluation.

/// Macro for compile-time raster algebra expressions
///
/// # Examples
///
/// ```ignore
/// use oxigdal_algorithms::raster;
///
/// // Simple NDVI calculation
/// let ndvi = raster!((NIR - RED) / (NIR + RED));
///
/// // Complex multi-band analysis with conditions
/// let result = raster! {
///     let ndvi = (B8 - B4) / (B8 + B4);
///     if ndvi > 0.6 then 1 else 0
/// };
/// ```
#[macro_export]
macro_rules! raster {
    // Single expression
    ($expr:expr) => {{
        use $crate::dsl::{parse_expression, CompiledProgram};
        use $crate::dsl::ast::{Program, Statement};

        let expr_str = stringify!($expr);
        let parsed = parse_expression(expr_str)?;
        let program = Program {
            statements: vec![Statement::Expr(Box::new(parsed))],
        };
        CompiledProgram::new(program)
    }};

    // Block with multiple statements
    ({ $($stmt:stmt);+ }) => {{
        use $crate::dsl::{parse_program, CompiledProgram};

        let program_str = stringify!({ $($stmt);+ });
        let parsed = parse_program(program_str)?;
        CompiledProgram::new(parsed)
    }};
}

/// Helper macro for defining custom DSL functions
///
/// # Examples
///
/// ```ignore
/// dsl_function! {
///     fn ndvi(nir, red) = (nir - red) / (nir + red);
/// }
/// ```
#[macro_export]
macro_rules! dsl_function {
    (fn $name:ident ( $($param:ident),* ) = $body:expr ;) => {
        pub fn $name() -> &'static str {
            concat!(
                "fn ",
                stringify!($name),
                "(",
                $( stringify!($param), "," ),*
                ") = ",
                stringify!($body),
                ";"
            )
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_macro_expansion() {
        // This test just ensures the macros compile
        // Actual functionality is tested in integration tests
    }
}
