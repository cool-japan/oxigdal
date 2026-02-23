//! Comprehensive tests for metadata crate.

use oxigdal_metadata::common::{BoundingBox, ContactInfo, TemporalExtent};
use oxigdal_metadata::datacite::*;
use oxigdal_metadata::dcat::{Agent, AgentType, Dataset};
use oxigdal_metadata::fgdc::*;
use oxigdal_metadata::inspire::*;
use oxigdal_metadata::iso19115::{Iso19115Metadata, ResponsibleParty, Role};
use oxigdal_metadata::*;

#[test]
fn test_iso19115_builder() {
    let metadata = Iso19115Metadata::builder()
        .title("Test Dataset")
        .abstract_text("This is a test dataset")
        .keywords(vec!["test", "dataset", "geospatial"])
        .bbox(BoundingBox::new(-10.0, 10.0, 40.0, 50.0))
        .build();

    assert!(metadata.is_ok());
    let metadata = metadata.expect("Failed to build metadata");
    assert_eq!(
        metadata.identification_info[0].citation.title,
        "Test Dataset"
    );
    assert_eq!(
        metadata.identification_info[0].abstract_text,
        "This is a test dataset"
    );
}

#[test]
fn test_iso19115_validation() {
    let metadata = Iso19115Metadata::builder()
        .title("Valid Dataset")
        .abstract_text("A valid dataset with all required fields")
        .build()
        .expect("Failed to build ISO19115 metadata");

    let validation =
        validate::validate_iso19115(&metadata).expect("Failed to validate ISO19115 metadata");
    assert!(validation.is_valid);
}

#[test]
fn test_iso19115_missing_fields() {
    // Create metadata without using builder to test validation
    let metadata = Iso19115Metadata::default();

    let validation = validate::validate_iso19115(&metadata)
        .expect("Failed to validate default ISO19115 metadata");
    assert!(!validation.is_valid);
    assert!(!validation.missing_required.is_empty());
}

#[test]
fn test_fgdc_builder() {
    let metadata = FgdcMetadata::builder()
        .title("FGDC Test Dataset")
        .abstract_text("Testing FGDC metadata")
        .purpose("Testing purposes")
        .keywords("General", vec!["fgdc", "metadata"])
        .build();

    assert!(metadata.is_ok());
    let metadata = metadata.expect("Failed to build metadata");
    assert_eq!(metadata.idinfo.citation.citeinfo.title, "FGDC Test Dataset");
}

#[test]
fn test_fgdc_validation() {
    let metadata = FgdcMetadata::builder()
        .title("Valid FGDC")
        .abstract_text("Valid abstract")
        .build()
        .expect("Failed to build valid FGDC metadata");

    let validation = validate::validate_fgdc(&metadata).expect("Failed to validate FGDC metadata");
    assert!(validation.is_valid);
}

#[test]
fn test_inspire_builder() {
    let locator = ResourceLocator {
        url: "https://example.com/data".to_string(),
        description: Some("Test resource".to_string()),
        function: ResourceLocatorFunction::Download,
    };

    let metadata = InspireMetadata::builder()
        .title("INSPIRE Dataset")
        .resource_locator(locator)
        .unique_identifier(UniqueResourceIdentifier::new("test-id"))
        .build();

    assert!(metadata.is_ok());
}

#[test]
fn test_inspire_validation() {
    let locator = ResourceLocator {
        url: "https://example.com/data".to_string(),
        description: Some("Test resource".to_string()),
        function: ResourceLocatorFunction::Download,
    };

    let mut metadata = InspireMetadata::default();
    metadata.resource_locator.push(locator);
    metadata
        .unique_resource_identifier
        .push(UniqueResourceIdentifier::new("test-id"));
    metadata.responsible_organisation.push(ResponsibleParty {
        individual_name: Some("Test Person".to_string()),
        organization_name: Some("Test Org".to_string()),
        position_name: None,
        contact_info: None,
        role: Role::PointOfContact,
    });
    metadata
        .conditions_for_access_and_use
        .push("Public".to_string());
    metadata
        .limitations_on_public_access
        .push("None".to_string());

    let validation = metadata
        .validate()
        .expect("Failed to validate INSPIRE metadata");
    assert!(validation.is_valid);
}

#[test]
fn test_datacite_builder() {
    let metadata = DataCiteMetadata::builder()
        .identifier("10.5281/zenodo.123456", IdentifierType::Doi)
        .creator(Creator::new("Smith, John"))
        .title("Research Dataset")
        .publisher("Zenodo")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Dataset)
        .build();

    assert!(metadata.is_ok());
    let metadata = metadata.expect("Failed to build metadata");
    assert_eq!(metadata.identifier.identifier, "10.5281/zenodo.123456");
    assert_eq!(metadata.publication_year, 2024);
}

#[test]
fn test_datacite_validation() {
    let metadata = DataCiteMetadata::builder()
        .identifier("10.0000/test", IdentifierType::Doi)
        .creator(Creator::new("Test Creator"))
        .title("Test Title")
        .publisher("Test Publisher")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Dataset)
        .subject(Subject::new("testing"))
        .build()
        .expect("Failed to build DataCite metadata for validation test");

    let validation =
        validate::validate_datacite(&metadata).expect("Failed to validate DataCite metadata");
    assert!(validation.is_valid);
}

#[test]
fn test_dcat_builder() {
    let dataset = Dataset::builder()
        .title("DCAT Dataset")
        .description("Testing DCAT")
        .keyword("catalog")
        .keyword("data")
        .build();

    assert!(dataset.is_ok());
    let dataset = dataset.expect("Failed to build DCAT dataset");
    assert_eq!(dataset.title[0].value, "DCAT Dataset");
}

#[test]
fn test_dcat_validation() {
    let dataset = Dataset::builder()
        .title("Test Dataset")
        .description("Test Description")
        .keyword("test")
        .theme("testing")
        .build()
        .expect("Failed to build DCAT dataset for validation");

    let validation = validate::validate_dcat(&dataset).expect("Failed to validate DCAT dataset");
    assert!(validation.is_valid);
}

#[test]
fn test_bounding_box_validation() {
    // Valid bounding box
    let valid_bbox = BoundingBox::new(-10.0, 10.0, 40.0, 50.0);
    assert!(valid_bbox.is_valid());

    // Invalid: west > east
    let invalid_bbox = BoundingBox::new(10.0, -10.0, 40.0, 50.0);
    assert!(!invalid_bbox.is_valid());

    // Invalid: south > north
    let invalid_bbox = BoundingBox::new(-10.0, 10.0, 50.0, 40.0);
    assert!(!invalid_bbox.is_valid());

    // Invalid: out of range
    let invalid_bbox = BoundingBox::new(-200.0, 10.0, 40.0, 50.0);
    assert!(!invalid_bbox.is_valid());
}

#[test]
fn test_iso_to_fgdc_transform() {
    let iso = Iso19115Metadata::builder()
        .title("Transform Test")
        .abstract_text("Testing transformation")
        .keywords(vec!["transform", "test"])
        .build()
        .expect("Failed to build ISO19115 metadata for transformation test");

    let fgdc = transform::iso19115_to_fgdc(&iso).expect("Failed to transform ISO19115 to FGDC");
    assert_eq!(fgdc.idinfo.citation.citeinfo.title, "Transform Test");
    assert_eq!(fgdc.idinfo.descript.abstract_text, "Testing transformation");
}

#[test]
fn test_fgdc_to_iso_transform() {
    let fgdc = FgdcMetadata::builder()
        .title("FGDC Transform")
        .abstract_text("Testing reverse transformation")
        .build()
        .expect("Failed to build FGDC metadata for reverse transformation");

    let iso = transform::fgdc_to_iso19115(&fgdc).expect("Failed to transform FGDC to ISO19115");
    assert_eq!(iso.identification_info[0].citation.title, "FGDC Transform");
}

#[test]
fn test_datacite_to_iso_transform() {
    let datacite = DataCiteMetadata::builder()
        .identifier("10.0000/test", IdentifierType::Doi)
        .creator(Creator::new("Test Author"))
        .title("DataCite Test")
        .publisher("Test Pub")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Dataset)
        .build()
        .expect("Failed to build DataCite metadata for transformation");

    let iso = transform::datacite_to_iso19115(&datacite)
        .expect("Failed to transform DataCite to ISO19115");
    assert_eq!(iso.identification_info[0].citation.title, "DataCite Test");
}

#[test]
fn test_metadata_extractor() {
    let extractor = extract::MetadataExtractor::new()
        .with_spatial(true)
        .with_temporal(true)
        .with_max_keywords(10);

    assert!(extractor.extract_spatial);
    assert!(extractor.extract_temporal);
    assert_eq!(extractor.max_keywords, 10);
}

#[test]
fn test_extracted_metadata_to_iso() {
    let extracted = extract::ExtractedMetadata {
        title: Some("Extracted Dataset".to_string()),
        abstract_text: Some("Extracted from file".to_string()),
        bbox: Some(BoundingBox::new(-10.0, 10.0, 40.0, 50.0)),
        keywords: vec!["extracted".to_string(), "metadata".to_string()],
        ..Default::default()
    };

    let iso = extract::to_iso19115(&extracted).expect("Failed to extract metadata to ISO19115");
    assert_eq!(
        iso.identification_info[0].citation.title,
        "Extracted Dataset"
    );
}

#[test]
fn test_vocabulary_validator() {
    let vocab = validate::VocabularyValidator::new(
        vec!["dataset".to_string(), "service".to_string()],
        false,
    );

    assert!(vocab.validate("Dataset"));
    assert!(vocab.validate("SERVICE"));
    assert!(!vocab.validate("unknown"));
}

#[test]
fn test_field_mappings() {
    let mappings = transform::get_field_mappings("ISO19115", "FGDC");
    assert!(!mappings.is_empty());

    // Check for title mapping
    let has_title = mappings
        .iter()
        .any(|m| m.source_field.contains("title") && m.target_field.contains("title"));
    assert!(has_title);
}

#[test]
fn test_json_serialization_iso() {
    let metadata = Iso19115Metadata::builder()
        .title("JSON Test")
        .abstract_text("Testing JSON serialization")
        .build()
        .expect("Failed to build ISO19115 metadata for JSON serialization");

    let json = metadata
        .to_json()
        .expect("Failed to serialize ISO19115 metadata to JSON");
    assert!(json.contains("JSON Test"));

    let parsed = Iso19115Metadata::from_json(&json)
        .expect("Failed to deserialize ISO19115 metadata from JSON");
    assert_eq!(parsed.identification_info[0].citation.title, "JSON Test");
}

#[test]
fn test_json_serialization_datacite() {
    let metadata = DataCiteMetadata::builder()
        .identifier("10.0000/test", IdentifierType::Doi)
        .creator(Creator::new("Test"))
        .title("JSON DataCite")
        .publisher("Pub")
        .publication_year(2024)
        .resource_type(ResourceTypeGeneral::Dataset)
        .build()
        .expect("Failed to build DataCite metadata for JSON serialization");

    let json = metadata
        .to_json()
        .expect("Failed to serialize DataCite metadata to JSON");
    assert!(json.contains("JSON DataCite"));

    let parsed = DataCiteMetadata::from_json(&json)
        .expect("Failed to deserialize DataCite metadata from JSON");
    assert_eq!(parsed.titles[0].title, "JSON DataCite");
}

#[test]
fn test_validation_report_quality_score() {
    let mut report = validate::ValidationReport {
        completeness: 90.0,
        ..Default::default()
    };

    // No errors or warnings
    report.calculate_quality_score();
    assert_eq!(report.quality_score, 90.0);

    // Add an error
    report.errors.push(validate::ValidationError {
        field: "test".to_string(),
        message: "test error".to_string(),
        severity: validate::ErrorSeverity::Error,
    });
    report.calculate_quality_score();
    assert_eq!(report.quality_score, 85.0); // 90 - 5

    // Add a warning
    report.warnings.push(validate::ValidationWarning {
        field: "test".to_string(),
        message: "test warning".to_string(),
    });
    report.calculate_quality_score();
    assert_eq!(report.quality_score, 83.0); // 90 - 5 - 2
}

#[test]
fn test_temporal_extent() {
    let start = chrono::Utc::now();
    let end = start + chrono::Duration::days(30);

    let extent = TemporalExtent {
        start: Some(start),
        end: Some(end),
    };

    assert!(extent.start.is_some());
    assert!(extent.end.is_some());
}

#[test]
fn test_contact_info() {
    let contact = ContactInfo {
        individual_name: Some("John Doe".to_string()),
        organization_name: Some("Test Org".to_string()),
        position_name: Some("Data Manager".to_string()),
        email: Some("john@example.com".to_string()),
        phone: Some("+1-555-0100".to_string()),
        address: None,
        online_resource: Some("https://example.com".to_string()),
    };

    assert_eq!(
        contact
            .individual_name
            .expect("Contact individual_name should be present"),
        "John Doe"
    );
    assert_eq!(
        contact.email.expect("Contact email should be present"),
        "john@example.com"
    );
}

#[test]
fn test_metadata_transformer() {
    let transformer = transform::MetadataTransformer::new()
        .with_preserve_all(true)
        .with_strict_mode(false);

    let iso = Iso19115Metadata::builder()
        .title("Transformer Test")
        .abstract_text("Testing transformer")
        .build()
        .expect("Failed to build ISO19115 metadata for transformer test");

    let fgdc = transformer
        .transform_iso_to_fgdc(&iso)
        .expect("Failed to transform ISO19115 to FGDC");
    assert_eq!(fgdc.idinfo.citation.citeinfo.title, "Transformer Test");

    let iso_back = transformer
        .transform_fgdc_to_iso(&fgdc)
        .expect("Failed to transform FGDC back to ISO19115");
    assert_eq!(
        iso_back.identification_info[0].citation.title,
        "Transformer Test"
    );
}

#[test]
fn test_inspire_themes() {
    // Test that all INSPIRE themes are defined
    let theme = InspireTheme::Elevation;
    let _theme2 = InspireTheme::Hydrography;
    let _theme3 = InspireTheme::ProtectedSites;

    // Just ensure they compile and are accessible
    assert!(matches!(theme, InspireTheme::Elevation));
}

#[test]
fn test_datacite_contributor_types() {
    let contributor = Contributor {
        name: "Test Contributor".to_string(),
        contributor_type: ContributorType::DataCurator,
        name_type: Some(NameType::Personal),
        given_name: Some("Test".to_string()),
        family_name: Some("Contributor".to_string()),
        name_identifiers: Vec::new(),
        affiliations: vec!["Test University".to_string()],
    };

    assert_eq!(contributor.name, "Test Contributor");
    assert!(matches!(
        contributor.contributor_type,
        ContributorType::DataCurator
    ));
}

#[test]
fn test_dcat_agent() {
    let org = Agent::organization("Test Organization");
    assert_eq!(org.name, "Test Organization");
    assert!(matches!(org.agent_type, Some(AgentType::Organization)));

    let person = Agent::person("John Doe");
    assert_eq!(person.name, "John Doe");
    assert!(matches!(person.agent_type, Some(AgentType::Person)));
}

#[test]
fn test_reference_system_identifier() {
    use oxigdal_metadata::iso19115::reference_system::Identifier;

    let epsg = Identifier::epsg(4326);
    assert_eq!(epsg.code, "4326");
    assert_eq!(epsg.code_space, Some("EPSG".to_string()));

    let wkt = Identifier::wkt("GEOGCS[...]");
    assert_eq!(wkt.code_space, Some("WKT".to_string()));
}

#[test]
fn test_iso19115_default() {
    let metadata = Iso19115Metadata::default();
    assert_eq!(metadata.metadata_standard_name, "ISO 19115:2014");
    assert_eq!(metadata.language, Some("eng".to_string()));
}

#[test]
fn test_comprehensive_iso_metadata() {
    let bbox = BoundingBox::new(-180.0, 180.0, -90.0, 90.0);
    let contact = ResponsibleParty {
        individual_name: Some("Test Contact".to_string()),
        organization_name: Some("Test Org".to_string()),
        position_name: Some("Manager".to_string()),
        contact_info: Some(ContactInfo {
            individual_name: None,
            organization_name: None,
            position_name: None,
            email: Some("test@example.com".to_string()),
            phone: None,
            address: None,
            online_resource: None,
        }),
        role: Role::PointOfContact,
    };

    let metadata = Iso19115Metadata::builder()
        .title("Comprehensive Test")
        .abstract_text("A comprehensive metadata test")
        .keywords(vec!["comprehensive", "test", "metadata"])
        .bbox(bbox)
        .contact(contact)
        .file_identifier("test-id-123")
        .build()
        .expect("Failed to build ISO19115 metadata");

    let validation =
        validate::validate_iso19115(&metadata).expect("Failed to validate ISO19115 metadata");
    assert!(validation.is_valid);
    assert!(validation.quality_score > 70.0);
}
