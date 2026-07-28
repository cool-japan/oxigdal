//! Macro support for embedding DSL source in Rust
//!
//! This module provides the `raster!` and `dsl_function!` macros. Both are
//! ergonomic front-ends for writing raster-algebra DSL *source* inside Rust code.
//!
//! Note: despite living in a module historically named for "compile-time"
//! evaluation, these macros do NOT evaluate the DSL at compile time. `raster!`
//! `stringify!`s the tokens and parses/compiles them at run time (returning a
//! [`CompiledProgram`](crate::dsl::CompiledProgram)); `dsl_function!` generates a
//! helper that returns the function's DSL *source definition* as a `String`,
//! ready to be prepended to a DSL program before the function is called.

/// Build a [`CompiledProgram`](crate::dsl::CompiledProgram) from raster-algebra
/// DSL tokens written inline in Rust.
///
/// The tokens are `stringify!`d and then parsed and compiled **at run time**
/// (not at compile time). The macro expands to an expression of type
/// `CompiledProgram`, so it must be used in a context where the `?` operator can
/// propagate a parse error (i.e. a function returning
/// [`Result`](crate::error::Result)).
///
/// # Examples
///
/// ```ignore
/// use oxigeo_algorithms::raster;
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
        use $crate::dsl::ast::{Program, Statement};
        use $crate::dsl::{parse_expression, CompiledProgram};

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

/// Define a helper that returns the DSL *source definition* of a reusable
/// raster-algebra function.
///
/// This does not register a Rust-callable function and does not evaluate
/// anything: it expands to `pub fn <name>() -> String` returning a syntactically
/// valid DSL function declaration such as
/// `"fn ndvi(nir, red) = (nir - red) / (nir + red);"`. Prepend that string to a
/// DSL program and then call the function from a DSL expression to use it.
///
/// # Examples
///
/// ```ignore
/// use oxigeo_algorithms::dsl_function;
/// use oxigeo_algorithms::dsl::RasterDsl;
///
/// dsl_function! {
///     fn ndvi(nir, red) = (nir - red) / (nir + red);
/// }
///
/// // `ndvi()` yields the DSL source; splice it into a program and call it.
/// let program = format!("{}\nndvi(B1, B2);", ndvi());
/// let dsl = RasterDsl::new();
/// // dsl.execute(&program, &[nir_band, red_band]) -> resulting raster
/// ```
#[macro_export]
macro_rules! dsl_function {
    (fn $name:ident ( $($param:ident),* $(,)? ) = $body:expr ;) => {
        /// Returns the DSL source definition for this function, suitable for
        /// prepending to a DSL program before the function is called.
        pub fn $name() -> ::std::string::String {
            let params: &[&str] = &[ $( stringify!($param) ),* ];
            let mut out = ::std::string::String::from("fn ");
            out.push_str(stringify!($name));
            out.push('(');
            out.push_str(&params.join(", "));
            out.push_str(") = ");
            out.push_str(stringify!($body));
            out.push(';');
            out
        }
    };
}

#[cfg(test)]
mod tests {
    // Generate a helper via the macro at item scope.
    dsl_function! {
        fn ndvi(nir, red) = (nir - red) / (nir + red);
    }

    #[test]
    fn test_dsl_function_emits_valid_source() {
        // The generated helper returns a syntactically valid DSL definition
        // (no trailing comma in the parameter list).
        let src = ndvi();
        assert_eq!(src, "fn ndvi(nir, red) = (nir - red) / (nir + red);");
    }

    #[test]
    fn test_dsl_function_source_is_usable_in_a_program() {
        use crate::dsl::{CompiledProgram, parse_program};
        use oxigeo_core::buffer::RasterBuffer;
        use oxigeo_core::types::RasterDataType;

        // The emitted source must parse as a DSL program on its own...
        let def = ndvi();
        parse_program(&def).expect("generated dsl_function source must parse");

        // ...and be callable end-to-end when spliced into a program and driven
        // through the real compiler. (We use parse_program + CompiledProgram
        // directly rather than RasterDsl::execute, whose expression fast-path
        // would otherwise pick up only the trailing call statement.)
        let program_src = format!("{def}\nndvi(B1, B2);");
        let program = parse_program(&program_src).expect("full program must parse");
        let compiled = CompiledProgram::new(program);

        let mut nir = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        let mut red = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        for y in 0..2 {
            for x in 0..2 {
                nir.set_pixel(x, y, 3.0).expect("set nir");
                red.set_pixel(x, y, 1.0).expect("set red");
            }
        }

        let result = compiled
            .execute(&[nir, red])
            .expect("dsl_function program should execute");

        // NDVI = (3 - 1) / (3 + 1) = 0.5 at every pixel.
        for y in 0..2 {
            for x in 0..2 {
                let v = result.get_pixel(x, y).expect("pixel readable");
                assert!((v - 0.5).abs() < 1e-6, "unexpected NDVI value {v}");
            }
        }
    }
}
