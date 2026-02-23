//! SQL-like query language and cost-based optimizer for geospatial data.
//!
//! This crate provides a complete query engine with:
//! - SQL-like query language parsing
//! - Cost-based query optimization
//! - Parallel query execution
//! - Result caching
//! - Index selection
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_query::*;
//!
//! # async fn example() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! // Parse SQL query
//! let sql = "SELECT id, name FROM users WHERE age > 18";
//! let statement = parser::sql::parse_sql(sql)?;
//!
//! // Optimize query
//! let optimizer = optimizer::Optimizer::new();
//! let optimized = optimizer.optimize(statement)?;
//!
//! // Execute query
//! let mut executor = executor::Executor::new();
//! // Register data sources...
//! let results = executor.execute(&optimized.statement).await?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

pub mod cache;
pub mod error;
pub mod executor;
pub mod explain;
pub mod index;
pub mod optimizer;
pub mod parallel;
pub mod parser;

pub use cache::{CacheConfig, QueryCache};
pub use error::{QueryError, Result};
pub use executor::Executor;
pub use explain::ExplainPlan;
pub use optimizer::{OptimizedQuery, Optimizer, OptimizerConfig};
pub use parser::{Statement, parse_sql};

/// Query engine that combines all components.
pub struct QueryEngine {
    /// Parser (stateless, no storage needed).
    _parser_marker: std::marker::PhantomData<()>,
    /// Optimizer.
    optimizer: Optimizer,
    /// Executor.
    executor: Executor,
    /// Cache.
    cache: QueryCache,
}

impl QueryEngine {
    /// Create a new query engine with default configuration.
    pub fn new() -> Self {
        Self {
            _parser_marker: std::marker::PhantomData,
            optimizer: Optimizer::new(),
            executor: Executor::new(),
            cache: QueryCache::new(CacheConfig::default()),
        }
    }

    /// Create a new query engine with custom configuration.
    pub fn with_config(optimizer_config: OptimizerConfig, cache_config: CacheConfig) -> Self {
        Self {
            _parser_marker: std::marker::PhantomData,
            optimizer: Optimizer::with_config(optimizer_config),
            executor: Executor::new(),
            cache: QueryCache::new(cache_config),
        }
    }

    /// Get the optimizer.
    pub fn optimizer(&self) -> &Optimizer {
        &self.optimizer
    }

    /// Get the executor.
    pub fn executor(&mut self) -> &mut Executor {
        &mut self.executor
    }

    /// Get the cache.
    pub fn cache(&self) -> &QueryCache {
        &self.cache
    }

    /// Execute a SQL query.
    pub async fn execute_sql(&mut self, sql: &str) -> Result<Vec<executor::scan::RecordBatch>> {
        // Parse SQL
        let statement = parse_sql(sql)?;

        // Check cache
        if let Some(cached) = self.cache.get(&statement) {
            return Ok(cached);
        }

        // Optimize
        let optimized = self.optimizer.optimize(statement.clone())?;

        // Execute
        let results = self.executor.execute(&optimized.statement).await?;

        // Cache results
        self.cache.put(&statement, results.clone());

        Ok(results)
    }

    /// Explain a SQL query.
    pub fn explain_sql(&self, sql: &str) -> Result<ExplainPlan> {
        let statement = parse_sql(sql)?;
        let optimized = self.optimizer.optimize(statement)?;
        Ok(ExplainPlan::from_optimized(&optimized))
    }

    /// Register a data source.
    pub fn register_data_source(
        &mut self,
        name: String,
        source: std::sync::Arc<dyn executor::scan::DataSource>,
    ) {
        self.executor.register_data_source(name, source);
    }

    /// Clear the query cache.
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Get cache statistics.
    pub fn cache_statistics(&self) -> cache::CacheStatistics {
        self.cache.statistics()
    }
}

impl Default for QueryEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_engine_creation() {
        let engine = QueryEngine::new();
        assert!(engine.cache_statistics().hits == 0);
    }

    #[test]
    fn test_parse_simple_query() -> Result<()> {
        let sql = "SELECT id, name FROM users";
        let statement = parse_sql(sql)?;

        match statement {
            Statement::Select(select) => {
                assert_eq!(select.projection.len(), 2);
                assert!(select.from.is_some());
            }
        }

        Ok(())
    }

    #[test]
    fn test_optimizer() -> Result<()> {
        let sql = "SELECT * FROM users WHERE 1 + 1 = 2";
        let statement = parse_sql(sql)?;

        let optimizer = Optimizer::new();
        let optimized = optimizer.optimize(statement)?;

        assert!(optimized.original_cost.total() >= 0.0);
        assert!(optimized.optimized_cost.total() >= 0.0);

        Ok(())
    }
}
