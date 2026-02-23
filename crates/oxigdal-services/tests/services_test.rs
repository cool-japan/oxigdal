//! Integration tests for OGC web services

use oxigdal_services::{csw, wcs, wfs, wps};

#[test]
fn test_wfs_state_creation() {
    let info = wfs::ServiceInfo {
        title: "Test WFS".to_string(),
        abstract_text: Some("Test service".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    };

    let state = wfs::WfsState::new(info);
    assert_eq!(state.service_info.title, "Test WFS");

    // Add feature type
    let feature_type = wfs::FeatureTypeInfo {
        name: "test_layer".to_string(),
        title: "Test Layer".to_string(),
        abstract_text: None,
        default_crs: "EPSG:4326".to_string(),
        other_crs: vec![],
        bbox: Some((-180.0, -90.0, 180.0, 90.0)),
        source: wfs::FeatureSource::Memory(vec![]),
    };

    assert!(
        state.add_feature_type(feature_type).is_ok(),
        "Failed to add feature type"
    );

    // Retrieve feature type
    let retrieved = state.get_feature_type("test_layer");
    assert!(retrieved.is_some());
    assert_eq!(
        retrieved.as_ref().map(|ft| &ft.name),
        Some(&"test_layer".to_string())
    );
}

#[test]
fn test_wcs_state_creation() {
    let info = wcs::ServiceInfo {
        title: "Test WCS".to_string(),
        abstract_text: Some("Test service".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "http://localhost/wcs".to_string(),
        versions: vec!["2.0.1".to_string()],
    };

    let state = wcs::WcsState::new(info);
    assert_eq!(state.service_info.title, "Test WCS");

    // Add coverage
    let coverage = wcs::CoverageInfo {
        coverage_id: "test_coverage".to_string(),
        title: "Test Coverage".to_string(),
        abstract_text: None,
        native_crs: "EPSG:4326".to_string(),
        bbox: (-180.0, -90.0, 180.0, 90.0),
        grid_size: (1024, 512),
        grid_origin: (-180.0, 90.0),
        grid_resolution: (0.35, -0.35),
        band_count: 3,
        band_names: vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()],
        data_type: "Byte".to_string(),
        source: wcs::CoverageSource::Memory,
        formats: vec!["image/tiff".to_string()],
    };

    assert!(
        state.add_coverage(coverage).is_ok(),
        "Failed to add coverage"
    );

    // Retrieve coverage
    let retrieved = state.get_coverage("test_coverage");
    assert!(retrieved.is_some());
    assert_eq!(
        retrieved.as_ref().map(|c| &c.coverage_id),
        Some(&"test_coverage".to_string())
    );
}

#[test]
fn test_wps_state_creation() {
    let info = wps::ServiceInfo {
        title: "Test WPS".to_string(),
        abstract_text: Some("Test service".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "http://localhost/wps".to_string(),
        versions: vec!["2.0.0".to_string()],
    };

    let state = wps::WpsState::new(info);
    assert_eq!(state.service_info.title, "Test WPS");

    // Built-in processes should be registered
    assert!(!state.processes.is_empty());

    // Check for built-in buffer process
    let buffer_process = state.get_process("buffer");
    assert!(buffer_process.is_some());
    assert_eq!(
        buffer_process.as_ref().map(|p| p.identifier()),
        Some("buffer")
    );
}

#[test]
fn test_csw_state_creation() {
    let info = csw::ServiceInfo {
        title: "Test CSW".to_string(),
        abstract_text: Some("Test service".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "http://localhost/csw".to_string(),
        versions: vec!["2.0.2".to_string()],
    };

    let state = csw::CswState::new(info);
    assert_eq!(state.service_info.title, "Test CSW");

    // Add record
    let record = csw::MetadataRecord {
        identifier: "test_record".to_string(),
        title: "Test Record".to_string(),
        abstract_text: Some("Test metadata record".to_string()),
        keywords: vec!["test".to_string()],
        bbox: Some((-180.0, -90.0, 180.0, 90.0)),
    };

    assert!(state.add_record(record).is_ok(), "Failed to add record");

    // Retrieve record
    let retrieved = state.records.get("test_record");
    assert!(retrieved.is_some());
}

#[test]
fn test_error_types() {
    use oxigdal_services::ServiceError;

    let err = ServiceError::MissingParameter("VERSION".to_string());
    assert_eq!(err.to_string(), "Missing required parameter: VERSION");

    let err = ServiceError::InvalidParameter("BBOX".to_string(), "malformed".to_string());
    assert_eq!(err.to_string(), "Invalid parameter 'BBOX': malformed");

    let err = ServiceError::NotFound("layer1".to_string());
    assert_eq!(err.to_string(), "Resource not found: layer1");
}

#[test]
fn test_version() {
    assert!(!oxigdal_services::VERSION.is_empty());
}
