//! Query plan explanation and visualization.

use crate::optimizer::OptimizedQuery;
use crate::optimizer::cost_model::Cost;
use crate::parser::ast::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Query explain output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainPlan {
    /// Query plan nodes.
    pub nodes: Vec<ExplainNode>,
    /// Total estimated cost.
    pub total_cost: Cost,
    /// Execution statistics (if available).
    pub statistics: Option<ExecutionStatistics>,
}

impl ExplainPlan {
    /// Create explain plan from optimized query.
    pub fn from_optimized(query: &OptimizedQuery) -> Self {
        let nodes = Self::build_nodes(&query.statement);
        Self {
            nodes,
            total_cost: query.optimized_cost,
            statistics: None,
        }
    }

    /// Build explain nodes from statement.
    fn build_nodes(stmt: &Statement) -> Vec<ExplainNode> {
        match stmt {
            Statement::Select(select) => Self::build_select_nodes(select),
        }
    }

    /// Build nodes for SELECT statement.
    fn build_select_nodes(select: &SelectStatement) -> Vec<ExplainNode> {
        let mut nodes = Vec::new();
        let mut node_id = 0;

        // FROM clause
        if let Some(ref table_ref) = select.from {
            Self::build_table_nodes(table_ref, &mut nodes, &mut node_id, 0);
        }

        // WHERE clause
        if select.selection.is_some() {
            nodes.push(ExplainNode {
                id: node_id,
                node_type: NodeType::Filter,
                description: "Filter".to_string(),
                details: format!("Predicate: {:?}", select.selection),
                cost: Cost::zero(),
                rows: None,
                depth: 0,
            });
            node_id += 1;
        }

        // GROUP BY / Aggregation
        if !select.group_by.is_empty() {
            nodes.push(ExplainNode {
                id: node_id,
                node_type: NodeType::Aggregate,
                description: "Aggregate".to_string(),
                details: format!("Group by: {:?}", select.group_by),
                cost: Cost::zero(),
                rows: None,
                depth: 0,
            });
            node_id += 1;
        }

        // ORDER BY
        if !select.order_by.is_empty() {
            nodes.push(ExplainNode {
                id: node_id,
                node_type: NodeType::Sort,
                description: "Sort".to_string(),
                details: format!("Order by: {:?}", select.order_by),
                cost: Cost::zero(),
                rows: None,
                depth: 0,
            });
            node_id += 1;
        }

        // LIMIT
        if select.limit.is_some() {
            nodes.push(ExplainNode {
                id: node_id,
                node_type: NodeType::Limit,
                description: "Limit".to_string(),
                details: format!("Limit: {:?}", select.limit),
                cost: Cost::zero(),
                rows: select.limit,
                depth: 0,
            });
        }

        nodes
    }

    /// Build nodes for table reference.
    fn build_table_nodes(
        table_ref: &TableReference,
        nodes: &mut Vec<ExplainNode>,
        node_id: &mut usize,
        depth: usize,
    ) {
        match table_ref {
            TableReference::Table { name, .. } => {
                nodes.push(ExplainNode {
                    id: *node_id,
                    node_type: NodeType::TableScan,
                    description: "Table Scan".to_string(),
                    details: format!("Table: {}", name),
                    cost: Cost::zero(),
                    rows: None,
                    depth,
                });
                *node_id += 1;
            }
            TableReference::Join {
                left,
                right,
                join_type,
                ..
            } => {
                Self::build_table_nodes(left, nodes, node_id, depth + 1);
                Self::build_table_nodes(right, nodes, node_id, depth + 1);

                nodes.push(ExplainNode {
                    id: *node_id,
                    node_type: NodeType::Join,
                    description: format!("{:?} Join", join_type),
                    details: String::new(),
                    cost: Cost::zero(),
                    rows: None,
                    depth,
                });
                *node_id += 1;
            }
            TableReference::Subquery { query, alias } => {
                let subquery_nodes = Self::build_select_nodes(query);
                nodes.extend(subquery_nodes);

                nodes.push(ExplainNode {
                    id: *node_id,
                    node_type: NodeType::Subquery,
                    description: "Subquery".to_string(),
                    details: format!("Alias: {}", alias),
                    cost: Cost::zero(),
                    rows: None,
                    depth,
                });
                *node_id += 1;
            }
        }
    }

    /// Format as text.
    pub fn format_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Query Execution Plan:\n");
        output.push_str(&format!("Total Cost: {:.2}\n", self.total_cost.total()));
        output.push('\n');

        for node in &self.nodes {
            let indent = "  ".repeat(node.depth);
            output.push_str(&format!(
                "{}[{}] {}: {}\n",
                indent, node.id, node.description, node.details
            ));
            if let Some(rows) = node.rows {
                output.push_str(&format!("{}    Rows: {}\n", indent, rows));
            }
            output.push_str(&format!("{}    Cost: {:.2}\n", indent, node.cost.total()));
        }

        if let Some(ref stats) = self.statistics {
            output.push_str("\nExecution Statistics:\n");
            output.push_str(&format!("  Execution Time: {:?}\n", stats.execution_time));
            output.push_str(&format!("  Rows Processed: {}\n", stats.rows_processed));
            output.push_str(&format!("  Rows Returned: {}\n", stats.rows_returned));
        }

        output
    }

    /// Format as JSON.
    pub fn format_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Query plan node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainNode {
    /// Node ID.
    pub id: usize,
    /// Node type.
    pub node_type: NodeType,
    /// Description.
    pub description: String,
    /// Additional details.
    pub details: String,
    /// Estimated cost.
    pub cost: Cost,
    /// Estimated rows.
    pub rows: Option<usize>,
    /// Tree depth.
    pub depth: usize,
}

/// Node type in query plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    /// Table scan.
    TableScan,
    /// Index scan.
    IndexScan,
    /// Filter operation.
    Filter,
    /// Join operation.
    Join,
    /// Aggregate operation.
    Aggregate,
    /// Sort operation.
    Sort,
    /// Limit operation.
    Limit,
    /// Subquery.
    Subquery,
}

/// Execution statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStatistics {
    /// Total execution time.
    pub execution_time: std::time::Duration,
    /// Number of rows processed.
    pub rows_processed: usize,
    /// Number of rows returned.
    pub rows_returned: usize,
    /// Peak memory usage in bytes.
    pub peak_memory: Option<usize>,
}

impl fmt::Display for ExplainPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::sql::parse_sql;

    #[test]
    fn test_explain_plan() {
        let sql = "SELECT id, name FROM users WHERE age > 18 ORDER BY name LIMIT 10";
        let stmt = parse_sql(sql).ok();

        if let Some(stmt) = stmt {
            let nodes = ExplainPlan::build_nodes(&stmt);
            assert!(!nodes.is_empty());
        }
    }

    #[test]
    fn test_explain_format_text() {
        let plan = ExplainPlan {
            nodes: vec![ExplainNode {
                id: 0,
                node_type: NodeType::TableScan,
                description: "Table Scan".to_string(),
                details: "Table: users".to_string(),
                cost: Cost::zero(),
                rows: Some(1000),
                depth: 0,
            }],
            total_cost: Cost::new(100.0, 1000.0, 100.0, 0.0),
            statistics: None,
        };

        let text = plan.format_text();
        assert!(text.contains("Query Execution Plan"));
        assert!(text.contains("Table Scan"));
    }
}
