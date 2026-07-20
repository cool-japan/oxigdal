//! Error types for query processing.
//!
//! # Error Codes
//!
//! Each error variant has an associated error code (e.g., Q001, Q002) for easier
//! debugging and documentation. Error codes are stable across versions.
//!
//! # Helper Methods
//!
//! All error types provide:
//! - `code()` - Returns the error code
//! - `suggestion()` - Returns helpful hints including alternative query structures
//! - `context()` - Returns additional context including rule identification and query fragments

/// Result type for query operations.
pub type Result<T> = std::result::Result<T, QueryError>;

/// Errors that can occur during query processing.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// Parse error with position information.
    #[error("Parse error at line {line}, column {column}: {message}")]
    ParseError {
        /// Error message.
        message: String,
        /// Line number (1-based).
        line: usize,
        /// Column number (1-based).
        column: usize,
    },

    /// Semantic error in query.
    #[error("Semantic error: {0}")]
    SemanticError(String),

    /// Optimization error.
    #[error("Optimization error: {0}")]
    OptimizationError(String),

    /// Execution error.
    #[error("Execution error: {0}")]
    ExecutionError(String),

    /// Type mismatch error.
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Expected type.
        expected: String,
        /// Actual type.
        actual: String,
    },

    /// Column not found error.
    #[error("Column not found: {0}")]
    ColumnNotFound(String),

    /// Table not found error.
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// Function not found error.
    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    /// Invalid argument error.
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Index not found error.
    #[error("Index not found: {0}")]
    IndexNotFound(String),

    /// IO error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Internal error.
    #[error("Internal error: {0}")]
    InternalError(String),

    /// Unsupported operation.
    #[error("Unsupported operation: {0}")]
    Unsupported(String),

    /// SQL parser error.
    #[error("SQL parser error: {0}")]
    SqlParserError(String),

    /// Cache error.
    #[error("Cache error: {0}")]
    CacheError(String),

    /// Parallel execution error.
    #[error("Parallel execution error: {0}")]
    ParallelError(String),
}

impl From<sqlparser::parser::ParserError> for QueryError {
    fn from(err: sqlparser::parser::ParserError) -> Self {
        QueryError::SqlParserError(err.to_string())
    }
}

impl QueryError {
    /// Create a parse error.
    pub fn parse_error(message: impl Into<String>, line: usize, column: usize) -> Self {
        QueryError::ParseError {
            message: message.into(),
            line,
            column,
        }
    }

    /// Create a semantic error.
    pub fn semantic(message: impl Into<String>) -> Self {
        QueryError::SemanticError(message.into())
    }

    /// Create an optimization error.
    pub fn optimization(message: impl Into<String>) -> Self {
        QueryError::OptimizationError(message.into())
    }

    /// Create an execution error.
    pub fn execution(message: impl Into<String>) -> Self {
        QueryError::ExecutionError(message.into())
    }

    /// Create a type mismatch error.
    pub fn type_mismatch(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        QueryError::TypeMismatch {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Create an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        QueryError::InternalError(message.into())
    }

    /// Create an unsupported operation error.
    pub fn unsupported(message: impl Into<String>) -> Self {
        QueryError::Unsupported(message.into())
    }

    /// Get the error code for this query error
    ///
    /// Error codes are stable across versions and can be used for documentation
    /// and error handling.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ParseError { .. } => "Q001",
            Self::SemanticError(_) => "Q002",
            Self::OptimizationError(_) => "Q003",
            Self::ExecutionError(_) => "Q004",
            Self::TypeMismatch { .. } => "Q005",
            Self::ColumnNotFound(_) => "Q006",
            Self::TableNotFound(_) => "Q007",
            Self::FunctionNotFound(_) => "Q008",
            Self::InvalidArgument(_) => "Q009",
            Self::IndexNotFound(_) => "Q010",
            Self::IoError(_) => "Q011",
            Self::InternalError(_) => "Q012",
            Self::Unsupported(_) => "Q013",
            Self::SqlParserError(_) => "Q014",
            Self::CacheError(_) => "Q015",
            Self::ParallelError(_) => "Q016",
        }
    }

    /// Get a helpful suggestion for fixing this query error
    ///
    /// Returns a human-readable suggestion including alternative query structures.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::ParseError { .. } => Some(
                "Check SQL syntax. Common issues: missing commas, unmatched parentheses, or incorrect keywords",
            ),
            Self::SemanticError(msg) => {
                if msg.contains("aggregate") {
                    Some(
                        "When using aggregate functions, non-aggregated columns must be in GROUP BY",
                    )
                } else if msg.contains("subquery") {
                    Some("Ensure subqueries return the expected number of columns")
                } else {
                    Some("Verify table and column references are correct")
                }
            }
            Self::OptimizationError(msg) => {
                if msg.contains("join") {
                    Some("Try simplifying the join structure or adding indexes on join columns")
                } else if msg.contains("predicate") {
                    Some("Rewrite complex predicates or break into multiple simpler conditions")
                } else {
                    Some("Simplify the query or add appropriate indexes")
                }
            }
            Self::ExecutionError(_) => Some(
                "Check data values and constraints. Ensure operations are valid for the data types",
            ),
            Self::TypeMismatch { .. } => {
                Some("Cast values to the expected type or modify the query to use compatible types")
            }
            Self::ColumnNotFound(_) => Some(
                "Use DESCRIBE or SELECT * to list available columns. Check for typos or case sensitivity",
            ),
            Self::TableNotFound(_) => {
                Some("Verify the table name is correct. Use SHOW TABLES to list available tables")
            }
            Self::FunctionNotFound(_) => {
                Some("Check function name spelling. Use built-in functions or create a UDF")
            }
            Self::InvalidArgument(_) => {
                Some("Check function documentation for correct argument types and count")
            }
            Self::IndexNotFound(_) => {
                Some("Create an index using CREATE INDEX or use a different access pattern")
            }
            Self::IoError(_) => {
                Some("Check file permissions and disk space. Ensure data files are accessible")
            }
            Self::InternalError(_) => {
                Some("This is likely a bug. Please report it with the query that triggered it")
            }
            Self::Unsupported(_) => {
                Some("Use an alternative query structure or feature that is supported")
            }
            Self::SqlParserError(_) => {
                Some("Fix SQL syntax errors. Refer to SQL standard or documentation")
            }
            Self::CacheError(_) => Some("Clear cache or increase cache size"),
            Self::ParallelError(_) => {
                Some("Reduce parallelism level or check for data race conditions")
            }
        }
    }

    /// Get additional context about this query error
    ///
    /// Returns structured context including rule identification and query fragments.
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::ParseError {
                message,
                line,
                column,
            } => ErrorContext::new("parse_error")
                .with_detail("message", message.clone())
                .with_detail("line", line.to_string())
                .with_detail("column", column.to_string()),
            Self::SemanticError(msg) => {
                ErrorContext::new("semantic_error").with_detail("message", msg.clone())
            }
            Self::OptimizationError(msg) => ErrorContext::new("optimization_error")
                .with_detail("message", msg.clone())
                .with_detail("phase", self.extract_optimization_phase(msg)),
            Self::ExecutionError(msg) => {
                ErrorContext::new("execution_error").with_detail("message", msg.clone())
            }
            Self::TypeMismatch { expected, actual } => ErrorContext::new("type_mismatch")
                .with_detail("expected", expected.clone())
                .with_detail("actual", actual.clone()),
            Self::ColumnNotFound(name) => {
                ErrorContext::new("column_not_found").with_detail("column", name.clone())
            }
            Self::TableNotFound(name) => {
                ErrorContext::new("table_not_found").with_detail("table", name.clone())
            }
            Self::FunctionNotFound(name) => {
                ErrorContext::new("function_not_found").with_detail("function", name.clone())
            }
            Self::InvalidArgument(msg) => {
                ErrorContext::new("invalid_argument").with_detail("message", msg.clone())
            }
            Self::IndexNotFound(name) => {
                ErrorContext::new("index_not_found").with_detail("index", name.clone())
            }
            Self::IoError(e) => ErrorContext::new("io_error").with_detail("error", e.to_string()),
            Self::InternalError(msg) => {
                ErrorContext::new("internal_error").with_detail("message", msg.clone())
            }
            Self::Unsupported(msg) => {
                ErrorContext::new("unsupported").with_detail("message", msg.clone())
            }
            Self::SqlParserError(msg) => {
                ErrorContext::new("sql_parser_error").with_detail("message", msg.clone())
            }
            Self::CacheError(msg) => {
                ErrorContext::new("cache_error").with_detail("message", msg.clone())
            }
            Self::ParallelError(msg) => {
                ErrorContext::new("parallel_error").with_detail("message", msg.clone())
            }
        }
    }

    /// Extract optimization phase from error message
    fn extract_optimization_phase(&self, msg: &str) -> String {
        if msg.contains("predicate pushdown") {
            "predicate_pushdown".to_string()
        } else if msg.contains("join reorder") {
            "join_reordering".to_string()
        } else if msg.contains("projection") {
            "projection_pushdown".to_string()
        } else if msg.contains("CSE") || msg.contains("common subexpression") {
            "common_subexpression_elimination".to_string()
        } else {
            "unknown".to_string()
        }
    }
}

/// Additional context information for query errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Error category for grouping similar errors
    pub category: &'static str,
    /// Additional details including rule identification and query fragments
    pub details: Vec<(String, String)>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(category: &'static str) -> Self {
        Self {
            category,
            details: Vec::new(),
        }
    }

    /// Add a detail to the context
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push((key.into(), value.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        let err = QueryError::ParseError {
            message: "test".to_string(),
            line: 1,
            column: 5,
        };
        assert_eq!(err.code(), "Q001");

        let err = QueryError::OptimizationError("join reorder failed".to_string());
        assert_eq!(err.code(), "Q003");

        let err = QueryError::ColumnNotFound("id".to_string());
        assert_eq!(err.code(), "Q006");
    }

    #[test]
    fn test_error_suggestions() {
        let err = QueryError::OptimizationError("join reorder failed".to_string());
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("join")));

        let err = QueryError::ColumnNotFound("id".to_string());
        assert!(err.suggestion().is_some());
        assert!(
            err.suggestion()
                .is_some_and(|s| s.contains("DESCRIBE") || s.contains("SELECT *"))
        );

        let err = QueryError::SemanticError("aggregate function".to_string());
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("GROUP BY")));
    }

    #[test]
    fn test_error_context() {
        let err = QueryError::ParseError {
            message: "unexpected token".to_string(),
            line: 10,
            column: 25,
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "parse_error");
        assert!(ctx.details.iter().any(|(k, v)| k == "line" && v == "10"));
        assert!(ctx.details.iter().any(|(k, v)| k == "column" && v == "25"));

        let err = QueryError::TypeMismatch {
            expected: "INTEGER".to_string(),
            actual: "TEXT".to_string(),
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "type_mismatch");
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "expected" && v == "INTEGER")
        );
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "actual" && v == "TEXT")
        );
    }

    #[test]
    fn test_optimization_phase_extraction() {
        let err = QueryError::OptimizationError("predicate pushdown failed".to_string());
        let ctx = err.context();
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "phase" && v == "predicate_pushdown")
        );

        let err = QueryError::OptimizationError("join reorder failed".to_string());
        let ctx = err.context();
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "phase" && v == "join_reordering")
        );

        let err = QueryError::OptimizationError("CSE failed".to_string());
        let ctx = err.context();
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "phase" && v == "common_subexpression_elimination")
        );
    }

    #[test]
    fn test_helper_constructors() {
        let err = QueryError::parse_error("test", 1, 5);
        assert!(matches!(err, QueryError::ParseError { .. }));

        let err = QueryError::type_mismatch("int", "string");
        assert!(matches!(err, QueryError::TypeMismatch { .. }));

        let err = QueryError::optimization("test");
        assert!(matches!(err, QueryError::OptimizationError(_)));
    }
}
