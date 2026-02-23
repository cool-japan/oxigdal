//! Optimization rules.

pub mod cse;
pub mod join_reordering;
pub mod projection_pushdown;

use crate::error::{QueryError, Result};
use crate::parser::ast::*;
use oxigdal_core::error::OxiGdalError;
use std::collections::HashSet;

// Re-export rule structs for convenience
pub use cse::CommonSubexpressionElimination;
pub use join_reordering::JoinReordering;
pub use projection_pushdown::ProjectionPushdown;

/// Optimization rule trait.
pub trait OptimizationRule {
    /// Apply the rule to a select statement.
    fn apply(&self, stmt: SelectStatement) -> Result<SelectStatement>;
}

/// Predicate pushdown rule.
///
/// Pushes filter predicates down to table scans and joins.
/// This optimization reduces the amount of data processed by applying
/// filters as early as possible in the query execution plan.
pub struct PredicatePushdown;

impl OptimizationRule for PredicatePushdown {
    fn apply(&self, mut stmt: SelectStatement) -> Result<SelectStatement> {
        // Only proceed if we have both a selection and a FROM clause
        if stmt.selection.is_none() || stmt.from.is_none() {
            return Ok(stmt);
        }

        // Safe to unwrap since we checked above
        let selection = stmt
            .selection
            .take()
            .ok_or_else(|| QueryError::optimization("Internal error: selection disappeared"))?;
        let from = stmt
            .from
            .take()
            .ok_or_else(|| QueryError::optimization("Internal error: from disappeared"))?;

        // Extract all predicates from the WHERE clause
        let mut predicates = Vec::new();
        extract_predicates(&selection, &mut predicates);

        // Collect all table aliases from the FROM clause
        let table_aliases = collect_table_aliases(&from);

        // Validate that we have table references
        if table_aliases.is_empty() {
            return Err(QueryError::optimization(
                OxiGdalError::invalid_state_builder(
                    "Cannot apply predicate pushdown without table references",
                )
                .with_operation("predicate_pushdown")
                .with_parameter("predicate_count", predicates.len().to_string())
                .with_suggestion("Ensure the FROM clause contains valid table references")
                .build()
                .to_string(),
            ));
        }

        // Try to push each predicate down
        let mut pushed_predicates: Vec<Expr> = Vec::new();
        let mut remaining_predicates: Vec<Expr> = Vec::new();

        for predicate in predicates {
            let predicate_tables = get_predicate_tables(&predicate);

            // Validate predicate references known tables
            if !predicate_tables.is_empty()
                && !predicate_tables.iter().any(|t| table_aliases.contains(t))
            {
                return Err(QueryError::optimization(
                    OxiGdalError::invalid_operation_builder("Predicate references unknown table")
                        .with_operation("predicate_pushdown")
                        .with_parameter(
                            "unknown_tables",
                            predicate_tables
                                .iter()
                                .filter(|t| !table_aliases.contains(*t))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                        .with_parameter(
                            "available_tables",
                            table_aliases.iter().cloned().collect::<Vec<_>>().join(", "),
                        )
                        .with_suggestion("Check table names and aliases in the FROM clause")
                        .build()
                        .to_string(),
                ));
            }

            // Check if predicate can be pushed to a single table
            if predicate_tables.len() == 1 {
                if let Some(table_name) = predicate_tables.iter().next() {
                    if table_aliases.contains(table_name) {
                        pushed_predicates.push(predicate);
                        continue;
                    }
                }
            }
            remaining_predicates.push(predicate);
        }

        // Push predicates into joins where possible
        let optimized_from = push_predicates_to_joins(from, &mut pushed_predicates);

        // Reconstruct the remaining WHERE clause
        let new_selection = if remaining_predicates.is_empty() && pushed_predicates.is_empty() {
            None
        } else {
            let all_remaining: Vec<Expr> = remaining_predicates
                .into_iter()
                .chain(pushed_predicates)
                .collect();
            combine_predicates_with_and(all_remaining)
        };

        stmt.from = Some(optimized_from);
        stmt.selection = new_selection;

        Ok(stmt)
    }
}

// ---------------------------------------------------------------------------
// Shared helper functions (used by submodules via `super::`)
// ---------------------------------------------------------------------------

/// Extract all AND-connected predicates from an expression.
pub(crate) fn extract_predicates(expr: &Expr, predicates: &mut Vec<Expr>) {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            extract_predicates(left, predicates);
            extract_predicates(right, predicates);
        }
        _ => {
            predicates.push(expr.clone());
        }
    }
}

/// Collect all table names and aliases from a table reference.
pub(crate) fn collect_table_aliases(table_ref: &TableReference) -> HashSet<String> {
    let mut aliases = HashSet::new();
    collect_table_aliases_recursive(table_ref, &mut aliases);
    aliases
}

fn collect_table_aliases_recursive(table_ref: &TableReference, aliases: &mut HashSet<String>) {
    match table_ref {
        TableReference::Table { name, alias } => {
            aliases.insert(alias.clone().unwrap_or_else(|| name.clone()));
            aliases.insert(name.clone());
        }
        TableReference::Join { left, right, .. } => {
            collect_table_aliases_recursive(left, aliases);
            collect_table_aliases_recursive(right, aliases);
        }
        TableReference::Subquery { alias, .. } => {
            aliases.insert(alias.clone());
        }
    }
}

/// Get all table references from a predicate expression.
pub(crate) fn get_predicate_tables(expr: &Expr) -> HashSet<String> {
    let mut tables = HashSet::new();
    collect_predicate_tables(expr, &mut tables);
    tables
}

fn collect_predicate_tables(expr: &Expr, tables: &mut HashSet<String>) {
    match expr {
        Expr::Column { table, .. } => {
            if let Some(t) = table {
                tables.insert(t.clone());
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_predicate_tables(left, tables);
            collect_predicate_tables(right, tables);
        }
        Expr::UnaryOp { expr, .. } => {
            collect_predicate_tables(expr, tables);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_predicate_tables(arg, tables);
            }
        }
        Expr::Case {
            operand,
            when_then,
            else_result,
        } => {
            if let Some(op) = operand {
                collect_predicate_tables(op, tables);
            }
            for (when, then) in when_then {
                collect_predicate_tables(when, tables);
                collect_predicate_tables(then, tables);
            }
            if let Some(else_expr) = else_result {
                collect_predicate_tables(else_expr, tables);
            }
        }
        Expr::Cast { expr, .. } => {
            collect_predicate_tables(expr, tables);
        }
        Expr::IsNull(expr) | Expr::IsNotNull(expr) => {
            collect_predicate_tables(expr, tables);
        }
        Expr::InList { expr, list, .. } => {
            collect_predicate_tables(expr, tables);
            for item in list {
                collect_predicate_tables(item, tables);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_predicate_tables(expr, tables);
            collect_predicate_tables(low, tables);
            collect_predicate_tables(high, tables);
        }
        Expr::Subquery(subquery) => {
            // For subqueries, we need to collect from the subquery's FROM clause
            if let Some(ref from) = subquery.from {
                for alias in collect_table_aliases(from) {
                    tables.insert(alias);
                }
            }
        }
        Expr::Literal(_) | Expr::Wildcard => {}
    }
}

/// Push predicates into join conditions where applicable.
fn push_predicates_to_joins(
    table_ref: TableReference,
    predicates: &mut Vec<Expr>,
) -> TableReference {
    match table_ref {
        TableReference::Join {
            left,
            right,
            join_type,
            on,
        } => {
            // First, recursively optimize children
            let optimized_left = push_predicates_to_joins(*left, predicates);
            let optimized_right = push_predicates_to_joins(*right, predicates);

            // Get table aliases from left and right
            let left_tables = collect_table_aliases(&optimized_left);
            let right_tables = collect_table_aliases(&optimized_right);
            let all_tables: HashSet<String> = left_tables
                .iter()
                .chain(right_tables.iter())
                .cloned()
                .collect();

            // Find predicates that can be pushed into this join
            let mut join_predicates = Vec::new();
            let mut remaining = Vec::new();

            for predicate in predicates.drain(..) {
                let pred_tables = get_predicate_tables(&predicate);

                // Check if predicate references only tables in this join
                let can_push =
                    !pred_tables.is_empty() && pred_tables.iter().all(|t| all_tables.contains(t));

                // For inner joins, we can push predicates that reference both sides
                // For outer joins, we need to be more careful
                if can_push && join_type == JoinType::Inner {
                    join_predicates.push(predicate);
                } else if can_push && join_type == JoinType::Cross {
                    // Warn: pushing predicates through CROSS joins can be problematic
                    // but we allow it and convert to INNER
                    join_predicates.push(predicate);
                } else {
                    remaining.push(predicate);
                }
            }

            *predicates = remaining;

            // Combine with existing ON condition
            let new_on = match (on, combine_predicates_with_and(join_predicates)) {
                (Some(existing), Some(new_pred)) => Some(Expr::BinaryOp {
                    left: Box::new(existing),
                    op: BinaryOperator::And,
                    right: Box::new(new_pred),
                }),
                (Some(existing), None) => Some(existing),
                (None, Some(new_pred)) => Some(new_pred),
                (None, None) => None,
            };

            TableReference::Join {
                left: Box::new(optimized_left),
                right: Box::new(optimized_right),
                join_type,
                on: new_on,
            }
        }
        TableReference::Subquery { query, alias } => {
            // Get predicates that only reference this subquery
            let mut subquery_predicates = Vec::new();
            let mut remaining = Vec::new();

            for predicate in predicates.drain(..) {
                let pred_tables = get_predicate_tables(&predicate);
                if pred_tables.len() == 1 && pred_tables.contains(&alias) {
                    subquery_predicates.push(predicate);
                } else {
                    remaining.push(predicate);
                }
            }

            *predicates = remaining;

            // Push predicates into subquery's WHERE clause
            let mut optimized_query = *query;
            if !subquery_predicates.is_empty() {
                let combined = combine_predicates_with_and(subquery_predicates);
                optimized_query.selection = match (optimized_query.selection, combined) {
                    (Some(existing), Some(new_pred)) => Some(Expr::BinaryOp {
                        left: Box::new(existing),
                        op: BinaryOperator::And,
                        right: Box::new(new_pred),
                    }),
                    (Some(existing), None) => Some(existing),
                    (None, Some(new_pred)) => Some(new_pred),
                    (None, None) => None,
                };
            }

            TableReference::Subquery {
                query: Box::new(optimized_query),
                alias,
            }
        }
        other => other,
    }
}

/// Combine predicates with AND operator.
pub(crate) fn combine_predicates_with_and(predicates: Vec<Expr>) -> Option<Expr> {
    if predicates.is_empty() {
        return None;
    }

    let mut iter = predicates.into_iter();
    let first = iter.next()?;

    Some(iter.fold(first, |acc, pred| Expr::BinaryOp {
        left: Box::new(acc),
        op: BinaryOperator::And,
        right: Box::new(pred),
    }))
}

/// Collect all column references from an expression.
pub(crate) fn collect_column_refs(expr: &Expr, columns: &mut HashSet<String>) {
    match expr {
        Expr::Column { table, name } => {
            let full_name = if let Some(t) = table {
                format!("{}.{}", t, name)
            } else {
                name.clone()
            };
            columns.insert(full_name);
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_column_refs(left, columns);
            collect_column_refs(right, columns);
        }
        Expr::UnaryOp { expr, .. } => {
            collect_column_refs(expr, columns);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_column_refs(arg, columns);
            }
        }
        Expr::Case {
            operand,
            when_then,
            else_result,
        } => {
            if let Some(op) = operand {
                collect_column_refs(op, columns);
            }
            for (when, then) in when_then {
                collect_column_refs(when, columns);
                collect_column_refs(then, columns);
            }
            if let Some(else_expr) = else_result {
                collect_column_refs(else_expr, columns);
            }
        }
        Expr::Cast { expr, .. } => {
            collect_column_refs(expr, columns);
        }
        Expr::IsNull(expr) | Expr::IsNotNull(expr) => {
            collect_column_refs(expr, columns);
        }
        Expr::InList { expr, list, .. } => {
            collect_column_refs(expr, columns);
            for item in list {
                collect_column_refs(item, columns);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_column_refs(expr, columns);
            collect_column_refs(low, columns);
            collect_column_refs(high, columns);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Constant folding
// ---------------------------------------------------------------------------

/// Constant folding rule.
///
/// Evaluates constant expressions at compile time.
pub struct ConstantFolding;

impl OptimizationRule for ConstantFolding {
    fn apply(&self, mut stmt: SelectStatement) -> Result<SelectStatement> {
        // Fold constants in projection
        stmt.projection = stmt.projection.into_iter().map(fold_select_item).collect();

        // Fold constants in WHERE clause
        if let Some(selection) = stmt.selection {
            stmt.selection = Some(fold_expr(selection));
        }

        // Fold constants in HAVING clause
        if let Some(having) = stmt.having {
            stmt.having = Some(fold_expr(having));
        }

        // Fold constants in ORDER BY
        stmt.order_by = stmt
            .order_by
            .into_iter()
            .map(|order| OrderByExpr {
                expr: fold_expr(order.expr),
                asc: order.asc,
                nulls_first: order.nulls_first,
            })
            .collect();

        Ok(stmt)
    }
}

fn fold_select_item(item: SelectItem) -> SelectItem {
    match item {
        SelectItem::Expr { expr, alias } => SelectItem::Expr {
            expr: fold_expr(expr),
            alias,
        },
        other => other,
    }
}

fn fold_expr(expr: Expr) -> Expr {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let left = fold_expr(*left);
            let right = fold_expr(*right);

            // Try to fold if both are literals
            if let (Expr::Literal(l), Expr::Literal(r)) = (&left, &right) {
                if let Some(result) = try_fold_binary(l, op, r) {
                    return Expr::Literal(result);
                }
            }

            Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            }
        }
        Expr::UnaryOp { op, expr } => {
            let expr = fold_expr(*expr);
            if let Expr::Literal(lit) = &expr {
                if let Some(result) = try_fold_unary(op, lit) {
                    return Expr::Literal(result);
                }
            }
            Expr::UnaryOp {
                op,
                expr: Box::new(expr),
            }
        }
        Expr::Function { name, args } => {
            let args = args.into_iter().map(fold_expr).collect();
            Expr::Function { name, args }
        }
        Expr::Case {
            operand,
            when_then,
            else_result,
        } => {
            let operand = operand.map(|e| Box::new(fold_expr(*e)));
            let when_then = when_then
                .into_iter()
                .map(|(w, t)| (fold_expr(w), fold_expr(t)))
                .collect();
            let else_result = else_result.map(|e| Box::new(fold_expr(*e)));
            Expr::Case {
                operand,
                when_then,
                else_result,
            }
        }
        other => other,
    }
}

fn try_fold_binary(left: &Literal, op: BinaryOperator, right: &Literal) -> Option<Literal> {
    match (left, right) {
        (Literal::Integer(l), Literal::Integer(r)) => match op {
            BinaryOperator::Plus => Some(Literal::Integer(l + r)),
            BinaryOperator::Minus => Some(Literal::Integer(l - r)),
            BinaryOperator::Multiply => Some(Literal::Integer(l * r)),
            BinaryOperator::Divide if *r != 0 => Some(Literal::Integer(l / r)),
            BinaryOperator::Modulo if *r != 0 => Some(Literal::Integer(l % r)),
            BinaryOperator::Eq => Some(Literal::Boolean(l == r)),
            BinaryOperator::NotEq => Some(Literal::Boolean(l != r)),
            BinaryOperator::Lt => Some(Literal::Boolean(l < r)),
            BinaryOperator::LtEq => Some(Literal::Boolean(l <= r)),
            BinaryOperator::Gt => Some(Literal::Boolean(l > r)),
            BinaryOperator::GtEq => Some(Literal::Boolean(l >= r)),
            _ => None,
        },
        (Literal::Float(l), Literal::Float(r)) => match op {
            BinaryOperator::Plus => Some(Literal::Float(l + r)),
            BinaryOperator::Minus => Some(Literal::Float(l - r)),
            BinaryOperator::Multiply => Some(Literal::Float(l * r)),
            BinaryOperator::Divide if *r != 0.0 => Some(Literal::Float(l / r)),
            BinaryOperator::Eq => Some(Literal::Boolean((l - r).abs() < f64::EPSILON)),
            BinaryOperator::NotEq => Some(Literal::Boolean((l - r).abs() >= f64::EPSILON)),
            BinaryOperator::Lt => Some(Literal::Boolean(l < r)),
            BinaryOperator::LtEq => Some(Literal::Boolean(l <= r)),
            BinaryOperator::Gt => Some(Literal::Boolean(l > r)),
            BinaryOperator::GtEq => Some(Literal::Boolean(l >= r)),
            _ => None,
        },
        (Literal::Boolean(l), Literal::Boolean(r)) => match op {
            BinaryOperator::And => Some(Literal::Boolean(*l && *r)),
            BinaryOperator::Or => Some(Literal::Boolean(*l || *r)),
            BinaryOperator::Eq => Some(Literal::Boolean(l == r)),
            BinaryOperator::NotEq => Some(Literal::Boolean(l != r)),
            _ => None,
        },
        (Literal::String(l), Literal::String(r)) => match op {
            BinaryOperator::Concat => Some(Literal::String(format!("{}{}", l, r))),
            BinaryOperator::Eq => Some(Literal::Boolean(l == r)),
            BinaryOperator::NotEq => Some(Literal::Boolean(l != r)),
            _ => None,
        },
        _ => None,
    }
}

fn try_fold_unary(op: UnaryOperator, lit: &Literal) -> Option<Literal> {
    match (op, lit) {
        (UnaryOperator::Minus, Literal::Integer(i)) => Some(Literal::Integer(-i)),
        (UnaryOperator::Minus, Literal::Float(f)) => Some(Literal::Float(-f)),
        (UnaryOperator::Not, Literal::Boolean(b)) => Some(Literal::Boolean(!b)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Filter fusion
// ---------------------------------------------------------------------------

/// Filter fusion rule.
///
/// Combines multiple consecutive filters into a single filter.
pub struct FilterFusion;

impl OptimizationRule for FilterFusion {
    fn apply(&self, mut stmt: SelectStatement) -> Result<SelectStatement> {
        // Combine multiple AND conditions in WHERE clause
        if let Some(selection) = stmt.selection {
            stmt.selection = Some(fuse_filters(selection));
        }
        Ok(stmt)
    }
}

fn fuse_filters(expr: Expr) -> Expr {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let left = fuse_filters(*left);
            let right = fuse_filters(*right);

            // Flatten nested ANDs
            let mut conditions = Vec::new();
            collect_and_conditions(&left, &mut conditions);
            collect_and_conditions(&right, &mut conditions);

            if conditions.len() > 1 {
                // Rebuild as flat AND chain
                let mut result = conditions[0].clone();
                for cond in &conditions[1..] {
                    result = Expr::BinaryOp {
                        left: Box::new(result),
                        op: BinaryOperator::And,
                        right: Box::new(cond.clone()),
                    };
                }
                result
            } else {
                Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOperator::And,
                    right: Box::new(right),
                }
            }
        }
        other => other,
    }
}

fn collect_and_conditions(expr: &Expr, conditions: &mut Vec<Expr>) {
    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::And,
        right,
    } = expr
    {
        collect_and_conditions(left, conditions);
        collect_and_conditions(right, conditions);
    } else {
        conditions.push(expr.clone());
    }
}

// ---------------------------------------------------------------------------
// Pipeline entry point
// ---------------------------------------------------------------------------

/// Optimize a query by applying all optimization rules.
pub fn optimize_with_rules(stmt: SelectStatement) -> Result<SelectStatement> {
    let rules: Vec<Box<dyn OptimizationRule>> = vec![
        Box::new(ConstantFolding),
        Box::new(FilterFusion),
        Box::new(ProjectionPushdown),
        Box::new(PredicatePushdown),
        Box::new(JoinReordering),
        Box::new(CommonSubexpressionElimination),
    ];

    let mut current = stmt;
    for rule in rules {
        current = rule.apply(current)?;
    }

    Ok(current)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_folding_arithmetic() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Literal(Literal::Integer(10))),
            op: BinaryOperator::Plus,
            right: Box::new(Expr::Literal(Literal::Integer(20))),
        };
        let folded = fold_expr(expr);
        assert_eq!(folded, Expr::Literal(Literal::Integer(30)));
    }

    #[test]
    fn test_constant_folding_boolean() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Literal(Literal::Boolean(true))),
            op: BinaryOperator::And,
            right: Box::new(Expr::Literal(Literal::Boolean(false))),
        };
        let folded = fold_expr(expr);
        assert_eq!(folded, Expr::Literal(Literal::Boolean(false)));
    }

    #[test]
    fn test_unary_folding() {
        let expr = Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr: Box::new(Expr::Literal(Literal::Integer(42))),
        };
        let folded = fold_expr(expr);
        assert_eq!(folded, Expr::Literal(Literal::Integer(-42)));
    }

    #[test]
    fn test_full_optimization_pipeline() {
        let a_plus_b = Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: None,
                name: "a".to_string(),
            }),
            op: BinaryOperator::Plus,
            right: Box::new(Expr::Column {
                table: None,
                name: "b".to_string(),
            }),
        };

        let stmt = SelectStatement {
            projection: vec![
                SelectItem::Expr {
                    expr: a_plus_b.clone(),
                    alias: None,
                },
                SelectItem::Expr {
                    expr: Expr::BinaryOp {
                        left: Box::new(Expr::Literal(Literal::Integer(1))),
                        op: BinaryOperator::Plus,
                        right: Box::new(Expr::Literal(Literal::Integer(2))),
                    },
                    alias: Some("constant".to_string()),
                },
            ],
            from: Some(TableReference::Table {
                name: "t".to_string(),
                alias: None,
            }),
            selection: Some(Expr::BinaryOp {
                left: Box::new(a_plus_b),
                op: BinaryOperator::Gt,
                right: Box::new(Expr::Literal(Literal::Integer(10))),
            }),
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let result = optimize_with_rules(stmt).expect("Full optimization should succeed");

        // Constant folding: 1 + 2 -> 3
        if let SelectItem::Expr { expr, .. } = &result.projection[1] {
            assert_eq!(*expr, Expr::Literal(Literal::Integer(3)));
        }
    }
}
