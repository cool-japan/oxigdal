//! Extended tests for metadata standards

use oxigdal_metadata::common::{
    Address, BoundingBox, ContactInfo, Keyword, License, TemporalExtent,
};
use oxigdal_metadata::datacite::*;
use oxigdal_metadata::dcat::{Agent, AgentType, Dataset};
use oxigdal_metadata::error::MetadataError;
use oxigdal_metadata::fgdc::*;
use oxigdal_metadata::inspire::*;
use oxigdal_metadata::iso19115::{Iso19115Metadata, ResponsibleParty, Role};
use oxigdal_metadata::*;

// ============================================================
// BoundingBox edge cases
// ============================================================

#[test]
fn test_bounding_box_at_poles() {
    let north_pole = BoundingBox::new(-180.0, 180.0, 89.9, 90.0);
    assert!(north_pole.is_valid());

    let south_pole = BoundingBox::new(-180.0, 180.0, -90.0, -89.9);
    assert!(south_pole.is_valid());
}

#[test]
fn test_bounding_box_single_point() {
    let point = BoundingBox::new(35.0, 35.0, 45.0, 45.0);
    assert!(point.is_valid()); // west == east is valid (degenerate case)
}

#[test]
fn test_bounding_box_out_of_range_longitude() {
    let invalid = BoundingBox::new(-181.0, 10.0, 40.0, 50.0);
    assert!(!invalid.is_valid());

    let invalid2 = BoundingBox::new(-10.0, 181.0, 40.0, 50.0);
    assert!(!invalid2.is_valid());
}

#[test]
fn test_bounding_box_out_of_range_latitude() {
    let invalid = BoundingBox::new(-10.0, 10.0, -91.0, 50.0);
    assert!(!invalid.is_valid());

    let invalid2 = BoundingBox::new(-10.0, 10.0, 40.0, 91.0);
    assert!(!invalid2.is_valid());
}

#[test]
fn test_bounding_box_fields() {
    let bbox = BoundingBox::new(-15.5, 15.5, -5.0, 5.0);
    assert_eq!(bbox.west, -15.5);
    assert_eq!(bbox.east, 15.5);
    assert_eq!(bbox.south, -5.0);
    assert_eq!(bbox.north, 5.0);
}

#[test]
fn test_bounding_box_serialization() {
    let bbox = BoundingBox::new(-180.0, 180.0, -90.0, 90.0);
    let json = serde_json::to_string(&bbox);
    assert!(json.is_ok());
    let deserialized: serde_json::Result<BoundingBox> =
        serde_json::from_str(&json.expect("serialized bbox"));
    assert!(deserialized.is_ok());
    let bb = deserialized.expect("deserialized bbox");
    assert_eq!(bb.west, -180.0);
    assert_eq!(bb.east, 180.0);
}

// ============================================================
// ISO 19115 extended tests
// ============================================================

#[test]
fn test_iso19115_default_values() {
    let meta = Iso19115Metadata::default();
    assert_eq!(meta.metadata_standard_name, "ISO 19115:2014");
    assert_eq!(meta.language, Some("eng".to_string()));
}

#[test]
fn test_iso19115_builder_with_keywords() {
    let meta = Iso19115Metadata::builder()
        .title("Keyword Test Dataset")
        .abstract_text("Dataset with multiple keywords")
        .keywords(vec![
            "geospatial",
            "elevation",
            "dem",
            "srtm",
            "remote-sensing",
        ])
        .build()
        .expect("should build with 5 keywords");

    let keywords = &meta.identification_info[0].keywords;
    assert_eq!(keywords.len(), 1);
    assert_eq!(keywords[0].len(), 5);
}

#[test]
fn test_iso19115_with_bbox_validation() {
    let bbox = BoundingBox::new(-10.0, 10.0, 40.0, 60.0);
    assert!(bbox.is_valid());

    let meta = Iso19115Metadata::builder()
        .title("Bbox Validation Test")
        .abstract_text("Test abstract")
        .bbox(bbox)
        .build()
        .expect("should build with valid bbox");

    let extent = &meta.identification_info[0].extent;
    assert!(extent.geographic_extent.is_some());
}

#[test]
fn test_iso19115_builder_file_identifier() {
    let meta = Iso19115Metadata::builder()
        .title("Test")
        .abstract_text("Abstract")
        .file_identifier("unique-identifier-12345")
        .build()
        .expect("should build with file identifier");

    assert_eq!(
        meta.file_identifier,
        Some("unique-identifier-12345".to_string())
    );
}

#[test]
fn test_iso19115_serialization_round_trip() {
    let meta = Iso19115Metadata::builder()
        .title("Round-trip Test")
        .abstract_text("Testing serialization")
        .keywords(vec!["test", "roundtrip"])
        .build()
        .expect("build should succeed");

    let json = meta.to_json().expect("to_json should succeed");
    let restored = Iso19115Metadata::from_json(&json).expect("from_json should succeed");

    assert_eq!(
        restored.identification_info[0].citation.title,
        "Round-trip Test"
    );
}

#[test]
fn test_iso19115_to_json_contains_title() {
    let meta = Iso19115Metadata::builder()
        .title("JSON Validation Test")
        .abstract_text("Testing JSON output")
        .build()
        .expect("build should succeed");

    let json = meta.to_json().expect("JSON serialization");
    assert!(json.contains("JSON Validation Test"));
}

#[test]
fn test_iso19115_invalid_json_input() {
    let result = Iso19115Metadata::from_json("{ not valid json }");
    assert!(result.is_err());
}

#[test]
fn test_iso19115_quality_score_with_full_metadata() {
    let contact = ResponsibleParty {
        individual_name: Some("Dr. Jane Smith".to_string()),
        organization_name: Some("Research Institute".to_string()),
        position_name: Some("Data Manager".to_string()),
        contact_info: Some(ContactInfo {
            individual_name: None,
            organization_name: None,
            position_name: None,
            email: Some("jane@example.org".to_string()),
            phone: None,
            address: None,
            online_resource: None,
        }),
        role: Role::PointOfContact,
    };

    let meta = Iso19115Metadata::builder()
        .title("Comprehensive Dataset")
        .abstract_text("A well-described dataset for quality testing")
        .keywords(vec!["quality", "comprehensive", "test"])
        .bbox(BoundingBox::new(-180.0, 180.0, -90.0, 90.0))
        .contact(contact)
        .file_identifier("comp-test-001")
        .build()
        .expect("should build comprehensive metadata");

    let report = validate::validate_iso19115(&meta).expect("should validate");
    assert!(report.quality_score >= 70.0);
}

// ============================================================
// Validation report tests
// ============================================================

#[test]
fn test_validation_report_default_is_valid() {
    let report = validate::ValidationReport::default();
    assert!(report.is_valid);
    assert!(report.errors.is_empty());
    assert!(report.warnings.is_empty());
    assert!(report.missing_required.is_empty());
}

#[test]
fn test_validation_report_is_complete() {
    let report = validate::ValidationReport {
        is_valid: true,
        missing_required: vec![],
        ..Default::default()
    };
    assert!(report.is_complete());
}

#[test]
fn test_validation_report_not_complete_if_missing_fields() {
    let report = validate::ValidationReport {
        is_valid: true,
        missing_required: vec!["title".to_string()],
        ..Default::default()
    };
    assert!(!report.is_complete());
}

#[test]
fn test_validation_report_missing_fields_combines() {
    let report = validate::ValidationReport {
        is_valid: true,
        missing_required: vec!["title".to_string()],
        missing_recommended: vec!["contact".to_string()],
        ..Default::default()
    };
    let all_missing = report.missing_fields();
    assert_eq!(all_missing.len(), 2);
    assert!(all_missing.contains(&"title".to_string()));
    assert!(all_missing.contains(&"contact".to_string()));
}

#[test]
fn test_validation_report_quality_score_with_warnings() {
    let mut report = validate::ValidationReport {
        completeness: 80.0,
        ..Default::default()
    };
    report.warnings.push(validate::ValidationWarning {
        field: "contact".to_string(),
        message: "Contact info missing".to_string(),
    });
    report.warnings.push(validate::ValidationWarning {
        field: "keywords".to_string(),
        message: "No keywords".to_string(),
    });
    report.calculate_quality_score();
    // 80 - 2*2 = 76
    assert_eq!(report.quality_score, 76.0);
}

#[test]
fn test_validation_error_severity_error() {
    let err = validate::ValidationError {
        field: "title".to_string(),
        message: "Title is required".to_string(),
        severity: validate::ErrorSeverity::Error,
    };
    assert!(matches!(err.severity, validate::ErrorSeverity::Error));
}

#[test]
fn test_validation_error_severity_error_variant_critical() {
    let err = validate::ValidationError {
        field: "abstract".to_string(),
        message: "Abstract is required".to_string(),
        severity: validate::ErrorSeverity::Critical,
    };
    assert!(matches!(err.severity, validate::ErrorSeverity::Critical));
}

// ============================================================
// MetadataError tests
// ============================================================

#[test]
fn test_metadata_error_invalid_format() {
    let err = MetadataError::InvalidFormat("unknown format".to_string());
    assert!(err.to_string().contains("unknown format"));
}

#[test]
fn test_metadata_error_missing_field() {
    let err = MetadataError::MissingField("title".to_string());
    assert!(err.to_string().contains("title"));
}

#[test]
fn test_metadata_error_invalid_value() {
    let err = MetadataError::InvalidValue {
        field: "date".to_string(),
        reason: "not ISO 8601".to_string(),
    };
    assert!(err.to_string().contains("date"));
    assert!(err.to_string().contains("not ISO 8601"));
}

#[test]
fn test_metadata_error_json() {
    let err = MetadataError::JsonError("unexpected token".to_string());
    assert!(err.to_string().contains("unexpected token"));
}

#[test]
fn test_metadata_error_transform() {
    let err = MetadataError::TransformError("conversion failed".to_string());
    assert!(err.to_string().contains("conversion failed"));
}

#[test]
fn test_metadata_error_validation() {
    let err = MetadataError::ValidationError("required field missing".to_string());
    assert!(err.to_string().contains("required field missing"));
}

// ============================================================
// DCAT extended tests
// ============================================================

#[test]
fn test_dcat_dataset_with_identifier() {
    let mut dataset = Dataset::builder()
        .title("Identified Dataset")
        .description("Testing identifier")
        .build()
        .expect("build should succeed");

    dataset
        .identifier
        .push("urn:example:dataset:001".to_string());
    assert!(
        dataset
            .identifier
            .contains(&"urn:example:dataset:001".to_string())
    );
}

#[test]
fn test_dcat_dataset_with_multiple_keywords() {
    let dataset = Dataset::builder()
        .title("Multi-keyword Dataset")
        .description("Dataset with many keywords")
        .keyword("satellite")
        .keyword("imagery")
        .keyword("remote-sensing")
        .keyword("optical")
        .build()
        .expect("build should succeed");

    assert_eq!(dataset.keyword.len(), 4);
    assert!(dataset.keyword.contains(&"satellite".to_string()));
}

#[test]
fn test_dcat_dataset_with_theme() {
    let dataset = Dataset::builder()
        .title("Themed Dataset")
        .description("Dataset about land use")
        .theme("http://inspire.ec.europa.eu/theme/lu")
        .build()
        .expect("build should succeed");

    assert!(!dataset.theme.is_empty());
}

#[test]
fn test_dcat_dataset_serialization() {
    let dataset = Dataset::builder()
        .title("Serializable Dataset")
        .description("Testing DCAT serialization")
        .keyword("test")
        .build()
        .expect("build should succeed");

    let json = serde_json::to_string(&dataset);
    assert!(json.is_ok());
    let restored: serde_json::Result<Dataset> =
        serde_json::from_str(&json.expect("json serialized"));
    assert!(restored.is_ok());
}

#[test]
fn test_dcat_agent_organization() {
    let org = Agent::organization("COOLJAPAN OU");
    assert_eq!(org.name, "COOLJAPAN OU");
    assert!(matches!(org.agent_type, Some(AgentType::Organization)));
}

#[test]
fn test_dcat_agent_person() {
    let person = Agent::person("Alice Brown");
    assert_eq!(person.name, "Alice Brown");
    assert!(matches!(person.agent_type, Some(AgentType::Person)));
}

#[test]
fn test_dcat_validation_with_full_dataset() {
    let mut dataset = Dataset::builder()
        .title("Full DCAT Dataset")
        .description("Comprehensive DCAT dataset for validation")
        .keyword("test")
        .keyword("full")
        .theme("environment")
        .build()
        .expect("build should succeed");

    dataset.identifier.push("dcat:full:001".to_string());
    let report = validate::validate_dcat(&dataset).expect("validation should succeed");
    assert!(report.is_valid);
}

// ============================================================
// DataCite extended tests
// ============================================================

#[test]
fn test_datacite_with_multiple_creators() {
    let meta = DataCiteMetadata::builder()
        .identifier("10.0000/multi-creator", IdentifierType::Doi)
        .creator(Creator::new("Smith, John"))
        .creator(Creator::new("Jones, Sarah"))
        .creator(Creator::new("Brown, Alice"))
        .title("Multi-Creator Dataset")
        .publisher("Zenodo")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Dataset)
        .build()
        .expect("build should succeed");

    assert_eq!(meta.creators.len(), 3);
}

#[test]
fn test_datacite_with_subject() {
    let meta = DataCiteMetadata::builder()
        .identifier("10.0000/subject-test", IdentifierType::Doi)
        .creator(Creator::new("Author"))
        .title("Subject Test")
        .publisher("Publisher")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Dataset)
        .subject(Subject::new("geospatial"))
        .subject(Subject::new("satellite imagery"))
        .build()
        .expect("build should succeed");

    assert!(!meta.subjects.is_empty());
}

#[test]
fn test_datacite_resource_type_software() {
    let meta = DataCiteMetadata::builder()
        .identifier("10.0000/software-test", IdentifierType::Doi)
        .creator(Creator::new("Dev Team"))
        .title("Software Tool")
        .publisher("GitHub")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Software)
        .build()
        .expect("build should succeed");

    assert!(matches!(
        meta.resource_type.resource_type_general,
        ResourceTypeGeneral::Software
    ));
}

#[test]
fn test_datacite_identifier_type_doi() {
    let meta = DataCiteMetadata::builder()
        .identifier("10.5281/zenodo.123456", IdentifierType::Doi)
        .creator(Creator::new("Author"))
        .title("DOI Test")
        .publisher("Zenodo")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Dataset)
        .build()
        .expect("build should succeed");

    assert!(matches!(
        meta.identifier.identifier_type,
        IdentifierType::Doi
    ));
    assert!(meta.identifier.identifier.starts_with("10."));
}

#[test]
fn test_datacite_json_round_trip() {
    let meta = DataCiteMetadata::builder()
        .identifier("10.0000/rt-test", IdentifierType::Doi)
        .creator(Creator::new("Round Trip Author"))
        .title("Round-trip DataCite")
        .publisher("Test Publisher")
        .publication_year(2025)
        .resource_type(ResourceTypeGeneral::Dataset)
        .build()
        .expect("build should succeed");

    let json = meta.to_json().expect("should serialize to JSON");
    let restored = DataCiteMetadata::from_json(&json).expect("should deserialize from JSON");
    assert_eq!(restored.titles[0].title, "Round-trip DataCite");
    assert_eq!(restored.publication_year, 2025);
}

// ============================================================
// INSPIRE extended tests
// ============================================================

#[test]
fn test_inspire_themes_accessible() {
    // Test all INSPIRE themes are accessible
    let themes = vec![
        InspireTheme::Elevation,
        InspireTheme::Hydrography,
        InspireTheme::ProtectedSites,
        InspireTheme::TransportNetworks,
        InspireTheme::LandUse,
        InspireTheme::Geology,
    ];

    for theme in &themes {
        // Just ensure they're accessible and can be cloned
        let _cloned = *theme;
    }
    assert_eq!(themes.len(), 6);
}

#[test]
fn test_inspire_resource_locator_functions() {
    let download = ResourceLocator {
        url: "https://example.com/wfs".to_string(),
        description: Some("WFS endpoint".to_string()),
        function: ResourceLocatorFunction::Download,
    };

    let order = ResourceLocator {
        url: "https://example.com/wms".to_string(),
        description: Some("WMS endpoint".to_string()),
        function: ResourceLocatorFunction::Order,
    };

    let info = ResourceLocator {
        url: "https://example.com/info".to_string(),
        description: None,
        function: ResourceLocatorFunction::Information,
    };

    assert!(matches!(
        download.function,
        ResourceLocatorFunction::Download
    ));
    assert!(matches!(order.function, ResourceLocatorFunction::Order));
    assert!(matches!(
        info.function,
        ResourceLocatorFunction::Information
    ));
}

#[test]
fn test_inspire_unique_resource_identifier() {
    let uri = UniqueResourceIdentifier::new("http://example.com/datasets/001");
    assert!(uri.code.contains("001") || !uri.code.is_empty());
}

#[test]
fn test_inspire_metadata_default() {
    let meta = InspireMetadata::default();
    assert!(!meta.metadata_language.is_empty());
}

// ============================================================
// FGDC extended tests
// ============================================================

#[test]
fn test_fgdc_builder_with_purpose() {
    let meta = FgdcMetadata::builder()
        .title("FGDC Purpose Test")
        .abstract_text("Testing purpose field")
        .purpose("Scientific research and analysis")
        .build()
        .expect("build should succeed");

    assert_eq!(
        meta.idinfo.descript.purpose.as_deref(),
        Some("Scientific research and analysis")
    );
}

#[test]
fn test_fgdc_builder_with_keywords() {
    let meta = FgdcMetadata::builder()
        .title("FGDC Keywords Test")
        .abstract_text("Testing keyword field")
        .keywords("General", vec!["fgdc", "geospatial", "elevation"])
        .build()
        .expect("build should succeed");

    assert!(!meta.idinfo.keywords.is_empty());
}

#[test]
fn test_fgdc_json_round_trip() {
    let meta = FgdcMetadata::builder()
        .title("FGDC Round-trip")
        .abstract_text("Testing JSON serialization")
        .build()
        .expect("build should succeed");

    let json = meta.to_json().expect("should serialize");
    let restored = FgdcMetadata::from_json(&json).expect("should deserialize");
    assert_eq!(restored.idinfo.citation.citeinfo.title, "FGDC Round-trip");
}

#[test]
fn test_fgdc_invalid_json_input() {
    let result = FgdcMetadata::from_json("{ invalid }");
    assert!(result.is_err());
}

// ============================================================
// Common types tests
// ============================================================

#[test]
fn test_keyword_with_thesaurus() {
    let kw = Keyword {
        keyword: "environment".to_string(),
        thesaurus: Some("GEMET".to_string()),
    };
    assert_eq!(kw.keyword, "environment");
    assert_eq!(kw.thesaurus.as_deref(), Some("GEMET"));
}

#[test]
fn test_keyword_without_thesaurus() {
    let kw = Keyword {
        keyword: "land use".to_string(),
        thesaurus: None,
    };
    assert_eq!(kw.keyword, "land use");
    assert!(kw.thesaurus.is_none());
}

#[test]
fn test_license_with_url() {
    let license = License {
        name: "CC BY 4.0".to_string(),
        url: Some("https://creativecommons.org/licenses/by/4.0/".to_string()),
    };
    assert_eq!(license.name, "CC BY 4.0");
    assert!(license.url.is_some());
}

#[test]
fn test_address_struct() {
    let addr = Address {
        delivery_point: Some("123 Main Street".to_string()),
        city: Some("Tokyo".to_string()),
        administrative_area: Some("Tokyo".to_string()),
        postal_code: Some("100-0001".to_string()),
        country: Some("Japan".to_string()),
    };
    assert_eq!(addr.city.as_deref(), Some("Tokyo"));
    assert_eq!(addr.country.as_deref(), Some("Japan"));
}

#[test]
fn test_temporal_extent_both_dates() {
    let now = chrono::Utc::now();
    let later = now + chrono::Duration::days(365);

    let extent = TemporalExtent {
        start: Some(now),
        end: Some(later),
    };

    assert!(extent.start.is_some());
    assert!(extent.end.is_some());
    let end = extent.end.expect("end should be Some");
    let start = extent.start.expect("start should be Some");
    assert!(end > start);
}

#[test]
fn test_temporal_extent_open_ended() {
    let now = chrono::Utc::now();
    let extent = TemporalExtent {
        start: Some(now),
        end: None,
    };
    assert!(extent.start.is_some());
    assert!(extent.end.is_none());
}
