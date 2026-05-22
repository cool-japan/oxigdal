//! SQL-like attribute WHERE filter with predicate pushdown for GeoPackage tables.
//!
//! This module provides:
//!
//! * [`Comparator`] — the full set of SQL comparison operators (including LIKE, IS NULL, etc.)
//! * [`FilterOperand`] — either a column index reference or a literal [`CellValue`]
//! * [`Predicate`] — a single comparison: `left op right`
//! * [`FilterExpr`] — composable expression tree (AND / OR / NOT / leaf predicate)
//! * [`evaluate`] — row-level evaluation entry point
//!
//! # Example
//! ```
//! use oxigdal_gpkg::filter::{FilterExpr, evaluate};
//! use oxigdal_gpkg::CellValue;
//!
//! let row = vec![CellValue::Integer(42), CellValue::Text("hello".into())];
//! let expr = FilterExpr::and(
//!     FilterExpr::col_gte(0, CellValue::Integer(10)),
//!     FilterExpr::col_like(1, "hell%"),
//! );
//! assert!(evaluate(&expr, &row));
//! ```

use crate::btree::CellValue;

// ─────────────────────────────────────────────────────────────────────────────
// Comparator
// ─────────────────────────────────────────────────────────────────────────────

/// SQL comparison operator for a [`Predicate`].
///
/// Numeric comparisons (`Lt`, `Lte`, `Gt`, `Gte`) promote both operands to
/// `f64` when possible, falling back to lexicographic text comparison for TEXT
/// vs TEXT pairs, and returning `false` for mismatched or incompatible types.
///
/// `IsNull` / `IsNotNull` ignore the `right` operand of the [`Predicate`]
/// (it should be `None`).
///
/// `Like` / `NotLike` support SQL wildcard syntax: `%` matches any sequence of
/// characters, `_` matches exactly one character.  Use `\` to escape a literal
/// `%`, `_`, or `\`.  The match is case-sensitive.
#[derive(Debug, Clone, PartialEq)]
pub enum Comparator {
    /// `=` — equality (NULL = NULL is false, per SQL three-valued logic).
    Eq,
    /// `<>` / `!=` — inequality.
    Ne,
    /// `<` — strictly less than.
    Lt,
    /// `<=` — less than or equal.
    Lte,
    /// `>` — strictly greater than.
    Gt,
    /// `>=` — greater than or equal.
    Gte,
    /// SQL `LIKE` with `%` and `_` wildcards, escaped with `\`.
    Like,
    /// SQL `NOT LIKE`.
    NotLike,
    /// SQL `IS NULL` — matches [`CellValue::Null`] or an out-of-bounds column.
    IsNull,
    /// SQL `IS NOT NULL` — does not match [`CellValue::Null`] or missing column.
    IsNotNull,
}

// ─────────────────────────────────────────────────────────────────────────────
// FilterOperand
// ─────────────────────────────────────────────────────────────────────────────

/// One side of a [`Predicate`] comparison: either a column reference or a
/// constant literal value.
#[derive(Debug, Clone)]
pub enum FilterOperand {
    /// Zero-based column index into the row value vector.
    Column(usize),
    /// A compile-time constant [`CellValue`].
    Literal(CellValue),
}

// ─────────────────────────────────────────────────────────────────────────────
// Predicate
// ─────────────────────────────────────────────────────────────────────────────

/// A single two-sided comparison: `left <op> right`.
///
/// For [`Comparator::IsNull`] and [`Comparator::IsNotNull`] the `right`
/// field must be `None`; it is ignored during evaluation.
#[derive(Debug, Clone)]
pub struct Predicate {
    /// The left-hand operand.
    pub left: FilterOperand,
    /// The comparison operator.
    pub op: Comparator,
    /// The right-hand operand.  `None` is valid only for IS NULL / IS NOT NULL.
    pub right: Option<FilterOperand>,
}

// ─────────────────────────────────────────────────────────────────────────────
// FilterExpr
// ─────────────────────────────────────────────────────────────────────────────

/// A composable filter expression tree.
///
/// Leaf nodes are [`Predicate`] comparisons; interior nodes are boolean
/// combinators ([`FilterExpr::And`], [`FilterExpr::Or`], [`FilterExpr::Not`]).
///
/// Use the builder helpers (`col_eq`, `col_like`, `and`, `or`, `not`, …) to
/// construct expressions without touching the internal representation directly.
#[derive(Debug, Clone)]
pub enum FilterExpr {
    /// Leaf: a single predicate comparison.
    Pred(Predicate),
    /// Boolean AND of two sub-expressions (short-circuits).
    And(Box<FilterExpr>, Box<FilterExpr>),
    /// Boolean OR of two sub-expressions (short-circuits).
    Or(Box<FilterExpr>, Box<FilterExpr>),
    /// Boolean NOT of a sub-expression.
    Not(Box<FilterExpr>),
}

impl FilterExpr {
    // ── Leaf builders ────────────────────────────────────────────────────────

    /// `col[col_idx] = val`
    pub fn col_eq(col_idx: usize, val: CellValue) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::Eq,
            right: Some(FilterOperand::Literal(val)),
        })
    }

    /// `col[col_idx] <> val`
    pub fn col_ne(col_idx: usize, val: CellValue) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::Ne,
            right: Some(FilterOperand::Literal(val)),
        })
    }

    /// `col[col_idx] < val`
    pub fn col_lt(col_idx: usize, val: CellValue) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::Lt,
            right: Some(FilterOperand::Literal(val)),
        })
    }

    /// `col[col_idx] <= val`
    pub fn col_lte(col_idx: usize, val: CellValue) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::Lte,
            right: Some(FilterOperand::Literal(val)),
        })
    }

    /// `col[col_idx] > val`
    pub fn col_gt(col_idx: usize, val: CellValue) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::Gt,
            right: Some(FilterOperand::Literal(val)),
        })
    }

    /// `col[col_idx] >= val`
    pub fn col_gte(col_idx: usize, val: CellValue) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::Gte,
            right: Some(FilterOperand::Literal(val)),
        })
    }

    /// `col[col_idx] LIKE pattern`
    ///
    /// `pattern` may contain `%` (any sequence) and `_` (any single character)
    /// wildcards.  Use `\` to escape a literal `%`, `_`, or `\`.
    pub fn col_like(col_idx: usize, pattern: &str) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::Like,
            right: Some(FilterOperand::Literal(CellValue::Text(pattern.to_owned()))),
        })
    }

    /// `col[col_idx] NOT LIKE pattern`
    pub fn col_not_like(col_idx: usize, pattern: &str) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::NotLike,
            right: Some(FilterOperand::Literal(CellValue::Text(pattern.to_owned()))),
        })
    }

    /// `col[col_idx] IS NULL`
    pub fn col_is_null(col_idx: usize) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::IsNull,
            right: None,
        })
    }

    /// `col[col_idx] IS NOT NULL`
    pub fn col_is_not_null(col_idx: usize) -> Self {
        Self::Pred(Predicate {
            left: FilterOperand::Column(col_idx),
            op: Comparator::IsNotNull,
            right: None,
        })
    }

    // ── Combinator builders ──────────────────────────────────────────────────

    /// `a AND b` — short-circuit evaluation.
    pub fn and(a: FilterExpr, b: FilterExpr) -> Self {
        Self::And(Box::new(a), Box::new(b))
    }

    /// `a OR b` — short-circuit evaluation.
    pub fn or(a: FilterExpr, b: FilterExpr) -> Self {
        Self::Or(Box::new(a), Box::new(b))
    }

    /// `NOT e`
    #[allow(clippy::should_implement_trait)]
    pub fn not(e: FilterExpr) -> Self {
        Self::Not(Box::new(e))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public evaluation entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Evaluate a [`FilterExpr`] against a single table row.
///
/// `row` is a slice of [`CellValue`] in column order (index 0 = first column).
///
/// Returns `true` when the row satisfies the expression, `false` otherwise.
///
/// # Semantics
/// * AND short-circuits on the first `false` branch.
/// * OR short-circuits on the first `true` branch.
/// * NULL comparisons follow SQL three-valued logic: `NULL = x` is `false`,
///   `NULL IS NULL` is `true`.
/// * `IsNull` matches both [`CellValue::Null`] and an out-of-range column index.
/// * `IsNotNull` is the complement of `IsNull`.
#[must_use]
pub fn evaluate(expr: &FilterExpr, row: &[CellValue]) -> bool {
    match expr {
        FilterExpr::Pred(pred) => eval_predicate(pred, row),
        FilterExpr::And(a, b) => evaluate(a, row) && evaluate(b, row),
        FilterExpr::Or(a, b) => evaluate(a, row) || evaluate(b, row),
        FilterExpr::Not(e) => !evaluate(e, row),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve a [`FilterOperand`] to a reference into `row`, or `None` when the
/// column index is out of range.
fn resolve_operand<'row>(
    operand: &'row FilterOperand,
    row: &'row [CellValue],
) -> Option<&'row CellValue> {
    match operand {
        FilterOperand::Column(idx) => row.get(*idx),
        FilterOperand::Literal(v) => Some(v),
    }
}

/// Evaluate a leaf [`Predicate`] against `row`.
fn eval_predicate(pred: &Predicate, row: &[CellValue]) -> bool {
    match pred.op {
        Comparator::IsNull => {
            let left = resolve_operand(&pred.left, row);
            // Either the column is missing (out of range) or its value is NULL.
            matches!(left, Some(CellValue::Null) | None)
        }
        Comparator::IsNotNull => {
            let left = resolve_operand(&pred.left, row);
            !matches!(left, Some(CellValue::Null) | None)
        }
        _ => {
            // All other operators require both left and right operands.
            let left = resolve_operand(&pred.left, row);
            let right = pred.right.as_ref().and_then(|r| resolve_operand(r, row));

            match (left, right) {
                (Some(l), Some(r)) => compare(l, &pred.op, r),
                // Any NULL operand in a comparison ⟹ false (SQL semantics).
                _ => false,
            }
        }
    }
}

/// Dispatch to the appropriate comparison function.
fn compare(left: &CellValue, op: &Comparator, right: &CellValue) -> bool {
    match op {
        Comparator::Eq => cell_eq(left, right),
        Comparator::Ne => !cell_eq(left, right),
        Comparator::Lt => numeric_compare(left, right)
            .map(|o| o.is_lt())
            .unwrap_or(false),
        Comparator::Lte => numeric_compare(left, right)
            .map(|o| o.is_le())
            .unwrap_or(false),
        Comparator::Gt => numeric_compare(left, right)
            .map(|o| o.is_gt())
            .unwrap_or(false),
        Comparator::Gte => numeric_compare(left, right)
            .map(|o| o.is_ge())
            .unwrap_or(false),
        Comparator::Like => like_match(left, right),
        Comparator::NotLike => !like_match(left, right),
        // IsNull / IsNotNull are handled before `compare` is called.
        Comparator::IsNull | Comparator::IsNotNull => {
            unreachable!("IsNull / IsNotNull must be handled in eval_predicate, not compare")
        }
    }
}

/// Strict value equality following SQL three-valued logic.
///
/// `NULL = NULL` is `false`.  `Integer(5) != Float(5.0)` (no implicit coercion).
fn cell_eq(a: &CellValue, b: &CellValue) -> bool {
    match (a, b) {
        (CellValue::Integer(x), CellValue::Integer(y)) => x == y,
        (CellValue::Float(x), CellValue::Float(y)) => x == y,
        (CellValue::Text(x), CellValue::Text(y)) => x == y,
        (CellValue::Blob(x), CellValue::Blob(y)) => x == y,
        // NULL is never equal to anything, including itself.
        _ => false,
    }
}

/// Cast a [`CellValue`] to `f64` for numeric ordering.
///
/// Returns `None` for `Text`, `Blob`, and `Null`.
fn cell_as_f64(v: &CellValue) -> Option<f64> {
    match v {
        CellValue::Integer(i) => Some(*i as f64),
        CellValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Determine the ordering between two [`CellValue`]s.
///
/// Priority:
/// 1. Both numeric (`Integer` or `Float`) → compare as `f64`.
/// 2. Both `Text` → lexicographic comparison.
/// 3. Otherwise → `None` (incomparable types → false for all relational ops).
fn numeric_compare(a: &CellValue, b: &CellValue) -> Option<std::cmp::Ordering> {
    // Try numeric promotion first (Integer ↔ Float cross-comparisons work here).
    if let (Some(x), Some(y)) = (cell_as_f64(a), cell_as_f64(b)) {
        return x.partial_cmp(&y);
    }
    // Fall back to lexicographic text comparison.
    if let (CellValue::Text(x), CellValue::Text(y)) = (a, b) {
        return Some(x.cmp(y));
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// SQL LIKE matching
// ─────────────────────────────────────────────────────────────────────────────

/// Evaluate SQL `LIKE` matching between `value` and `pattern`.
///
/// Both operands must be [`CellValue::Text`]; any other type returns `false`.
fn like_match(value: &CellValue, pattern: &CellValue) -> bool {
    match (value, pattern) {
        (CellValue::Text(v), CellValue::Text(p)) => like_matches_bytes(v.as_bytes(), p.as_bytes()),
        _ => false,
    }
}

/// Recursive byte-level LIKE pattern matching.
///
/// Wildcards:
/// * `%` — matches any sequence of zero or more bytes.
/// * `_` — matches exactly one byte.
/// * `\%`, `\_`, `\\` — literal `%`, `_`, `\`.
///
/// All other bytes match themselves literally.
fn like_matches_bytes(s: &[u8], pat: &[u8]) -> bool {
    match pat {
        // Empty pattern matches only empty string.
        [] => s.is_empty(),

        // `%` matches 0 .. s.len() characters — try each split point.
        [b'%', rest @ ..] => {
            // Optimisation: if the rest of the pattern contains no further
            // wildcards and no escapes, check via a simple suffix search.
            for i in 0..=s.len() {
                if like_matches_bytes(&s[i..], rest) {
                    return true;
                }
            }
            false
        }

        // `_` matches exactly one character.
        [b'_', rest @ ..] => !s.is_empty() && like_matches_bytes(&s[1..], rest),

        // Escaped literal: `\%`, `\_`, `\\`.
        [b'\\', c, rest @ ..] if matches!(c, b'%' | b'_' | b'\\') => {
            !s.is_empty() && s[0] == *c && like_matches_bytes(&s[1..], rest)
        }

        // Literal character.
        [c, rest @ ..] => !s.is_empty() && s[0] == *c && like_matches_bytes(&s[1..], rest),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── cell_eq ──────────────────────────────────────────────────────────────

    #[test]
    fn test_cell_eq_integer_same() {
        assert!(cell_eq(&CellValue::Integer(42), &CellValue::Integer(42)));
    }

    #[test]
    fn test_cell_eq_integer_different() {
        assert!(!cell_eq(&CellValue::Integer(42), &CellValue::Integer(43)));
    }

    #[test]
    fn test_cell_eq_null_null_is_false() {
        // SQL: NULL = NULL is FALSE
        assert!(!cell_eq(&CellValue::Null, &CellValue::Null));
    }

    #[test]
    fn test_cell_eq_integer_float_no_coercion() {
        // Integer(5) != Float(5.0) — strict type matching
        assert!(!cell_eq(&CellValue::Integer(5), &CellValue::Float(5.0)));
    }

    // ── numeric_compare ──────────────────────────────────────────────────────

    #[test]
    fn test_numeric_compare_integer_cross_float() {
        // Integer(3) < Float(4.5)
        let ord = numeric_compare(&CellValue::Integer(3), &CellValue::Float(4.5));
        assert_eq!(ord, Some(std::cmp::Ordering::Less));
    }

    #[test]
    fn test_numeric_compare_text_lexicographic() {
        let ord = numeric_compare(
            &CellValue::Text("abc".into()),
            &CellValue::Text("abd".into()),
        );
        assert_eq!(ord, Some(std::cmp::Ordering::Less));
    }

    #[test]
    fn test_numeric_compare_incompatible_types_is_none() {
        let ord = numeric_compare(&CellValue::Text("abc".into()), &CellValue::Integer(1));
        assert_eq!(ord, None);
    }

    // ── LIKE matching ─────────────────────────────────────────────────────────

    #[test]
    fn test_like_percent_matches_empty() {
        assert!(like_matches_bytes(b"", b"%"));
    }

    #[test]
    fn test_like_percent_matches_any() {
        assert!(like_matches_bytes(b"hello world", b"hello%"));
        assert!(like_matches_bytes(b"hello world", b"%world"));
        assert!(like_matches_bytes(b"hello world", b"%llo%"));
    }

    #[test]
    fn test_like_underscore_matches_one_char() {
        assert!(like_matches_bytes(b"abc", b"a_c"));
        assert!(!like_matches_bytes(b"ac", b"a_c"));
        assert!(!like_matches_bytes(b"abbc", b"a_c"));
    }

    #[test]
    fn test_like_escape_literal_percent() {
        // \% matches literal %
        assert!(like_matches_bytes(b"50%", b"50\\%"));
        assert!(!like_matches_bytes(b"50x", b"50\\%"));
    }

    #[test]
    fn test_like_escape_literal_underscore() {
        assert!(like_matches_bytes(b"a_b", b"a\\_b"));
        assert!(!like_matches_bytes(b"axb", b"a\\_b"));
    }

    #[test]
    fn test_like_exact_match() {
        assert!(like_matches_bytes(b"exact", b"exact"));
        assert!(!like_matches_bytes(b"exact", b"eXact"));
    }

    #[test]
    fn test_like_non_text_returns_false() {
        assert!(!like_match(
            &CellValue::Integer(42),
            &CellValue::Text("%".into())
        ));
    }
}
