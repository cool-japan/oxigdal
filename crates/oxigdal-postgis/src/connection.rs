//! Database connection management for PostGIS
//!
//! This module provides connection pooling and management for PostgreSQL/PostGIS databases.

use crate::error::{ConnectionError, PostGisError, Result};
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use std::time::Duration;
use tokio_postgres::NoTls;
use tracing::{debug, warn};

/// PostgreSQL connection configuration
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Database host
    pub host: Option<String>,
    /// Database port
    pub port: u16,
    /// Database name
    pub dbname: String,
    /// Username
    pub user: String,
    /// Password
    pub password: Option<String>,
    /// Connection timeout in seconds
    pub connect_timeout: u64,
    /// Application name
    pub application_name: Option<String>,
    /// SSL mode
    pub sslmode: SslMode,
}

/// SSL mode for PostgreSQL connections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    /// Disable SSL
    Disable,
    /// Prefer SSL if available
    Prefer,
    /// Require SSL
    Require,
}

impl SslMode {
    /// Converts to PostgreSQL sslmode string
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
        }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: Some("localhost".to_string()),
            port: 5432,
            dbname: "postgres".to_string(),
            user: "postgres".to_string(),
            password: None,
            connect_timeout: 30,
            application_name: Some("oxigdal-postgis".to_string()),
            sslmode: SslMode::Prefer,
        }
    }
}

impl ConnectionConfig {
    /// Creates a new connection configuration
    pub fn new(dbname: impl Into<String>) -> Self {
        Self {
            dbname: dbname.into(),
            ..Default::default()
        }
    }

    /// Sets the host
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Sets the port
    pub const fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Sets the user
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Sets the password
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Sets the connection timeout
    pub const fn connect_timeout(mut self, seconds: u64) -> Self {
        self.connect_timeout = seconds;
        self
    }

    /// Sets the application name
    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }

    /// Sets the SSL mode
    pub const fn sslmode(mut self, mode: SslMode) -> Self {
        self.sslmode = mode;
        self
    }

    /// Builds a connection string
    pub fn to_connection_string(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref host) = self.host {
            parts.push(format!("host={host}"));
        }

        parts.push(format!("port={}", self.port));
        parts.push(format!("dbname={}", self.dbname));
        parts.push(format!("user={}", self.user));

        if let Some(ref password) = self.password {
            parts.push(format!("password={password}"));
        }

        parts.push(format!("connect_timeout={}", self.connect_timeout));

        if let Some(ref app_name) = self.application_name {
            parts.push(format!("application_name={app_name}"));
        }

        parts.push(format!("sslmode={}", self.sslmode.as_str()));

        parts.join(" ")
    }

    /// Parses a connection string into configuration
    pub fn from_connection_string(conn_str: &str) -> Result<Self> {
        let mut config = Self::default();

        for part in conn_str.split_whitespace() {
            if let Some((key, value)) = part.split_once('=') {
                match key {
                    "host" => config.host = Some(value.to_string()),
                    "port" => {
                        config.port = value.parse().map_err(|_| {
                            ConnectionError::InvalidConnectionString {
                                message: format!("Invalid port: {value}"),
                            }
                        })?;
                    }
                    "dbname" => config.dbname = value.to_string(),
                    "user" => config.user = value.to_string(),
                    "password" => config.password = Some(value.to_string()),
                    "connect_timeout" => {
                        config.connect_timeout = value.parse().map_err(|_| {
                            ConnectionError::InvalidConnectionString {
                                message: format!("Invalid connect_timeout: {value}"),
                            }
                        })?;
                    }
                    "application_name" => config.application_name = Some(value.to_string()),
                    "sslmode" => {
                        config.sslmode = match value {
                            "disable" => SslMode::Disable,
                            "prefer" => SslMode::Prefer,
                            "require" => SslMode::Require,
                            _ => {
                                return Err(ConnectionError::InvalidConnectionString {
                                    message: format!("Invalid sslmode: {value}"),
                                }
                                .into());
                            }
                        };
                    }
                    _ => {
                        warn!("Unknown connection string parameter: {key}");
                    }
                }
            }
        }

        Ok(config)
    }
}

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum pool size
    pub max_size: usize,
    /// Connection timeout
    pub timeout: Duration,
    /// Recycling method
    pub recycling_method: RecyclingMethod,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 16,
            timeout: Duration::from_secs(30),
            recycling_method: RecyclingMethod::Fast,
        }
    }
}

impl PoolConfig {
    /// Creates a new pool configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum pool size
    pub const fn max_size(mut self, size: usize) -> Self {
        self.max_size = size;
        self
    }

    /// Sets the connection timeout
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the recycling method
    pub fn recycling_method(mut self, method: RecyclingMethod) -> Self {
        self.recycling_method = method;
        self
    }
}

/// Connection pool for PostgreSQL/PostGIS
pub struct ConnectionPool {
    pool: Pool,
    config: ConnectionConfig,
}

impl ConnectionPool {
    /// Creates a new connection pool
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        let pool_config = PoolConfig::default();
        Self::with_pool_config(config, pool_config)
    }

    /// Creates a new connection pool with custom pool configuration
    pub fn with_pool_config(config: ConnectionConfig, pool_config: PoolConfig) -> Result<Self> {
        let conn_str = config.to_connection_string();
        debug!("Creating connection pool with config: {}", conn_str);

        let mut pg_config = Config::new();
        if let Some(ref host) = config.host {
            pg_config.host = Some(host.clone());
        }
        pg_config.port = Some(config.port);
        pg_config.dbname = Some(config.dbname.clone());
        pg_config.user = Some(config.user.clone());
        pg_config.password = config.password.clone();
        pg_config.connect_timeout = Some(Duration::from_secs(config.connect_timeout));
        pg_config.application_name = config.application_name.clone();

        pg_config.manager = Some(ManagerConfig {
            recycling_method: pool_config.recycling_method,
        });

        let pool = pg_config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| ConnectionError::PoolError {
                message: e.to_string(),
            })?;

        Ok(Self { pool, config })
    }

    /// Creates a connection pool from a connection string
    pub fn from_connection_string(conn_str: &str) -> Result<Self> {
        let config = ConnectionConfig::from_connection_string(conn_str)?;
        Self::new(config)
    }

    /// Gets a connection from the pool
    pub async fn get(&self) -> Result<deadpool_postgres::Object> {
        self.pool.get().await.map_err(|e| {
            ConnectionError::PoolError {
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Gets the pool status
    pub fn status(&self) -> PoolStatus {
        let status = self.pool.status();
        PoolStatus {
            size: status.size,
            available: status.available,
            max_size: status.max_size,
        }
    }

    /// Checks if PostGIS extension is installed
    pub async fn check_postgis(&self) -> Result<bool> {
        let client = self.get().await?;

        let query = "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'postgis')";
        let row = client.query_one(query, &[]).await.map_err(|e| {
            PostGisError::Query(crate::error::QueryError::ExecutionFailed {
                message: e.to_string(),
            })
        })?;

        let exists: bool = row.get(0);
        Ok(exists)
    }

    /// Checks the PostGIS version
    pub async fn postgis_version(&self) -> Result<String> {
        let client = self.get().await?;

        let query = "SELECT PostGIS_Version()";
        let row = client.query_one(query, &[]).await.map_err(|e| {
            PostGisError::Query(crate::error::QueryError::ExecutionFailed {
                message: e.to_string(),
            })
        })?;

        let version: String = row.get(0);
        Ok(version)
    }

    /// Performs a health check on the connection pool
    pub async fn health_check(&self) -> Result<HealthCheckResult> {
        let start = std::time::Instant::now();

        // Try to get a connection
        let client = self.get().await?;

        // Execute a simple query
        client.query_one("SELECT 1", &[]).await.map_err(|e| {
            PostGisError::Query(crate::error::QueryError::ExecutionFailed {
                message: e.to_string(),
            })
        })?;

        let latency = start.elapsed();

        // Check PostGIS
        let postgis_installed = self.check_postgis().await?;
        let postgis_version = if postgis_installed {
            self.postgis_version().await.ok()
        } else {
            None
        };

        Ok(HealthCheckResult {
            connected: true,
            latency,
            pool_status: self.status(),
            postgis_installed,
            postgis_version,
        })
    }

    /// Returns the connection configuration
    pub const fn config(&self) -> &ConnectionConfig {
        &self.config
    }
}

/// Pool status information
#[derive(Debug, Clone)]
pub struct PoolStatus {
    /// Current pool size
    pub size: usize,
    /// Available connections
    pub available: usize,
    /// Maximum pool size
    pub max_size: usize,
}

/// Health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// Whether the connection is established
    pub connected: bool,
    /// Connection latency
    pub latency: Duration,
    /// Pool status
    pub pool_status: PoolStatus,
    /// Whether PostGIS is installed
    pub postgis_installed: bool,
    /// PostGIS version (if installed)
    pub postgis_version: Option<String>,
}

impl HealthCheckResult {
    /// Returns true if the connection is healthy
    pub fn is_healthy(&self) -> bool {
        self.connected && self.postgis_installed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_config_default() {
        let config = ConnectionConfig::default();
        assert_eq!(config.port, 5432);
        assert_eq!(config.dbname, "postgres");
        assert_eq!(config.user, "postgres");
    }

    #[test]
    fn test_connection_config_builder() {
        let config = ConnectionConfig::new("test_db")
            .host("localhost")
            .port(5433)
            .user("test_user")
            .password("test_pass")
            .connect_timeout(60)
            .application_name("test_app")
            .sslmode(SslMode::Require);

        assert_eq!(config.dbname, "test_db");
        assert_eq!(config.host, Some("localhost".to_string()));
        assert_eq!(config.port, 5433);
        assert_eq!(config.user, "test_user");
        assert_eq!(config.password, Some("test_pass".to_string()));
        assert_eq!(config.connect_timeout, 60);
        assert_eq!(config.application_name, Some("test_app".to_string()));
        assert_eq!(config.sslmode, SslMode::Require);
    }

    #[test]
    fn test_connection_string_generation() {
        let config = ConnectionConfig::new("test_db")
            .host("localhost")
            .user("test_user")
            .password("test_pass");

        let conn_str = config.to_connection_string();
        assert!(conn_str.contains("host=localhost"));
        assert!(conn_str.contains("dbname=test_db"));
        assert!(conn_str.contains("user=test_user"));
        assert!(conn_str.contains("password=test_pass"));
    }

    #[test]
    fn test_connection_string_parsing() {
        let conn_str = "host=localhost port=5432 dbname=test_db user=test_user password=test_pass";
        let config = ConnectionConfig::from_connection_string(conn_str).ok();
        assert!(config.is_some());

        let config = config.expect("config parsing failed");
        assert_eq!(config.host, Some("localhost".to_string()));
        assert_eq!(config.port, 5432);
        assert_eq!(config.dbname, "test_db");
        assert_eq!(config.user, "test_user");
        assert_eq!(config.password, Some("test_pass".to_string()));
    }

    #[test]
    fn test_sslmode() {
        assert_eq!(SslMode::Disable.as_str(), "disable");
        assert_eq!(SslMode::Prefer.as_str(), "prefer");
        assert_eq!(SslMode::Require.as_str(), "require");
    }

    #[test]
    fn test_pool_config() {
        let config = PoolConfig::new()
            .max_size(32)
            .timeout(Duration::from_secs(60));

        assert_eq!(config.max_size, 32);
        assert_eq!(config.timeout, Duration::from_secs(60));
    }
}
