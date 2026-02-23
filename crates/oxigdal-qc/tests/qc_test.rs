//! Integration tests for oxigdal-qc.

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_core::vector::{
    Coordinate, Feature, FeatureCollection, Geometry, LineString, Point, Polygon,
};
use oxigdal_qc::prelude::*;
use std::collections::HashMap;

#[test]
fn test_raster_completeness() {
    let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
    let checker = CompletenessChecker::new();
    let result = checker.check_buffer(&buffer);

    assert!(result.is_ok());
    let result = result.expect("completeness check should return valid result");
    assert_eq!(result.total_pixels, 10000);
    assert_eq!(result.valid_pixels, 10000);
}

#[test]
fn test_raster_consistency() {
    let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
    let checker = ConsistencyChecker::new();
    let result = checker.check_buffer(&buffer);

    assert!(result.is_ok());
}

#[test]
fn test_raster_accuracy() {
    let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
    let bbox = BoundingBox::new(0.0, 0.0, 100.0, 100.0)
        .expect("bounding box should be created with valid coordinates");
    let geotransform = GeoTransform::from_bounds(&bbox, 100, 100)
        .expect("geotransform should be created from valid bounds");

    let checker = AccuracyChecker::new();
    let result = checker.check_raster(&buffer, &geotransform, Some(&bbox));

    assert!(result.is_ok());
}

#[test]
fn test_vector_topology() {
    let point = Point::new(0.0, 0.0);
    let feature = Feature::new(Geometry::Point(point));
    let collection = FeatureCollection {
        features: vec![feature],
        metadata: None,
    };

    let checker = TopologyChecker::new();
    let result = checker.validate(&collection);

    assert!(result.is_ok());
    let result = result.expect("topology validation should return valid result");
    assert_eq!(result.feature_count, 1);
}

#[test]
fn test_vector_attribution() {
    let feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    let collection = FeatureCollection {
        features: vec![feature],
        metadata: None,
    };

    let checker = AttributionChecker::new();
    let result = checker.validate(&collection);

    assert!(result.is_ok());
}

#[test]
fn test_metadata_validation() {
    let mut metadata = HashMap::new();
    metadata.insert("title".to_string(), "Test Dataset".to_string());
    metadata.insert("abstract".to_string(), "Test description".to_string());
    metadata.insert("topic_category".to_string(), "elevation".to_string());
    metadata.insert("contact".to_string(), "test@example.com".to_string());
    metadata.insert("date".to_string(), "2024-01-01".to_string());
    metadata.insert("spatial_extent".to_string(), "-180,-90,180,90".to_string());

    let checker = MetadataChecker::new();
    let result = checker.check(&metadata);

    assert!(result.is_ok());
    let result = result.expect("metadata check should return valid result");
    assert_eq!(result.required_fields_missing, 0);
}

#[test]
fn test_quality_report() {
    let mut report = QualityReport::new("Test QC Report");

    let section = ReportSection {
        title: "Test Section".to_string(),
        description: "A test section".to_string(),
        results: vec![
            ("Check 1".to_string(), "Passed".to_string()),
            ("Check 2".to_string(), "Failed".to_string()),
        ],
        issues: vec![],
        passed: true,
    };

    report.add_section(section);
    report.finalize();

    assert_eq!(report.summary.total_checks, 1);
    assert_eq!(report.summary.passed_checks, 1);
}

#[test]
fn test_rules_engine() {
    let mut ruleset = RuleSet::new("Test Rules", "Test rule set");

    let rule = RuleBuilder::new("R001", "Max Value Check")
        .description("Checks if value is within threshold")
        .category(RuleCategory::Raster)
        .severity(Severity::Major)
        .threshold("max_value", ComparisonOperator::LessThanOrEqual, 100.0)
        .build();

    ruleset.add_rule(rule);

    let engine = RulesEngine::new(ruleset);

    let mut data = HashMap::new();
    data.insert("max_value".to_string(), 150.0);

    let result = engine.execute_all(&data);
    assert!(result.is_ok());

    let issues = result.expect("rules engine should execute successfully and return issues");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].severity, Severity::Major);
}

#[test]
fn test_topology_fixer() {
    let linestring = LineString {
        coords: vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(0.0, 0.0), // Duplicate
            Coordinate::new_2d(1.0, 1.0),
        ],
    };

    let feature = Feature::new(Geometry::LineString(linestring));
    let collection = FeatureCollection {
        features: vec![feature],
        metadata: None,
    };

    let fixer = TopologyFixer::new(FixStrategy::Conservative);
    let result = fixer.fix_topology(&collection);

    assert!(result.is_ok());
    let (_fixed_collection, fix_result) =
        result.expect("topology fixer should return fixed collection and results");
    assert!(fix_result.features_fixed > 0 || fix_result.features_unchanged > 0);
}

#[test]
fn test_severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Minor);
    assert!(Severity::Minor < Severity::Major);
    assert!(Severity::Major < Severity::Critical);
}

#[test]
fn test_quality_assessment() {
    let assessment = QualityAssessment::Excellent;
    assert_eq!(format!("{:?}", assessment), "Excellent");
}

#[test]
fn test_fix_strategy() {
    let strategy = FixStrategy::Conservative;
    assert_eq!(strategy, FixStrategy::Conservative);
}

#[test]
fn test_invalid_polygon_detection() {
    // Create a polygon with too few points
    let polygon = Polygon {
        exterior: LineString {
            coords: vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 0.0)],
        },
        interiors: vec![],
    };

    let feature = Feature::new(Geometry::Polygon(polygon));
    let collection = FeatureCollection {
        features: vec![feature],
        metadata: None,
    };

    let checker = TopologyChecker::new();
    let result = checker.validate(&collection);

    assert!(result.is_ok());
    let result =
        result.expect("topology validation should return valid result for invalid polygon test");
    assert!(result.invalid_geometries > 0);
}
