//! SQL `WHERE` fragment → `AttributeFilter` lowering.
//!
//! Parses `SELECT * FROM t WHERE {expr}` with `sqlparser`'s
//! `GenericDialect` and lowers the selection into
//! `oxigeo-geoparquet` [`AttributeFilter`]s: AND chains flatten, `=`
//! becomes `Eq`, comparison operators become `Cmp`, `BETWEEN` becomes
//! `Range`, `IN` becomes `In`. Unsupported constructs (OR, NOT,
//! `IS NULL`, functions, column-column comparisons, subqueries) are
//! rejected with typed errors naming the construct.
//!
//! # Supported grammar
//!
//! | Expression                   | Lowered to                              |
//! |------------------------------|-----------------------------------------|
//! | `col = lit` / `lit = col`    | [`AttributeFilter::Eq`]                 |
//! | `col <> lit`, `col != lit`   | [`AttributeFilter::Cmp`] ([`CmpOp::NotEq`]) |
//! | `col > lit`                  | [`AttributeFilter::Cmp`] ([`CmpOp::Gt`]) |
//! | `col >= lit`                 | [`AttributeFilter::Cmp`] ([`CmpOp::Ge`]) |
//! | `col < lit`                  | [`AttributeFilter::Cmp`] ([`CmpOp::Lt`]) |
//! | `col <= lit`                 | [`AttributeFilter::Cmp`] ([`CmpOp::Le`]) |
//! | `col BETWEEN lo AND hi`      | [`AttributeFilter::Range`]              |
//! | `col IN (lit, ...)`          | [`AttributeFilter::In`]                 |
//! | `pred AND pred AND ...`      | flattened conjunction (`Vec` of filters) |
//!
//! Reversed operand order (`1000 < area_in_meters`) is normalized to the
//! column-on-the-left form with the operator flipped (`area_in_meters > 1000`).
//!
//! # Literal mapping
//!
//! - integral numbers → [`ScalarValue::Int64`] (falling back to
//!   [`ScalarValue::Float64`] when the value exceeds the `i64` range)
//! - decimal / exponent numbers → [`ScalarValue::Float64`]
//! - single-quoted strings → [`ScalarValue::Utf8`] (double quotes denote
//!   *identifiers* in SQL, not strings)
//! - `TRUE` / `FALSE` → [`ScalarValue::Bool`] — note that the predicate
//!   compiler in `oxigeo-geoparquet` currently evaluates only
//!   `Int64` / `Float64` / `Utf8` scalars, so boolean filters surface a
//!   type-mismatch error at execution time
//! - unary `-` / `+` on numeric literals is folded into the value
//!
//! Implemented by WP C3 (GeoParquet Live lane); stub created by WP W0.

use oxigeo_geoparquet::{AttributeFilter, CmpOp, ScalarValue};
use sqlparser::ast::{BinaryOperator, Expr, GroupByExpr, SetExpr, Statement, UnaryOperator, Value};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use thiserror::Error;

// ── Errors ──────────────────────────────────────────────────────────────────

/// Typed rejection reasons for a filter expression.
///
/// Every unsupported SQL construct is reported with a variant (and error
/// text) that names the construct, so the demo UI can show an actionable
/// message instead of a generic parse failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FilterExprError {
    /// The expression was empty or all whitespace.
    #[error("filter expression is empty")]
    Empty,
    /// `sqlparser` could not parse the expression at all.
    #[error("filter expression parse error: {0}")]
    Parse(String),
    /// The expression smuggled in additional SQL statements (e.g. via `;`).
    #[error("filter expression must be a single expression, not multiple SQL statements")]
    MultipleStatements,
    /// `OR` disjunctions cannot be pushed down.
    #[error("unsupported construct OR: only AND-combined predicates can be pushed down")]
    Or,
    /// `NOT` / `NOT BETWEEN` / `NOT IN` negations cannot be pushed down.
    #[error("unsupported construct {0}: negation cannot be pushed down")]
    Not(String),
    /// `IS NULL` / `IS NOT NULL` tests cannot be pushed down.
    #[error("unsupported construct {0}: null tests cannot be pushed down")]
    NullTest(String),
    /// Function calls (e.g. `UPPER(name)`) cannot be pushed down.
    #[error("unsupported construct: function call `{0}`")]
    Function(String),
    /// Column-to-column comparisons cannot be pushed down.
    #[error(
        "unsupported construct: column-to-column comparison `{left}` vs `{right}` \
         (string literals need single quotes: 'value'; double quotes denote identifiers)"
    )]
    ColumnColumn {
        /// Left-hand column name.
        left: String,
        /// Right-hand column name.
        right: String,
    },
    /// Subqueries (`IN (SELECT ...)`, `EXISTS (...)`, scalar subqueries).
    #[error("unsupported construct: subquery")]
    Subquery,
    /// A comparison/predicate operator outside the supported set.
    #[error(
        "unsupported operator `{0}` \
         (supported: =, <>, !=, >, >=, <, <=, BETWEEN ... AND ..., IN (...), AND)"
    )]
    UnsupportedOperator(String),
    /// An operand that is neither a bare column name nor a literal.
    #[error("unsupported operand `{0}`: each comparison must be between a column and a literal")]
    UnsupportedOperand(String),
    /// A literal kind outside numbers / single-quoted strings / booleans.
    #[error("unsupported literal `{0}`")]
    UnsupportedLiteral(String),
    /// A `NULL` literal used as a comparison value.
    #[error("NULL is not a comparable literal (and IS NULL tests cannot be pushed down)")]
    NullLiteral,
    /// A comparison where neither side is a column.
    #[error("comparison `{0}` has no column operand: one side must be a column name")]
    NoColumn(String),
    /// A bare column reference used as a predicate (`WHERE active`).
    #[error("bare column `{0}` is not a predicate; write an explicit comparison")]
    BareColumn(String),
    /// A bare literal used as a predicate (`WHERE 1`).
    #[error("`{0}` is not a filter predicate; expected e.g. `column = value`")]
    NotAPredicate(String),
    /// A qualified (dotted) column name; only bare top-level columns work.
    #[error("qualified column `{0}` is not supported; use a bare top-level column name")]
    QualifiedColumn(String),
    /// A numeric literal that fits neither `i64` nor `f64`.
    #[error("invalid numeric literal `{0}`")]
    InvalidNumber(String),
    /// A trailing SQL clause smuggled after the expression.
    #[error("unsupported clause {0} in filter expression")]
    UnsupportedClause(String),
    /// Any other SQL construct.
    #[error("unsupported construct `{0}`")]
    Unsupported(String),
}

// ── Entry point ─────────────────────────────────────────────────────────────

/// Parses a SQL `WHERE` fragment into pushdown-ready [`AttributeFilter`]s.
///
/// The fragment is embedded into `SELECT * FROM t WHERE {expr}` and parsed
/// with the `GenericDialect`; the resulting selection is lowered into a
/// conjunction (`Vec`) of single-column filters. See the module docs for
/// the supported grammar and the literal mapping.
///
/// # Errors
///
/// Returns a [`FilterExprError`] naming the offending construct when the
/// expression cannot be represented as an AND-chain of `Eq` / `Cmp` /
/// `Range` / `In` filters.
// Consumed by the wasm-only `session` module (WP C4); until that consumer
// lands, the inline tests are the only native callers, so this root is
// explicitly kept alive for rustc's dead-code analysis.
#[allow(dead_code)]
pub fn parse_filter_expr(input: &str) -> Result<Vec<AttributeFilter>, FilterExprError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(FilterExprError::Empty);
    }
    let sql = format!("SELECT * FROM t WHERE {trimmed}");
    let mut statements = Parser::parse_sql(&GenericDialect {}, &sql)
        .map_err(|e| FilterExprError::Parse(e.to_string()))?;
    if statements.len() > 1 {
        return Err(FilterExprError::MultipleStatements);
    }
    let Some(statement) = statements.pop() else {
        return Err(FilterExprError::Parse(
            "expression produced no SQL statement".to_string(),
        ));
    };
    let selection = extract_selection(statement)?;
    let mut filters = Vec::new();
    lower_conjunction(&selection, &mut filters)?;
    Ok(filters)
}

// ── Statement-level extraction ──────────────────────────────────────────────

/// Unwraps the parsed statement down to the `WHERE` selection expression,
/// rejecting any smuggled trailing clauses (`ORDER BY`, `LIMIT`, `UNION`,
/// `GROUP BY`, `HAVING`, ...).
fn extract_selection(statement: Statement) -> Result<Expr, FilterExprError> {
    let Statement::Query(query) = statement else {
        return Err(FilterExprError::Unsupported(
            "non-SELECT statement".to_string(),
        ));
    };
    if query.with.is_some() {
        return Err(FilterExprError::UnsupportedClause("WITH".to_string()));
    }
    if query.order_by.is_some() {
        return Err(FilterExprError::UnsupportedClause("ORDER BY".to_string()));
    }
    if query.limit_clause.is_some() {
        return Err(FilterExprError::UnsupportedClause("LIMIT".to_string()));
    }
    if query.fetch.is_some() {
        return Err(FilterExprError::UnsupportedClause("FETCH".to_string()));
    }
    let select = match *query.body {
        SetExpr::Select(select) => *select,
        SetExpr::SetOperation { op, .. } => {
            return Err(FilterExprError::UnsupportedClause(op.to_string()));
        }
        other => {
            return Err(FilterExprError::Unsupported(other.to_string()));
        }
    };
    if select.having.is_some() {
        return Err(FilterExprError::UnsupportedClause("HAVING".to_string()));
    }
    match &select.group_by {
        GroupByExpr::Expressions(exprs, modifiers) if exprs.is_empty() && modifiers.is_empty() => {}
        _ => {
            return Err(FilterExprError::UnsupportedClause("GROUP BY".to_string()));
        }
    }
    select
        .selection
        .ok_or_else(|| FilterExprError::Parse("missing WHERE clause".to_string()))
}

// ── Conjunction flattening ──────────────────────────────────────────────────

/// Recursively flattens `a AND b AND c` (with arbitrary parenthesization)
/// into individual predicates, appended to `out` in textual order.
fn lower_conjunction(expr: &Expr, out: &mut Vec<AttributeFilter>) -> Result<(), FilterExprError> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            lower_conjunction(left, out)?;
            lower_conjunction(right, out)
        }
        Expr::Nested(inner) => lower_conjunction(inner, out),
        other => {
            out.push(lower_predicate(other)?);
            Ok(())
        }
    }
}

/// Lowers a single (non-AND) predicate into one [`AttributeFilter`].
fn lower_predicate(expr: &Expr) -> Result<AttributeFilter, FilterExprError> {
    match expr {
        Expr::BinaryOp { left, op, right } => lower_binary(left, op, right),
        Expr::Between {
            expr: operand,
            negated,
            low,
            high,
        } => {
            if *negated {
                return Err(FilterExprError::Not("NOT BETWEEN".to_string()));
            }
            let col = require_column(operand, "BETWEEN")?;
            let lo = require_literal(low)?;
            let hi = require_literal(high)?;
            Ok(AttributeFilter::Range { col, lo, hi })
        }
        Expr::InList {
            expr: operand,
            list,
            negated,
        } => {
            if *negated {
                return Err(FilterExprError::Not("NOT IN".to_string()));
            }
            let col = require_column(operand, "IN")?;
            let values = list
                .iter()
                .map(require_literal)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(AttributeFilter::In { col, values })
        }
        Expr::InSubquery { .. } | Expr::Subquery(_) | Expr::Exists { .. } => {
            Err(FilterExprError::Subquery)
        }
        Expr::IsNull(_) => Err(FilterExprError::NullTest("IS NULL".to_string())),
        Expr::IsNotNull(_) => Err(FilterExprError::NullTest("IS NOT NULL".to_string())),
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            ..
        } => Err(FilterExprError::Not("NOT".to_string())),
        Expr::Like { negated, .. } => Err(FilterExprError::UnsupportedOperator(
            if *negated { "NOT LIKE" } else { "LIKE" }.to_string(),
        )),
        Expr::ILike { negated, .. } => Err(FilterExprError::UnsupportedOperator(
            if *negated { "NOT ILIKE" } else { "ILIKE" }.to_string(),
        )),
        Expr::SimilarTo { .. } => Err(FilterExprError::UnsupportedOperator(
            "SIMILAR TO".to_string(),
        )),
        Expr::Function(func) => Err(FilterExprError::Function(func.name.to_string())),
        Expr::Identifier(ident) => Err(FilterExprError::BareColumn(ident.value.clone())),
        Expr::CompoundIdentifier(parts) => {
            Err(FilterExprError::QualifiedColumn(join_idents(parts)))
        }
        Expr::Value(_) => Err(FilterExprError::NotAPredicate(expr.to_string())),
        other => Err(FilterExprError::Unsupported(other.to_string())),
    }
}

// ── Binary comparison lowering ──────────────────────────────────────────────

/// Comparison operators normalized to the column-on-the-left orientation.
#[derive(Debug, Clone, Copy)]
enum NormOp {
    /// `=`
    Eq,
    /// `<>` / `!=`
    NotEq,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `<=`
    Le,
}

impl NormOp {
    /// Maps a supported SQL binary operator; `None` for everything else.
    fn from_sql(op: &BinaryOperator) -> Option<Self> {
        match op {
            BinaryOperator::Eq => Some(Self::Eq),
            BinaryOperator::NotEq => Some(Self::NotEq),
            BinaryOperator::Gt => Some(Self::Gt),
            BinaryOperator::GtEq => Some(Self::Ge),
            BinaryOperator::Lt => Some(Self::Lt),
            BinaryOperator::LtEq => Some(Self::Le),
            _ => None,
        }
    }

    /// Mirror for reversed operand order: `lit < col` ⇒ `col > lit`.
    fn flip(self) -> Self {
        match self {
            Self::Eq => Self::Eq,
            Self::NotEq => Self::NotEq,
            Self::Gt => Self::Lt,
            Self::Ge => Self::Le,
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
        }
    }

    /// Builds the final filter with the column on the left.
    fn into_filter(self, col: String, value: ScalarValue) -> AttributeFilter {
        match self {
            Self::Eq => AttributeFilter::Eq { col, value },
            Self::NotEq => AttributeFilter::Cmp {
                col,
                op: CmpOp::NotEq,
                value,
            },
            Self::Gt => AttributeFilter::Cmp {
                col,
                op: CmpOp::Gt,
                value,
            },
            Self::Ge => AttributeFilter::Cmp {
                col,
                op: CmpOp::Ge,
                value,
            },
            Self::Lt => AttributeFilter::Cmp {
                col,
                op: CmpOp::Lt,
                value,
            },
            Self::Le => AttributeFilter::Cmp {
                col,
                op: CmpOp::Le,
                value,
            },
        }
    }
}

/// Lowers `left <op> right`, normalizing reversed operand order.
fn lower_binary(
    left: &Expr,
    op: &BinaryOperator,
    right: &Expr,
) -> Result<AttributeFilter, FilterExprError> {
    let norm = match op {
        BinaryOperator::Or => return Err(FilterExprError::Or),
        other => NormOp::from_sql(other)
            .ok_or_else(|| FilterExprError::UnsupportedOperator(other.to_string()))?,
    };
    match (classify(left)?, classify(right)?) {
        (Operand::Column(col), Operand::Literal(value)) => Ok(norm.into_filter(col, value)),
        (Operand::Literal(value), Operand::Column(col)) => Ok(norm.flip().into_filter(col, value)),
        (Operand::Column(left_col), Operand::Column(right_col)) => {
            Err(FilterExprError::ColumnColumn {
                left: left_col,
                right: right_col,
            })
        }
        (Operand::Literal(_), Operand::Literal(_)) => {
            Err(FilterExprError::NoColumn(op.to_string()))
        }
        (Operand::Other(other), _) | (_, Operand::Other(other)) => {
            Err(FilterExprError::UnsupportedOperand(other))
        }
    }
}

// ── Operand classification ──────────────────────────────────────────────────

/// One side of a comparison after classification.
enum Operand {
    /// A bare column name.
    Column(String),
    /// A literal scalar value.
    Literal(ScalarValue),
    /// Anything else, rendered back to SQL text for the error message.
    Other(String),
}

/// Classifies an expression as a column, a literal, or something else,
/// rejecting constructs (functions, subqueries, NOT, qualified names,
/// NULL literals) with their typed errors.
fn classify(expr: &Expr) -> Result<Operand, FilterExprError> {
    match expr {
        Expr::Nested(inner) => classify(inner),
        Expr::Identifier(ident) => Ok(Operand::Column(ident.value.clone())),
        Expr::CompoundIdentifier(parts) => {
            Err(FilterExprError::QualifiedColumn(join_idents(parts)))
        }
        Expr::Value(value) => Ok(Operand::Literal(lower_value(&value.value)?)),
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr: inner,
        } => signed_literal(inner, true),
        Expr::UnaryOp {
            op: UnaryOperator::Plus,
            expr: inner,
        } => signed_literal(inner, false),
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            ..
        } => Err(FilterExprError::Not("NOT".to_string())),
        Expr::Function(func) => Err(FilterExprError::Function(func.name.to_string())),
        Expr::Subquery(_) => Err(FilterExprError::Subquery),
        other => Ok(Operand::Other(other.to_string())),
    }
}

/// Applies a unary `-` / `+` sign to a numeric literal operand.
fn signed_literal(inner: &Expr, negate: bool) -> Result<Operand, FilterExprError> {
    let rendered = || format!("{}{inner}", if negate { "-" } else { "+" });
    match classify(inner)? {
        Operand::Literal(ScalarValue::Int64(value)) => {
            let signed = if negate {
                value
                    .checked_neg()
                    .ok_or_else(|| FilterExprError::InvalidNumber(rendered()))?
            } else {
                value
            };
            Ok(Operand::Literal(ScalarValue::Int64(signed)))
        }
        Operand::Literal(ScalarValue::Float64(value)) => {
            Ok(Operand::Literal(ScalarValue::Float64(if negate {
                -value
            } else {
                value
            })))
        }
        _ => Err(FilterExprError::UnsupportedOperand(rendered())),
    }
}

/// Requires a bare column name (for `BETWEEN` / `IN` subjects).
fn require_column(expr: &Expr, construct: &str) -> Result<String, FilterExprError> {
    match classify(expr)? {
        Operand::Column(col) => Ok(col),
        Operand::Literal(_) => Err(FilterExprError::NoColumn(construct.to_string())),
        Operand::Other(other) => Err(FilterExprError::UnsupportedOperand(other)),
    }
}

/// Requires a literal scalar (for `BETWEEN` bounds / `IN` list members).
fn require_literal(expr: &Expr) -> Result<ScalarValue, FilterExprError> {
    match classify(expr)? {
        Operand::Literal(value) => Ok(value),
        Operand::Column(col) => Err(FilterExprError::UnsupportedOperand(col)),
        Operand::Other(other) => Err(FilterExprError::UnsupportedOperand(other)),
    }
}

// ── Literal mapping ─────────────────────────────────────────────────────────

/// Maps a SQL literal value to a [`ScalarValue`].
fn lower_value(value: &Value) -> Result<ScalarValue, FilterExprError> {
    match value {
        Value::Number(number, _) => parse_number(number),
        Value::SingleQuotedString(text) | Value::DoubleQuotedString(text) => {
            Ok(ScalarValue::Utf8(text.clone()))
        }
        Value::Boolean(flag) => Ok(ScalarValue::Bool(*flag)),
        Value::Null => Err(FilterExprError::NullLiteral),
        other => Err(FilterExprError::UnsupportedLiteral(other.to_string())),
    }
}

/// Parses a numeric literal: integral text → `Int64`, otherwise (or on
/// `i64` overflow) → `Float64`.
fn parse_number(number: &str) -> Result<ScalarValue, FilterExprError> {
    let looks_integral = !number.contains(['.', 'e', 'E']);
    if looks_integral && let Ok(int) = number.parse::<i64>() {
        return Ok(ScalarValue::Int64(int));
    }
    number
        .parse::<f64>()
        .map(ScalarValue::Float64)
        .map_err(|_| FilterExprError::InvalidNumber(number.to_string()))
}

/// Joins compound-identifier parts back into dotted text for error messages.
fn join_idents(parts: &[sqlparser::ast::Ident]) -> String {
    parts
        .iter()
        .map(|part| part.value.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Parses `expr` expecting exactly one filter.
    fn one(expr: &str) -> AttributeFilter {
        let mut filters = parse_filter_expr(expr).unwrap();
        assert_eq!(filters.len(), 1, "expected exactly one filter for `{expr}`");
        filters.pop().unwrap()
    }

    /// Parses `expr` expecting a rejection.
    fn err(expr: &str) -> FilterExprError {
        parse_filter_expr(expr).unwrap_err()
    }

    /// Asserts an `Eq` filter's column and value.
    fn assert_eq_filter(filter: &AttributeFilter, col: &str, value: &ScalarValue) {
        match filter {
            AttributeFilter::Eq {
                col: actual_col,
                value: actual_value,
            } => {
                assert_eq!(actual_col, col);
                assert_eq!(actual_value, value);
            }
            other => panic!("expected Eq, got {other:?}"),
        }
    }

    /// Asserts a `Cmp` filter's column, operator, and value.
    fn assert_cmp_filter(filter: &AttributeFilter, col: &str, op: CmpOp, value: &ScalarValue) {
        match filter {
            AttributeFilter::Cmp {
                col: actual_col,
                op: actual_op,
                value: actual_value,
            } => {
                assert_eq!(actual_col, col);
                assert_eq!(*actual_op, op);
                assert_eq!(actual_value, value);
            }
            other => panic!("expected Cmp, got {other:?}"),
        }
    }

    // ── every operator ──────────────────────────────────────────────────

    #[test]
    fn eq_string_literal() {
        let filter = one("bf_source = 'osm'");
        assert_eq_filter(&filter, "bf_source", &ScalarValue::Utf8("osm".to_string()));
    }

    #[test]
    fn eq_integer_literal() {
        let filter = one("boundary_id = 42");
        assert_eq_filter(&filter, "boundary_id", &ScalarValue::Int64(42));
    }

    #[test]
    fn eq_float_literal() {
        let filter = one("confidence = 0.75");
        assert_eq_filter(&filter, "confidence", &ScalarValue::Float64(0.75));
    }

    #[test]
    fn eq_bool_true_and_false() {
        let filter = one("active = TRUE");
        assert_eq_filter(&filter, "active", &ScalarValue::Bool(true));
        let filter = one("active = false");
        assert_eq_filter(&filter, "active", &ScalarValue::Bool(false));
    }

    #[test]
    fn noteq_angle_brackets() {
        let filter = one("a <> 5");
        assert_cmp_filter(&filter, "a", CmpOp::NotEq, &ScalarValue::Int64(5));
    }

    #[test]
    fn noteq_bang_equals() {
        let filter = one("a != 5");
        assert_cmp_filter(&filter, "a", CmpOp::NotEq, &ScalarValue::Int64(5));
    }

    #[test]
    fn gt_integer() {
        let filter = one("area_in_meters > 1000");
        assert_cmp_filter(
            &filter,
            "area_in_meters",
            CmpOp::Gt,
            &ScalarValue::Int64(1000),
        );
    }

    #[test]
    fn ge_integer() {
        let filter = one("area_in_meters >= 1000");
        assert_cmp_filter(
            &filter,
            "area_in_meters",
            CmpOp::Ge,
            &ScalarValue::Int64(1000),
        );
    }

    #[test]
    fn lt_float() {
        let filter = one("confidence < 0.9");
        assert_cmp_filter(&filter, "confidence", CmpOp::Lt, &ScalarValue::Float64(0.9));
    }

    #[test]
    fn le_float() {
        let filter = one("confidence <= 0.9");
        assert_cmp_filter(&filter, "confidence", CmpOp::Le, &ScalarValue::Float64(0.9));
    }

    #[test]
    fn between_floats_lowers_to_range() {
        match one("confidence BETWEEN 0.5 AND 0.9") {
            AttributeFilter::Range { col, lo, hi } => {
                assert_eq!(col, "confidence");
                assert_eq!(lo, ScalarValue::Float64(0.5));
                assert_eq!(hi, ScalarValue::Float64(0.9));
            }
            other => panic!("expected Range, got {other:?}"),
        }
    }

    #[test]
    fn between_integers_lowers_to_range() {
        match one("area_in_meters BETWEEN 100 AND 5000") {
            AttributeFilter::Range { col, lo, hi } => {
                assert_eq!(col, "area_in_meters");
                assert_eq!(lo, ScalarValue::Int64(100));
                assert_eq!(hi, ScalarValue::Int64(5000));
            }
            other => panic!("expected Range, got {other:?}"),
        }
    }

    #[test]
    fn in_list_of_strings() {
        match one("bf_source IN ('osm', 'google', 'microsoft')") {
            AttributeFilter::In { col, values } => {
                assert_eq!(col, "bf_source");
                assert_eq!(
                    values,
                    vec![
                        ScalarValue::Utf8("osm".to_string()),
                        ScalarValue::Utf8("google".to_string()),
                        ScalarValue::Utf8("microsoft".to_string()),
                    ]
                );
            }
            other => panic!("expected In, got {other:?}"),
        }
    }

    #[test]
    fn in_list_of_numbers_keeps_each_literal_type() {
        match one("x IN (1, 2.5, -3)") {
            AttributeFilter::In { col, values } => {
                assert_eq!(col, "x");
                assert_eq!(
                    values,
                    vec![
                        ScalarValue::Int64(1),
                        ScalarValue::Float64(2.5),
                        ScalarValue::Int64(-3),
                    ]
                );
            }
            other => panic!("expected In, got {other:?}"),
        }
    }

    // ── AND chains ──────────────────────────────────────────────────────

    #[test]
    fn and_chain_of_two_preserves_order() {
        let filters = parse_filter_expr("a > 1 AND b = 'x'").unwrap();
        assert_eq!(filters.len(), 2);
        assert_cmp_filter(&filters[0], "a", CmpOp::Gt, &ScalarValue::Int64(1));
        assert_eq_filter(&filters[1], "b", &ScalarValue::Utf8("x".to_string()));
    }

    #[test]
    fn and_chain_of_three_mixed_predicates() {
        let filters = parse_filter_expr("a > 1 AND b BETWEEN 2 AND 3 AND c IN (1, 2)").unwrap();
        assert_eq!(filters.len(), 3);
        assert_cmp_filter(&filters[0], "a", CmpOp::Gt, &ScalarValue::Int64(1));
        match &filters[1] {
            AttributeFilter::Range { col, lo, hi } => {
                assert_eq!(col, "b");
                assert_eq!(*lo, ScalarValue::Int64(2));
                assert_eq!(*hi, ScalarValue::Int64(3));
            }
            other => panic!("expected Range, got {other:?}"),
        }
        match &filters[2] {
            AttributeFilter::In { col, values } => {
                assert_eq!(col, "c");
                assert_eq!(values.len(), 2);
            }
            other => panic!("expected In, got {other:?}"),
        }
    }

    #[test]
    fn and_chain_with_arbitrary_parentheses_flattens() {
        let filters = parse_filter_expr("(a = 1) AND ((b = 2) AND (c = 3))").unwrap();
        assert_eq!(filters.len(), 3);
        assert_eq_filter(&filters[0], "a", &ScalarValue::Int64(1));
        assert_eq_filter(&filters[1], "b", &ScalarValue::Int64(2));
        assert_eq_filter(&filters[2], "c", &ScalarValue::Int64(3));
    }

    #[test]
    fn lowercase_keywords_are_accepted() {
        let filters = parse_filter_expr("a between 1 and 2 and b in (3) and c = true").unwrap();
        assert_eq!(filters.len(), 3);
    }

    #[test]
    fn doubly_nested_single_predicate() {
        let filter = one("((area_in_meters >= 10))");
        assert_cmp_filter(
            &filter,
            "area_in_meters",
            CmpOp::Ge,
            &ScalarValue::Int64(10),
        );
    }

    // ── reversed operand normalization ──────────────────────────────────

    #[test]
    fn reversed_eq_normalizes_column_left() {
        let filter = one("'osm' = bf_source");
        assert_eq_filter(&filter, "bf_source", &ScalarValue::Utf8("osm".to_string()));
    }

    #[test]
    fn reversed_noteq_keeps_operator() {
        let filter = one("5 <> a");
        assert_cmp_filter(&filter, "a", CmpOp::NotEq, &ScalarValue::Int64(5));
    }

    #[test]
    fn reversed_lt_becomes_gt() {
        let filter = one("1000 < area_in_meters");
        assert_cmp_filter(
            &filter,
            "area_in_meters",
            CmpOp::Gt,
            &ScalarValue::Int64(1000),
        );
    }

    #[test]
    fn reversed_le_becomes_ge() {
        let filter = one("1000 <= area_in_meters");
        assert_cmp_filter(
            &filter,
            "area_in_meters",
            CmpOp::Ge,
            &ScalarValue::Int64(1000),
        );
    }

    #[test]
    fn reversed_gt_becomes_lt() {
        let filter = one("0.9 > confidence");
        assert_cmp_filter(&filter, "confidence", CmpOp::Lt, &ScalarValue::Float64(0.9));
    }

    #[test]
    fn reversed_ge_becomes_le() {
        let filter = one("0.9 >= confidence");
        assert_cmp_filter(&filter, "confidence", CmpOp::Le, &ScalarValue::Float64(0.9));
    }

    // ── literal mapping ─────────────────────────────────────────────────

    #[test]
    fn i64_max_stays_int64() {
        let filter = one("a = 9223372036854775807");
        assert_eq_filter(&filter, "a", &ScalarValue::Int64(i64::MAX));
    }

    #[test]
    fn integer_beyond_i64_falls_back_to_float64() {
        let filter = one("a > 9223372036854775808");
        assert_cmp_filter(
            &filter,
            "a",
            CmpOp::Gt,
            &ScalarValue::Float64(9.223_372_036_854_776e18),
        );
    }

    #[test]
    fn exponent_notation_is_float64() {
        let filter = one("a = 1.5e3");
        assert_eq_filter(&filter, "a", &ScalarValue::Float64(1500.0));
        let filter = one("a = 2E2");
        assert_eq_filter(&filter, "a", &ScalarValue::Float64(200.0));
    }

    #[test]
    fn negative_integer_literal() {
        let filter = one("a > -5");
        assert_cmp_filter(&filter, "a", CmpOp::Gt, &ScalarValue::Int64(-5));
    }

    #[test]
    fn negative_float_literal() {
        let filter = one("a < -0.25");
        assert_cmp_filter(&filter, "a", CmpOp::Lt, &ScalarValue::Float64(-0.25));
    }

    #[test]
    fn reversed_negative_literal_normalizes() {
        let filter = one("-5 < a");
        assert_cmp_filter(&filter, "a", CmpOp::Gt, &ScalarValue::Int64(-5));
    }

    #[test]
    fn plus_prefixed_literal_is_identity() {
        let filter = one("a > +5");
        assert_cmp_filter(&filter, "a", CmpOp::Gt, &ScalarValue::Int64(5));
    }

    #[test]
    fn negative_between_bounds() {
        match one("a BETWEEN -10 AND -1") {
            AttributeFilter::Range { col, lo, hi } => {
                assert_eq!(col, "a");
                assert_eq!(lo, ScalarValue::Int64(-10));
                assert_eq!(hi, ScalarValue::Int64(-1));
            }
            other => panic!("expected Range, got {other:?}"),
        }
    }

    #[test]
    fn escaped_single_quote_in_string() {
        let filter = one("name = 'O''Hara'");
        assert_eq_filter(&filter, "name", &ScalarValue::Utf8("O'Hara".to_string()));
    }

    #[test]
    fn empty_string_literal() {
        let filter = one("name = ''");
        assert_eq_filter(&filter, "name", &ScalarValue::Utf8(String::new()));
    }

    // ── rejection: OR / NOT ─────────────────────────────────────────────

    #[test]
    fn reject_top_level_or() {
        assert_eq!(err("a = 1 OR b = 2"), FilterExprError::Or);
    }

    #[test]
    fn reject_nested_or_inside_and() {
        assert_eq!(err("a = 1 AND (b = 2 OR c = 3)"), FilterExprError::Or);
    }

    #[test]
    fn reject_not() {
        assert_eq!(err("NOT a = 1"), FilterExprError::Not("NOT".to_string()));
    }

    #[test]
    fn reject_not_between() {
        assert_eq!(
            err("a NOT BETWEEN 1 AND 2"),
            FilterExprError::Not("NOT BETWEEN".to_string())
        );
    }

    #[test]
    fn reject_not_in() {
        assert_eq!(
            err("a NOT IN (1, 2)"),
            FilterExprError::Not("NOT IN".to_string())
        );
    }

    // ── rejection: null tests / NULL literal ────────────────────────────

    #[test]
    fn reject_is_null() {
        assert_eq!(
            err("a IS NULL"),
            FilterExprError::NullTest("IS NULL".to_string())
        );
    }

    #[test]
    fn reject_is_not_null() {
        assert_eq!(
            err("a IS NOT NULL"),
            FilterExprError::NullTest("IS NOT NULL".to_string())
        );
    }

    #[test]
    fn reject_null_literal_in_comparison() {
        assert_eq!(err("a = NULL"), FilterExprError::NullLiteral);
    }

    // ── rejection: functions ────────────────────────────────────────────

    #[test]
    fn reject_function_on_left() {
        assert_eq!(
            err("UPPER(name) = 'X'"),
            FilterExprError::Function("UPPER".to_string())
        );
    }

    #[test]
    fn reject_function_on_right() {
        assert_eq!(
            err("name = LOWER('X')"),
            FilterExprError::Function("LOWER".to_string())
        );
    }

    #[test]
    fn reject_bare_function_predicate() {
        assert_eq!(
            err("STARTS_WITH(name, 'a')"),
            FilterExprError::Function("STARTS_WITH".to_string())
        );
    }

    // ── rejection: column-column / operand shapes ───────────────────────

    #[test]
    fn reject_column_column_comparison() {
        assert_eq!(
            err("a = b"),
            FilterExprError::ColumnColumn {
                left: "a".to_string(),
                right: "b".to_string(),
            }
        );
    }

    #[test]
    fn double_quoted_string_is_identifier_not_literal() {
        // In SQL, "osm" is a quoted *identifier*; the error text points the
        // user at single quotes.
        let error = err("bf_source = \"osm\"");
        assert_eq!(
            error,
            FilterExprError::ColumnColumn {
                left: "bf_source".to_string(),
                right: "osm".to_string(),
            }
        );
        assert!(error.to_string().contains("single quotes"), "{error}");
    }

    #[test]
    fn reject_literal_literal_comparison() {
        assert_eq!(err("1 = 2"), FilterExprError::NoColumn("=".to_string()));
    }

    #[test]
    fn reject_arithmetic_operand() {
        assert_eq!(
            err("a > b + 1"),
            FilterExprError::UnsupportedOperand("b + 1".to_string())
        );
    }

    #[test]
    fn reject_modulo_operand() {
        assert_eq!(
            err("a % 2 = 0"),
            FilterExprError::UnsupportedOperand("a % 2".to_string())
        );
    }

    #[test]
    fn reject_negated_string_literal() {
        assert_eq!(
            err("a > -'x'"),
            FilterExprError::UnsupportedOperand("-'x'".to_string())
        );
    }

    #[test]
    fn reject_column_bound_in_between() {
        assert_eq!(
            err("a BETWEEN b AND 10"),
            FilterExprError::UnsupportedOperand("b".to_string())
        );
    }

    #[test]
    fn reject_literal_subject_in_between() {
        assert_eq!(
            err("5 BETWEEN 1 AND 10"),
            FilterExprError::NoColumn("BETWEEN".to_string())
        );
    }

    #[test]
    fn reject_column_member_in_in_list() {
        assert_eq!(
            err("a IN (1, b)"),
            FilterExprError::UnsupportedOperand("b".to_string())
        );
    }

    #[test]
    fn reject_qualified_column() {
        assert_eq!(
            err("t.a = 1"),
            FilterExprError::QualifiedColumn("t.a".to_string())
        );
    }

    #[test]
    fn reject_bare_qualified_column_predicate() {
        assert_eq!(
            err("t.a"),
            FilterExprError::QualifiedColumn("t.a".to_string())
        );
    }

    // ── rejection: subqueries ───────────────────────────────────────────

    #[test]
    fn reject_in_subquery() {
        assert_eq!(err("a IN (SELECT x FROM y)"), FilterExprError::Subquery);
    }

    #[test]
    fn reject_scalar_subquery() {
        assert_eq!(err("a = (SELECT 1)"), FilterExprError::Subquery);
    }

    #[test]
    fn reject_exists_subquery() {
        assert_eq!(err("EXISTS (SELECT 1)"), FilterExprError::Subquery);
    }

    // ── rejection: bare / non-predicate expressions ─────────────────────

    #[test]
    fn reject_bare_column() {
        assert_eq!(
            err("active"),
            FilterExprError::BareColumn("active".to_string())
        );
    }

    #[test]
    fn reject_bare_boolean_literal() {
        assert_eq!(
            err("TRUE"),
            FilterExprError::NotAPredicate("true".to_string())
        );
    }

    #[test]
    fn reject_bare_number_literal() {
        assert_eq!(err("1"), FilterExprError::NotAPredicate("1".to_string()));
    }

    // ── rejection: unsupported operators / constructs ───────────────────

    #[test]
    fn reject_like() {
        assert_eq!(
            err("name LIKE 'x%'"),
            FilterExprError::UnsupportedOperator("LIKE".to_string())
        );
    }

    #[test]
    fn reject_not_like() {
        assert_eq!(
            err("name NOT LIKE 'x%'"),
            FilterExprError::UnsupportedOperator("NOT LIKE".to_string())
        );
    }

    #[test]
    fn reject_ilike() {
        assert_eq!(
            err("name ILIKE 'x%'"),
            FilterExprError::UnsupportedOperator("ILIKE".to_string())
        );
    }

    #[test]
    fn reject_arithmetic_predicate_operator() {
        assert_eq!(
            err("a + 1"),
            FilterExprError::UnsupportedOperator("+".to_string())
        );
    }

    #[test]
    fn reject_is_true() {
        match err("a IS TRUE") {
            FilterExprError::Unsupported(text) => {
                assert!(text.contains("IS TRUE"), "{text}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn reject_placeholder_literal() {
        assert_eq!(
            err("a = $1"),
            FilterExprError::UnsupportedLiteral("$1".to_string())
        );
    }

    // ── rejection: statement smuggling ──────────────────────────────────

    #[test]
    fn reject_multiple_statements_injection() {
        assert_eq!(
            err("1 = 1; DROP TABLE t"),
            FilterExprError::MultipleStatements
        );
    }

    #[test]
    fn reject_order_by_smuggle() {
        assert_eq!(
            err("a = 1 ORDER BY b"),
            FilterExprError::UnsupportedClause("ORDER BY".to_string())
        );
    }

    #[test]
    fn reject_limit_smuggle() {
        assert_eq!(
            err("a = 1 LIMIT 5"),
            FilterExprError::UnsupportedClause("LIMIT".to_string())
        );
    }

    #[test]
    fn reject_group_by_smuggle() {
        assert_eq!(
            err("a = 1 GROUP BY b"),
            FilterExprError::UnsupportedClause("GROUP BY".to_string())
        );
    }

    #[test]
    fn reject_having_smuggle() {
        assert_eq!(
            err("a = 1 HAVING b = 2"),
            FilterExprError::UnsupportedClause("HAVING".to_string())
        );
    }

    #[test]
    fn reject_union_smuggle() {
        assert_eq!(
            err("a = 1 UNION SELECT * FROM u"),
            FilterExprError::UnsupportedClause("UNION".to_string())
        );
    }

    // ── rejection: empty / malformed input ──────────────────────────────

    #[test]
    fn reject_empty_expression() {
        assert_eq!(err(""), FilterExprError::Empty);
        assert_eq!(err("   \t\n "), FilterExprError::Empty);
    }

    #[test]
    fn reject_garbage_is_parse_error() {
        match err(">>>") {
            FilterExprError::Parse(_) => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn reject_unclosed_string_is_parse_error() {
        match err("name = 'oops") {
            FilterExprError::Parse(_) => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn reject_empty_in_list() {
        assert!(parse_filter_expr("a IN ()").is_err());
    }

    // ── error text names the construct ──────────────────────────────────

    #[test]
    fn error_messages_name_the_construct() {
        assert!(FilterExprError::Or.to_string().contains("OR"));
        assert!(
            FilterExprError::Not("NOT BETWEEN".to_string())
                .to_string()
                .contains("NOT BETWEEN")
        );
        assert!(
            FilterExprError::NullTest("IS NULL".to_string())
                .to_string()
                .contains("IS NULL")
        );
        assert!(
            FilterExprError::Function("UPPER".to_string())
                .to_string()
                .contains("UPPER")
        );
        let column_column = FilterExprError::ColumnColumn {
            left: "a".to_string(),
            right: "b".to_string(),
        }
        .to_string();
        assert!(column_column.contains("`a`") && column_column.contains("`b`"));
        assert!(FilterExprError::Subquery.to_string().contains("subquery"));
        let op_error = FilterExprError::UnsupportedOperator("LIKE".to_string()).to_string();
        assert!(op_error.contains("LIKE") && op_error.contains("BETWEEN"));
    }

    // ── realistic demo expression ───────────────────────────────────────

    #[test]
    fn vida_demo_expression_lowers_fully() {
        let filters = parse_filter_expr(
            "area_in_meters > 1000 AND confidence >= 0.8 AND bf_source IN ('osm', 'google')",
        )
        .unwrap();
        assert_eq!(filters.len(), 3);
        assert_cmp_filter(
            &filters[0],
            "area_in_meters",
            CmpOp::Gt,
            &ScalarValue::Int64(1000),
        );
        assert_cmp_filter(
            &filters[1],
            "confidence",
            CmpOp::Ge,
            &ScalarValue::Float64(0.8),
        );
        match &filters[2] {
            AttributeFilter::In { col, values } => {
                assert_eq!(col, "bf_source");
                assert_eq!(values.len(), 2);
            }
            other => panic!("expected In, got {other:?}"),
        }
    }
}
