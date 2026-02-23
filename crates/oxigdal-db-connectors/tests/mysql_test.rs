//! MySQL connector tests.
#![allow(clippy::panic)]

use geo_types::{Geometry, point};
use oxigdal_db_connectors::mysql::{MySqlConfig, geometry_to_wkt, wkt_to_geometry};

#[test]
fn test_mysql_config_default() {
    let config = MySqlConfig::default();
    assert_eq!(config.host, "localhost");
    assert_eq!(config.port, 3306);
    assert_eq!(config.database, "gis");
}

#[test]
fn test_geometry_conversions() {
    let point = Geometry::Point(point!(x: 1.0, y: 2.0));
    let wkt = geometry_to_wkt(&point).expect("Failed to convert to WKT");
    assert_eq!(wkt, "POINT(1 2)");

    let geom = wkt_to_geometry(&wkt).expect("Failed to parse WKT");
    match geom {
        Geometry::Point(p) => {
            assert_eq!(p.x(), 1.0);
            assert_eq!(p.y(), 2.0);
        }
        _ => panic!("Expected Point geometry"),
    }
}

// Note: Integration tests with actual MySQL connection would require a running MySQL instance
// These can be enabled with feature flags or environment variables
