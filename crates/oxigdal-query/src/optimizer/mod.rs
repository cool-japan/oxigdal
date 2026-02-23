//! Query optimizer.

pub mod cost_model;
pub mod rules;

use crate::error::{QueryError, Result};
use crate::parser::ast::*;
use cost_model::CostModel;
use oxigdal_core::error::OxiGdalError;
use rules::optimize_with_rules;
use serde::{Deserialize, Serialize};

/// Query optimizer.
pub struct Optimizer {
    /// Cost model for estimating query costs.
    cost_model: CostModel,
    /// Configuration.
    _config: OptimizerConfig,
}

/// Optimizer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    /// Maximum optimization passes.
    pub max_passes: usize,
    /// Enable predicate pushdown.
    pub enable_predicate_pushdown: bool,
    /// Enable join reordering.
    pub enable_join_reordering: bool,
    /// Enable constant folding.
    pub enable_constant_folding: bool,
    /// Enable common subexpression elimination.
    pub enable_cse: bool,
    /// Enable filter fusion.
    pub enable_filter_fusion: bool,
    /// Enable projection pushdown.
    pub enable_projection_pushdown: bool,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            max_passes: 10,
            enable_predicate_pushdown: true,
            enable_join_reordering: true,
            enable_constant_folding: true,
            enable_cse: true,
            enable_filter_fusion: true,
            enable_projection_pushdown: true,
        }
    }
}

impl Optimizer {
    /// Create a new optimizer with default configuration.
    pub fn new() -> Self {
        Self::with_config(OptimizerConfig::default())
    }

    /// Create a new optimizer with custom configuration.
    pub fn with_config(_config: OptimizerConfig) -> Self {
        Self {
            cost_model: CostModel::new(),
            _config,
        }
    }

    /// Get the cost model.
    pub fn cost_model(&self) -> &CostModel {
        &self.cost_model
    }

    /// Optimize a query.
    pub fn optimize(&self, stmt: Statement) -> Result<OptimizedQuery> {
        match stmt {
            Statement::Select(select) => {
                let original_cost = self.estimate_cost(&select);

                // Validate original cost is reasonable
                if !original_cost.total().is_finite() || original_cost.total() < 0.0 {
                    return Err(QueryError::optimization(
                        OxiGdalError::invalid_state_builder(
                            "Invalid cost estimation for original query",
                        )
                        .with_operation("cost_estimation")
                        .with_parameter("estimated_cost", original_cost.total().to_string())
                        .with_suggestion("Query may be too complex or contain invalid operations")
                        .build()
                        .to_string(),
                    ));
                }

                // Apply rule-based optimization
                let optimized = optimize_with_rules(select)?;

                let optimized_cost = self.estimate_cost(&optimized);

                // Validate optimized cost
                if !optimized_cost.total().is_finite() || optimized_cost.total() < 0.0 {
                    return Err(QueryError::optimization(
                        OxiGdalError::invalid_state_builder(
                            "Invalid cost estimation after optimization",
                        )
                        .with_operation("optimization")
                        .with_parameter("original_cost", original_cost.total().to_string())
                        .with_parameter("optimized_cost", optimized_cost.total().to_string())
                        .with_suggestion("Optimization may have introduced invalid transformations")
                        .build()
                        .to_string(),
                    ));
                }

                Ok(OptimizedQuery {
                    statement: Statement::Select(optimized),
                    original_cost,
                    optimized_cost,
                })
            }
        }
    }

    /// Estimate the cost of executing a select statement.
    fn estimate_cost(&self, stmt: &SelectStatement) -> cost_model::Cost {
        let mut total_cost = cost_model::Cost::zero();

        // Estimate FROM clause cost
        if let Some(ref table_ref) = stmt.from {
            total_cost = total_cost.add(&self.estimate_table_cost(table_ref));
        }

        // Estimate WHERE clause cost
        if stmt.selection.is_some() {
            // Add filter cost
            total_cost = total_cost.add(&cost_model::Cost::new(1000.0, 0.0, 0.0, 0.0));
        }

        // Estimate GROUP BY cost
        if !stmt.group_by.is_empty() {
            total_cost = total_cost.add(&self.cost_model.aggregate_cost(1000, 100));
        }

        // Estimate ORDER BY cost
        if !stmt.order_by.is_empty() {
            total_cost = total_cost.add(&self.cost_model.sort_cost(1000));
        }

        total_cost
    }

    /// Estimate cost of a table reference.
    fn estimate_table_cost(&self, table_ref: &TableReference) -> cost_model::Cost {
        match table_ref {
            TableReference::Table { name, .. } => self.cost_model.scan_cost(name),
            TableReference::Join {
                left,
                right,
                join_type,
                ..
            } => {
                let left_cost = self.estimate_table_cost(left);
                let right_cost = self.estimate_table_cost(right);
                let join_cost = self.cost_model.join_cost(1000, 1000, *join_type);
                left_cost.add(&right_cost).add(&join_cost)
            }
            TableReference::Subquery { query, .. } => self.estimate_cost(query),
        }
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// An optimized query with cost information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizedQuery {
    /// The optimized statement.
    pub statement: Statement,
    /// Original cost estimate.
    pub original_cost: cost_model::Cost,
    /// Optimized cost estimate.
    pub optimized_cost: cost_model::Cost,
}

impl OptimizedQuery {
    /// Get the improvement ratio.
    pub fn improvement_ratio(&self) -> f64 {
        let original = self.original_cost.total();
        let optimized = self.optimized_cost.total();
        if original > 0.0 {
            (original - optimized) / original
        } else {
            0.0
        }
    }

    /// Get the speedup factor.
    pub fn speedup_factor(&self) -> f64 {
        let original = self.original_cost.total();
        let optimized = self.optimized_cost.total();
        if optimized > 0.0 {
            original / optimized
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::sql::parse_sql;

    #[test]
    fn test_optimizer_creation() {
        let optimizer = Optimizer::new();
        assert!(optimizer._config.enable_constant_folding);
    }

    #[test]
    fn test_optimize_simple_query() -> Result<()> {
        let sql = "SELECT id, name FROM users WHERE 1 = 1";
        let stmt = parse_sql(sql)?;

        let optimizer = Optimizer::new();
        let optimized = optimizer.optimize(stmt)?;

        assert!(optimized.original_cost.total() >= 0.0);
        assert!(optimized.optimized_cost.total() >= 0.0);

        Ok(())
    }

    #[test]
    fn test_cost_estimation() {
        let optimizer = Optimizer::new();
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

        let cost = optimizer.estimate_cost(&stmt);
        assert!(cost.total() > 0.0);
    }
}
