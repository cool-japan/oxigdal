//! ClickHouse connector tests.

use oxigdal_db_connectors::clickhouse::{ClickHouseConfig, types::Point};

#[test]
fn test_clickhouse_config_default() {
    let config = ClickHouseConfig::default();
    assert_eq!(config.database, "default");
    assert_eq!(config.username, "default");
}

#[test]
fn test_point_type() {
    let p = Point::new(1.0, 2.0);
    assert_eq!(p.x, 1.0);
    assert_eq!(p.y, 2.0);

    let tuple = p.to_tuple();
    assert_eq!(tuple, (1.0, 2.0));

    let p2 = Point::from_tuple(tuple);
    assert_eq!(p2.x, 1.0);
    assert_eq!(p2.y, 2.0);
}

#[test]
fn test_point_conversion() {
    let geo_point = geo_types::Point::new(1.0, 2.0);
    let ch_point: Point = geo_point.into();
    assert_eq!(ch_point.x, 1.0);
    assert_eq!(ch_point.y, 2.0);

    let geo_point2: geo_types::Point<f64> = ch_point.into();
    assert_eq!(geo_point2.x(), 1.0);
    assert_eq!(geo_point2.y(), 2.0);
}

// Note: Integration tests with actual ClickHouse connection would require a running ClickHouse instance
