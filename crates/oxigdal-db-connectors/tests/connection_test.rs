//! Connection management tests.

use oxigdal_db_connectors::connection::{
    ConnectionString, DatabaseType,
    health::{HealthCheckConfig, HealthTracker},
    pool::{PoolConfig, PoolStats},
};
use std::time::Duration;

#[test]
fn test_connection_string_mysql() {
    let conn_str = "mysql://user:pass@localhost:3306/mydb";
    let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

    assert_eq!(parsed.database_type(), DatabaseType::MySql);
    assert_eq!(parsed.host(), Some("localhost".to_string()));
    assert_eq!(parsed.port(), Some(3306));
    assert_eq!(parsed.database(), Some("mydb".to_string()));
    assert_eq!(parsed.username(), Some("user".to_string()));
    assert_eq!(parsed.password(), Some("pass".to_string()));
}

#[test]
fn test_connection_string_sqlite() {
    let conn_str = "sqlite:///path/to/db.sqlite";
    let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

    assert_eq!(parsed.database_type(), DatabaseType::SQLite);
}

#[test]
fn test_connection_string_mongodb() {
    let conn_str = "mongodb://localhost:27017/gis";
    let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

    assert_eq!(parsed.database_type(), DatabaseType::MongoDB);
    assert_eq!(parsed.host(), Some("localhost".to_string()));
    assert_eq!(parsed.port(), Some(27017));
}

#[test]
fn test_connection_string_postgresql() {
    let conn_str = "postgresql://user:pass@localhost:5432/timescale";
    let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

    assert_eq!(parsed.database_type(), DatabaseType::TimescaleDB);
    assert_eq!(parsed.host(), Some("localhost".to_string()));
    assert_eq!(parsed.port(), Some(5432));
    assert_eq!(parsed.database(), Some("timescale".to_string()));
}

#[test]
fn test_connection_string_invalid() {
    let conn_str = "invalid://localhost";
    let result = ConnectionString::parse(conn_str);
    assert!(result.is_err());
}

#[test]
fn test_pool_config_default() {
    let config = PoolConfig::default();
    assert_eq!(config.min_connections, 1);
    assert_eq!(config.max_connections, 10);
}

#[test]
fn test_pool_config_builder() {
    let config = PoolConfig::new()
        .with_min_connections(2)
        .with_max_connections(20)
        .with_connection_timeout(Duration::from_secs(60));

    assert_eq!(config.min_connections, 2);
    assert_eq!(config.max_connections, 20);
    assert_eq!(config.connection_timeout, Duration::from_secs(60));
}

#[test]
fn test_pool_config_validation() {
    let valid_config = PoolConfig::new()
        .with_min_connections(5)
        .with_max_connections(10);
    assert!(valid_config.validate().is_ok());

    let invalid_config = PoolConfig::new()
        .with_min_connections(10)
        .with_max_connections(5);
    assert!(invalid_config.validate().is_err());

    let zero_config = PoolConfig::new().with_max_connections(0);
    assert!(zero_config.validate().is_err());
}

#[test]
fn test_pool_stats() {
    let stats = PoolStats {
        active_connections: 5,
        idle_connections: 3,
        total_connections: 8,
        pending_requests: 0,
    };

    assert!(stats.is_healthy());
    assert_eq!(stats.total_connections, 8);
}

#[test]
fn test_health_check_config() {
    let config = HealthCheckConfig::new()
        .with_interval(Duration::from_secs(60))
        .with_timeout(Duration::from_secs(10))
        .with_failure_threshold(5)
        .with_success_threshold(3);

    assert_eq!(config.check_interval, Duration::from_secs(60));
    assert_eq!(config.check_timeout, Duration::from_secs(10));
    assert_eq!(config.failure_threshold, 5);
    assert_eq!(config.success_threshold, 3);
}

#[test]
fn test_health_tracker() {
    let config = HealthCheckConfig::default();
    let mut tracker = HealthTracker::new(config);

    assert!(tracker.is_healthy());

    // Record failures
    tracker.record_failure();
    tracker.record_failure();
    assert!(tracker.is_healthy()); // Still healthy (threshold is 3)
    assert_eq!(tracker.consecutive_failures(), 2);

    tracker.record_failure();
    assert!(!tracker.is_healthy()); // Now unhealthy
    assert_eq!(tracker.consecutive_failures(), 3);

    // Record successes
    tracker.record_success();
    tracker.record_success();
    assert!(tracker.is_healthy()); // Healthy again (threshold is 2)
    assert_eq!(tracker.consecutive_successes(), 2);
    assert_eq!(tracker.consecutive_failures(), 0);

    // Reset
    tracker.reset();
    assert!(tracker.is_healthy());
    assert_eq!(tracker.consecutive_failures(), 0);
    assert_eq!(tracker.consecutive_successes(), 0);
}

#[test]
fn test_database_type_display() {
    assert_eq!(DatabaseType::MySql.to_string(), "MySQL");
    assert_eq!(DatabaseType::SQLite.to_string(), "SQLite");
    assert_eq!(DatabaseType::MongoDB.to_string(), "MongoDB");
    assert_eq!(DatabaseType::ClickHouse.to_string(), "ClickHouse");
    assert_eq!(DatabaseType::TimescaleDB.to_string(), "TimescaleDB");
    assert_eq!(DatabaseType::Cassandra.to_string(), "Cassandra");
}
