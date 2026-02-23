//! Integration tests for OxiGDAL tile server
#![allow(clippy::expect_used)]
//!
//! Tests the complete server functionality including WMS, WMTS, and XYZ endpoints.

use oxigdal_server::{Config, ImageFormat, LayerConfig, ServerConfig, TileServer};
use std::path::PathBuf;
use tempfile::TempDir;

/// Create a test configuration
fn create_test_config() -> Config {
    Config {
        server: ServerConfig {
            host: "127.0.0.1".parse().expect("valid IP"),
            port: 0, // Let OS assign port for testing
            workers: 1,
            max_request_size: 10 * 1024 * 1024,
            timeout_seconds: 30,
            enable_cors: true,
            cors_origins: vec![],
        },
        cache: oxigdal_server::CacheConfig {
            memory_size_mb: 10, // 10 MB
            disk_cache: None,
            ttl_seconds: 3600,
            enable_stats: true,
            compression: false,
        },
        layers: vec![],
        metadata: oxigdal_server::MetadataConfig {
            title: "Test Server".to_string(),
            abstract_: "Test server for unit tests".to_string(),
            contact: None,
            keywords: vec![],
            online_resource: None,
        },
    }
}

#[test]
fn test_config_validation() {
    let config = create_test_config();
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_with_invalid_layer() {
    let mut config = create_test_config();

    // Add a layer with non-existent file
    config.layers.push(LayerConfig {
        name: "test".to_string(),
        title: Some("Test Layer".to_string()),
        abstract_: None,
        path: PathBuf::from("/nonexistent/file.tif"),
        formats: vec![ImageFormat::Png],
        tile_size: 256,
        min_zoom: 0,
        max_zoom: 18,
        tile_matrix_sets: vec!["WebMercatorQuad".to_string()],
        style: None,
        metadata: Default::default(),
        enabled: true,
    });

    // Validation should fail because file doesn't exist
    assert!(config.validate().is_err());
}

#[test]
fn test_server_creation() {
    let config = create_test_config();
    let server = TileServer::new(config);

    // Server creation should succeed with valid config
    assert!(server.is_ok());
}

#[test]
fn test_default_config() {
    let config = Config::default_config();
    assert_eq!(config.server.port, 8080);
    assert_eq!(config.cache.memory_size_mb, 256);
    assert!(config.layers.is_empty());
}

#[test]
fn test_config_serialization() {
    let config = create_test_config();

    // Serialize to TOML
    let toml_str = toml::to_string(&config).expect("serialization failed");
    assert!(toml_str.contains("[server]"));
    assert!(toml_str.contains("[cache]"));

    // Deserialize back
    let deserialized: Config = toml::from_str(&toml_str).expect("deserialization failed");
    assert_eq!(deserialized.server.port, config.server.port);
}

#[test]
fn test_image_format_parsing() {
    assert_eq!("png".parse::<ImageFormat>().ok(), Some(ImageFormat::Png));
    assert_eq!("jpeg".parse::<ImageFormat>().ok(), Some(ImageFormat::Jpeg));
    assert_eq!("jpg".parse::<ImageFormat>().ok(), Some(ImageFormat::Jpeg));
    assert_eq!("webp".parse::<ImageFormat>().ok(), Some(ImageFormat::Webp));
    assert!("invalid".parse::<ImageFormat>().is_err());
}

#[test]
fn test_tile_cache() {
    use bytes::Bytes;
    use oxigdal_server::{CacheKey, TileCache, TileCacheConfig};

    let config = TileCacheConfig {
        max_memory_bytes: 1024 * 1024, // 1 MB
        disk_cache_dir: None,
        ttl: std::time::Duration::from_secs(60),
        enable_stats: true,
        compression: false,
    };

    let cache = TileCache::new(config);

    // Test cache put/get
    let key = CacheKey::new("test".to_string(), 10, 512, 384, "png".to_string());
    let data = Bytes::from(vec![1, 2, 3, 4, 5]);

    cache.put(key.clone(), data.clone()).expect("put failed");
    let retrieved = cache.get(&key).expect("get failed");

    assert_eq!(retrieved, data);

    // Test cache stats
    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);
}

#[test]
fn test_cache_miss() {
    use oxigdal_server::{CacheKey, TileCache, TileCacheConfig};

    let config = TileCacheConfig::default();
    let cache = TileCache::new(config);

    let key = CacheKey::new("nonexistent".to_string(), 0, 0, 0, "png".to_string());

    assert!(cache.get(&key).is_none());

    let stats = cache.stats();
    assert_eq!(stats.misses, 1);
}

#[test]
fn test_cache_key_generation() {
    use oxigdal_server::CacheKey;

    let key = CacheKey::new("landsat".to_string(), 10, 512, 384, "png".to_string());

    assert_eq!(key.to_string(), "landsat/10/512/384.png");

    let key_with_style = key.with_style("default".to_string());
    assert_eq!(key_with_style.to_string(), "landsat/default/10/512/384.png");
}

#[test]
fn test_dataset_registry() {
    use oxigdal_server::DatasetRegistry;

    let registry = DatasetRegistry::new();
    assert_eq!(registry.layer_count(), 0);
    assert!(!registry.has_layer("test"));

    let result = registry.get_layer("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_web_mercator_bounds() {
    use oxigdal_server::handlers::tiles::WebMercatorBounds;

    // Test zoom 0 (single tile)
    let bounds = WebMercatorBounds::new(0, 0, 0);
    assert_eq!(bounds.num_tiles(), 1);
    assert!(bounds.is_valid());

    let (min_x, min_y, max_x, max_y) = bounds.bbox();
    assert!(min_x < max_x);
    assert!(min_y < max_y);

    // Test zoom 1 (2x2 tiles)
    let bounds = WebMercatorBounds::new(1, 0, 0);
    assert_eq!(bounds.num_tiles(), 2);
    assert!(bounds.is_valid());

    // Test invalid coordinates
    let bounds = WebMercatorBounds::new(1, 2, 0);
    assert!(!bounds.is_valid());
}

#[test]
fn test_config_from_toml() {
    let toml = r#"
        [server]
        host = "127.0.0.1"
        port = 9000
        workers = 8

        [cache]
        memory_size_mb = 512
        ttl_seconds = 7200

        [metadata]
        title = "Test Server"
    "#;

    let config = Config::from_toml(toml).expect("valid config");
    assert_eq!(config.server.host.to_string(), "127.0.0.1");
    assert_eq!(config.server.port, 9000);
    assert_eq!(config.server.workers, 8);
    assert_eq!(config.cache.memory_size_mb, 512);
    assert_eq!(config.metadata.title, "Test Server");
}

#[test]
fn test_invalid_tile_size() {
    let mut config = create_test_config();

    // Create a temporary file for testing
    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let temp_file = temp_dir.path().join("test.tif");
    std::fs::write(&temp_file, b"dummy content").expect("write failed");

    config.layers.push(LayerConfig {
        name: "test".to_string(),
        title: None,
        abstract_: None,
        path: temp_file,
        formats: vec![ImageFormat::Png],
        tile_size: 100, // Not a power of 2
        min_zoom: 0,
        max_zoom: 18,
        tile_matrix_sets: vec![],
        style: None,
        metadata: Default::default(),
        enabled: true,
    });

    // Should fail validation due to invalid tile size
    assert!(config.validate().is_err());
}

#[test]
fn test_invalid_zoom_levels() {
    let mut config = create_test_config();

    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let temp_file = temp_dir.path().join("test.tif");
    std::fs::write(&temp_file, b"dummy content").expect("write failed");

    config.layers.push(LayerConfig {
        name: "test".to_string(),
        title: None,
        abstract_: None,
        path: temp_file,
        formats: vec![ImageFormat::Png],
        tile_size: 256,
        min_zoom: 10,
        max_zoom: 5, // max < min
        tile_matrix_sets: vec![],
        style: None,
        metadata: Default::default(),
        enabled: true,
    });

    // Should fail validation due to invalid zoom levels
    assert!(config.validate().is_err());
}

#[test]
fn test_duplicate_layer_names() {
    let mut config = create_test_config();

    let temp_dir = TempDir::new().expect("temp dir creation failed");
    let temp_file1 = temp_dir.path().join("test1.tif");
    let temp_file2 = temp_dir.path().join("test2.tif");
    std::fs::write(&temp_file1, b"dummy1").expect("write failed");
    std::fs::write(&temp_file2, b"dummy2").expect("write failed");

    // Add two layers with the same name
    config.layers.push(LayerConfig {
        name: "duplicate".to_string(),
        title: None,
        abstract_: None,
        path: temp_file1,
        formats: vec![ImageFormat::Png],
        tile_size: 256,
        min_zoom: 0,
        max_zoom: 18,
        tile_matrix_sets: vec![],
        style: None,
        metadata: Default::default(),
        enabled: true,
    });

    config.layers.push(LayerConfig {
        name: "duplicate".to_string(),
        title: None,
        abstract_: None,
        path: temp_file2,
        formats: vec![ImageFormat::Png],
        tile_size: 256,
        min_zoom: 0,
        max_zoom: 18,
        tile_matrix_sets: vec![],
        style: None,
        metadata: Default::default(),
        enabled: true,
    });

    // Should fail validation due to duplicate names
    assert!(config.validate().is_err());
}
