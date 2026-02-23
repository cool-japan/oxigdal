//! Cost-based optimization model.

use crate::parser::ast::*;
use serde::{Deserialize, Serialize};

/// Cost estimate for a query plan.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Cost {
    /// CPU cost (operations).
    pub cpu: f64,
    /// IO cost (bytes read).
    pub io: f64,
    /// Memory cost (bytes used).
    pub memory: f64,
    /// Network cost (bytes transferred).
    pub network: f64,
}

impl Cost {
    /// Create a new cost.
    pub fn new(cpu: f64, io: f64, memory: f64, network: f64) -> Self {
        Self {
            cpu,
            io,
            memory,
            network,
        }
    }

    /// Zero cost.
    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    /// Total cost with weighted factors.
    pub fn total(&self) -> f64 {
        // Weights for different cost components
        const CPU_WEIGHT: f64 = 1.0;
        const IO_WEIGHT: f64 = 10.0;
        const MEMORY_WEIGHT: f64 = 0.1;
        const NETWORK_WEIGHT: f64 = 20.0;

        self.cpu * CPU_WEIGHT
            + self.io * IO_WEIGHT
            + self.memory * MEMORY_WEIGHT
            + self.network * NETWORK_WEIGHT
    }

    /// Add two costs.
    pub fn add(&self, other: &Cost) -> Cost {
        Cost::new(
            self.cpu + other.cpu,
            self.io + other.io,
            self.memory + other.memory,
            self.network + other.network,
        )
    }

    /// Multiply cost by a factor.
    pub fn multiply(&self, factor: f64) -> Cost {
        Cost::new(
            self.cpu * factor,
            self.io * factor,
            self.memory * factor,
            self.network * factor,
        )
    }
}

/// Statistics for a table or relation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statistics {
    /// Number of rows.
    pub row_count: usize,
    /// Average row size in bytes.
    pub row_size: usize,
    /// Column statistics.
    pub columns: Vec<ColumnStatistics>,
    /// Available indexes.
    pub indexes: Vec<IndexStatistics>,
}

impl Statistics {
    /// Create new statistics.
    pub fn new(row_count: usize, row_size: usize) -> Self {
        Self {
            row_count,
            row_size,
            columns: Vec::new(),
            indexes: Vec::new(),
        }
    }

    /// Total size in bytes.
    pub fn total_size(&self) -> usize {
        self.row_count * self.row_size
    }

    /// Add column statistics.
    pub fn with_column(mut self, col_stats: ColumnStatistics) -> Self {
        self.columns.push(col_stats);
        self
    }

    /// Add index statistics.
    pub fn with_index(mut self, idx_stats: IndexStatistics) -> Self {
        self.indexes.push(idx_stats);
        self
    }
}

/// Column statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnStatistics {
    /// Column name.
    pub name: String,
    /// Number of distinct values.
    pub distinct_count: usize,
    /// Number of null values.
    pub null_count: usize,
    /// Minimum value (if available).
    pub min_value: Option<Literal>,
    /// Maximum value (if available).
    pub max_value: Option<Literal>,
}

impl ColumnStatistics {
    /// Create new column statistics.
    pub fn new(name: String, distinct_count: usize, null_count: usize) -> Self {
        Self {
            name,
            distinct_count,
            null_count,
            min_value: None,
            max_value: None,
        }
    }

    /// Selectivity for equality predicate.
    pub fn equality_selectivity(&self, _total_rows: usize) -> f64 {
        if self.distinct_count == 0 {
            return 0.0;
        }
        1.0 / self.distinct_count as f64
    }

    /// Selectivity for range predicate.
    pub fn range_selectivity(&self, low: &Literal, high: &Literal) -> f64 {
        // Simplified range selectivity estimation
        match (&self.min_value, &self.max_value) {
            (Some(min), Some(max)) => {
                if let (Literal::Integer(min_val), Literal::Integer(max_val)) = (min, max) {
                    if let (Literal::Integer(low_val), Literal::Integer(high_val)) = (low, high) {
                        let range = (max_val - min_val) as f64;
                        if range > 0.0 {
                            let selected = (high_val - low_val) as f64;
                            return (selected / range).clamp(0.0, 1.0);
                        }
                    }
                }
                // Default range selectivity
                0.25
            }
            _ => 0.25, // Default range selectivity
        }
    }
}

/// Index statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatistics {
    /// Index name.
    pub name: String,
    /// Indexed columns.
    pub columns: Vec<String>,
    /// Index type (btree, rtree, hash).
    pub index_type: IndexType,
    /// Index size in bytes.
    pub size: usize,
    /// Height of index tree (for tree indexes).
    pub height: Option<usize>,
}

/// Index type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree index.
    BTree,
    /// R-tree index (spatial).
    RTree,
    /// Hash index.
    Hash,
}

impl IndexStatistics {
    /// Create new index statistics.
    pub fn new(name: String, columns: Vec<String>, index_type: IndexType, size: usize) -> Self {
        Self {
            name,
            columns,
            index_type,
            size,
            height: None,
        }
    }

    /// Cost of index lookup.
    pub fn lookup_cost(&self) -> Cost {
        match self.index_type {
            IndexType::BTree => {
                // B-tree lookup: log(n) * page_size
                let height = self.height.unwrap_or(4) as f64;
                Cost::new(height * 100.0, height * 8192.0, 0.0, 0.0)
            }
            IndexType::RTree => {
                // R-tree lookup: similar to B-tree
                let height = self.height.unwrap_or(4) as f64;
                Cost::new(height * 150.0, height * 8192.0, 0.0, 0.0)
            }
            IndexType::Hash => {
                // Hash lookup: O(1) average case
                Cost::new(50.0, 8192.0, 0.0, 0.0)
            }
        }
    }

    /// Cost of index scan.
    pub fn scan_cost(&self, selectivity: f64) -> Cost {
        let io = (self.size as f64 * selectivity).max(8192.0);
        Cost::new(io / 100.0, io, 0.0, 0.0)
    }
}

/// Cost model for query operations.
pub struct CostModel {
    /// Statistics cache.
    statistics: dashmap::DashMap<String, Statistics>,
}

impl CostModel {
    /// Create a new cost model.
    pub fn new() -> Self {
        Self {
            statistics: dashmap::DashMap::new(),
        }
    }

    /// Register statistics for a table.
    pub fn register_statistics(&self, table: String, stats: Statistics) {
        self.statistics.insert(table, stats);
    }

    /// Get statistics for a table.
    pub fn get_statistics(&self, table: &str) -> Option<Statistics> {
        self.statistics.get(table).map(|s| s.clone())
    }

    /// Estimate cost of a table scan.
    pub fn scan_cost(&self, table: &str) -> Cost {
        if let Some(stats) = self.get_statistics(table) {
            let total_size = stats.total_size() as f64;
            Cost::new(
                stats.row_count as f64 * 10.0,
                total_size,
                stats.row_size as f64,
                0.0,
            )
        } else {
            // Default cost for unknown table
            Cost::new(1_000_000.0, 1_000_000_000.0, 1000.0, 0.0)
        }
    }

    /// Estimate cost of a filter operation.
    pub fn filter_cost(&self, input_rows: usize, selectivity: f64) -> Cost {
        let output_rows = (input_rows as f64 * selectivity) as usize;
        Cost::new(
            input_rows as f64 * 2.0,
            0.0,
            output_rows as f64 * 100.0,
            0.0,
        )
    }

    /// Estimate cost of a join operation.
    pub fn join_cost(&self, left_rows: usize, right_rows: usize, join_type: JoinType) -> Cost {
        match join_type {
            JoinType::Inner | JoinType::Left | JoinType::Right | JoinType::Full => {
                // Hash join cost
                let build_cost = right_rows as f64 * 10.0;
                let probe_cost = left_rows as f64 * 5.0;
                let memory = right_rows as f64 * 100.0;
                Cost::new(build_cost + probe_cost, 0.0, memory, 0.0)
            }
            JoinType::Cross => {
                // Cross join cost (nested loop)
                let total_ops = (left_rows * right_rows) as f64;
                Cost::new(total_ops * 2.0, 0.0, total_ops * 100.0, 0.0)
            }
        }
    }

    /// Estimate cost of aggregation.
    pub fn aggregate_cost(&self, input_rows: usize, group_count: usize) -> Cost {
        Cost::new(
            input_rows as f64 * 5.0,
            0.0,
            group_count as f64 * 200.0,
            0.0,
        )
    }

    /// Estimate cost of sorting.
    pub fn sort_cost(&self, input_rows: usize) -> Cost {
        // Sort cost: O(n log n)
        let n = input_rows as f64;
        let ops = n * n.log2();
        Cost::new(ops * 10.0, 0.0, n * 100.0, 0.0)
    }

    /// Estimate selectivity of a predicate.
    pub fn estimate_selectivity(&self, table: &str, expr: &Expr) -> f64 {
        match expr {
            Expr::BinaryOp { left, op, right } => match op {
                BinaryOperator::Eq => {
                    if let Expr::Column { name, .. } = &**left {
                        if let Some(stats) = self.get_statistics(table) {
                            if let Some(col_stats) = stats.columns.iter().find(|c| c.name == *name)
                            {
                                return col_stats.equality_selectivity(stats.row_count);
                            }
                        }
                    }
                    0.1 // Default equality selectivity
                }
                BinaryOperator::Lt
                | BinaryOperator::LtEq
                | BinaryOperator::Gt
                | BinaryOperator::GtEq => 0.33, // Default range selectivity
                BinaryOperator::And => {
                    let left_sel = self.estimate_selectivity(table, left);
                    let right_sel = self.estimate_selectivity(table, right);
                    left_sel * right_sel
                }
                BinaryOperator::Or => {
                    let left_sel = self.estimate_selectivity(table, left);
                    let right_sel = self.estimate_selectivity(table, right);
                    left_sel + right_sel - (left_sel * right_sel)
                }
                _ => 0.5, // Default selectivity
            },
            Expr::UnaryOp {
                op: UnaryOperator::Not,
                expr,
            } => 1.0 - self.estimate_selectivity(table, expr),
            Expr::Function { name, .. } => {
                // Spatial predicates have lower selectivity
                match name.to_uppercase().as_str() {
                    "ST_INTERSECTS" | "ST_CONTAINS" | "ST_WITHIN" => 0.01,
                    _ => 0.5,
                }
            }
            _ => 0.5, // Default selectivity
        }
    }
}

impl Default for CostModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_total() {
        let cost = Cost::new(100.0, 1000.0, 100.0, 500.0);
        assert!(cost.total() > 0.0);
    }

    #[test]
    fn test_cost_add() {
        let cost1 = Cost::new(100.0, 1000.0, 100.0, 0.0);
        let cost2 = Cost::new(50.0, 500.0, 50.0, 0.0);
        let total = cost1.add(&cost2);
        assert_eq!(total.cpu, 150.0);
        assert_eq!(total.io, 1500.0);
    }

    #[test]
    fn test_statistics() {
        let stats = Statistics::new(1000, 100)
            .with_column(ColumnStatistics::new("id".to_string(), 1000, 0))
            .with_index(IndexStatistics::new(
                "idx_id".to_string(),
                vec!["id".to_string()],
                IndexType::BTree,
                10000,
            ));

        assert_eq!(stats.row_count, 1000);
        assert_eq!(stats.total_size(), 100_000);
    }

    #[test]
    fn test_cost_model() {
        let model = CostModel::new();
        let stats = Statistics::new(10000, 100);
        model.register_statistics("users".to_string(), stats);

        let scan_cost = model.scan_cost("users");
        assert!(scan_cost.total() > 0.0);
    }
}
