//! SQLite connector tests.
#![cfg(feature = "sqlite")]
#![allow(clippy::panic)]

use geo_types::{Geometry, point};
use oxigdal_db_connectors::sqlite::{SqliteConnector, geometry_to_wkb, wkb_to_geometry};

#[test]
fn test_sqlite_memory() {
    let connector = SqliteConnector::memory().expect("Failed to create memory database");
    assert!(connector.health_check().expect("Health check failed"));
}

#[test]
fn test_sqlite_version() {
    let connector = SqliteConnector::memory().expect("Failed to create memory database");
    let version = connector.version().expect("Failed to get version");
    assert!(!version.is_empty());
}

#[test]
fn test_wkb_conversions() {
    let point = Geometry::Point(point!(x: 1.0, y: 2.0));
    let wkb = geometry_to_wkb(&point).expect("Failed to convert to WKB");
    assert!(!wkb.is_empty());

    let geom = wkb_to_geometry(&wkb).expect("Failed to parse WKB");
    match geom {
        Geometry::Point(p) => {
            assert_eq!(p.x(), 1.0);
            assert_eq!(p.y(), 2.0);
        }
        _ => panic!("Expected Point geometry"),
    }
}

#[test]
fn test_create_drop_table() {
    let connector = SqliteConnector::memory().expect("Failed to create memory database");

    // Create table
    connector
        .create_spatial_table("test_table", "geometry", "POINT", 4326, &[])
        .expect("Failed to create table");

    // Check table exists
    let tables = connector.list_tables().expect("Failed to list tables");
    assert!(tables.contains(&"test_table".to_string()));

    // Drop table
    connector
        .drop_table("test_table")
        .expect("Failed to drop table");

    // Verify dropped
    let tables = connector.list_tables().expect("Failed to list tables");
    assert!(!tables.contains(&"test_table".to_string()));
}

#[test]
fn test_table_schema() {
    let connector = SqliteConnector::memory().expect("Failed to create memory database");

    connector
        .create_spatial_table(
            "schema_test",
            "geometry",
            "POINT",
            4326,
            &[("name".to_string(), "TEXT".to_string())],
        )
        .expect("Failed to create table");

    let schema = connector
        .table_schema("schema_test")
        .expect("Failed to get schema");

    assert!(!schema.is_empty());

    // Clean up
    connector.drop_table("schema_test").ok();
}
