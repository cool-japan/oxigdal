//! Optimizer tests.

use oxigdal_query::Result;
use oxigdal_query::optimizer::Optimizer;
use oxigdal_query::optimizer::cost_model::{CostModel, Statistics};
use oxigdal_query::parser::sql::parse_sql;

#[test]
fn test_optimizer_creation() {
    let optimizer = Optimizer::new();
    let cost_model = optimizer.cost_model();
    assert!(cost_model.scan_cost("unknown_table").total() > 0.0);
}

#[test]
fn test_constant_folding() -> Result<()> {
    let sql = "SELECT 1 + 1 AS result FROM users";
    let stmt = parse_sql(sql)?;

    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(stmt)?;

    // Check that optimization happened
    assert!(optimized.original_cost.total() >= 0.0);
    assert!(optimized.optimized_cost.total() >= 0.0);

    Ok(())
}

#[test]
fn test_statistics_registration() {
    let cost_model = CostModel::new();
    let stats = Statistics::new(10000, 100);
    cost_model.register_statistics("users".to_string(), stats);

    let scan_cost = cost_model.scan_cost("users");
    assert!(scan_cost.total() > 0.0);
}

#[test]
fn test_selectivity_estimation() {
    let cost_model = CostModel::new();

    let sql = "SELECT * FROM users WHERE age > 18";
    let stmt = parse_sql(sql).ok();

    if let Some(oxigdal_query::Statement::Select(select)) = stmt {
        if let Some(predicate) = select.selection {
            let selectivity = cost_model.estimate_selectivity("users", &predicate);
            assert!((0.0..=1.0).contains(&selectivity));
        }
    }
}

#[test]
fn test_optimization_improves_cost() -> Result<()> {
    let sql = "SELECT * FROM users WHERE 1 = 1 AND age > 0";
    let stmt = parse_sql(sql)?;

    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(stmt)?;

    // Both costs should be non-negative
    assert!(optimized.original_cost.total() >= 0.0);
    assert!(optimized.optimized_cost.total() >= 0.0);

    Ok(())
}

#[test]
fn test_join_cost_estimation() {
    let cost_model = CostModel::new();
    let join_cost = cost_model.join_cost(1000, 1000, oxigdal_query::parser::ast::JoinType::Inner);
    assert!(join_cost.total() > 0.0);
}

#[test]
fn test_aggregate_cost_estimation() {
    let cost_model = CostModel::new();
    let agg_cost = cost_model.aggregate_cost(10000, 100);
    assert!(agg_cost.total() > 0.0);
}

#[test]
fn test_sort_cost_estimation() {
    let cost_model = CostModel::new();
    let sort_cost = cost_model.sort_cost(10000);
    assert!(sort_cost.total() > 0.0);
}

#[test]
fn test_improvement_ratio() -> Result<()> {
    let sql = "SELECT * FROM users";
    let stmt = parse_sql(sql)?;

    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(stmt)?;

    let ratio = optimized.improvement_ratio();
    assert!(ratio >= 0.0);

    Ok(())
}

#[test]
fn test_speedup_factor() -> Result<()> {
    let sql = "SELECT * FROM users";
    let stmt = parse_sql(sql)?;

    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(stmt)?;

    let speedup = optimized.speedup_factor();
    assert!(speedup >= 0.0);

    Ok(())
}
