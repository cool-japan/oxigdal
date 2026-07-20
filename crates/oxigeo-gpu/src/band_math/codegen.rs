//! WGSL code generator for [`BandExpression`] ASTs.
//!
//! The generator emits a complete compute shader that reads each referenced
//! band from a `storage` buffer and writes the result into an `output` buffer.
//! A simple constant-folding pass runs first so that subtrees made of only
//! constants collapse to a single literal in the emitted shader.

use crate::algebra::BandExpression;
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// Generate a complete WGSL compute shader for `expr`.
///
/// `input_bindings` is a slice whose `i`-th entry gives the wgpu `binding`
/// index for the `i`-th distinct band referenced in the expression (sorted
/// ascending by 0-based band index). The output buffer is bound at
/// `input_bindings.len()` (i.e. one past the last input binding).
///
/// The shader has a fixed workgroup size of 64 along `x`.
///
/// # Panics
///
/// Does not panic — `String` formatting failures are infallible.
pub fn band_expression_to_wgsl(expr: &BandExpression, input_bindings: &[u32]) -> String {
    let folded = constant_fold(expr);

    // Collect band references in deterministic ascending order so the
    // `band_<n>` symbol the codegen emits lines up with `input_bindings`.
    let mut bands: BTreeSet<usize> = BTreeSet::new();
    collect_bands(&folded, &mut bands);
    let band_list: Vec<usize> = bands.into_iter().collect();

    let mut out = String::new();
    // Emit band bindings.
    for (i, band_idx) in band_list.iter().enumerate() {
        let binding = input_bindings.get(i).copied().unwrap_or(i as u32);
        // Emit using the source-level 1-based name (`band_1`, `band_2`, ...)
        // so a reader can pair them with `B1`, `B2`, ... in the formula.
        let _ = writeln!(
            out,
            "@group(0) @binding({}) var<storage, read> band_{}: array<f32>;",
            binding,
            band_idx + 1
        );
    }
    let output_binding = input_bindings
        .get(band_list.len())
        .copied()
        .unwrap_or(band_list.len() as u32);
    let _ = writeln!(
        out,
        "@group(0) @binding({}) var<storage, read_write> output: array<f32>;",
        output_binding
    );

    out.push('\n');
    out.push_str("@compute @workgroup_size(64)\n");
    out.push_str("fn main(@builtin(global_invocation_id) gid: vec3<u32>) {\n");
    out.push_str("    let idx = gid.x;\n");
    out.push_str("    if (idx >= arrayLength(&output)) { return; }\n");

    let expr_str = emit_expr(&folded);
    let _ = writeln!(out, "    output[idx] = {};", expr_str);
    out.push_str("}\n");

    out
}

/// Walk `expr` and accumulate every band-index it references.
fn collect_bands(expr: &BandExpression, out: &mut BTreeSet<usize>) {
    match expr {
        BandExpression::Band(i) => {
            out.insert(*i);
        }
        BandExpression::Constant(_) => {}
        BandExpression::Add(a, b)
        | BandExpression::Sub(a, b)
        | BandExpression::Mul(a, b)
        | BandExpression::Div(a, b)
        | BandExpression::Min(a, b)
        | BandExpression::Max(a, b)
        | BandExpression::Pow(a, b) => {
            collect_bands(a, out);
            collect_bands(b, out);
        }
        BandExpression::Sqrt(a)
        | BandExpression::Abs(a)
        | BandExpression::Neg(a)
        | BandExpression::Log(a)
        | BandExpression::Exp(a) => collect_bands(a, out),
        BandExpression::Clamp { value, lo, hi } => {
            collect_bands(value, out);
            collect_bands(lo, out);
            collect_bands(hi, out);
        }
    }
}

/// Recursively constant-fold the expression tree.
///
/// Any subtree whose operands are all `Constant(_)` collapses to a single
/// `Constant`. Division by literal zero is *not* folded — it's left in place
/// so callers can decide how to handle runtime nodata. (Note that the parser
/// already rejects literal `/ 0.0` at parse time.)
pub fn constant_fold(expr: &BandExpression) -> BandExpression {
    match expr {
        BandExpression::Band(i) => BandExpression::Band(*i),
        BandExpression::Constant(v) => BandExpression::Constant(*v),
        BandExpression::Add(a, b) => {
            let a = constant_fold(a);
            let b = constant_fold(b);
            match (&a, &b) {
                (BandExpression::Constant(x), BandExpression::Constant(y)) => {
                    BandExpression::Constant(x + y)
                }
                _ => BandExpression::Add(Box::new(a), Box::new(b)),
            }
        }
        BandExpression::Sub(a, b) => {
            let a = constant_fold(a);
            let b = constant_fold(b);
            match (&a, &b) {
                (BandExpression::Constant(x), BandExpression::Constant(y)) => {
                    BandExpression::Constant(x - y)
                }
                _ => BandExpression::Sub(Box::new(a), Box::new(b)),
            }
        }
        BandExpression::Mul(a, b) => {
            let a = constant_fold(a);
            let b = constant_fold(b);
            match (&a, &b) {
                (BandExpression::Constant(x), BandExpression::Constant(y)) => {
                    BandExpression::Constant(x * y)
                }
                _ => BandExpression::Mul(Box::new(a), Box::new(b)),
            }
        }
        BandExpression::Div(a, b) => {
            let a = constant_fold(a);
            let b = constant_fold(b);
            match (&a, &b) {
                (BandExpression::Constant(x), BandExpression::Constant(y)) if y.abs() > 1e-20 => {
                    BandExpression::Constant(x / y)
                }
                _ => BandExpression::Div(Box::new(a), Box::new(b)),
            }
        }
        BandExpression::Sqrt(a) => {
            let a = constant_fold(a);
            if let BandExpression::Constant(v) = a {
                BandExpression::Constant(v.max(0.0).sqrt())
            } else {
                BandExpression::Sqrt(Box::new(a))
            }
        }
        BandExpression::Abs(a) => {
            let a = constant_fold(a);
            if let BandExpression::Constant(v) = a {
                BandExpression::Constant(v.abs())
            } else {
                BandExpression::Abs(Box::new(a))
            }
        }
        BandExpression::Neg(a) => {
            let a = constant_fold(a);
            if let BandExpression::Constant(v) = a {
                BandExpression::Constant(-v)
            } else {
                BandExpression::Neg(Box::new(a))
            }
        }
        BandExpression::Min(a, b) => {
            let a = constant_fold(a);
            let b = constant_fold(b);
            match (&a, &b) {
                (BandExpression::Constant(x), BandExpression::Constant(y)) => {
                    BandExpression::Constant(x.min(*y))
                }
                _ => BandExpression::Min(Box::new(a), Box::new(b)),
            }
        }
        BandExpression::Max(a, b) => {
            let a = constant_fold(a);
            let b = constant_fold(b);
            match (&a, &b) {
                (BandExpression::Constant(x), BandExpression::Constant(y)) => {
                    BandExpression::Constant(x.max(*y))
                }
                _ => BandExpression::Max(Box::new(a), Box::new(b)),
            }
        }
        BandExpression::Pow(a, b) => {
            let a = constant_fold(a);
            let b = constant_fold(b);
            match (&a, &b) {
                (BandExpression::Constant(x), BandExpression::Constant(y)) => {
                    BandExpression::Constant(x.powf(*y))
                }
                _ => BandExpression::Pow(Box::new(a), Box::new(b)),
            }
        }
        BandExpression::Log(a) => {
            let a = constant_fold(a);
            if let BandExpression::Constant(v) = a {
                BandExpression::Constant(v.ln())
            } else {
                BandExpression::Log(Box::new(a))
            }
        }
        BandExpression::Exp(a) => {
            let a = constant_fold(a);
            if let BandExpression::Constant(v) = a {
                BandExpression::Constant(v.exp())
            } else {
                BandExpression::Exp(Box::new(a))
            }
        }
        BandExpression::Clamp { value, lo, hi } => {
            let v = constant_fold(value);
            let l = constant_fold(lo);
            let h = constant_fold(hi);
            match (&v, &l, &h) {
                (
                    BandExpression::Constant(vv),
                    BandExpression::Constant(ll),
                    BandExpression::Constant(hh),
                ) => BandExpression::Constant(vv.clamp(*ll, *hh)),
                _ => BandExpression::Clamp {
                    value: Box::new(v),
                    lo: Box::new(l),
                    hi: Box::new(h),
                },
            }
        }
    }
}

/// Format an `f32` literal in a way WGSL accepts as a `f32` value.
///
/// WGSL requires either an explicit decimal point (`1.0`) or an `f` suffix.
/// We emit the shortest round-trip-safe form with a decimal point.
fn fmt_f32_literal(v: f32) -> String {
    if v.is_nan() {
        // WGSL has no NaN literal; emit `0.0/0.0` so the compiler folds it.
        return "(0.0 / 0.0)".to_string();
    }
    if v.is_infinite() {
        // Likewise, emit a division producing infinity.
        return if v > 0.0 {
            "(1.0 / 0.0)".to_string()
        } else {
            "(-1.0 / 0.0)".to_string()
        };
    }
    // Use the default Display, then ensure a decimal point is present.
    let mut s = format!("{}", v);
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        s.push_str(".0");
    }
    s
}

/// Recursively render `expr` as a WGSL expression string.
fn emit_expr(expr: &BandExpression) -> String {
    match expr {
        BandExpression::Band(i) => format!("band_{}[idx]", i + 1),
        BandExpression::Constant(v) => fmt_f32_literal(*v),
        BandExpression::Add(a, b) => format!("({} + {})", emit_expr(a), emit_expr(b)),
        BandExpression::Sub(a, b) => format!("({} - {})", emit_expr(a), emit_expr(b)),
        BandExpression::Mul(a, b) => format!("({} * {})", emit_expr(a), emit_expr(b)),
        BandExpression::Div(a, b) => format!("({} / {})", emit_expr(a), emit_expr(b)),
        BandExpression::Sqrt(a) => format!("sqrt(max(0.0, {}))", emit_expr(a)),
        BandExpression::Abs(a) => format!("abs({})", emit_expr(a)),
        BandExpression::Neg(a) => format!("(-{})", emit_expr(a)),
        BandExpression::Min(a, b) => format!("min({}, {})", emit_expr(a), emit_expr(b)),
        BandExpression::Max(a, b) => format!("max({}, {})", emit_expr(a), emit_expr(b)),
        BandExpression::Pow(a, b) => format!("pow({}, {})", emit_expr(a), emit_expr(b)),
        BandExpression::Log(a) => format!("log({})", emit_expr(a)),
        BandExpression::Exp(a) => format!("exp({})", emit_expr(a)),
        BandExpression::Clamp { value, lo, hi } => format!(
            "clamp({}, {}, {})",
            emit_expr(value),
            emit_expr(lo),
            emit_expr(hi)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_f32_literal_appends_decimal() {
        assert_eq!(fmt_f32_literal(5.0), "5.0");
        assert_eq!(fmt_f32_literal(1.5), "1.5");
        assert_eq!(fmt_f32_literal(-2.0), "-2.0");
    }

    #[test]
    fn test_collect_bands_unique_and_sorted() {
        let expr = BandExpression::Add(
            Box::new(BandExpression::Band(2)),
            Box::new(BandExpression::Sub(
                Box::new(BandExpression::Band(0)),
                Box::new(BandExpression::Band(2)),
            )),
        );
        let mut bands = BTreeSet::new();
        collect_bands(&expr, &mut bands);
        let v: Vec<usize> = bands.into_iter().collect();
        assert_eq!(v, vec![0, 2]);
    }

    #[test]
    fn test_constant_fold_addition() {
        let e = BandExpression::Add(
            Box::new(BandExpression::Constant(2.0)),
            Box::new(BandExpression::Constant(3.0)),
        );
        let folded = constant_fold(&e);
        let v = match folded {
            BandExpression::Constant(v) => v,
            other => {
                assert!(
                    matches!(other, BandExpression::Constant(_)),
                    "expected Constant variant"
                );
                0.0_f32
            }
        };
        assert!((v - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_codegen_contains_workgroup() {
        let e = BandExpression::Band(0);
        let s = band_expression_to_wgsl(&e, &[0]);
        assert!(s.contains("@workgroup_size(64)"));
        assert!(s.contains("band_1: array<f32>"));
    }
}
