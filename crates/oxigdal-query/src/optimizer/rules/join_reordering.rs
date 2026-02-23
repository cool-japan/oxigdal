//! Join reordering optimization rule.
//!
//! Reorders inner joins to minimize intermediate result sizes using a
//! greedy algorithm with heuristic cost estimation.
//!
//! Only inner and cross joins are reordered. Outer joins (LEFT, RIGHT, FULL)
//! preserve their original order since their semantics depend on operand position.
//!
//! The greedy algorithm at each step selects the pair of relations whose join
//! produces the smallest estimated intermediate result, based on:
//! - Heuristic table size estimates (default 10000 rows for base tables)
//! - Predicate selectivity estimates (equality: 0.1, range: 0.33, etc.)
//! - Hash join cost model (build + probe + output)

use crate::error::Result;
use crate::parser::ast::*;
use std::collections::HashSet;

use super::{
    OptimizationRule, collect_table_aliases, combine_predicates_with_and, extract_predicates,
    get_predicate_tables,
};

/// Join reordering rule.
pub struct JoinReordering;

impl OptimizationRule for JoinReordering {
    fn apply(&self, mut stmt: SelectStatement) -> Result<SelectStatement> {
        if let Some(from) = stmt.from.take() {
            stmt.from = Some(reorder_join_tree(from));
        }
        Ok(stmt)
    }
}

/// A component in the join reordering algorithm: a table reference with
/// metadata used for cost estimation.
struct JoinComponent {
    /// The table reference (base table, subquery, or already-optimized subtree).
    table_ref: TableReference,
    /// Set of all table names/aliases within this component.
    table_names: HashSet<String>,
    /// Estimated number of output rows.
    estimated_rows: f64,
}

/// Recursively reorder inner join chains in the table reference tree.
fn reorder_join_tree(table_ref: TableReference) -> TableReference {
    let is_inner = matches!(
        &table_ref,
        TableReference::Join { join_type, .. }
            if *join_type == JoinType::Inner || *join_type == JoinType::Cross
    );

    if is_inner {
        // Flatten the inner join chain into components and predicates
        let mut components: Vec<JoinComponent> = Vec::new();
        let mut predicates: Vec<Expr> = Vec::new();
        flatten_inner_join_chain(table_ref, &mut components, &mut predicates);

        if components.len() <= 1 {
            return components
                .into_iter()
                .next()
                .map(|c| c.table_ref)
                .unwrap_or(TableReference::Table {
                    name: String::new(),
                    alias: None,
                });
        }

        // Recursively optimize each leaf component
        for comp in &mut components {
            let old_ref = std::mem::replace(
                &mut comp.table_ref,
                TableReference::Table {
                    name: String::new(),
                    alias: None,
                },
            );
            comp.table_ref = reorder_join_tree(old_ref);
            comp.estimated_rows = heuristic_row_estimate(&comp.table_ref);
        }

        greedy_join_order(components, predicates)
    } else {
        match table_ref {
            TableReference::Join {
                left,
                right,
                join_type,
                on,
            } => TableReference::Join {
                left: Box::new(reorder_join_tree(*left)),
                right: Box::new(reorder_join_tree(*right)),
                join_type,
                on,
            },
            other => other,
        }
    }
}

/// Flatten a chain of inner/cross joins into base components and predicates.
fn flatten_inner_join_chain(
    table_ref: TableReference,
    components: &mut Vec<JoinComponent>,
    predicates: &mut Vec<Expr>,
) {
    let is_inner = matches!(
        &table_ref,
        TableReference::Join { join_type, .. }
            if *join_type == JoinType::Inner || *join_type == JoinType::Cross
    );

    if is_inner {
        if let TableReference::Join {
            left, right, on, ..
        } = table_ref
        {
            flatten_inner_join_chain(*left, components, predicates);
            flatten_inner_join_chain(*right, components, predicates);

            if let Some(on_expr) = on {
                let mut preds = Vec::new();
                extract_predicates(&on_expr, &mut preds);
                predicates.extend(preds);
            }
        }
    } else {
        // Leaf: base table, subquery, or non-inner join subtree
        let table_names = collect_table_aliases(&table_ref);
        let estimated_rows = heuristic_row_estimate(&table_ref);
        components.push(JoinComponent {
            table_ref,
            table_names,
            estimated_rows,
        });
    }
}

/// Estimate the number of rows a table reference produces, using heuristics.
fn heuristic_row_estimate(table_ref: &TableReference) -> f64 {
    match table_ref {
        TableReference::Table { .. } => 10_000.0,
        TableReference::Join {
            left,
            right,
            join_type,
            on,
        } => {
            let left_rows = heuristic_row_estimate(left);
            let right_rows = heuristic_row_estimate(right);

            let selectivity = if let Some(on_expr) = on {
                let mut preds = Vec::new();
                extract_predicates(on_expr, &mut preds);
                heuristic_selectivity(&preds)
            } else {
                1.0
            };

            match join_type {
                JoinType::Inner | JoinType::Cross => {
                    (left_rows * right_rows * selectivity).max(1.0)
                }
                JoinType::Left => left_rows.max(1.0),
                JoinType::Right => right_rows.max(1.0),
                JoinType::Full => (left_rows + right_rows).max(1.0),
            }
        }
        TableReference::Subquery { .. } => 1_000.0,
    }
}

/// Estimate selectivity of a single predicate expression using heuristics.
///
/// Selectivity ranges from 0.0 (filters everything) to 1.0 (filters nothing).
/// These values are based on standard database textbook defaults:
/// - Equality: 1/10 (assumes ~10 distinct values)
/// - Range: 1/3
/// - LIKE: 1/10
/// - IS NULL: 1/20
pub(crate) fn heuristic_single_selectivity(pred: &Expr) -> f64 {
    match pred {
        Expr::BinaryOp { left, op, right } => match op {
            BinaryOperator::Eq => 0.1,
            BinaryOperator::NotEq => 0.9,
            BinaryOperator::Lt
            | BinaryOperator::LtEq
            | BinaryOperator::Gt
            | BinaryOperator::GtEq => 0.33,
            BinaryOperator::And => {
                heuristic_single_selectivity(left) * heuristic_single_selectivity(right)
            }
            BinaryOperator::Or => {
                let l = heuristic_single_selectivity(left);
                let r = heuristic_single_selectivity(right);
                l + r - l * r
            }
            BinaryOperator::Like => 0.1,
            BinaryOperator::NotLike => 0.9,
            _ => 0.5,
        },
        Expr::IsNull(_) => 0.05,
        Expr::IsNotNull(_) => 0.95,
        Expr::InList { list, negated, .. } => {
            let sel = (list.len() as f64 * 0.1).min(0.9);
            if *negated { 1.0 - sel } else { sel }
        }
        Expr::Between { negated, .. } => {
            if *negated {
                0.75
            } else {
                0.25
            }
        }
        _ => 0.5,
    }
}

/// Combined selectivity of multiple predicates (assuming independence).
pub(crate) fn heuristic_selectivity(predicates: &[Expr]) -> f64 {
    if predicates.is_empty() {
        return 1.0;
    }
    predicates
        .iter()
        .map(heuristic_single_selectivity)
        .product::<f64>()
        .max(0.0001)
}

/// Estimate the cost of joining two components, considering applicable predicates.
///
/// Cost model:
/// - Find predicates that reference both sides (cross-component predicates)
/// - Estimate output rows = left_rows * right_rows * selectivity
/// - Total cost = hash_build + hash_probe + output_materialization
fn estimate_pair_join_cost(
    left: &JoinComponent,
    right: &JoinComponent,
    all_predicates: &[Expr],
) -> f64 {
    // Find predicates that reference tables from both sides
    let applicable: Vec<&Expr> = all_predicates
        .iter()
        .filter(|pred| {
            let tables = get_predicate_tables(pred);
            !tables.is_empty()
                && tables.iter().any(|t| left.table_names.contains(t))
                && tables.iter().any(|t| right.table_names.contains(t))
        })
        .collect();

    let selectivity = if applicable.is_empty() {
        1.0 // No cross-component predicates = cross join
    } else {
        applicable
            .iter()
            .map(|p| heuristic_single_selectivity(p))
            .product::<f64>()
            .max(0.0001)
    };

    let output_rows = left.estimated_rows * right.estimated_rows * selectivity;

    // Hash join cost: build from smaller side, probe from larger side
    let (build_rows, probe_rows) = if left.estimated_rows <= right.estimated_rows {
        (left.estimated_rows, right.estimated_rows)
    } else {
        (right.estimated_rows, left.estimated_rows)
    };

    let build_cost = build_rows * 10.0;
    let probe_cost = probe_rows * 5.0;
    let output_cost = output_rows * 2.0;

    build_cost + probe_cost + output_cost
}

/// Greedy join ordering: iteratively merge the cheapest pair until one tree remains.
///
/// At each iteration:
/// 1. Evaluate cost for every pair of remaining components
/// 2. Pick the pair with minimum estimated cost
/// 3. Merge them into a single component with a JOIN node
/// 4. Assign applicable predicates as the ON condition
/// 5. Repeat until only one component remains
fn greedy_join_order(
    mut components: Vec<JoinComponent>,
    mut all_predicates: Vec<Expr>,
) -> TableReference {
    // Validate we have components to reorder
    if components.is_empty() {
        return TableReference::Table {
            name: String::new(),
            alias: None,
        };
    }

    if components.len() == 1 {
        let mut result = components
            .into_iter()
            .next()
            .map(|c| c.table_ref)
            .unwrap_or(TableReference::Table {
                name: String::new(),
                alias: None,
            });

        // Apply any remaining predicates
        if !all_predicates.is_empty() {
            if let TableReference::Join { ref mut on, .. } = result {
                let remaining = super::combine_predicates_with_and(all_predicates);
                *on = match (on.take(), remaining) {
                    (Some(existing), Some(new_pred)) => Some(Expr::BinaryOp {
                        left: Box::new(existing),
                        op: BinaryOperator::And,
                        right: Box::new(new_pred),
                    }),
                    (Some(existing), None) => Some(existing),
                    (None, some_pred) => some_pred,
                };
            }
        }

        return result;
    }

    while components.len() > 1 {
        // Find the pair with minimum join cost
        let mut best_i = 0;
        let mut best_j = 1;
        let mut best_cost = f64::MAX;

        for i in 0..components.len() {
            for j in (i + 1)..components.len() {
                let cost = estimate_pair_join_cost(&components[i], &components[j], &all_predicates);
                if cost < best_cost {
                    best_cost = cost;
                    best_i = i;
                    best_j = j;
                }
            }
        }

        // Remove the two components (larger index first to avoid invalidation)
        let right_comp = components.remove(best_j);
        let left_comp = components.remove(best_i);

        // Partition predicates: applicable to this join vs. remaining
        let merged_tables: HashSet<String> = left_comp
            .table_names
            .iter()
            .chain(right_comp.table_names.iter())
            .cloned()
            .collect();

        let mut join_preds = Vec::new();
        let mut remaining_preds = Vec::new();

        for pred in all_predicates {
            let tables = get_predicate_tables(&pred);
            if !tables.is_empty() && tables.iter().all(|t| merged_tables.contains(t)) {
                join_preds.push(pred);
            } else {
                remaining_preds.push(pred);
            }
        }
        all_predicates = remaining_preds;

        // Estimate output size for the merged component
        let selectivity = heuristic_selectivity(&join_preds);
        let output_rows =
            (left_comp.estimated_rows * right_comp.estimated_rows * selectivity).max(1.0);

        let on_condition = combine_predicates_with_and(join_preds);

        components.push(JoinComponent {
            table_ref: TableReference::Join {
                left: Box::new(left_comp.table_ref),
                right: Box::new(right_comp.table_ref),
                join_type: JoinType::Inner,
                on: on_condition,
            },
            table_names: merged_tables,
            estimated_rows: output_rows,
        });
    }

    let mut result = components
        .into_iter()
        .next()
        .map(|c| c.table_ref)
        .unwrap_or(TableReference::Table {
            name: String::new(),
            alias: None,
        });

    // Apply any remaining predicates to the outermost join's ON condition
    if !all_predicates.is_empty() {
        if let TableReference::Join { ref mut on, .. } = result {
            let remaining = combine_predicates_with_and(all_predicates);
            *on = match (on.take(), remaining) {
                (Some(existing), Some(new_pred)) => Some(Expr::BinaryOp {
                    left: Box::new(existing),
                    op: BinaryOperator::And,
                    right: Box::new(new_pred),
                }),
                (Some(existing), None) => Some(existing),
                (None, some_pred) => some_pred,
            };
        }
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_join_reorder_preserves_outer_join() {
        let stmt = SelectStatement {
            projection: vec![SelectItem::Wildcard],
            from: Some(TableReference::Join {
                left: Box::new(TableReference::Table {
                    name: "a".to_string(),
                    alias: None,
                }),
                right: Box::new(TableReference::Table {
                    name: "b".to_string(),
                    alias: None,
                }),
                join_type: JoinType::Left,
                on: Some(Expr::BinaryOp {
                    left: Box::new(Expr::Column {
                        table: Some("a".to_string()),
                        name: "id".to_string(),
                    }),
                    op: BinaryOperator::Eq,
                    right: Box::new(Expr::Column {
                        table: Some("b".to_string()),
                        name: "id".to_string(),
                    }),
                }),
            }),
            selection: None,
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let reorder = JoinReordering;
        let result = reorder.apply(stmt);
        assert!(result.is_ok(), "Join reordering should succeed");
        let result = result.expect("Join reordering should succeed");

        // LEFT join should be preserved (not reordered)
        let Some(TableReference::Join { join_type, .. }) = &result.from else {
            panic!("FROM should contain a join");
        };
        assert_eq!(*join_type, JoinType::Left);
    }

    #[test]
    fn test_join_reorder_three_inner_tables() {
        // A INNER JOIN B ON a.id = b.id INNER JOIN C ON b.id = c.id
        let stmt = SelectStatement {
            projection: vec![SelectItem::Wildcard],
            from: Some(TableReference::Join {
                left: Box::new(TableReference::Join {
                    left: Box::new(TableReference::Table {
                        name: "a".to_string(),
                        alias: Some("a".to_string()),
                    }),
                    right: Box::new(TableReference::Table {
                        name: "b".to_string(),
                        alias: Some("b".to_string()),
                    }),
                    join_type: JoinType::Inner,
                    on: Some(Expr::BinaryOp {
                        left: Box::new(Expr::Column {
                            table: Some("a".to_string()),
                            name: "id".to_string(),
                        }),
                        op: BinaryOperator::Eq,
                        right: Box::new(Expr::Column {
                            table: Some("b".to_string()),
                            name: "id".to_string(),
                        }),
                    }),
                }),
                right: Box::new(TableReference::Table {
                    name: "c".to_string(),
                    alias: Some("c".to_string()),
                }),
                join_type: JoinType::Inner,
                on: Some(Expr::BinaryOp {
                    left: Box::new(Expr::Column {
                        table: Some("b".to_string()),
                        name: "id".to_string(),
                    }),
                    op: BinaryOperator::Eq,
                    right: Box::new(Expr::Column {
                        table: Some("c".to_string()),
                        name: "id".to_string(),
                    }),
                }),
            }),
            selection: None,
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let reorder = JoinReordering;
        let result = reorder.apply(stmt);
        assert!(result.is_ok(), "Join reordering should succeed");
        let result = result.expect("Join reordering should succeed");

        // All three tables should still be present in the result
        let Some(from) = result.from.as_ref() else {
            panic!("FROM should exist");
        };
        let aliases = collect_table_aliases(from);
        assert!(aliases.contains("a"), "Table a missing");
        assert!(aliases.contains("b"), "Table b missing");
        assert!(aliases.contains("c"), "Table c missing");
    }

    #[test]
    fn test_join_reorder_single_table() {
        let stmt = SelectStatement {
            projection: vec![SelectItem::Wildcard],
            from: Some(TableReference::Table {
                name: "users".to_string(),
                alias: None,
            }),
            selection: None,
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        let reorder = JoinReordering;
        let result = reorder.apply(stmt);
        assert!(result.is_ok(), "Join reordering should succeed");
        let result = result.expect("Join reordering should succeed");

        // Single table should be unchanged
        assert!(matches!(
            &result.from,
            Some(TableReference::Table { name, .. }) if name == "users"
        ));
    }

    #[test]
    fn test_heuristic_selectivity_values() {
        // Equality predicate
        let eq_pred = Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: None,
                name: "a".to_string(),
            }),
            op: BinaryOperator::Eq,
            right: Box::new(Expr::Literal(Literal::Integer(1))),
        };
        let sel = heuristic_single_selectivity(&eq_pred);
        assert!((sel - 0.1).abs() < 0.001);

        // Range predicate
        let lt_pred = Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: None,
                name: "a".to_string(),
            }),
            op: BinaryOperator::Lt,
            right: Box::new(Expr::Literal(Literal::Integer(10))),
        };
        let sel = heuristic_single_selectivity(&lt_pred);
        assert!((sel - 0.33).abs() < 0.001);

        // IS NULL
        let null_pred = Expr::IsNull(Box::new(Expr::Column {
            table: None,
            name: "a".to_string(),
        }));
        let sel = heuristic_single_selectivity(&null_pred);
        assert!((sel - 0.05).abs() < 0.001);

        // Combined AND
        let preds = vec![eq_pred, lt_pred];
        let combined = heuristic_selectivity(&preds);
        assert!((combined - 0.033).abs() < 0.001);

        // Empty predicates
        let empty: Vec<Expr> = vec![];
        assert!((heuristic_selectivity(&empty) - 1.0).abs() < 0.001);
    }
}
