//! Index selection logic.

use crate::index::{Index, IndexRegistry};
use crate::optimizer::cost_model::CostModel;
use crate::parser::ast::{BinaryOperator, Expr};

/// Index selector.
pub struct IndexSelector<'a> {
    /// Index registry.
    registry: &'a IndexRegistry,
    /// Cost model.
    cost_model: &'a CostModel,
}

impl<'a> IndexSelector<'a> {
    /// Create a new index selector.
    pub fn new(registry: &'a IndexRegistry, cost_model: &'a CostModel) -> Self {
        Self {
            registry,
            cost_model,
        }
    }

    /// Select best index for a table scan with predicate.
    pub fn select_index(&self, table: &str, predicate: &Expr) -> Option<IndexSelection> {
        let indexes = self.registry.get_indexes(table)?;

        let mut best_selection: Option<IndexSelection> = None;
        let mut best_cost = f64::INFINITY;

        for index in indexes {
            if let Some(selection) = self.can_use_index(index, predicate) {
                let cost = self.estimate_index_cost(&selection);
                if cost < best_cost {
                    best_cost = cost;
                    best_selection = Some(selection);
                }
            }
        }

        best_selection
    }

    /// Check if an index can be used for a predicate.
    fn can_use_index(&self, index: &Index, predicate: &Expr) -> Option<IndexSelection> {
        match predicate {
            Expr::BinaryOp { left, op, right } => {
                // Check for column = value or column < value etc.
                if let Expr::Column { name, .. } = &**left {
                    if index.columns.contains(name) {
                        let index_usage = match op {
                            BinaryOperator::Eq => IndexUsage::Equality,
                            BinaryOperator::Lt
                            | BinaryOperator::LtEq
                            | BinaryOperator::Gt
                            | BinaryOperator::GtEq => IndexUsage::Range,
                            _ => return None,
                        };

                        return Some(IndexSelection {
                            index: index.clone(),
                            usage: index_usage,
                            selectivity: self
                                .cost_model
                                .estimate_selectivity(&index.table, predicate),
                        });
                    }
                }

                // Check AND predicates
                if matches!(op, BinaryOperator::And) {
                    // Try left side
                    if let Some(selection) = self.can_use_index(index, left) {
                        return Some(selection);
                    }
                    // Try right side
                    if let Some(selection) = self.can_use_index(index, right) {
                        return Some(selection);
                    }
                }

                None
            }
            Expr::Function { name, args } => {
                // Check spatial functions
                let func_name = name.to_uppercase();
                if matches!(
                    func_name.as_str(),
                    "ST_INTERSECTS" | "ST_CONTAINS" | "ST_WITHIN"
                ) {
                    // Check if first argument is a column that has an R-tree index
                    if let Some(Expr::Column { name, .. }) = args.first() {
                        if index.columns.contains(name)
                            && matches!(
                                index.index_type,
                                crate::optimizer::cost_model::IndexType::RTree
                            )
                        {
                            return Some(IndexSelection {
                                index: index.clone(),
                                usage: IndexUsage::Spatial,
                                selectivity: 0.01, // Spatial predicates are highly selective
                            });
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Estimate cost of using an index.
    fn estimate_index_cost(&self, selection: &IndexSelection) -> f64 {
        let lookup_cost = selection.index.statistics.lookup_cost();
        let scan_cost = selection.index.statistics.scan_cost(selection.selectivity);

        lookup_cost.add(&scan_cost).total()
    }
}

/// Index selection result.
#[derive(Debug, Clone)]
pub struct IndexSelection {
    /// Selected index.
    pub index: Index,
    /// How the index is used.
    pub usage: IndexUsage,
    /// Estimated selectivity.
    pub selectivity: f64,
}

/// Index usage type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexUsage {
    /// Equality lookup.
    Equality,
    /// Range scan.
    Range,
    /// Spatial query.
    Spatial,
    /// Full index scan.
    FullScan,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizer::cost_model::{IndexStatistics, IndexType};
    use crate::parser::ast::Literal;

    #[test]
    fn test_equality_index_selection() {
        let mut registry = IndexRegistry::new();
        let cost_model = CostModel::new();

        let idx_stats = IndexStatistics::new(
            "idx_id".to_string(),
            vec!["id".to_string()],
            IndexType::BTree,
            10000,
        );

        let index = Index::new(
            "idx_id".to_string(),
            "users".to_string(),
            vec!["id".to_string()],
            IndexType::BTree,
            idx_stats,
        );

        registry.register_index("users".to_string(), index);

        let selector = IndexSelector::new(&registry, &cost_model);

        let predicate = Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: None,
                name: "id".to_string(),
            }),
            op: BinaryOperator::Eq,
            right: Box::new(Expr::Literal(Literal::Integer(42))),
        };

        let selection = selector.select_index("users", &predicate);
        assert!(selection.is_some());

        if let Some(sel) = selection {
            assert_eq!(sel.usage, IndexUsage::Equality);
        }
    }
}
