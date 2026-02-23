//! Projection pushdown optimization rule.
//!
//! Removes unnecessary columns early in the plan by rewriting subquery
//! projections to include only columns referenced by the outer query.

use crate::error::{QueryError, Result};
use crate::parser::ast::*;
use oxigdal_core::error::OxiGdalError;
use std::collections::HashSet;

use super::{OptimizationRule, collect_column_refs};

/// Projection pushdown rule.
///
/// Removes unnecessary columns early in the plan.
pub struct ProjectionPushdown;

impl OptimizationRule for ProjectionPushdown {
    fn apply(&self, stmt: SelectStatement) -> Result<SelectStatement> {
        // Validate projection is not empty
        if stmt.projection.is_empty() {
            return Err(QueryError::optimization(
                OxiGdalError::invalid_state_builder(
                    "Cannot apply projection pushdown with empty projection",
                )
                .with_operation("projection_pushdown")
                .with_suggestion("Ensure SELECT clause has at least one column or wildcard")
                .build()
                .to_string(),
            ));
        }

        // Collect all referenced columns
        let mut referenced_columns = HashSet::new();

        // Add columns from projection
        for item in &stmt.projection {
            match item {
                SelectItem::Wildcard | SelectItem::QualifiedWildcard(_) => {
                    // Keep all columns
                    return Ok(stmt);
                }
                SelectItem::Expr { expr, .. } => {
                    collect_column_refs(expr, &mut referenced_columns);
                }
            }
        }

        // Add columns from WHERE
        if let Some(ref selection) = stmt.selection {
            collect_column_refs(selection, &mut referenced_columns);
        }

        // Add columns from GROUP BY
        for expr in &stmt.group_by {
            collect_column_refs(expr, &mut referenced_columns);
        }

        // Add columns from HAVING
        if let Some(ref having) = stmt.having {
            collect_column_refs(having, &mut referenced_columns);
        }

        // Add columns from ORDER BY
        for order in &stmt.order_by {
            collect_column_refs(&order.expr, &mut referenced_columns);
        }

        // Push column projections into subqueries within the FROM clause.
        // This reduces the amount of data computed and materialized by inner queries.
        let mut optimized_stmt = stmt;
        if let Some(from) = optimized_stmt.from.take() {
            optimized_stmt.from = Some(push_column_projections(from, &referenced_columns));
        }

        Ok(optimized_stmt)
    }
}

/// Push column projections into subqueries within the FROM clause.
///
/// For each subquery, determines which columns are referenced by the outer
/// query and rewrites the subquery's projection accordingly. This reduces
/// the amount of data computed and materialized by inner queries.
///
/// For wildcard subqueries (`SELECT * FROM ...`), the wildcard is replaced
/// with explicit column references for only the needed columns.
/// For non-wildcard subqueries, unnecessary projection items are pruned.
fn push_column_projections(
    table_ref: TableReference,
    referenced_columns: &HashSet<String>,
) -> TableReference {
    match table_ref {
        TableReference::Subquery { query, alias } => {
            // Determine which columns from this subquery the outer query needs
            let needed: HashSet<String> = referenced_columns
                .iter()
                .filter_map(|col| {
                    // Match qualified references: alias.column
                    if let Some(stripped) = col.strip_prefix(&format!("{}.", alias)) {
                        Some(stripped.to_string())
                    } else if !col.contains('.') {
                        // Unqualified column might come from this subquery
                        Some(col.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if needed.is_empty() {
                return TableReference::Subquery { query, alias };
            }

            let mut new_query = *query;

            let has_wildcard = new_query
                .projection
                .iter()
                .any(|p| matches!(p, SelectItem::Wildcard | SelectItem::QualifiedWildcard(_)));

            if has_wildcard {
                // Replace wildcard with only the needed columns
                let mut new_projection: Vec<SelectItem> = Vec::new();

                // Keep non-wildcard items that produce needed columns
                for item in &new_query.projection {
                    match item {
                        SelectItem::Expr { alias: Some(a), .. } if needed.contains(a.as_str()) => {
                            new_projection.push(item.clone());
                        }
                        SelectItem::Expr {
                            expr: Expr::Column { name, .. },
                            alias: None,
                        } if needed.contains(name.as_str()) => {
                            new_projection.push(item.clone());
                        }
                        SelectItem::Wildcard | SelectItem::QualifiedWildcard(_) => {
                            // Replaced below
                        }
                        _ => {}
                    }
                }

                // Add needed columns not yet in projection
                let existing: HashSet<String> = new_projection
                    .iter()
                    .filter_map(|item| match item {
                        SelectItem::Expr { alias: Some(a), .. } => Some(a.clone()),
                        SelectItem::Expr {
                            expr: Expr::Column { name, .. },
                            alias: None,
                        } => Some(name.clone()),
                        _ => None,
                    })
                    .collect();

                for col in &needed {
                    if !existing.contains(col) {
                        new_projection.push(SelectItem::Expr {
                            expr: Expr::Column {
                                table: None,
                                name: col.clone(),
                            },
                            alias: None,
                        });
                    }
                }

                if !new_projection.is_empty() {
                    new_query.projection = new_projection;
                }
            } else {
                // No wildcard: filter existing projection to keep only needed items.
                // First collect internally referenced columns (from WHERE, GROUP BY, etc.)
                // to avoid removing columns the subquery itself needs.
                let mut internal_refs = HashSet::new();
                if let Some(ref sel) = new_query.selection {
                    collect_column_refs(sel, &mut internal_refs);
                }
                for gexpr in &new_query.group_by {
                    collect_column_refs(gexpr, &mut internal_refs);
                }
                if let Some(ref hav) = new_query.having {
                    collect_column_refs(hav, &mut internal_refs);
                }
                for ord in &new_query.order_by {
                    collect_column_refs(&ord.expr, &mut internal_refs);
                }

                new_query.projection.retain(|item| match item {
                    SelectItem::Wildcard | SelectItem::QualifiedWildcard(_) => true,
                    SelectItem::Expr { alias: Some(a), .. } => needed.contains(a.as_str()),
                    SelectItem::Expr {
                        expr: Expr::Column { name, .. },
                        alias: None,
                    } => {
                        needed.contains(name.as_str())
                            || internal_refs
                                .iter()
                                .any(|r| r == name || r.ends_with(&format!(".{}", name)))
                    }
                    SelectItem::Expr { expr, alias: None } => {
                        let key = format!("{}", expr);
                        needed.contains(&key)
                    }
                });

                // Safety: never produce an empty projection
                if new_query.projection.is_empty() {
                    new_query.projection = vec![SelectItem::Wildcard];
                }
            }

            // Recursively push projections into the subquery's own FROM clause
            if let Some(inner_from) = new_query.from.take() {
                let mut sub_refs = HashSet::new();
                for item in &new_query.projection {
                    if let SelectItem::Expr { expr, .. } = item {
                        collect_column_refs(expr, &mut sub_refs);
                    }
                }
                if let Some(ref sel) = new_query.selection {
                    collect_column_refs(sel, &mut sub_refs);
                }
                for gexpr in &new_query.group_by {
                    collect_column_refs(gexpr, &mut sub_refs);
                }
                if let Some(ref hav) = new_query.having {
                    collect_column_refs(hav, &mut sub_refs);
                }
                for ord in &new_query.order_by {
                    collect_column_refs(&ord.expr, &mut sub_refs);
                }
                new_query.from = Some(push_column_projections(inner_from, &sub_refs));
            }

            TableReference::Subquery {
                query: Box::new(new_query),
                alias,
            }
        }
        TableReference::Join {
            left,
            right,
            join_type,
            on,
        } => {
            // Include join condition columns in the referenced set
            let mut extended_refs = referenced_columns.clone();
            if let Some(ref on_expr) = on {
                collect_column_refs(on_expr, &mut extended_refs);
            }

            TableReference::Join {
                left: Box::new(push_column_projections(*left, &extended_refs)),
                right: Box::new(push_column_projections(*right, &extended_refs)),
                join_type,
                on,
            }
        }
        other => other,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_projection_pushdown_subquery_wildcard() {
        // SELECT sub.x FROM (SELECT * FROM t) AS sub
        // -> SELECT sub.x FROM (SELECT x FROM t) AS sub
        let stmt = SelectStatement {
            projection: vec![SelectItem::Expr {
                expr: Expr::Column {
                    table: Some("sub".to_string()),
                    name: "x".to_string(),
                },
                alias: None,
            }],
            from: Some(TableReference::Subquery {
                query: Box::new(SelectStatement {
                    projection: vec![SelectItem::Wildcard],
                    from: Some(TableReference::Table {
                        name: "t".to_string(),
                        alias: None,
                    }),
                    selection: None,
                    group_by: Vec::new(),
                    having: None,
                    order_by: Vec::new(),
                    limit: None,
                    offset: None,
                }),
                alias: "sub".to_string(),
            }),
            selection: None,
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let pushdown = ProjectionPushdown;
        let result = pushdown.apply(stmt);
        assert!(result.is_ok(), "Projection pushdown should succeed");
        let result = result.expect("Projection pushdown should succeed");

        // The subquery should no longer have a wildcard
        let Some(TableReference::Subquery { query, .. }) = &result.from else {
            panic!("FROM should be a subquery");
        };
        let has_wildcard = query
            .projection
            .iter()
            .any(|p| matches!(p, SelectItem::Wildcard));
        assert!(
            !has_wildcard,
            "Wildcard should be replaced with specific columns"
        );
        // Should have exactly one column (x)
        assert_eq!(query.projection.len(), 1);
    }

    #[test]
    fn test_projection_pushdown_outer_wildcard_skips() {
        // SELECT * FROM (SELECT * FROM t) AS sub
        // Outer wildcard means all columns are needed; no pushdown possible
        let stmt = SelectStatement {
            projection: vec![SelectItem::Wildcard],
            from: Some(TableReference::Subquery {
                query: Box::new(SelectStatement {
                    projection: vec![SelectItem::Wildcard],
                    from: Some(TableReference::Table {
                        name: "t".to_string(),
                        alias: None,
                    }),
                    selection: None,
                    group_by: Vec::new(),
                    having: None,
                    order_by: Vec::new(),
                    limit: None,
                    offset: None,
                }),
                alias: "sub".to_string(),
            }),
            selection: None,
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let pushdown = ProjectionPushdown;
        let result = pushdown.apply(stmt);
        assert!(result.is_ok(), "Projection pushdown should succeed");
        let result = result.expect("Projection pushdown should succeed");

        // Subquery should still have wildcard (early return for outer wildcard)
        if let Some(TableReference::Subquery { query, .. }) = &result.from {
            assert!(
                query
                    .projection
                    .iter()
                    .any(|p| matches!(p, SelectItem::Wildcard))
            );
        }
    }

    #[test]
    fn test_projection_pushdown_with_where_columns() {
        // SELECT sub.x FROM (SELECT * FROM t) AS sub WHERE sub.y > 10
        // -> SELECT sub.x FROM (SELECT x, y FROM t) AS sub WHERE sub.y > 10
        let stmt = SelectStatement {
            projection: vec![SelectItem::Expr {
                expr: Expr::Column {
                    table: Some("sub".to_string()),
                    name: "x".to_string(),
                },
                alias: None,
            }],
            from: Some(TableReference::Subquery {
                query: Box::new(SelectStatement {
                    projection: vec![SelectItem::Wildcard],
                    from: Some(TableReference::Table {
                        name: "t".to_string(),
                        alias: None,
                    }),
                    selection: None,
                    group_by: Vec::new(),
                    having: None,
                    order_by: Vec::new(),
                    limit: None,
                    offset: None,
                }),
                alias: "sub".to_string(),
            }),
            selection: Some(Expr::BinaryOp {
                left: Box::new(Expr::Column {
                    table: Some("sub".to_string()),
                    name: "y".to_string(),
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

        let pushdown = ProjectionPushdown;
        let result = pushdown.apply(stmt);
        assert!(result.is_ok(), "Projection pushdown should succeed");
        let result = result.expect("Projection pushdown should succeed");

        // Subquery should have exactly 2 columns: x and y
        if let Some(TableReference::Subquery { query, .. }) = &result.from {
            assert_eq!(query.projection.len(), 2, "Subquery should project x and y");
        }
    }
}
