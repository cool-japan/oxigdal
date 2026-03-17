//! Extended WMS tests for OxiGDAL services

use oxigdal_services::ServiceError;
use oxigdal_services::wcs::{
    CoverageInfo, CoverageSource, ServiceInfo as WcsServiceInfo, WcsState,
};
use oxigdal_services::wfs::{
    FeatureSource, FeatureTypeInfo, ServiceInfo as WfsServiceInfo, WfsState,
};
use oxigdal_services::wps::{
    ComplexDataType, DataType, InputDescription, LiteralDataType, OutputDescription, ProcessInputs,
    ProcessOutputs, ServiceInfo as WpsServiceInfo, WpsState,
};

// ============================================================
// WCS extended tests
// ============================================================

#[test]
fn test_wcs_add_multiple_coverages() {
    let state = WcsState::new(WcsServiceInfo {
        title: "Multi-Coverage WCS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wcs".to_string(),
        versions: vec!["2.0.1".to_string()],
    });

    for i in 0..5 {
        let coverage = CoverageInfo {
            coverage_id: format!("coverage_{}", i),
            title: format!("Coverage {}", i),
            abstract_text: None,
            native_crs: "EPSG:4326".to_string(),
            bbox: (-180.0, -90.0, 180.0, 90.0),
            grid_size: (512, 256),
            grid_origin: (-180.0, 90.0),
            grid_resolution: (0.7, -0.7),
            band_count: 1,
            band_names: vec!["Band1".to_string()],
            data_type: "Float32".to_string(),
            source: CoverageSource::Memory,
            formats: vec!["image/tiff".to_string()],
        };
        assert!(state.add_coverage(coverage).is_ok());
    }

    for i in 0..5 {
        let id = format!("coverage_{}", i);
        assert!(state.get_coverage(&id).is_some());
    }
}

#[test]
fn test_wcs_coverage_not_found() {
    let state = WcsState::new(WcsServiceInfo {
        title: "WCS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wcs".to_string(),
        versions: vec!["2.0.1".to_string()],
    });

    assert!(state.get_coverage("nonexistent").is_none());
}

#[test]
fn test_wcs_coverage_info_fields() {
    let state = WcsState::new(WcsServiceInfo {
        title: "WCS".to_string(),
        abstract_text: Some("A WCS service".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "http://localhost/wcs".to_string(),
        versions: vec!["2.0.1".to_string()],
    });

    let cov = CoverageInfo {
        coverage_id: "dem".to_string(),
        title: "Digital Elevation Model".to_string(),
        abstract_text: Some("30m SRTM".to_string()),
        native_crs: "EPSG:32654".to_string(),
        bbox: (130.0, 30.0, 145.0, 45.0),
        grid_size: (1500, 1500),
        grid_origin: (130.0, 45.0),
        grid_resolution: (0.01, -0.01),
        band_count: 1,
        band_names: vec!["Elevation".to_string()],
        data_type: "Int16".to_string(),
        source: CoverageSource::Memory,
        formats: vec!["image/tiff".to_string(), "image/png".to_string()],
    };

    assert!(state.add_coverage(cov).is_ok());
    let retrieved = state.get_coverage("dem");
    assert!(retrieved.is_some());
    let c = retrieved.expect("dem coverage should exist");
    assert_eq!(c.title, "Digital Elevation Model");
    assert_eq!(c.band_count, 1);
    assert_eq!(c.grid_size, (1500, 1500));
    assert_eq!(c.formats.len(), 2);
}

#[test]
fn test_wcs_service_info_fields() {
    let state = WcsState::new(WcsServiceInfo {
        title: "Earth Observation WCS".to_string(),
        abstract_text: Some("Remote sensing data service".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "https://example.com/wcs".to_string(),
        versions: vec!["1.1.0".to_string(), "2.0.1".to_string()],
    });

    assert_eq!(state.service_info.title, "Earth Observation WCS");
    assert_eq!(state.service_info.versions.len(), 2);
    assert!(state.service_info.abstract_text.is_some());
}

// ============================================================
// WFS extended tests
// ============================================================

#[test]
fn test_wfs_multiple_feature_types() {
    let state = WfsState::new(WfsServiceInfo {
        title: "Multi-Layer WFS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    let layers = vec!["rivers", "lakes", "mountains", "roads", "cities"];
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
        let ft = ft.expect("feature type should exist");
        assert_eq!(&ft.name, name);
        assert_eq!(ft.default_crs, "EPSG:4326");
        assert_eq!(ft.other_crs.len(), 1);
    }
}

#[test]
fn test_wfs_feature_type_not_found() {
    let state = WfsState::new(WfsServiceInfo {
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
    let state = WfsState::new(WfsServiceInfo {
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
    let mut state = WfsState::new(WfsServiceInfo {
        title: "WFS-T".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    state.enable_transactions();
    assert!(state.transactions_enabled);
}

#[test]
fn test_wfs_feature_source_variants() {
    let state = WfsState::new(WfsServiceInfo {
        title: "WFS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    // Memory source
    let ft = FeatureTypeInfo {
        name: "memory_layer".to_string(),
        title: "Memory Layer".to_string(),
        abstract_text: Some("In-memory features".to_string()),
        default_crs: "EPSG:4326".to_string(),
        other_crs: vec![],
        bbox: None,
        source: FeatureSource::Memory(vec![]),
    };
    assert!(state.add_feature_type(ft).is_ok());

    // File source
    let ft_file = FeatureTypeInfo {
        name: "file_layer".to_string(),
        title: "File Layer".to_string(),
        abstract_text: None,
        default_crs: "EPSG:4326".to_string(),
        other_crs: vec![],
        bbox: Some((-10.0, -10.0, 10.0, 10.0)),
        source: FeatureSource::File(std::path::PathBuf::from("/data/features.geojson")),
    };
    assert!(state.add_feature_type(ft_file).is_ok());

    assert!(state.get_feature_type("memory_layer").is_some());
    assert!(state.get_feature_type("file_layer").is_some());
}

// ============================================================
// WPS extended tests
// ============================================================

#[test]
fn test_wps_builtin_processes_registered() {
    let state = WpsState::new(WpsServiceInfo {
        title: "WPS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wps".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    assert!(!state.processes.is_empty());
    let buffer_process = state.get_process("buffer");
    assert!(buffer_process.is_some());
    let proc = buffer_process.expect("buffer process should exist");
    assert_eq!(proc.identifier(), "buffer");
    assert!(!proc.inputs().is_empty());
    assert!(!proc.outputs().is_empty());
}

#[test]
fn test_wps_process_inputs_outputs_structure() {
    let state = WpsState::new(WpsServiceInfo {
        title: "WPS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wps".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    let buffer_proc = state
        .get_process("buffer")
        .expect("buffer process should exist");

    // buffer process should have geometry and distance inputs
    let inputs = buffer_proc.inputs();
    assert!(!inputs.is_empty());

    let outputs = buffer_proc.outputs();
    assert!(!outputs.is_empty());
}

#[test]
fn test_wps_process_not_found() {
    let state = WpsState::new(WpsServiceInfo {
        title: "WPS".to_string(),
        abstract_text: None,
        provider: "Test".to_string(),
        service_url: "http://localhost/wps".to_string(),
        versions: vec!["2.0.0".to_string()],
    });

    assert!(state.get_process("nonexistent_process").is_none());
}

#[test]
fn test_wps_input_description_serialization() {
    let input = InputDescription {
        identifier: "geometry".to_string(),
        title: "Input Geometry".to_string(),
        abstract_text: Some("The geometry to process".to_string()),
        data_type: DataType::Complex(ComplexDataType {
            mime_type: "application/geo+json".to_string(),
            encoding: None,
            schema: None,
        }),
        min_occurs: 1,
        max_occurs: Some(1),
    };

    let json = serde_json::to_string(&input);
    assert!(json.is_ok());
    let serialized = json.expect("input description should serialize");
    assert!(serialized.contains("geometry"));
}

#[test]
fn test_wps_output_description_serialization() {
    let output = OutputDescription {
        identifier: "result".to_string(),
        title: "Result Geometry".to_string(),
        abstract_text: None,
        data_type: DataType::Complex(ComplexDataType {
            mime_type: "application/geo+json".to_string(),
            encoding: Some("UTF-8".to_string()),
            schema: None,
        }),
    };

    let json = serde_json::to_string(&output);
    assert!(json.is_ok());
}

#[test]
fn test_wps_literal_data_type() {
    let data_type = DataType::Literal(LiteralDataType {
        data_type: "double".to_string(),
        allowed_values: Some(vec!["10.0".to_string(), "100.0".to_string()]),
    });

    let json = serde_json::to_string(&data_type);
    assert!(json.is_ok());
    assert!(json.expect("should serialize").contains("double"));
}

#[test]
fn test_wps_process_inputs_empty_by_default() {
    let inputs = ProcessInputs::default();
    assert!(inputs.inputs.is_empty());
}

#[test]
fn test_wps_process_outputs_empty_by_default() {
    let outputs = ProcessOutputs::default();
    assert!(outputs.outputs.is_empty());
}

// ============================================================
// ServiceError extended tests
// ============================================================

#[test]
fn test_service_error_variants() {
    let errors = vec![
        ServiceError::InvalidParameter("BBOX".to_string(), "malformed".to_string()),
        ServiceError::MissingParameter("VERSION".to_string()),
        ServiceError::NotFound("layer1".to_string()),
        ServiceError::InvalidCrs("EPSG:99999".to_string()),
        ServiceError::InvalidBbox("not a bbox".to_string()),
        ServiceError::UnsupportedFormat("application/pdf".to_string()),
        ServiceError::UnsupportedOperation("LOCKFEATURE".to_string()),
        ServiceError::InvalidXml("unclosed tag".to_string()),
        ServiceError::InvalidGeoJson("missing type field".to_string()),
        ServiceError::Transaction("rollback failed".to_string()),
        ServiceError::ProcessExecution("computation error".to_string()),
        ServiceError::Coverage("band out of range".to_string()),
        ServiceError::Catalog("no results".to_string()),
        ServiceError::Serialization("invalid json".to_string()),
        ServiceError::Xml("malformed xml".to_string()),
        ServiceError::Internal("null pointer dereference".to_string()),
    ];

    for err in &errors {
        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "Error message should not be empty for {:?}",
            err
        );
    }
}

#[test]
fn test_service_error_not_found_message() {
    let err = ServiceError::NotFound("test_layer".to_string());
    assert!(err.to_string().contains("test_layer"));
}

#[test]
fn test_service_error_invalid_crs_message() {
    let err = ServiceError::InvalidCrs("EPSG:999999".to_string());
    assert!(err.to_string().contains("EPSG:999999"));
}

#[test]
fn test_service_error_unsupported_operation_message() {
    let err = ServiceError::UnsupportedOperation("LOCKFEATURE".to_string());
    assert!(err.to_string().contains("LOCKFEATURE"));
}
