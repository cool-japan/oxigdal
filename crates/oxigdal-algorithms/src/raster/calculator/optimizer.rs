//! Expression optimizer: constant folding, algebraic simplifications, and
//! common subexpression elimination (CSE) via hash-consing.

use super::ast::{BinaryOp, Expr, UnaryOp};

use std::collections::HashMap;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Expression optimizer for constant folding, algebraic simplifications, and CSE.
pub(super) struct Optimizer;

impl Optimizer {
    /// Optimize an expression tree.
    ///
    /// Returns `(optimized_expr, cache_slots)` where `cache_slots[i]` holds the original
    /// (pre-rewrite) expression for CSE slot `i`.  When the CSE pass finds no repeated
    /// sub-expressions the returned `Vec` is empty and the expression is unchanged.
    pub(super) fn optimize(expr: Expr) -> (Expr, Vec<Expr>) {
        let expr = constant_fold(expr);
        let expr = algebraic_simplify(expr);
        eliminate_common_subexpressions(expr)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass 1: constant folding
// ─────────────────────────────────────────────────────────────────────────────

fn constant_fold(expr: Expr) -> Expr {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let left = constant_fold(*left);
            let right = constant_fold(*right);

            if let (Expr::Number(l), Expr::Number(r)) = (&left, &right) {
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
                    BinaryOp::Power => l.powf(*r),
                    BinaryOp::Greater => {
                        if l > r {
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
                    BinaryOp::GreaterEqual => {
                        if l >= r {
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
                    BinaryOp::And => {
                        if *l != 0.0 && *r != 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::Or => {
                        if *l != 0.0 || *r != 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                return Expr::Number(result);
            }

            Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            }
        }
        Expr::UnaryOp { op, expr } => {
            let expr = constant_fold(*expr);
            if let Expr::Number(n) = expr {
                let result = match op {
                    UnaryOp::Negate => -n,
                };
                return Expr::Number(result);
            }
            Expr::UnaryOp {
                op,
                expr: Box::new(expr),
            }
        }
        Expr::Function { name, args } => {
            let args: Vec<Expr> = args.into_iter().map(constant_fold).collect();

            let all_const = args.iter().all(|arg| matches!(arg, Expr::Number(_)));
            if all_const {
                let arg_vals: Vec<f64> = args
                    .iter()
                    .filter_map(|arg| {
                        if let Expr::Number(n) = arg {
                            Some(*n)
                        } else {
                            None
                        }
                    })
                    .collect();

                let result = match name.as_str() {
                    "sqrt" if arg_vals.len() == 1 => Some(arg_vals[0].sqrt()),
                    "abs" if arg_vals.len() == 1 => Some(arg_vals[0].abs()),
                    "log" if arg_vals.len() == 1 => Some(arg_vals[0].ln()),
                    "log10" if arg_vals.len() == 1 => Some(arg_vals[0].log10()),
                    "exp" if arg_vals.len() == 1 => Some(arg_vals[0].exp()),
                    "sin" if arg_vals.len() == 1 => Some(arg_vals[0].sin()),
                    "cos" if arg_vals.len() == 1 => Some(arg_vals[0].cos()),
                    "tan" if arg_vals.len() == 1 => Some(arg_vals[0].tan()),
                    "floor" if arg_vals.len() == 1 => Some(arg_vals[0].floor()),
                    "ceil" if arg_vals.len() == 1 => Some(arg_vals[0].ceil()),
                    "round" if arg_vals.len() == 1 => Some(arg_vals[0].round()),
                    "min" if !arg_vals.is_empty() => {
                        Some(arg_vals.iter().copied().fold(f64::INFINITY, f64::min))
                    }
                    "max" if !arg_vals.is_empty() => {
                        Some(arg_vals.iter().copied().fold(f64::NEG_INFINITY, f64::max))
                    }
                    _ => None,
                };

                if let Some(val) = result {
                    return Expr::Number(val);
                }
            }

            Expr::Function { name, args }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let condition = constant_fold(*condition);
            let then_expr = constant_fold(*then_expr);
            let else_expr = constant_fold(*else_expr);

            if let Expr::Number(cond) = condition {
                if cond != 0.0 {
                    return then_expr;
                } else {
                    return else_expr;
                }
            }

            Expr::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            }
        }
        other => other,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass 2: algebraic simplification
// ─────────────────────────────────────────────────────────────────────────────

fn algebraic_simplify(expr: Expr) -> Expr {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let left = algebraic_simplify(*left);
            let right = algebraic_simplify(*right);

            match (&left, op, &right) {
                // x + 0 = x, 0 + x = x
                (_, BinaryOp::Add, Expr::Number(n)) if n.abs() < f64::EPSILON => left,
                (Expr::Number(n), BinaryOp::Add, _) if n.abs() < f64::EPSILON => right,

                // x - 0 = x
                (_, BinaryOp::Subtract, Expr::Number(n)) if n.abs() < f64::EPSILON => left,

                // x * 0 = 0, 0 * x = 0
                (_, BinaryOp::Multiply, Expr::Number(n))
                | (Expr::Number(n), BinaryOp::Multiply, _)
                    if n.abs() < f64::EPSILON =>
                {
                    Expr::Number(0.0)
                }

                // x * 1 = x, 1 * x = x
                (_, BinaryOp::Multiply, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => left,
                (Expr::Number(n), BinaryOp::Multiply, _) if (n - 1.0).abs() < f64::EPSILON => right,

                // x / 1 = x
                (_, BinaryOp::Divide, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => left,

                // x ^ 0 = 1
                (_, BinaryOp::Power, Expr::Number(n)) if n.abs() < f64::EPSILON => {
                    Expr::Number(1.0)
                }

                // x ^ 1 = x
                (_, BinaryOp::Power, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => left,

                _ => Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
            }
        }
        Expr::UnaryOp { op, expr } => {
            let expr = algebraic_simplify(*expr);
            Expr::UnaryOp {
                op,
                expr: Box::new(expr),
            }
        }
        Expr::Function { name, args } => {
            let args = args.into_iter().map(algebraic_simplify).collect();
            Expr::Function { name, args }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => Expr::Conditional {
            condition: Box::new(algebraic_simplify(*condition)),
            then_expr: Box::new(algebraic_simplify(*then_expr)),
            else_expr: Box::new(algebraic_simplify(*else_expr)),
        },
        other => other,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass 3: Common Subexpression Elimination (CSE)
// ─────────────────────────────────────────────────────────────────────────────

/// Walk the expression tree in post-order and count how often each unique
/// sub-expression appears.
///
/// Leaf nodes (`Number`, `Band`) are not tracked — they are trivially cheap to
/// re-evaluate and produce noise in the frequency map.  `CacheRef` nodes are
/// likewise skipped because they are already replaced references.
fn collect_subexpr_counts(expr: &Expr, counts: &mut HashMap<Expr, usize>) {
    match expr {
        // Leaves — not worth caching individually
        Expr::Number(_) | Expr::Band(_) | Expr::CacheRef(_) => {}

        Expr::BinaryOp { left, op: _, right } => {
            collect_subexpr_counts(left, counts);
            collect_subexpr_counts(right, counts);
            *counts.entry(expr.clone()).or_insert(0) += 1;
        }
        Expr::UnaryOp { op: _, expr: inner } => {
            collect_subexpr_counts(inner, counts);
            *counts.entry(expr.clone()).or_insert(0) += 1;
        }
        Expr::Function { name: _, args } => {
            for arg in args {
                collect_subexpr_counts(arg, counts);
            }
            *counts.entry(expr.clone()).or_insert(0) += 1;
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_subexpr_counts(condition, counts);
            collect_subexpr_counts(then_expr, counts);
            collect_subexpr_counts(else_expr, counts);
            *counts.entry(expr.clone()).or_insert(0) += 1;
        }
    }
}

/// Rewrite the expression tree replacing every sub-tree that is present in
/// `slot_map` with the corresponding `CacheRef`.  The replacement is pre-order:
/// when an entire sub-tree matches a slot we emit a `CacheRef` and do NOT
/// recurse further into that sub-tree (the slot expression itself is stored
/// separately and will be evaluated on first access).
fn rewrite_with_cache_refs(expr: Expr, slot_map: &HashMap<Expr, u32>) -> Expr {
    // Pre-order check: does this exact node map to a slot?
    if let Some(&slot_id) = slot_map.get(&expr) {
        return Expr::CacheRef(slot_id);
    }

    // Recurse into children
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let left = rewrite_with_cache_refs(*left, slot_map);
            let right = rewrite_with_cache_refs(*right, slot_map);
            Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            }
        }
        Expr::UnaryOp { op, expr: inner } => {
            let inner = rewrite_with_cache_refs(*inner, slot_map);
            Expr::UnaryOp {
                op,
                expr: Box::new(inner),
            }
        }
        Expr::Function { name, args } => {
            let args = args
                .into_iter()
                .map(|arg| rewrite_with_cache_refs(arg, slot_map))
                .collect();
            Expr::Function { name, args }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let condition = rewrite_with_cache_refs(*condition, slot_map);
            let then_expr = rewrite_with_cache_refs(*then_expr, slot_map);
            let else_expr = rewrite_with_cache_refs(*else_expr, slot_map);
            Expr::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            }
        }
        // Leaves and CacheRef pass through unchanged
        other => other,
    }
}

/// Common Subexpression Elimination via hash-consing.
///
/// Returns `(rewritten_expr, slot_exprs)`.  `slot_exprs[i]` is the original
/// expression that must be evaluated (at most once per pixel) to supply the
/// value for slot `i`.  When no sub-expressions are repeated the returned
/// `Vec` is empty and the expression is returned as-is.
///
/// # Algorithm
///
/// 1. Walk the tree counting how many times each unique sub-tree appears.
/// 2. Collect those with count ≥ 2 and assign sequential slot IDs.
/// 3. Walk the tree again (pre-order), replacing matching sub-trees with
///    `CacheRef(slot_id)`.
/// 4. Return the rewritten tree plus the original expressions for each slot.
fn eliminate_common_subexpressions(expr: Expr) -> (Expr, Vec<Expr>) {
    // Step 1 — count occurrences of each unique sub-tree
    let mut counts: HashMap<Expr, usize> = HashMap::new();
    collect_subexpr_counts(&expr, &mut counts);

    // Step 2 — filter to those appearing >= 2 times; assign slot IDs
    //
    // We sort the entries deterministically (by a stable key) so that repeated
    // runs produce the same slot numbering.  We use the debug representation as
    // a surrogate sort key; it is unique for structurally distinct expressions.
    let mut repeated: Vec<(Expr, usize)> = counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .collect();

    // Sort by count descending (most-repeated first), break ties by debug repr
    repeated.sort_by(|(ea, ca), (eb, cb)| {
        cb.cmp(ca)
            .then_with(|| format!("{ea:?}").cmp(&format!("{eb:?}")))
    });

    if repeated.is_empty() {
        return (expr, Vec::new());
    }

    // Build slot_map: expression -> slot_id, and slot_exprs: slot_id -> expression
    let mut slot_map: HashMap<Expr, u32> = HashMap::with_capacity(repeated.len());
    let mut slot_exprs: Vec<Expr> = Vec::with_capacity(repeated.len());

    for (slot_expr, _count) in repeated {
        let slot_id = slot_exprs.len() as u32;
        slot_map.insert(slot_expr.clone(), slot_id);
        slot_exprs.push(slot_expr);
    }

    // Step 3 — rewrite the tree
    let rewritten = rewrite_with_cache_refs(expr, &slot_map);

    (rewritten, slot_exprs)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::raster::calculator::{evaluator::Evaluator, lexer::Lexer, parser::Parser};
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn parse(src: &str) -> Expr {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens);
        parser.parse().expect("parse")
    }

    fn count_cache_refs(expr: &Expr) -> usize {
        match expr {
            Expr::CacheRef(_) => 1,
            Expr::BinaryOp { left, op: _, right } => {
                count_cache_refs(left) + count_cache_refs(right)
            }
            Expr::UnaryOp { op: _, expr: inner } => count_cache_refs(inner),
            Expr::Function { name: _, args } => args.iter().map(count_cache_refs).sum(),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                count_cache_refs(condition)
                    + count_cache_refs(then_expr)
                    + count_cache_refs(else_expr)
            }
            _ => 0,
        }
    }

    // ── CSE tests ────────────────────────────────────────────────────────────

    /// (B1 + B2) appears twice in (B1 + B2) * (B1 + B2); CSE must eliminate it.
    #[test]
    fn test_cse_eliminates_repeated_slope() {
        // Expression: (B1 + B2) * (B1 + B2)
        let expr = parse("(B1 + B2) * (B1 + B2)");
        let (rewritten, slots) = eliminate_common_subexpressions(expr);

        // Should have produced exactly one slot
        assert_eq!(slots.len(), 1, "expected one CSE slot for (B1+B2)");

        // The rewritten tree must contain exactly two CacheRef(0) nodes
        let ref_count = count_cache_refs(&rewritten);
        assert_eq!(
            ref_count, 2,
            "expected two CacheRef nodes in rewritten tree, got {ref_count}"
        );

        // The slot expression must be the original (B1+B2)
        let expected_slot = parse("B1 + B2");
        assert_eq!(slots[0], expected_slot, "slot 0 should hold (B1 + B2)");
    }

    /// When every sub-expression is unique, no CacheRef nodes should appear.
    #[test]
    fn test_cse_no_change_on_unique_subexprs() {
        let expr = parse("B1 + B2 * B1");
        let (rewritten, slots) = eliminate_common_subexpressions(expr);
        // B1 is a leaf — not counted; B2 * B1 appears once.  No slots expected.
        assert!(
            slots.is_empty(),
            "no repeated sub-expressions; slots should be empty"
        );
        assert_eq!(
            count_cache_refs(&rewritten),
            0,
            "no CacheRef expected in unique expression"
        );
    }

    /// Optimized and non-optimized evaluation must yield identical pixel values.
    #[test]
    fn test_cse_preserves_semantics_via_eval_equality() {
        // Use a band with varying pixel values to stress-test semantic preservation.
        let mut band = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        for y in 0..4u64 {
            for x in 0..4u64 {
                band.set_pixel(x, y, (x * 3 + y * 7 + 1) as f64).ok();
            }
        }
        let bands = [band.clone()];

        // An expression where (B1 + B1) appears twice: (B1 + B1) * (B1 + B1)
        let raw_expr = parse("(B1 + B1) * (B1 + B1)");
        let (opt_expr, slots) = eliminate_common_subexpressions(raw_expr.clone());

        // Slots must be non-empty (there IS a repeated sub-expression)
        assert!(!slots.is_empty());

        let evaluator_raw = Evaluator::new(&bands, &[]);
        let evaluator_opt = Evaluator::new(&bands, &slots);

        for y in 0..4u64 {
            for x in 0..4u64 {
                let mut cache = vec![None; slots.len()];
                let raw_val = evaluator_raw
                    .eval_pixel(&raw_expr, x, y, &mut vec![])
                    .expect("raw eval");
                let opt_val = evaluator_opt
                    .eval_pixel(&opt_expr, x, y, &mut cache)
                    .expect("opt eval");
                assert!(
                    (raw_val - opt_val).abs() < f64::EPSILON,
                    "semantic mismatch at ({x},{y}): raw={raw_val} opt={opt_val}"
                );
            }
        }
    }

    /// Each CacheRef must be evaluated exactly once per pixel — the cache slot
    /// must be populated on first access and reused on subsequent accesses.
    #[test]
    fn test_cse_cache_ref_evaluation_caches_once() {
        let mut band = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        band.set_pixel(0, 0, 3.0).ok();
        let bands = [band];

        // (B1 + B1) * (B1 + B1) → slot 0 = B1 + B1
        let expr = parse("(B1 + B1) * (B1 + B1)");
        let (opt_expr, slots) = eliminate_common_subexpressions(expr);

        assert_eq!(slots.len(), 1, "should have exactly one CSE slot");

        let evaluator = Evaluator::new(&bands, &slots);
        let mut pixel_cache: Vec<Option<f64>> = vec![None; slots.len()];

        let result = evaluator
            .eval_pixel(&opt_expr, 0, 0, &mut pixel_cache)
            .expect("eval");

        // B1=3 → B1+B1=6 → 6*6=36
        assert!(
            (result - 36.0).abs() < f64::EPSILON,
            "expected 36.0, got {result}"
        );

        // After evaluation the cache slot must be populated
        assert_eq!(pixel_cache[0], Some(6.0), "slot 0 should be cached as 6.0");
    }

    /// Nested repeated sub-expressions: both the inner and the outer repetitions
    /// should be captured by the CSE pass.
    ///
    /// Expression: `(B1 * B1) + (B1 * B1)` — `B1 * B1` appears twice.
    #[test]
    fn test_cse_handles_nested_repeated_subexpressions() {
        let mut band = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        for y in 0..2u64 {
            for x in 0..2u64 {
                band.set_pixel(x, y, 5.0).ok();
            }
        }
        let bands = [band];

        let expr = parse("(B1 * B1) + (B1 * B1)");
        let (opt_expr, slots) = eliminate_common_subexpressions(expr.clone());

        assert!(
            !slots.is_empty(),
            "B1*B1 appears twice — must produce a slot"
        );

        let evaluator_raw = Evaluator::new(&bands, &[]);
        let evaluator_opt = Evaluator::new(&bands, &slots);

        for y in 0..2u64 {
            for x in 0..2u64 {
                let raw_val = evaluator_raw
                    .eval_pixel(&expr, x, y, &mut vec![])
                    .expect("raw eval");
                let mut cache = vec![None; slots.len()];
                let opt_val = evaluator_opt
                    .eval_pixel(&opt_expr, x, y, &mut cache)
                    .expect("opt eval");

                // B1=5 → B1*B1=25 → 25+25=50
                assert!(
                    (raw_val - 50.0).abs() < f64::EPSILON,
                    "raw value should be 50, got {raw_val}"
                );
                assert!(
                    (opt_val - 50.0).abs() < f64::EPSILON,
                    "opt value should be 50, got {opt_val}"
                );
            }
        }
    }
}
