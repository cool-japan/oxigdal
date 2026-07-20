//! SQL query builder for Kinesis Analytics

use serde::{Deserialize, Serialize};

/// SQL query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlQuery {
    /// Query text
    pub query: String,
}

impl SqlQuery {
    /// Creates a new SQL query
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
        }
    }

    /// Gets the query text
    pub fn as_str(&self) -> &str {
        &self.query
    }
}

/// Query builder for Kinesis Analytics SQL
pub struct QueryBuilder {
    select: Vec<String>,
    from: Option<String>,
    where_clause: Vec<String>,
    group_by: Vec<String>,
    having: Vec<String>,
    order_by: Vec<String>,
    limit: Option<usize>,
    window: Option<String>,
}

impl QueryBuilder {
    /// Creates a new query builder
    pub fn new() -> Self {
        Self {
            select: Vec::new(),
            from: None,
            where_clause: Vec::new(),
            group_by: Vec::new(),
            having: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            window: None,
        }
    }

    /// Adds a SELECT column
    pub fn select(mut self, column: impl Into<String>) -> Self {
        self.select.push(column.into());
        self
    }

    /// Sets the FROM table/stream
    pub fn from(mut self, table: impl Into<String>) -> Self {
        self.from = Some(table.into());
        self
    }

    /// Adds a WHERE condition
    pub fn where_clause(mut self, condition: impl Into<String>) -> Self {
        self.where_clause.push(condition.into());
        self
    }

    /// Adds a GROUP BY column
    pub fn group_by(mut self, column: impl Into<String>) -> Self {
        self.group_by.push(column.into());
        self
    }

    /// Adds a HAVING condition
    pub fn having(mut self, condition: impl Into<String>) -> Self {
        self.having.push(condition.into());
        self
    }

    /// Adds an ORDER BY column
    pub fn order_by(mut self, column: impl Into<String>) -> Self {
        self.order_by.push(column.into());
        self
    }

    /// Sets the LIMIT
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Sets the window
    pub fn window(mut self, window: impl Into<String>) -> Self {
        self.window = Some(window.into());
        self
    }

    /// Builds the SQL query
    pub fn build(self) -> SqlQuery {
        let mut query = String::new();

        // SELECT clause
        query.push_str("SELECT ");
        if self.select.is_empty() {
            query.push('*');
        } else {
            query.push_str(&self.select.join(", "));
        }

        // FROM clause
        if let Some(from) = &self.from {
            query.push_str("\nFROM ");
            query.push_str(from);

            // Window specification
            if let Some(window) = &self.window {
                query.push_str(&format!("\n{}", window));
            }
        }

        // WHERE clause
        if !self.where_clause.is_empty() {
            query.push_str("\nWHERE ");
            query.push_str(&self.where_clause.join(" AND "));
        }

        // GROUP BY clause
        if !self.group_by.is_empty() {
            query.push_str("\nGROUP BY ");
            query.push_str(&self.group_by.join(", "));
        }

        // HAVING clause
        if !self.having.is_empty() {
            query.push_str("\nHAVING ");
            query.push_str(&self.having.join(" AND "));
        }

        // ORDER BY clause
        if !self.order_by.is_empty() {
            query.push_str("\nORDER BY ");
            query.push_str(&self.order_by.join(", "));
        }

        // LIMIT clause
        if let Some(limit) = self.limit {
            query.push_str(&format!("\nLIMIT {}", limit));
        }

        SqlQuery::new(query)
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Common SQL functions for Kinesis Analytics
pub struct SqlFunctions;

impl SqlFunctions {
    /// COUNT aggregation
    pub fn count(column: &str) -> String {
        format!("COUNT({})", column)
    }

    /// SUM aggregation
    pub fn sum(column: &str) -> String {
        format!("SUM({})", column)
    }

    /// AVG aggregation
    pub fn avg(column: &str) -> String {
        format!("AVG({})", column)
    }

    /// MIN aggregation
    pub fn min(column: &str) -> String {
        format!("MIN({})", column)
    }

    /// MAX aggregation
    pub fn max(column: &str) -> String {
        format!("MAX({})", column)
    }

    /// FLOOR function
    pub fn floor(expr: &str) -> String {
        format!("FLOOR({})", expr)
    }

    /// CEIL function
    pub fn ceil(expr: &str) -> String {
        format!("CEIL({})", expr)
    }

    /// ROUND function
    pub fn round(expr: &str, decimals: i32) -> String {
        format!("ROUND({}, {})", expr, decimals)
    }

    /// CURRENT_TIMESTAMP
    pub fn current_timestamp() -> String {
        "CURRENT_TIMESTAMP".to_string()
    }

    /// CAST function
    pub fn cast(expr: &str, data_type: &str) -> String {
        format!("CAST({} AS {})", expr, data_type)
    }

    /// SUBSTRING function
    pub fn substring(expr: &str, start: i32, length: i32) -> String {
        format!("SUBSTRING({}, {}, {})", expr, start, length)
    }

    /// UPPER function
    pub fn upper(expr: &str) -> String {
        format!("UPPER({})", expr)
    }

    /// LOWER function
    pub fn lower(expr: &str) -> String {
        format!("LOWER({})", expr)
    }

    /// CONCAT function
    pub fn concat(exprs: &[&str]) -> String {
        format!("CONCAT({})", exprs.join(", "))
    }

    /// LAG window function
    pub fn lag(column: &str, offset: i32) -> String {
        format!("LAG({}, {})", column, offset)
    }

    /// LEAD window function
    pub fn lead(column: &str, offset: i32) -> String {
        format!("LEAD({}, {})", column, offset)
    }

    /// ROW_NUMBER window function
    pub fn row_number() -> String {
        "ROW_NUMBER()".to_string()
    }

    /// RANK window function
    pub fn rank() -> String {
        "RANK()".to_string()
    }

    /// DENSE_RANK window function
    pub fn dense_rank() -> String {
        "DENSE_RANK()".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_query() {
        let query = QueryBuilder::new()
            .select("*")
            .from("SOURCE_SQL_STREAM")
            .build();

        assert!(query.as_str().contains("SELECT *"));
        assert!(query.as_str().contains("FROM SOURCE_SQL_STREAM"));
    }

    #[test]
    fn test_query_with_where() {
        let query = QueryBuilder::new()
            .select("userId")
            .select("amount")
            .from("TRANSACTIONS")
            .where_clause("amount > 100")
            .where_clause("status = 'COMPLETED'")
            .build();

        assert!(query.as_str().contains("SELECT userId, amount"));
        assert!(
            query
                .as_str()
                .contains("WHERE amount > 100 AND status = 'COMPLETED'")
        );
    }

    #[test]
    fn test_query_with_group_by() {
        let query = QueryBuilder::new()
            .select("userId")
            .select("COUNT(*) as transaction_count")
            .from("TRANSACTIONS")
            .group_by("userId")
            .having("COUNT(*) > 10")
            .build();

        assert!(query.as_str().contains("GROUP BY userId"));
        assert!(query.as_str().contains("HAVING COUNT(*) > 10"));
    }

    #[test]
    fn test_query_with_order_and_limit() {
        let query = QueryBuilder::new()
            .select("*")
            .from("EVENTS")
            .order_by("timestamp DESC")
            .limit(100)
            .build();

        assert!(query.as_str().contains("ORDER BY timestamp DESC"));
        assert!(query.as_str().contains("LIMIT 100"));
    }

    #[test]
    fn test_sql_functions() {
        assert_eq!(SqlFunctions::count("*"), "COUNT(*)");
        assert_eq!(SqlFunctions::sum("amount"), "SUM(amount)");
        assert_eq!(SqlFunctions::avg("price"), "AVG(price)");
        assert_eq!(SqlFunctions::max("temperature"), "MAX(temperature)");
        assert_eq!(SqlFunctions::min("temperature"), "MIN(temperature)");
    }

    #[test]
    fn test_string_functions() {
        assert_eq!(SqlFunctions::upper("name"), "UPPER(name)");
        assert_eq!(SqlFunctions::lower("NAME"), "LOWER(NAME)");
        assert_eq!(
            SqlFunctions::concat(&["first_name", "last_name"]),
            "CONCAT(first_name, last_name)"
        );
        assert_eq!(
            SqlFunctions::substring("text", 1, 10),
            "SUBSTRING(text, 1, 10)"
        );
    }

    #[test]
    fn test_window_functions() {
        assert_eq!(SqlFunctions::lag("price", 1), "LAG(price, 1)");
        assert_eq!(SqlFunctions::lead("price", 1), "LEAD(price, 1)");
        assert_eq!(SqlFunctions::row_number(), "ROW_NUMBER()");
        assert_eq!(SqlFunctions::rank(), "RANK()");
        assert_eq!(SqlFunctions::dense_rank(), "DENSE_RANK()");
    }

    #[test]
    fn test_math_functions() {
        assert_eq!(SqlFunctions::floor("value"), "FLOOR(value)");
        assert_eq!(SqlFunctions::ceil("value"), "CEIL(value)");
        assert_eq!(SqlFunctions::round("value", 2), "ROUND(value, 2)");
        assert_eq!(
            SqlFunctions::cast("value", "DOUBLE"),
            "CAST(value AS DOUBLE)"
        );
    }
}
