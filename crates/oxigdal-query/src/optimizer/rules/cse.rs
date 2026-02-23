//! Common subexpression elimination rule.
//!
//! Identifies repeated expressions across the query and eliminates redundant
//! computation by replacing duplicates with references to pre-computed results.
//!
//! The algorithm works in three phases:
//! 1. **Registry**: Build a map of projection expressions keyed by their
//!    canonical string form (using the `Display` trait).
//! 2. **Detection**: Scan non-projection clauses (WHERE, GROUP BY, HAVING,
//!    ORDER BY) for subexpressions matching any projection expression.
//! 3. **Replacement**: Replace detected matches with column references to
//!    projection aliases, assigning synthetic aliases where needed.
//!
//! This is safe because SQL allows referencing SELECT aliases in GROUP BY,
//! HAVING, and ORDER BY clauses.

use crate::error::{QueryError, Result};
use crate::parser::ast::*;
use oxigdal_core::error::OxiGdalError;
use std::collections::HashMap;

use super::OptimizationRule;

/// Maximum number of CSE candidates to track (prevents excessive memory use)
const MAX_CSE_CANDIDATES: usize = 1000;

/// Common subexpression elimination rule.
pub struct CommonSubexpressionElimination;

impl OptimizationRule for CommonSubexpressionElimination {
    fn apply(&self, mut stmt: SelectStatement) -> Result<SelectStatement> {
        // Phase 1: Build a registry of projection expressions
        // Key: canonical string form, Value: (projection index, existing alias if any)
        let mut proj_registry: HashMap<String, (usize, Option<String>)> = HashMap::new();

        for (idx, item) in stmt.projection.iter().enumerate() {
            if let SelectItem::Expr { expr, alias } = item {
                if is_cse_candidate(expr) {
                    let key = format!("{}", expr);
                    proj_registry.insert(key, (idx, alias.clone()));
                }
            }
        }

        // Check complexity limit
        if proj_registry.len() > MAX_CSE_CANDIDATES {
            return Err(QueryError::optimization(
                OxiGdalError::invalid_operation_builder("Too many CSE candidates in query")
                    .with_operation("common_subexpression_elimination")
                    .with_parameter("candidate_count", proj_registry.len().to_string())
                    .with_parameter("max_allowed", MAX_CSE_CANDIDATES.to_string())
                    .with_suggestion(
                        "Simplify the query or reduce the number of complex expressions in SELECT",
                    )
                    .build()
                    .to_string(),
            ));
        }

        if proj_registry.is_empty() {
            return Ok(stmt);
        }

        // Phase 2: Detect common subexpressions in non-projection clauses
        let mut replacement_map: HashMap<String, String> = HashMap::new();
        let mut proj_alias_assignments: HashMap<usize, String> = HashMap::new();
        let mut next_cse_id = 0usize;

        if let Some(ref selection) = stmt.selection {
            detect_cse_matches(
                selection,
                &proj_registry,
                &mut replacement_map,
                &mut proj_alias_assignments,
                &mut next_cse_id,
            );
        }
        for expr in &stmt.group_by {
            detect_cse_matches(
                expr,
                &proj_registry,
                &mut replacement_map,
                &mut proj_alias_assignments,
                &mut next_cse_id,
            );
        }
        if let Some(ref having) = stmt.having {
            detect_cse_matches(
                having,
                &proj_registry,
                &mut replacement_map,
                &mut proj_alias_assignments,
                &mut next_cse_id,
            );
        }
        for order in &stmt.order_by {
            detect_cse_matches(
                &order.expr,
                &proj_registry,
                &mut replacement_map,
                &mut proj_alias_assignments,
                &mut next_cse_id,
            );
        }

        if replacement_map.is_empty() {
            return Ok(stmt);
        }

        // Phase 3: Assign aliases to projection items that need them
        for (idx, alias_name) in &proj_alias_assignments {
            if let Some(SelectItem::Expr { alias, .. }) = stmt.projection.get_mut(*idx) {
                if alias.is_none() {
                    *alias = Some(alias_name.clone());
                }
            }
        }

        // Replace common subexpressions in non-projection clauses
        if let Some(selection) = stmt.selection.take() {
            stmt.selection = Some(replace_cse(selection, &replacement_map));
        }
        stmt.group_by = stmt
            .group_by
            .into_iter()
            .map(|expr| replace_cse(expr, &replacement_map))
            .collect();
        if let Some(having) = stmt.having.take() {
            stmt.having = Some(replace_cse(having, &replacement_map));
        }
        stmt.order_by = stmt
            .order_by
            .into_iter()
            .map(|order| OrderByExpr {
                expr: replace_cse(order.expr, &replacement_map),
                asc: order.asc,
                nulls_first: order.nulls_first,
            })
            .collect();

        Ok(stmt)
    }
}

/// Check if an expression is a candidate for CSE (non-trivial computation).
/// Simple column references and literals are never worth extracting.
pub(crate) fn is_cse_candidate(expr: &Expr) -> bool {
    !matches!(
        expr,
        Expr::Column { .. } | Expr::Literal(_) | Expr::Wildcard
    )
}

/// Walk an expression tree looking for subexpressions that match entries
/// in `proj_registry`. When a match is found, record the mapping in
/// `replacement_map` and, if needed, generate a synthetic alias in
/// `proj_alias_assignments`.
fn detect_cse_matches(
    expr: &Expr,
    proj_registry: &HashMap<String, (usize, Option<String>)>,
    replacement_map: &mut HashMap<String, String>,
    proj_alias_assignments: &mut HashMap<usize, String>,
    next_cse_id: &mut usize,
) {
    let key = format!("{}", expr);

    // Check if this (sub)expression matches a projection expression
    if let Some((idx, existing_alias)) = proj_registry.get(&key) {
        let alias = if let Some(a) = existing_alias {
            a.clone()
        } else if let Some(a) = proj_alias_assignments.get(idx) {
            a.clone()
        } else {
            let a = format!("__cse_{}", *next_cse_id);
            *next_cse_id += 1;
            proj_alias_assignments.insert(*idx, a.clone());
            a
        };
        replacement_map.insert(key, alias);
        return; // Whole expression will be replaced; no need to recurse deeper
    }

    // Recurse into children to find deeper matches
    match expr {
        Expr::BinaryOp { left, right, .. } => {
            detect_cse_matches(
                left,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
            detect_cse_matches(
                right,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
        }
        Expr::UnaryOp { expr: inner, .. } => {
            detect_cse_matches(
                inner,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
        }
        Expr::Function { args, .. } => {
            for arg in args {
                detect_cse_matches(
                    arg,
                    proj_registry,
                    replacement_map,
                    proj_alias_assignments,
                    next_cse_id,
                );
            }
        }
        Expr::Case {
            operand,
            when_then,
            else_result,
        } => {
            if let Some(op) = operand {
                detect_cse_matches(
                    op,
                    proj_registry,
                    replacement_map,
                    proj_alias_assignments,
                    next_cse_id,
                );
            }
            for (when, then) in when_then {
                detect_cse_matches(
                    when,
                    proj_registry,
                    replacement_map,
                    proj_alias_assignments,
                    next_cse_id,
                );
                detect_cse_matches(
                    then,
                    proj_registry,
                    replacement_map,
                    proj_alias_assignments,
                    next_cse_id,
                );
            }
            if let Some(else_expr) = else_result {
                detect_cse_matches(
                    else_expr,
                    proj_registry,
                    replacement_map,
                    proj_alias_assignments,
                    next_cse_id,
                );
            }
        }
        Expr::Cast { expr: inner, .. } => {
            detect_cse_matches(
                inner,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
        }
        Expr::IsNull(inner) | Expr::IsNotNull(inner) => {
            detect_cse_matches(
                inner,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
        }
        Expr::InList {
            expr: inner, list, ..
        } => {
            detect_cse_matches(
                inner,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
            for item in list {
                detect_cse_matches(
                    item,
                    proj_registry,
                    replacement_map,
                    proj_alias_assignments,
                    next_cse_id,
                );
            }
        }
        Expr::Between {
            expr: inner,
            low,
            high,
            ..
        } => {
            detect_cse_matches(
                inner,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
            detect_cse_matches(
                low,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
            detect_cse_matches(
                high,
                proj_registry,
                replacement_map,
                proj_alias_assignments,
                next_cse_id,
            );
        }
        // Terminals and subqueries (different scope) - no recursion
        Expr::Column { .. } | Expr::Literal(_) | Expr::Wildcard | Expr::Subquery(_) => {}
    }
}

/// Replace common subexpressions with column references (top-down traversal).
/// Checks the current node first; if it matches, replaces the whole subtree.
/// Otherwise, recurses into children.
fn replace_cse(expr: Expr, replacements: &HashMap<String, String>) -> Expr {
    let key = format!("{}", expr);
    if let Some(alias) = replacements.get(&key) {
        return Expr::Column {
            table: None,
            name: alias.clone(),
        };
    }

    match expr {
        Expr::BinaryOp { left, op, right } => Expr::BinaryOp {
            left: Box::new(replace_cse(*left, replacements)),
            op,
            right: Box::new(replace_cse(*right, replacements)),
        },
        Expr::UnaryOp { op, expr: inner } => Expr::UnaryOp {
            op,
            expr: Box::new(replace_cse(*inner, replacements)),
        },
        Expr::Function { name, args } => Expr::Function {
            name,
            args: args
                .into_iter()
                .map(|a| replace_cse(a, replacements))
                .collect(),
        },
        Expr::Case {
            operand,
            when_then,
            else_result,
        } => Expr::Case {
            operand: operand.map(|e| Box::new(replace_cse(*e, replacements))),
            when_then: when_then
                .into_iter()
                .map(|(w, t)| (replace_cse(w, replacements), replace_cse(t, replacements)))
                .collect(),
            else_result: else_result.map(|e| Box::new(replace_cse(*e, replacements))),
        },
        Expr::Cast {
            expr: inner,
            data_type,
        } => Expr::Cast {
            expr: Box::new(replace_cse(*inner, replacements)),
            data_type,
        },
        Expr::IsNull(inner) => Expr::IsNull(Box::new(replace_cse(*inner, replacements))),
        Expr::IsNotNull(inner) => Expr::IsNotNull(Box::new(replace_cse(*inner, replacements))),
        Expr::InList {
            expr: inner,
            list,
            negated,
        } => Expr::InList {
            expr: Box::new(replace_cse(*inner, replacements)),
            list: list
                .into_iter()
                .map(|i| replace_cse(i, replacements))
                .collect(),
            negated,
        },
        Expr::Between {
            expr: inner,
            low,
            high,
            negated,
        } => Expr::Between {
            expr: Box::new(replace_cse(*inner, replacements)),
            low: Box::new(replace_cse(*low, replacements)),
            high: Box::new(replace_cse(*high, replacements)),
            negated,
        },
        // Column, Literal, Wildcard, Subquery: return as-is
        other => other,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_cse_projection_to_where() {
        // SELECT (a + b), x FROM t WHERE (a + b) > 10
        // -> SELECT (a + b) AS __cse_0, x FROM t WHERE __cse_0 > 10
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
                    expr: Expr::Column {
                        table: None,
                        name: "x".to_string(),
                    },
                    alias: None,
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

        let cse = CommonSubexpressionElimination;
        let result = cse.apply(stmt);
        assert!(result.is_ok(), "CSE should succeed");
        let result = result.expect("CSE should succeed");

        // Projection should have an alias assigned
        if let SelectItem::Expr { alias, .. } = &result.projection[0] {
            assert!(
                alias.is_some(),
                "CSE should assign alias to common expression"
            );
        }

        // WHERE should use a column reference instead of the expression
        if let Some(Expr::BinaryOp { left, .. }) = &result.selection {
            assert!(
                matches!(**left, Expr::Column { .. }),
                "CSE should replace expression in WHERE with column ref"
            );
        }
    }

    #[test]
    fn test_cse_with_existing_alias() {
        // SELECT (a + b) AS total FROM t ORDER BY (a + b)
        // -> SELECT (a + b) AS total FROM t ORDER BY total
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
            projection: vec![SelectItem::Expr {
                expr: a_plus_b.clone(),
                alias: Some("total".to_string()),
            }],
            from: Some(TableReference::Table {
                name: "t".to_string(),
                alias: None,
            }),
            selection: None,
            group_by: Vec::new(),
            having: None,
            order_by: vec![OrderByExpr {
                expr: a_plus_b,
                asc: true,
                nulls_first: false,
            }],
            limit: None,
            offset: None,
        };

        let cse = CommonSubexpressionElimination;
        let result = cse.apply(stmt);
        assert!(result.is_ok(), "CSE should succeed");
        let result = result.expect("CSE should succeed");

        // ORDER BY should now reference "total"
        let Expr::Column { name, .. } = &result.order_by[0].expr else {
            panic!("ORDER BY should be a column reference after CSE");
        };
        assert_eq!(name, "total");
    }

    #[test]
    fn test_cse_no_common_expressions() {
        // SELECT a FROM t WHERE b > 5
        // No common subexpressions between projection and WHERE
        let stmt = SelectStatement {
            projection: vec![SelectItem::Expr {
                expr: Expr::Column {
                    table: None,
                    name: "a".to_string(),
                },
                alias: None,
            }],
            from: Some(TableReference::Table {
                name: "t".to_string(),
                alias: None,
            }),
            selection: Some(Expr::BinaryOp {
                left: Box::new(Expr::Column {
                    table: None,
                    name: "b".to_string(),
                }),
                op: BinaryOperator::Gt,
                right: Box::new(Expr::Literal(Literal::Integer(5))),
            }),
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let cse = CommonSubexpressionElimination;
        let result = cse.apply(stmt);
        assert!(result.is_ok(), "CSE should succeed");
        let result = result.expect("CSE should succeed");

        // No aliases should be assigned (column ref is trivial, not a CSE candidate)
        if let SelectItem::Expr { alias, .. } = &result.projection[0] {
            assert!(alias.is_none());
        }
    }

    #[test]
    fn test_cse_subexpression_in_where() {
        // SELECT (a + b) FROM t WHERE ((a + b) * 2) > 10
        // -> SELECT (a + b) AS __cse_0 FROM t WHERE (__cse_0 * 2) > 10
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
            projection: vec![SelectItem::Expr {
                expr: a_plus_b.clone(),
                alias: None,
            }],
            from: Some(TableReference::Table {
                name: "t".to_string(),
                alias: None,
            }),
            selection: Some(Expr::BinaryOp {
                left: Box::new(Expr::BinaryOp {
                    left: Box::new(a_plus_b),
                    op: BinaryOperator::Multiply,
                    right: Box::new(Expr::Literal(Literal::Integer(2))),
                }),
                op: BinaryOperator::Gt,
                right: Box::new(Expr::Literal(Literal::Integer(10))),
            }),
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let cse = CommonSubexpressionElimination;
        let result = cse.apply(stmt);
        assert!(result.is_ok(), "CSE should succeed");
        let result = result.expect("CSE should succeed");

        // Projection should have alias
        if let SelectItem::Expr { alias, .. } = &result.projection[0] {
            assert!(alias.is_some());
        }

        // WHERE: ((a+b)*2) > 10 should become (__cse_0 * 2) > 10
        if let Some(Expr::BinaryOp {
            left: outer_left, ..
        }) = &result.selection
        {
            if let Expr::BinaryOp {
                left: inner_left, ..
            } = outer_left.as_ref()
            {
                assert!(
                    matches!(inner_left.as_ref(), Expr::Column { .. }),
                    "a+b should be replaced with column ref inside larger expression"
                );
            }
        }
    }

    #[test]
    fn test_is_cse_candidate() {
        // Column reference: not a candidate
        assert!(!is_cse_candidate(&Expr::Column {
            table: None,
            name: "a".to_string()
        }));

        // Literal: not a candidate
        assert!(!is_cse_candidate(&Expr::Literal(Literal::Integer(42))));

        // Wildcard: not a candidate
        assert!(!is_cse_candidate(&Expr::Wildcard));

        // Binary op: is a candidate
        assert!(is_cse_candidate(&Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: None,
                name: "a".to_string()
            }),
            op: BinaryOperator::Plus,
            right: Box::new(Expr::Column {
                table: None,
                name: "b".to_string()
            }),
        }));

        // Function call: is a candidate
        assert!(is_cse_candidate(&Expr::Function {
            name: "SUM".to_string(),
            args: vec![Expr::Column {
                table: None,
                name: "x".to_string()
            }],
        }));
    }
}
