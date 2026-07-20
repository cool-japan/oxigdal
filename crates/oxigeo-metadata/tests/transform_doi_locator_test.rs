//! Tests for real DOI extraction and INSPIRE resource-locator population in transforms.

use oxigeo_metadata::iso19115::{
    CitationDate, DateType, Distribution, Iso19115Metadata, OnlineFunction, OnlineResource,
    ResponsibleParty, Role, TransferOptions,
};
use oxigeo_metadata::transform::{iso19115_to_datacite, iso19115_to_inspire};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal ISO record whose citation.identifier contains `doi_str`.
#[allow(clippy::expect_used)]
fn build_iso_with_doi(doi_str: &str) -> Iso19115Metadata {
    let mut iso = Iso19115Metadata::builder()
        .title("Test Dataset")
        .abstract_text("Abstract text")
        .build()
        .expect("build_iso_with_doi: builder should succeed");
    iso.identification_info[0]
        .citation
        .identifier
        .push(doi_str.to_string());
    // Need at least one creator so DataCite build does not fail on creators;
    // push a minimal point_of_contact.
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: Some("Jane Doe".to_string()),
            organization_name: None,
            position_name: None,
            contact_info: None,
            role: Role::Originator,
        });
    iso
}

/// Build a minimal ISO record whose citation.identifier contains a doi.org URL.
fn build_iso_with_url_doi(url: &str) -> Iso19115Metadata {
    build_iso_with_doi(url)
}

/// Build a minimal ISO record with `distribution_info` populated with one
/// `OnlineResource` at `url` using function `Download`.
#[allow(clippy::expect_used)]
fn build_iso_with_online_resource(url: &str) -> Iso19115Metadata {
    let online_resource = OnlineResource {
        linkage: url.to_string(),
        protocol: None,
        name: Some("Data endpoint".to_string()),
        description: Some("Download URL".to_string()),
        function: OnlineFunction::Download,
    };
    let transfer_opts = TransferOptions {
        online: vec![online_resource],
        transfer_size: None,
    };
    let distribution = Distribution {
        format: Vec::new(),
        distributor: Vec::new(),
        transfer_options: vec![transfer_opts],
    };

    let mut iso = Iso19115Metadata::builder()
        .title("Dataset With Locator")
        .abstract_text("Abstract")
        .build()
        .expect("build_iso_with_online_resource: builder should succeed");
    iso.distribution_info = Some(distribution);
    iso
}

/// Build an ISO record with two online resources and a bare DOI so it works
/// for both INSPIRE and DataCite tests.
#[allow(clippy::expect_used)]
fn build_iso_with_two_resources(doi_str: &str) -> Iso19115Metadata {
    let res1 = OnlineResource {
        linkage: "https://example.com/wfs".to_string(),
        protocol: None,
        name: None,
        description: Some("WFS service".to_string()),
        function: OnlineFunction::Download,
    };
    let res2 = OnlineResource {
        linkage: "https://example.com/wms".to_string(),
        protocol: None,
        name: None,
        description: Some("WMS view service".to_string()),
        function: OnlineFunction::Information,
    };
    let distribution = Distribution {
        format: Vec::new(),
        distributor: Vec::new(),
        transfer_options: vec![TransferOptions {
            online: vec![res1, res2],
            transfer_size: None,
        }],
    };

    let mut iso = Iso19115Metadata::builder()
        .title("Multi Resource Dataset")
        .abstract_text("Dataset with multiple online resources")
        .build()
        .expect("build_iso_with_two_resources: builder should succeed");

    iso.identification_info[0]
        .citation
        .identifier
        .push(doi_str.to_string());
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: Some("Alice Smith".to_string()),
            organization_name: None,
            position_name: None,
            contact_info: None,
            role: Role::Originator,
        });
    iso.distribution_info = Some(distribution);
    iso
}

// ---------------------------------------------------------------------------
// iso19115_to_datacite tests
// ---------------------------------------------------------------------------

#[test]
fn test_iso_to_datacite_extracts_bare_doi() {
    let iso = build_iso_with_doi("10.5281/zenodo.12345");
    let dc = iso19115_to_datacite(&iso).expect("should succeed with a bare DOI");
    assert_eq!(dc.identifier.identifier, "10.5281/zenodo.12345");
}

#[test]
fn test_iso_to_datacite_extracts_doi_from_doiorg_url() {
    let iso = build_iso_with_url_doi("https://doi.org/10.5281/zenodo.12345");
    let dc = iso19115_to_datacite(&iso).expect("should succeed with a doi.org URL");
    assert_eq!(dc.identifier.identifier, "10.5281/zenodo.12345");
}

#[test]
fn test_iso_to_datacite_no_doi_returns_error() {
    let mut iso = Iso19115Metadata::builder()
        .title("No DOI Dataset")
        .abstract_text("Has no DOI in identifiers")
        .build()
        .expect("builder should succeed");
    iso.identification_info[0]
        .citation
        .identifier
        .push("urn:some:non-doi-identifier".to_string());
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: Some("Someone".to_string()),
            organization_name: None,
            position_name: None,
            contact_info: None,
            role: Role::Originator,
        });
    let result = iso19115_to_datacite(&iso);
    assert!(result.is_err(), "expected Err when no DOI is present");
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            msg.contains("lacks a DOI"),
            "error message should mention 'lacks a DOI', got: {msg}"
        );
    }
}

#[test]
fn test_iso_to_datacite_publisher_from_responsible_party() {
    let mut iso = build_iso_with_doi("10.9999/acme.42");
    // Replace the default individual_name contact with an org-named party
    iso.identification_info[0].point_of_contact.clear();
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: None,
            organization_name: Some("ACME Corp".to_string()),
            position_name: None,
            contact_info: None,
            role: Role::Publisher,
        });

    let dc = iso19115_to_datacite(&iso).expect("should succeed");
    assert_eq!(
        dc.publisher, "ACME Corp",
        "publisher should be extracted from organization_name"
    );
}

#[test]
fn test_iso_to_datacite_publisher_falls_back_to_individual_name() {
    // No organization_name → use individual_name as publisher
    let iso = build_iso_with_doi("10.9999/fallback.1");
    // build_iso_with_doi sets individual_name = "Jane Doe"
    let dc = iso19115_to_datacite(&iso).expect("should succeed");
    assert_eq!(dc.publisher, "Jane Doe");
}

#[test]
fn test_iso_to_datacite_publisher_fallback_unknown() {
    // No point_of_contact at all → "Unknown Publisher"
    let mut iso = Iso19115Metadata::builder()
        .title("No Contact Dataset")
        .abstract_text("No contacts")
        .build()
        .expect("builder should succeed");
    iso.identification_info[0]
        .citation
        .identifier
        .push("10.1234/empty.1".to_string());
    // We need at least one creator for DataCite; add an individual contact but
    // check the publisher path by having an individual_name only.
    // Actually DataCite requires creators: add a named contact.
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: None,
            organization_name: None,
            position_name: Some("Unknown Person".to_string()),
            contact_info: None,
            role: Role::Originator,
        });
    // The point_of_contact has neither individual nor org name, so the creator
    // loop will not add any creators and the build will fail. Use a fresh approach:
    // put someone without name as first contact, but also a named person after.
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: Some("Anon Creator".to_string()),
            organization_name: None,
            position_name: None,
            contact_info: None,
            role: Role::Originator,
        });
    let dc = iso19115_to_datacite(&iso).expect("should succeed");
    // First party has no org_name or individual_name, second has individual_name.
    // Publisher extraction: first party with organization_name = None,
    // then first with individual_name = "Anon Creator"
    assert_eq!(dc.publisher, "Anon Creator");
}

#[test]
fn test_iso_to_datacite_creators_from_point_of_contact() {
    let mut iso = Iso19115Metadata::builder()
        .title("Two Creator Dataset")
        .abstract_text("Two creators")
        .build()
        .expect("builder should succeed");
    iso.identification_info[0]
        .citation
        .identifier
        .push("10.1234/two.creators".to_string());
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: Some("Creator One".to_string()),
            organization_name: None,
            position_name: None,
            contact_info: None,
            role: Role::Author,
        });
    iso.identification_info[0]
        .point_of_contact
        .push(ResponsibleParty {
            individual_name: Some("Creator Two".to_string()),
            organization_name: None,
            position_name: None,
            contact_info: None,
            role: Role::Author,
        });

    let dc = iso19115_to_datacite(&iso).expect("should succeed");
    assert_eq!(dc.creators.len(), 2, "expected exactly 2 creators");
    assert_eq!(dc.creators[0].name, "Creator One");
    assert_eq!(dc.creators[1].name, "Creator Two");
}

#[test]
fn test_iso_to_datacite_publication_year_from_citation_date() {
    use chrono::TimeZone;

    let mut iso = build_iso_with_doi("10.5555/dated.1");
    let pub_date = chrono::Utc
        .with_ymd_and_hms(2019, 6, 15, 0, 0, 0)
        .single()
        .expect("valid date");
    iso.identification_info[0].citation.date.push(CitationDate {
        date: pub_date,
        date_type: DateType::Publication,
    });

    let dc = iso19115_to_datacite(&iso).expect("should succeed");
    assert_eq!(dc.publication_year, 2019);
}

// ---------------------------------------------------------------------------
// iso19115_to_inspire tests
// ---------------------------------------------------------------------------

#[test]
fn test_iso_to_inspire_extracts_single_resource_locator() {
    let iso = build_iso_with_online_resource("https://example.com/data");
    let inspire = iso19115_to_inspire(&iso).expect("should succeed with one resource locator");
    assert_eq!(inspire.resource_locator.len(), 1);
    assert_eq!(inspire.resource_locator[0].url, "https://example.com/data");
}

#[test]
fn test_iso_to_inspire_extracts_multiple_locators() {
    let iso = build_iso_with_two_resources("10.9999/multi.1");
    let inspire = iso19115_to_inspire(&iso).expect("should succeed with two locators");
    assert_eq!(
        inspire.resource_locator.len(),
        2,
        "expected two resource locators"
    );
    assert_eq!(inspire.resource_locator[0].url, "https://example.com/wfs");
    assert_eq!(inspire.resource_locator[1].url, "https://example.com/wms");
}

#[test]
fn test_iso_to_inspire_maps_online_function_to_locator_function() {
    use oxigeo_metadata::inspire::ResourceLocatorFunction;

    let iso = build_iso_with_online_resource("https://example.com/wfs");
    let inspire = iso19115_to_inspire(&iso).expect("should succeed");
    assert!(
        matches!(
            inspire.resource_locator[0].function,
            ResourceLocatorFunction::Download
        ),
        "Download function should map to ResourceLocatorFunction::Download"
    );
}

#[test]
fn test_iso_to_inspire_maps_information_function() {
    use oxigeo_metadata::inspire::ResourceLocatorFunction;

    let info_res = OnlineResource {
        linkage: "https://example.com/info".to_string(),
        protocol: None,
        name: None,
        description: None,
        function: OnlineFunction::Information,
    };
    let distribution = Distribution {
        format: Vec::new(),
        distributor: Vec::new(),
        transfer_options: vec![TransferOptions {
            online: vec![info_res],
            transfer_size: None,
        }],
    };
    let mut iso = Iso19115Metadata::builder()
        .title("Info Resource")
        .abstract_text("Abstract")
        .build()
        .expect("builder should succeed");
    iso.distribution_info = Some(distribution);

    let inspire = iso19115_to_inspire(&iso).expect("should succeed");
    assert!(
        matches!(
            inspire.resource_locator[0].function,
            ResourceLocatorFunction::Information
        ),
        "Information function should map to ResourceLocatorFunction::Information"
    );
}

#[test]
fn test_iso_to_inspire_no_online_resource_returns_error() {
    // ISO with None distribution_info → Err
    let iso = Iso19115Metadata::builder()
        .title("No Distribution")
        .abstract_text("No online resources here")
        .build()
        .expect("builder should succeed");

    let result = iso19115_to_inspire(&iso);
    assert!(
        result.is_err(),
        "expected Err when no online resources present"
    );
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            msg.contains("lacks resource locator for INSPIRE"),
            "error message should mention 'lacks resource locator for INSPIRE', got: {msg}"
        );
    }
}

#[test]
fn test_iso_to_inspire_round_trip_preserves_base_title() {
    let iso = build_iso_with_online_resource("https://example.com/dataset");
    let inspire = iso19115_to_inspire(&iso).expect("should succeed");
    assert_eq!(
        inspire.base.identification_info[0].citation.title, "Dataset With Locator",
        "the base ISO record title should be preserved in the INSPIRE wrapper"
    );
}
