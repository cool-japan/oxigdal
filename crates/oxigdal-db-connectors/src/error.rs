//! Error types for database connectors.

/// Result type for database operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Database connector errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Connection error.
    #[error("Connection error: {0}")]
    Connection(String),

    /// Query execution error.
    #[error("Query error: {0}")]
    Query(String),

    /// Type conversion error.
    #[error("Type conversion error: {0}")]
    TypeConversion(String),

    /// Geometry parsing error.
    #[error("Geometry parsing error: {0}")]
    GeometryParsing(String),

    /// Invalid connection string.
    #[error("Invalid connection string: {0}")]
    InvalidConnectionString(String),

    /// Database not supported.
    #[error("Database not supported: {0}")]
    UnsupportedDatabase(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Pool error.
    #[error("Pool error: {0}")]
    Pool(String),

    /// Timeout error.
    #[error("Timeout error: {0}")]
    Timeout(String),

    /// Authentication error.
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// MySQL error.
    #[error("MySQL error: {0}")]
    MySql(String),

    /// SQLite error.
    #[error("SQLite error: {0}")]
    SQLite(String),

    /// MongoDB error.
    #[error("MongoDB error: {0}")]
    MongoDB(String),

    /// ClickHouse error.
    #[error("ClickHouse error: {0}")]
    ClickHouse(String),

    /// TimescaleDB error.
    #[error("TimescaleDB error: {0}")]
    TimescaleDB(String),

    /// Cassandra error.
    #[error("Cassandra error: {0}")]
    Cassandra(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// GeoJSON error.
    #[error("GeoJSON error: {0}")]
    GeoJson(String),

    /// WKT parsing error.
    #[error("WKT parsing error: {0}")]
    Wkt(String),

    /// Core error.
    #[error("Core error: {0}")]
    Core(String),
}

#[cfg(feature = "mysql")]
impl From<mysql_async::Error> for Error {
    fn from(err: mysql_async::Error) -> Self {
        Error::MySql(err.to_string())
    }
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::SQLite(err.to_string())
    }
}

#[cfg(feature = "mongodb")]
impl From<mongodb::error::Error> for Error {
    fn from(err: mongodb::error::Error) -> Self {
        Error::MongoDB(err.to_string())
    }
}

#[cfg(feature = "mongodb")]
impl From<bson::error::Error> for Error {
    fn from(err: bson::error::Error) -> Self {
        Error::MongoDB(err.to_string())
    }
}

#[cfg(feature = "clickhouse")]
impl From<clickhouse::error::Error> for Error {
    fn from(err: clickhouse::error::Error) -> Self {
        Error::ClickHouse(err.to_string())
    }
}

#[cfg(feature = "cassandra")]
impl From<scylla::errors::NewSessionError> for Error {
    fn from(err: scylla::errors::NewSessionError) -> Self {
        Error::Cassandra(err.to_string())
    }
}

#[cfg(feature = "cassandra")]
impl From<scylla::errors::ExecutionError> for Error {
    fn from(err: scylla::errors::ExecutionError) -> Self {
        Error::Cassandra(err.to_string())
    }
}

impl From<geojson::Error> for Error {
    fn from(err: geojson::Error) -> Self {
        Error::GeoJson(err.to_string())
    }
}

// Note: wkt 0.11 doesn't expose a public Error type
// WKT parsing errors are handled inline where needed

impl From<url::ParseError> for Error {
    fn from(err: url::ParseError) -> Self {
        Error::InvalidConnectionString(err.to_string())
    }
}
