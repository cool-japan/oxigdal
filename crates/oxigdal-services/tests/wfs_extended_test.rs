//! Extended WFS filter and database tests for OxiGDAL services

use oxigdal_services::ServiceError;
use oxigdal_services::wfs::{
    BboxFilter, CountCacheConfig, CqlFilter, DatabaseFeatureCounter, DatabaseSource, DatabaseType,
    FeatureSource, FeatureTypeInfo, ServiceInfo, WfsState,
};
use std::time::Duration;

// ============================================================
// DatabaseSource tests
// ============================================================

#[test]
fn test_database_source_connection_string() {
    let source = DatabaseSource::new("postgresql://user:pass@localhost/mydb", "buildings");
    assert_eq!(source.table_name, "buildings");
    assert_eq!(
        source.connection_string,
        "postgresql://user:pass@localhost/mydb"
    );
}

#[test]
fn test_database_source_geometry_column_default() {
    let source = DatabaseSource::new("postgresql://localhost/db", "rivers");
    assert_eq!(source.geometry_column, "geom");
}

#[test]
fn test_database_source_with_schema() {
    let source = DatabaseSource::new("postgresql://localhost/db", "roads").with_schema("public");
    let qualified = source.qualified_table_name();
    assert!(qualified.contains("roads"));
    assert!(qualified.contains("public"));
}

#[test]
fn test_database_source_qualified_table_name_no_schema() {
    let source = DatabaseSource::new("postgresql://localhost/db", "streets");
    let qualified = source.qualified_table_name();
    assert!(qualified.contains("streets"));
}

#[test]
fn test_database_source_with_id_column() {
    let source = DatabaseSource::new("postgresql://localhost/db", "parcels").with_id_column("gid");
    assert_eq!(source.id_column.as_deref(), Some("gid"));
}

#[test]
fn test_database_source_with_geometry_column() {
    let source = DatabaseSource::new("postgresql://localhost/db", "parcels")
        .with_geometry_column("the_geom");
    assert_eq!(source.geometry_column, "the_geom");
}

#[test]
fn test_database_source_builder_chain() {
    let source = DatabaseSource::new("postgresql://localhost/gis", "buildings")
        .with_schema("cadastre")
        .with_geometry_column("shape")
        .with_id_column("gid")
        .with_database_type(DatabaseType::PostGis);
    assert_eq!(source.geometry_column, "shape");
    assert_eq!(source.id_column.as_deref(), Some("gid"));
    assert!(matches!(source.database_type, DatabaseType::PostGis));
}

#[test]
fn test_database_source_sqlite_type() {
    let source =
        DatabaseSource::new("mydb.sqlite", "features").with_database_type(DatabaseType::Sqlite);
    assert!(matches!(source.database_type, DatabaseType::Sqlite));
}

#[test]
fn test_database_source_mysql_type() {
    let source = DatabaseSource::new("mysql://localhost/db", "features")
        .with_database_type(DatabaseType::MySql);
    assert!(matches!(source.database_type, DatabaseType::MySql));
}

#[test]
fn test_database_source_generic_type() {
    let source = DatabaseSource::new("jdbc:something://localhost/db", "features")
        .with_database_type(DatabaseType::Generic);
    assert!(matches!(source.database_type, DatabaseType::Generic));
}

#[test]
fn test_database_source_with_srid() {
    let source = DatabaseSource::new("postgresql://localhost/db", "layers").with_srid(32654);
    assert_eq!(source.srid, Some(32654));
}

#[test]
fn test_database_source_without_count_cache() {
    let source =
        DatabaseSource::new("postgresql://localhost/db", "large_table").without_count_cache();
    assert!(source.count_cache.is_none());
}

#[test]
fn test_database_source_qualified_name_with_schema_and_table() {
    let source = DatabaseSource::new("conn", "my_features").with_schema("myschema");
    let q = source.qualified_table_name();
    // Should produce "myschema"."my_features"
    assert!(q.contains("myschema"));
    assert!(q.contains("my_features"));
}

// ============================================================
// BboxFilter tests
// ============================================================

#[test]
fn test_bbox_filter_from_valid_string() {
    let result = BboxFilter::from_bbox_string("-10.0,-20.0,30.0,40.0");
    assert!(result.is_ok());
    let bbox = result.expect("valid bbox should parse");
    assert_eq!(bbox.min_x, -10.0);
    assert_eq!(bbox.min_y, -20.0);
    assert_eq!(bbox.max_x, 30.0);
    assert_eq!(bbox.max_y, 40.0);
}

#[test]
fn test_bbox_filter_from_valid_string_with_crs() {
    let result = BboxFilter::from_bbox_string("-180.0,-90.0,180.0,90.0,EPSG:4326");
    assert!(result.is_ok());
    let bbox = result.expect("bbox with CRS should parse");
    assert_eq!(bbox.min_x, -180.0);
    assert_eq!(bbox.max_y, 90.0);
}

#[test]
fn test_bbox_filter_insufficient_parts() {
    let result = BboxFilter::from_bbox_string("10.0,20.0,30.0");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ServiceError::InvalidBbox(_)));
}

#[test]
fn test_bbox_filter_non_numeric_values() {
    let result = BboxFilter::from_bbox_string("a,b,c,d");
    assert!(result.is_err());
}

#[test]
fn test_bbox_filter_empty_string() {
    let result = BboxFilter::from_bbox_string("");
    assert!(result.is_err());
}

#[test]
fn test_bbox_filter_global_extent() {
    let result = BboxFilter::from_bbox_string("-180,-90,180,90");
    assert!(result.is_ok());
    let bbox = result.expect("global extent should parse");
    assert_eq!(bbox.min_x, -180.0);
    assert_eq!(bbox.max_x, 180.0);
    assert_eq!(bbox.min_y, -90.0);
    assert_eq!(bbox.max_y, 90.0);
}

// ============================================================
// CqlFilter tests
// ============================================================

#[test]
fn test_cql_filter_creation() {
    let filter = CqlFilter::new("population > 1000000");
    assert_eq!(filter.expression, "population > 1000000");
}

#[test]
fn test_cql_filter_to_sql_equality() {
    let filter = CqlFilter::new("name = 'Tokyo'");
    let sql = filter.to_sql(&DatabaseType::PostGis);
    assert!(sql.is_ok());
    let clause = sql.expect("SQL clause should be generated");
    assert!(!clause.is_empty());
}

#[test]
fn test_cql_filter_to_sql_greater_than() {
    let filter = CqlFilter::new("area > 1000");
    let sql = filter.to_sql(&DatabaseType::PostGis);
    assert!(sql.is_ok());
}

#[test]
fn test_cql_filter_to_sql_less_than() {
    let filter = CqlFilter::new("elevation < 500");
    let sql = filter.to_sql(&DatabaseType::PostGis);
    assert!(sql.is_ok());
}

#[test]
fn test_cql_filter_and_condition() {
    let filter = CqlFilter::new("population > 100000 AND area < 50000");
    let sql = filter.to_sql(&DatabaseType::PostGis);
    assert!(sql.is_ok());
}

#[test]
fn test_cql_filter_for_sqlite() {
    let filter = CqlFilter::new("name = 'Paris'");
    let sql = filter.to_sql(&DatabaseType::Sqlite);
    assert!(sql.is_ok());
}

#[test]
fn test_cql_filter_for_mysql() {
    let filter = CqlFilter::new("population > 500000");
    let sql = filter.to_sql(&DatabaseType::MySql);
    assert!(sql.is_ok());
}

// ============================================================
// DatabaseFeatureCounter tests
// ============================================================

#[test]
fn test_database_feature_counter_creation() {
    let config = CountCacheConfig::default();
    let counter = DatabaseFeatureCounter::new(config);
    let stats = counter.cache_stats();
    assert_eq!(stats.total_entries, 0);
    assert_eq!(stats.valid_entries, 0);
}

#[test]
fn test_database_feature_counter_custom_config() {
    let config = CountCacheConfig {
        ttl: Duration::from_secs(300),
        max_entries: 50,
        use_estimation_threshold: Some(500_000),
    };
    let counter = DatabaseFeatureCounter::new(config);
    let stats = counter.cache_stats();
    assert_eq!(stats.total_entries, 0);
    assert_eq!(stats.max_entries, 50);
}

#[test]
fn test_database_feature_counter_cache_clear() {
    let config = CountCacheConfig::default();
    let counter = DatabaseFeatureCounter::new(config);
    counter.clear_cache();
    let stats = counter.cache_stats();
    assert_eq!(stats.total_entries, 0);
}

// ============================================================
// WfsState with geojson features tests
// ============================================================

#[test]
fn test_wfs_state_with_geojson_features() {
    let info = ServiceInfo {
        title: "WFS with features".to_string(),
        abstract_text: None,
        provider: "COOLJAPAN OU".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    };

    let state = WfsState::new(info);

    // Create a simple GeoJSON feature
    let feature: geojson::Feature = geojson::Feature {
        bbox: None,
        geometry: Some(geojson::Geometry::new(geojson::Value::Point(vec![
            139.0, 35.0,
        ]))),
        id: Some(geojson::feature::Id::String("feature.1".to_string())),
        properties: Some(
            [("name".to_string(), serde_json::json!("Tokyo"))]
                .into_iter()
                .collect(),
        ),
        foreign_members: None,
    };

    let ft = FeatureTypeInfo {
        name: "cities".to_string(),
        title: "Cities".to_string(),
        abstract_text: None,
        default_crs: "EPSG:4326".to_string(),
        other_crs: vec![],
        bbox: Some((138.0, 34.0, 140.0, 36.0)),
        source: FeatureSource::Memory(vec![feature]),
    };

    assert!(state.add_feature_type(ft).is_ok());
    let retrieved = state.get_feature_type("cities");
    assert!(retrieved.is_some());

    if let Some(ft) = retrieved {
        if let FeatureSource::Memory(features) = ft.source {
            assert_eq!(features.len(), 1);
        }
    }
}

#[test]
fn test_wfs_service_info_with_multiple_versions() {
    let info = ServiceInfo {
        title: "Multi-Version WFS".to_string(),
        abstract_text: Some("Supports WFS 2.0 and 3.0".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "https://geo.example.com/wfs".to_string(),
        versions: vec!["2.0.0".to_string(), "3.0".to_string()],
    };

    let state = WfsState::new(info);
    assert_eq!(state.service_info.versions.len(), 2);
    assert!(state.service_info.versions.contains(&"2.0.0".to_string()));
    assert!(state.service_info.versions.contains(&"3.0".to_string()));
}

#[test]
fn test_wfs_multiple_feature_types() {
    let state = WfsState::new(ServiceInfo {
        title: "Multi-Layer WFS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    let layers = vec!["rivers", "lakes", "mountains"];
    for name in &layers {
        let ft = FeatureTypeInfo {
            name: name.to_string(),
            title: format!("{} Layer", name),
            abstract_text: None,
            default_crs: "EPSG:4326".to_string(),
            other_crs: vec!["EPSG:3857".to_string()],
            bbox: Some((-180.0, -90.0, 180.0, 90.0)),
            source: FeatureSource::Memory(vec![]),
        };
        assert!(state.add_feature_type(ft).is_ok());
    }

    for name in &layers {
        let ft = state.get_feature_type(name);
        assert!(ft.is_some(), "Feature type '{}' should be found", name);
    }
}

#[test]
fn test_wfs_feature_type_not_found() {
    let state = WfsState::new(ServiceInfo {
        title: "WFS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    assert!(state.get_feature_type("nonexistent").is_none());
}

#[test]
fn test_wfs_transactions_disabled_by_default() {
    let state = WfsState::new(ServiceInfo {
        title: "WFS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    assert!(!state.transactions_enabled);
}

#[test]
fn test_wfs_enable_transactions() {
    let mut state = WfsState::new(ServiceInfo {
        title: "WFS-T".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    state.enable_transactions();
    assert!(state.transactions_enabled);
}
