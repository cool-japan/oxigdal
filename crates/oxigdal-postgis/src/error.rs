//! Error types for OxiGDAL PostGIS operations
//!
//! This module provides comprehensive error types for PostgreSQL/PostGIS integration.
//! All error types implement [`std::error::Error`] via [`thiserror`].

use thiserror::Error;

/// The main result type for PostGIS operations
pub type Result<T> = std::result::Result<T, PostGisError>;

/// The main error type for PostGIS operations
#[derive(Debug, Error)]
pub enum PostGisError {
    /// Connection error occurred
    #[error("Connection error: {0}")]
    Connection(#[from] ConnectionError),

    /// Query execution error
    #[error("Query error: {0}")]
    Query(#[from] QueryError),

    /// Type conversion error
    #[error("Conversion error: {0}")]
    Conversion(#[from] ConversionError),

    /// Transaction error
    #[error("Transaction error: {0}")]
    Transaction(#[from] TransactionError),

    /// WKB encoding/decoding error
    #[error("WKB error: {0}")]
    Wkb(#[from] WkbError),

    /// SQL generation error
    #[error("SQL error: {0}")]
    Sql(#[from] SqlError),

    /// OxiGDAL core error
    #[error("Core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// Invalid parameter
    #[error("Invalid parameter '{parameter}': {message}")]
    InvalidParameter {
        /// The parameter name
        parameter: &'static str,
        /// Error message
        message: String,
    },

    /// Operation not supported
    #[error("Not supported: {operation}")]
    NotSupported {
        /// The unsupported operation
        operation: String,
    },
}

/// Connection-related errors
#[derive(Debug, Error)]
pub enum ConnectionError {
    /// Failed to establish connection
    #[error("Failed to connect to database: {message}")]
    ConnectionFailed {
        /// Error message
        message: String,
    },

    /// Invalid connection string
    #[error("Invalid connection string: {message}")]
    InvalidConnectionString {
        /// Error message
        message: String,
    },

    /// Connection pool error
    #[error("Connection pool error: {message}")]
    PoolError {
        /// Error message
        message: String,
    },

    /// Connection timeout
    #[error("Connection timeout after {seconds} seconds")]
    Timeout {
        /// Timeout duration in seconds
        seconds: u64,
    },

    /// SSL/TLS error
    #[error("SSL/TLS error: {message}")]
    Ssl {
        /// Error message
        message: String,
    },

    /// Authentication failed
    #[error("Authentication failed: {message}")]
    AuthenticationFailed {
        /// Error message
        message: String,
    },

    /// Database not found
    #[error("Database not found: {database}")]
    DatabaseNotFound {
        /// Database name
        database: String,
    },

    /// PostGIS extension not installed
    #[error("PostGIS extension not installed or enabled")]
    PostGisNotInstalled,
}

/// Query execution errors
#[derive(Debug, Error)]
pub enum QueryError {
    /// Query execution failed
    #[error("Query execution failed: {message}")]
    ExecutionFailed {
        /// Error message
        message: String,
    },

    /// Syntax error in SQL query
    #[error("SQL syntax error: {message}")]
    SyntaxError {
        /// Error message
        message: String,
    },

    /// Table not found
    #[error("Table not found: {table}")]
    TableNotFound {
        /// Table name
        table: String,
    },

    /// Column not found
    #[error("Column not found: {column} in table {table}")]
    ColumnNotFound {
        /// Column name
        column: String,
        /// Table name
        table: String,
    },

    /// No rows returned
    #[error("No rows returned for query")]
    NoRows,

    /// Too many rows returned
    #[error("Expected {expected} rows, got {actual}")]
    TooManyRows {
        /// Expected number of rows
        expected: usize,
        /// Actual number of rows
        actual: usize,
    },

    /// Invalid spatial reference system
    #[error("Invalid SRID: {srid}")]
    InvalidSrid {
        /// SRID value
        srid: i32,
    },

    /// Spatial index not found
    #[error("Spatial index not found for table: {table}")]
    SpatialIndexNotFound {
        /// Table name
        table: String,
    },

    /// Query timeout
    #[error("Query timeout after {seconds} seconds")]
    Timeout {
        /// Timeout duration in seconds
        seconds: u64,
    },
}

/// Type conversion errors
#[derive(Debug, Error)]
pub enum ConversionError {
    /// Failed to convert PostgreSQL type to OxiGDAL type
    #[error("Failed to convert from PostgreSQL type '{pg_type}' to OxiGDAL type: {message}")]
    FromPostgres {
        /// PostgreSQL type name
        pg_type: String,
        /// Error message
        message: String,
    },

    /// Failed to convert OxiGDAL type to PostgreSQL type
    #[error("Failed to convert from OxiGDAL type to PostgreSQL type '{pg_type}': {message}")]
    ToPostgres {
        /// PostgreSQL type name
        pg_type: String,
        /// Error message
        message: String,
    },

    /// Unsupported geometry type
    #[error("Unsupported geometry type: {geometry_type}")]
    UnsupportedGeometry {
        /// Geometry type name
        geometry_type: String,
    },

    /// Invalid SRID
    #[error("Invalid SRID value: {srid}")]
    InvalidSrid {
        /// SRID value
        srid: i32,
    },

    /// NULL value encountered
    #[error("Unexpected NULL value in column: {column}")]
    UnexpectedNull {
        /// Column name
        column: String,
    },

    /// Type mismatch
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Expected type
        expected: String,
        /// Actual type
        actual: String,
    },

    /// Invalid dimension
    #[error("Invalid geometry dimension: {dimension}")]
    InvalidDimension {
        /// Dimension value
        dimension: u32,
    },
}

/// Transaction-related errors
#[derive(Debug, Error)]
pub enum TransactionError {
    /// Failed to begin transaction
    #[error("Failed to begin transaction: {message}")]
    BeginFailed {
        /// Error message
        message: String,
    },

    /// Failed to commit transaction
    #[error("Failed to commit transaction: {message}")]
    CommitFailed {
        /// Error message
        message: String,
    },

    /// Failed to rollback transaction
    #[error("Failed to rollback transaction: {message}")]
    RollbackFailed {
        /// Error message
        message: String,
    },

    /// Transaction already in progress
    #[error("Transaction already in progress")]
    AlreadyInTransaction,

    /// No active transaction
    #[error("No active transaction")]
    NoActiveTransaction,

    /// Savepoint error
    #[error("Savepoint error: {message}")]
    SavepointError {
        /// Error message
        message: String,
    },

    /// Deadlock detected
    #[error("Deadlock detected: {message}")]
    Deadlock {
        /// Error message
        message: String,
    },
}

/// WKB encoding/decoding errors
#[derive(Debug, Error)]
pub enum WkbError {
    /// Invalid WKB format
    #[error("Invalid WKB format: {message}")]
    InvalidFormat {
        /// Error message
        message: String,
    },

    /// Invalid byte order
    #[error("Invalid byte order marker: {byte}")]
    InvalidByteOrder {
        /// Byte order value
        byte: u8,
    },

    /// Unsupported geometry type
    #[error("Unsupported WKB geometry type: {type_code}")]
    UnsupportedGeometryType {
        /// WKB type code
        type_code: u32,
    },

    /// Invalid coordinates
    #[error("Invalid coordinates: {message}")]
    InvalidCoordinates {
        /// Error message
        message: String,
    },

    /// Buffer too short
    #[error("Buffer too short: expected at least {expected} bytes, got {actual}")]
    BufferTooShort {
        /// Expected size
        expected: usize,
        /// Actual size
        actual: usize,
    },

    /// Invalid ring
    #[error("Invalid ring: {message}")]
    InvalidRing {
        /// Error message
        message: String,
    },

    /// Encoding error
    #[error("Failed to encode geometry to WKB: {message}")]
    EncodingFailed {
        /// Error message
        message: String,
    },

    /// Decoding error
    #[error("Failed to decode WKB: {message}")]
    DecodingFailed {
        /// Error message
        message: String,
    },
}

/// SQL generation errors
#[derive(Debug, Error)]
pub enum SqlError {
    /// Invalid identifier
    #[error("Invalid SQL identifier: {identifier}")]
    InvalidIdentifier {
        /// The invalid identifier
        identifier: String,
    },

    /// SQL injection attempt detected
    #[error("Potential SQL injection detected in: {input}")]
    InjectionAttempt {
        /// The suspicious input
        input: String,
    },

    /// Invalid table name
    #[error("Invalid table name: {table}")]
    InvalidTableName {
        /// Table name
        table: String,
    },

    /// Invalid column name
    #[error("Invalid column name: {column}")]
    InvalidColumnName {
        /// Column name
        column: String,
    },

    /// Invalid spatial function
    #[error("Invalid spatial function: {function}")]
    InvalidSpatialFunction {
        /// Function name
        function: String,
    },

    /// Parameter binding error
    #[error("Parameter binding error: {message}")]
    ParameterBindingError {
        /// Error message
        message: String,
    },
}

// Implement conversions from external error types

impl From<tokio_postgres::Error> for PostGisError {
    fn from(err: tokio_postgres::Error) -> Self {
        Self::Query(QueryError::ExecutionFailed {
            message: err.to_string(),
        })
    }
}

impl From<deadpool_postgres::PoolError> for PostGisError {
    fn from(err: deadpool_postgres::PoolError) -> Self {
        Self::Connection(ConnectionError::PoolError {
            message: err.to_string(),
        })
    }
}

impl From<std::io::Error> for PostGisError {
    fn from(err: std::io::Error) -> Self {
        Self::Connection(ConnectionError::ConnectionFailed {
            message: err.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PostGisError::InvalidParameter {
            parameter: "table",
            message: "must not be empty".to_string(),
        };
        assert!(err.to_string().contains("table"));
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn test_connection_error() {
        let err = ConnectionError::DatabaseNotFound {
            database: "test_db".to_string(),
        };
        assert!(err.to_string().contains("test_db"));
    }

    #[test]
    fn test_query_error() {
        let err = QueryError::TableNotFound {
            table: "buildings".to_string(),
        };
        assert!(err.to_string().contains("buildings"));
    }

    #[test]
    fn test_conversion_error() {
        let err = ConversionError::UnsupportedGeometry {
            geometry_type: "Unknown".to_string(),
        };
        assert!(err.to_string().contains("Unknown"));
    }

    #[test]
    fn test_wkb_error() {
        let err = WkbError::BufferTooShort {
            expected: 100,
            actual: 50,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("50"));
    }

    #[test]
    fn test_error_conversion() {
        let conn_err = ConnectionError::ConnectionFailed {
            message: "test".to_string(),
        };
        let postgis_err: PostGisError = conn_err.into();
        assert!(matches!(
            postgis_err,
            PostGisError::Connection(ConnectionError::ConnectionFailed { .. })
        ));
    }
}
