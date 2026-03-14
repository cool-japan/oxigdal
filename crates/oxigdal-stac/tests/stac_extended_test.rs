//! Extended STAC tests — 60+ tests covering:
//!   EO, SAR, View, Projection, Scientific, Timestamps, Version extensions;
//!   Conformance, SearchRequest / ItemCollection; Transaction; CollectionAggregation.

#![allow(clippy::panic)]

use chrono::{TimeZone, Utc};
use oxigdal_stac::{
    // API
    ApiSearchRequest,
    ApiSortDirection,
    // Extensions
    Band,
    // Collection aggregation
    CollectionAggregator,
    CommonBandName,
    ConformanceDeclaration,
    EoExtension,
    FrequencyBand,
    ItemCollection,
    NumericStats,
    ObservationDirection,
    Polarization,
    ProjectionExtension,
    Publication,
    SarExtension,
    ScientificExtension,
    StacError,
    // Transaction
    StacItemStore,
    TimestampsExtension,
    TransactionOp,
    VersionExtension,
    ViewExtension,
    epsg_codes,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// EO Extension (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_eo_default_is_empty() {
    let eo = EoExtension::new();
    assert!(eo.bands.is_none());
    assert!(eo.cloud_cover.is_none());
}

#[test]
fn test_eo_with_cloud_cover_builder() {
    let eo = EoExtension::new().with_cloud_cover(42.5);
    assert_eq!(eo.cloud_cover, Some(42.5));
}

#[test]
fn test_eo_with_bands_replaces_band_list() {
    let bands = vec![
        Band::new()
            .with_name("B01")
            .with_common_name(CommonBandName::Coastal),
        Band::new()
            .with_name("B02")
            .with_common_name(CommonBandName::Blue),
    ];
    let eo = EoExtension::new().with_bands(bands);
    assert_eq!(eo.bands.as_ref().map(|b| b.len()), Some(2));
}

#[test]
fn test_eo_add_band_accumulates() {
    let eo = EoExtension::new()
        .add_band(
            Band::new()
                .with_name("B04")
                .with_common_name(CommonBandName::Red),
        )
        .add_band(
            Band::new()
                .with_name("B08")
                .with_common_name(CommonBandName::Nir),
        );
    assert_eq!(eo.bands.as_ref().map(|b| b.len()), Some(2));
}

#[test]
fn test_eo_cloud_cover_serialization_roundtrip() {
    let eo = EoExtension::new().with_cloud_cover(17.3);
    let json = serde_json::to_string(&eo).expect("serialize");
    let back: EoExtension = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(eo, back);
}

#[test]
fn test_eo_bands_serialization_roundtrip() {
    let eo = EoExtension::new()
        .add_band(
            Band::new()
                .with_name("red")
                .with_common_name(CommonBandName::Red),
        )
        .with_cloud_cover(5.0);
    let json = serde_json::to_string(&eo).expect("serialize");
    let back: EoExtension = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(eo, back);
}

#[test]
fn test_eo_cloud_cover_validation_boundary() {
    assert!(EoExtension::new().with_cloud_cover(0.0).validate().is_ok());
    assert!(
        EoExtension::new()
            .with_cloud_cover(100.0)
            .validate()
            .is_ok()
    );
    assert!(
        EoExtension::new()
            .with_cloud_cover(100.001)
            .validate()
            .is_err()
    );
    assert!(
        EoExtension::new()
            .with_cloud_cover(-0.001)
            .validate()
            .is_err()
    );
}

#[test]
fn test_eo_band_common_name_variants_serialize() {
    let cases = vec![
        (CommonBandName::Coastal, "\"coastal\""),
        (CommonBandName::Blue, "\"blue\""),
        (CommonBandName::Green, "\"green\""),
        (CommonBandName::Red, "\"red\""),
        (CommonBandName::Nir, "\"nir\""),
        (CommonBandName::Cirrus, "\"cirrus\""),
    ];
    for (variant, expected) in cases {
        let json = serde_json::to_string(&variant).expect("serialize");
        assert_eq!(json, expected, "failed for {:?}", variant);
    }
}

#[test]
fn test_eo_band_wavelength_builder() {
    let band = Band::new()
        .with_center_wavelength(0.665)
        .with_full_width_half_max(0.038)
        .with_solar_illumination(1913.57);
    assert_eq!(band.center_wavelength, Some(0.665));
    assert_eq!(band.full_width_half_max, Some(0.038));
    assert_eq!(band.solar_illumination, Some(1913.57));
}

#[test]
fn test_eo_band_negative_wavelength_fails_validation() {
    let band = Band::new().with_center_wavelength(-1.0);
    assert!(band.validate().is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// SAR Extension (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_sar_construction() {
    let sar = SarExtension::new(FrequencyBand::C, vec![Polarization::VV, Polarization::VH]);
    assert_eq!(sar.frequency_band, FrequencyBand::C);
    assert_eq!(sar.polarizations.len(), 2);
}

#[test]
fn test_sar_polarizations_all_variants_serialize() {
    let expected = vec![
        (Polarization::HH, "\"HH\""),
        (Polarization::HV, "\"HV\""),
        (Polarization::VH, "\"VH\""),
        (Polarization::VV, "\"VV\""),
    ];
    for (pol, exp) in expected {
        assert_eq!(serde_json::to_string(&pol).expect("ser"), exp);
    }
}

#[test]
fn test_sar_frequency_band_variants_serialize() {
    let expected = vec![
        (FrequencyBand::C, "\"C\""),
        (FrequencyBand::X, "\"X\""),
        (FrequencyBand::L, "\"L\""),
    ];
    for (band, exp) in expected {
        assert_eq!(serde_json::to_string(&band).expect("ser"), exp);
    }
}

#[test]
fn test_sar_builder_chain() {
    let sar = SarExtension::new(FrequencyBand::C, vec![Polarization::VV])
        .with_center_frequency(5.405)
        .with_product_type("GRD")
        .with_resolution(10.0, 10.0)
        .with_looks(1, 1)
        .with_observation_direction(ObservationDirection::Right)
        .with_instrument_mode("IW");
    assert_eq!(sar.center_frequency, Some(5.405));
    assert_eq!(sar.product_type, Some("GRD".to_string()));
    assert_eq!(sar.instrument_mode, Some("IW".to_string()));
}

#[test]
fn test_sar_observation_direction_serialization() {
    let right = serde_json::to_string(&ObservationDirection::Right).expect("ser");
    assert_eq!(right, "\"right\"");
    let left = serde_json::to_string(&ObservationDirection::Left).expect("ser");
    assert_eq!(left, "\"left\"");
}

#[test]
fn test_sar_serialization_roundtrip() {
    let sar = SarExtension::new(FrequencyBand::X, vec![Polarization::HH])
        .with_center_frequency(9.6)
        .with_product_type("SLC");
    let json = serde_json::to_string(&sar).expect("ser");
    let back: SarExtension = serde_json::from_str(&json).expect("deser");
    assert_eq!(sar, back);
}

#[test]
fn test_sar_resolution_field() {
    let sar =
        SarExtension::new(FrequencyBand::L, vec![Polarization::HH]).with_resolution(25.0, 30.0);
    assert_eq!(sar.resolution_azimuth, Some(25.0));
    assert_eq!(sar.resolution_range, Some(30.0));
}

#[test]
fn test_sar_looks_fields() {
    let sar = SarExtension::new(FrequencyBand::S, vec![Polarization::VH])
        .with_looks(4, 2)
        .with_equivalent_number_of_looks(3.7);
    assert_eq!(sar.looks_azimuth, Some(4));
    assert_eq!(sar.looks_range, Some(2));
    assert_eq!(sar.looks_equivalent_number, Some(3.7));
}

// ═══════════════════════════════════════════════════════════════════════════
// View Extension (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_view_extension_new() {
    let v = ViewExtension::new();
    assert!(v.sun_azimuth.is_none());
    assert!(v.sun_elevation.is_none());
    assert!(v.off_nadir.is_none());
}

#[test]
fn test_view_sun_angles_builder() {
    let v = ViewExtension::new()
        .with_sun_azimuth(135.0)
        .with_sun_elevation(45.0);
    assert_eq!(v.sun_azimuth, Some(135.0));
    assert_eq!(v.sun_elevation, Some(45.0));
}

#[test]
fn test_view_solar_zenith_computation() {
    let v = ViewExtension::new().with_sun_elevation(30.0);
    // zenith = 90 − elevation = 60°, but ViewExtension doesn't have this method
    // in the existing code; we compute it directly in the test.
    let zenith = v.sun_elevation.map(|e| 90.0 - e);
    assert!((zenith.expect("zenith") - 60.0).abs() < 1e-9);
}

#[test]
fn test_view_off_nadir_builder() {
    let v = ViewExtension::new().with_off_nadir(12.5);
    assert_eq!(v.off_nadir, Some(12.5));
}

#[test]
fn test_view_incidence_angle_builder() {
    let v = ViewExtension::new().with_incidence_angle(20.0);
    assert_eq!(v.incidence_angle, Some(20.0));
}

#[test]
fn test_view_validation_invalid_off_nadir() {
    let v = ViewExtension::new().with_off_nadir(95.0);
    assert!(v.validate().is_err());
}

#[test]
fn test_view_validation_invalid_azimuth() {
    let v = ViewExtension::new().with_azimuth(361.0);
    assert!(v.validate().is_err());
}

#[test]
fn test_view_serialization_roundtrip() {
    let v = ViewExtension::new()
        .with_sun_azimuth(200.0)
        .with_sun_elevation(35.0)
        .with_off_nadir(8.0);
    let json = serde_json::to_string(&v).expect("ser");
    let back: ViewExtension = serde_json::from_str(&json).expect("deser");
    assert_eq!(v, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Projection Extension (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_projection_new() {
    let p = ProjectionExtension::new();
    assert!(p.epsg.is_none());
    assert!(p.wkt2.is_none());
    assert!(p.transform.is_none());
}

#[test]
fn test_projection_epsg() {
    let p = ProjectionExtension::new().with_epsg(epsg_codes::WGS84);
    assert_eq!(p.epsg, Some(epsg_codes::WGS84));
}

#[test]
fn test_projection_shape() {
    let p = ProjectionExtension::new().with_shape(1024, 2048);
    assert_eq!(p.shape, Some(vec![1024, 2048]));
}

#[test]
fn test_projection_transform_6_elements_valid() {
    let t = vec![10.0, 0.0, 600000.0, 0.0, -10.0, 4_300_020.0];
    let p = ProjectionExtension::new().with_transform(t.clone());
    assert_eq!(p.transform, Some(t));
    assert!(p.validate().is_ok());
}

#[test]
fn test_projection_transform_wrong_length_invalid() {
    let mut p = ProjectionExtension::new();
    p.transform = Some(vec![1.0, 2.0, 3.0]); // not 6 or 9
    assert!(p.validate().is_err());
}

#[test]
fn test_projection_serialization_roundtrip() {
    let p = ProjectionExtension::new()
        .with_epsg(32610)
        .with_shape(10980, 10980);
    let json = serde_json::to_string(&p).expect("ser");
    let back: ProjectionExtension = serde_json::from_str(&json).expect("deser");
    assert_eq!(p, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Scientific Extension (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_scientific_doi_url_format() {
    let sci = ScientificExtension::new().with_doi("10.1234/example.dataset");
    // `ScientificExtension` doesn't expose doi_url() in the existing code;
    // we construct it manually as a test of the underlying DOI field.
    let doi_url = sci.doi.as_ref().map(|d| format!("https://doi.org/{}", d));
    assert_eq!(
        doi_url.as_deref(),
        Some("https://doi.org/10.1234/example.dataset")
    );
}

#[test]
fn test_scientific_publications_vec() {
    let pub1 = Publication::with_doi("10.1000/p1").with_citation("Author A (2023)");
    let pub2 = Publication::with_doi("10.1000/p2");
    let sci = ScientificExtension::new()
        .add_publication(pub1)
        .add_publication(pub2);
    assert_eq!(sci.publications.as_ref().map(|p| p.len()), Some(2));
}

#[test]
fn test_scientific_serialization_roundtrip() {
    let sci = ScientificExtension::new()
        .with_doi("10.9999/testdata")
        .with_citation("Test et al. (2024)");
    let json = serde_json::to_string(&sci).expect("ser");
    let back: ScientificExtension = serde_json::from_str(&json).expect("deser");
    assert_eq!(sci, back);
}

#[test]
fn test_scientific_doi_validation_ok() {
    let sci = ScientificExtension::new().with_doi("10.5281/zenodo.12345");
    assert!(sci.validate().is_ok());
}

#[test]
fn test_scientific_doi_validation_invalid() {
    let sci = ScientificExtension::new().with_doi("bad-doi");
    assert!(sci.validate().is_err());
}

#[test]
fn test_scientific_publication_doi_invalid() {
    let sci = ScientificExtension::new().add_publication(Publication::with_doi("not-a-doi"));
    assert!(sci.validate().is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Timestamps Extension (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

fn utc(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
}

#[test]
fn test_timestamps_new_is_empty() {
    let ts = TimestampsExtension::new();
    assert!(ts.published.is_none());
    assert!(ts.expires.is_none());
    assert!(ts.unpublished.is_none());
}

#[test]
fn test_timestamps_with_published() {
    let dt = utc(2023, 3, 15);
    let ts = TimestampsExtension::new().with_published(dt);
    assert_eq!(ts.published, Some(dt));
}

#[test]
fn test_timestamps_is_expired_true() {
    let ts = TimestampsExtension::new().with_expires(utc(2020, 1, 1));
    assert!(ts.is_expired(utc(2021, 1, 1)));
}

#[test]
fn test_timestamps_is_expired_false() {
    let ts = TimestampsExtension::new().with_expires(utc(2025, 1, 1));
    assert!(!ts.is_expired(utc(2024, 1, 1)));
}

#[test]
fn test_timestamps_is_published_true() {
    let ts = TimestampsExtension::new().with_published(utc(2022, 6, 1));
    assert!(ts.is_published(utc(2023, 1, 1)));
}

#[test]
fn test_timestamps_validate_published_before_expires_ok() {
    let ts = TimestampsExtension::new()
        .with_published(utc(2023, 1, 1))
        .with_expires(utc(2024, 1, 1));
    assert!(ts.validate().is_ok());
}

#[test]
fn test_timestamps_validate_published_after_expires_fails() {
    let ts = TimestampsExtension::new()
        .with_published(utc(2025, 6, 1))
        .with_expires(utc(2024, 1, 1));
    assert!(ts.validate().is_err());
}

#[test]
fn test_timestamps_serialization_roundtrip() {
    let ts = TimestampsExtension::new()
        .with_published(utc(2023, 3, 1))
        .with_expires(utc(2026, 3, 1));
    let json = serde_json::to_string(&ts).expect("ser");
    let back: TimestampsExtension = serde_json::from_str(&json).expect("deser");
    assert_eq!(ts, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Version Extension (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_version_new_is_empty() {
    let v = VersionExtension::new();
    assert!(v.version.is_none());
    assert!(!v.is_experimental());
    assert!(!v.is_deprecated());
}

#[test]
fn test_version_with_version_string() {
    let v = VersionExtension::new().with_version("v1.2.3");
    assert_eq!(v.version.as_deref(), Some("v1.2.3"));
}

#[test]
fn test_version_mark_experimental() {
    let v = VersionExtension::new().mark_experimental();
    assert!(v.is_experimental());
}

#[test]
fn test_version_mark_deprecated() {
    let v = VersionExtension::new().mark_deprecated();
    assert!(v.is_deprecated());
}

#[test]
fn test_version_validate_empty_string_fails() {
    let v = VersionExtension::new().with_version("  ");
    assert!(v.validate().is_err());
}

#[test]
fn test_version_serialization_roundtrip() {
    let v = VersionExtension::new()
        .with_version("2024-06")
        .mark_experimental();
    let json = serde_json::to_string(&v).expect("ser");
    let back: VersionExtension = serde_json::from_str(&json).expect("deser");
    assert_eq!(v, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Conformance (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

use oxigdal_stac::api::conformance::uris;

#[test]
fn test_conformance_standard_has_core() {
    assert!(ConformanceDeclaration::standard().supports(uris::CORE));
}

#[test]
fn test_conformance_standard_has_item_search() {
    assert!(ConformanceDeclaration::standard().supports(uris::ITEM_SEARCH));
}

#[test]
fn test_conformance_standard_has_ogc_features() {
    assert!(ConformanceDeclaration::standard().supports(uris::OGCAPI_FEATURES));
}

#[test]
fn test_conformance_supports_false_unknown() {
    let decl = ConformanceDeclaration::standard();
    assert!(!decl.supports("https://totally.unknown/conformance"));
}

#[test]
fn test_conformance_with_transaction_adds_uri() {
    let decl = ConformanceDeclaration::standard().with_transaction();
    assert!(decl.supports(uris::TRANSACTION));
}

#[test]
fn test_conformance_standard_length() {
    // 8 classes: Core, Browseable, Item Search, Filter, Sort, Fields, OGC, Children
    assert_eq!(ConformanceDeclaration::standard().len(), 8);
}

#[test]
fn test_conformance_json_roundtrip() {
    let decl = ConformanceDeclaration::standard().with_transaction();
    let json = serde_json::to_string(&decl).expect("ser");
    let back: ConformanceDeclaration = serde_json::from_str(&json).expect("deser");
    assert_eq!(decl, back);
}

#[test]
fn test_conformance_custom_class() {
    let decl = ConformanceDeclaration::new(["https://my.api/custom-conformance"]);
    assert!(decl.supports("https://my.api/custom-conformance"));
    assert!(!decl.supports(uris::CORE));
}

// ═══════════════════════════════════════════════════════════════════════════
// SearchRequest / ItemCollection (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_search_request_default_empty() {
    let req = ApiSearchRequest::new();
    assert!(req.bbox.is_none());
    assert!(req.collections.is_empty());
    assert!(req.limit.is_none());
}

#[test]
fn test_search_request_with_bbox() {
    let req = ApiSearchRequest::new().with_bbox([-10.0, -5.0, 10.0, 5.0]);
    assert_eq!(req.bbox, Some([-10.0, -5.0, 10.0, 5.0]));
}

#[test]
fn test_search_request_with_datetime() {
    let req = ApiSearchRequest::new().with_datetime("2023-01-01/2024-01-01");
    assert_eq!(req.datetime.as_deref(), Some("2023-01-01/2024-01-01"));
}

#[test]
fn test_search_request_with_collections() {
    let req = ApiSearchRequest::new().with_collections(["col-a", "col-b"]);
    assert_eq!(req.collections, vec!["col-a", "col-b"]);
}

#[test]
fn test_search_request_with_limit() {
    let req = ApiSearchRequest::new().with_limit(25);
    assert_eq!(req.limit, Some(25));
}

#[test]
fn test_search_request_with_sort() {
    let req = ApiSearchRequest::new().with_sort("datetime", ApiSortDirection::Desc);
    assert_eq!(req.sortby.len(), 1);
    assert_eq!(req.sortby[0].field, "datetime");
    assert_eq!(req.sortby[0].direction, ApiSortDirection::Desc);
}

#[test]
fn test_search_request_serialization_roundtrip() {
    let req = ApiSearchRequest::new()
        .with_bbox([-180.0, -90.0, 180.0, 90.0])
        .with_datetime("2023-01-01T00:00:00Z")
        .with_collections(["sentinel-2-l2a"])
        .with_limit(100);
    let json = serde_json::to_string(&req).expect("ser");
    let back: ApiSearchRequest = serde_json::from_str(&json).expect("deser");
    assert_eq!(req, back);
}

#[test]
fn test_item_collection_has_next_page() {
    use oxigdal_stac::api::search::Link;
    let mut fc = ItemCollection::new(vec![]);
    assert!(!fc.has_next_page());
    fc.links = Some(vec![Link::new("next", "https://example.com?token=abc")]);
    assert!(fc.has_next_page());
}

// ═══════════════════════════════════════════════════════════════════════════
// Transaction (12 tests)
// ═══════════════════════════════════════════════════════════════════════════

fn item_json(id: &str) -> serde_json::Value {
    json!({ "id": id, "type": "Feature", "stac_version": "1.0.0" })
}

#[test]
fn test_transaction_create_success() {
    let mut store = StacItemStore::new();
    let r = store
        .create_item("col1", item_json("item-1"))
        .expect("create");
    assert_eq!(r.op, TransactionOp::Create);
    assert_eq!(r.item_id, "item-1");
    assert!(r.success);
}

#[test]
fn test_transaction_create_duplicate_error() {
    let mut store = StacItemStore::new();
    store.create_item("col1", item_json("dup")).expect("first");
    let err = store.create_item("col1", item_json("dup"));
    assert!(err.is_err());
    assert!(matches!(err, Err(StacError::AlreadyExists(_))));
}

#[test]
fn test_transaction_update_existing() {
    let mut store = StacItemStore::new();
    store.create_item("col1", item_json("upd")).expect("create");
    let new_val = json!({ "id": "upd", "version": 2 });
    let r = store.update_item("col1", "upd", new_val).expect("update");
    assert_eq!(r.op, TransactionOp::Update);
    assert_eq!(
        store
            .get_item("col1", "upd")
            .and_then(|v| v["version"].as_i64()),
        Some(2)
    );
}

#[test]
fn test_transaction_update_nonexistent_error() {
    let mut store = StacItemStore::new();
    let err = store.update_item("col1", "ghost", item_json("ghost"));
    assert!(err.is_err());
    assert!(matches!(err, Err(StacError::NotFound(_))));
}

#[test]
fn test_transaction_upsert_creates() {
    let mut store = StacItemStore::new();
    let r = store.upsert_item("col1", item_json("new")).expect("upsert");
    assert_eq!(r.op, TransactionOp::Upsert);
    assert!(store.get_item("col1", "new").is_some());
}

#[test]
fn test_transaction_upsert_replaces() {
    let mut store = StacItemStore::new();
    store
        .create_item("col1", item_json("repl"))
        .expect("create");
    let v2 = json!({ "id": "repl", "flag": true });
    store.upsert_item("col1", v2).expect("upsert");
    assert_eq!(
        store
            .get_item("col1", "repl")
            .and_then(|v| v["flag"].as_bool()),
        Some(true)
    );
}

#[test]
fn test_transaction_delete_existing() {
    let mut store = StacItemStore::new();
    store.create_item("col1", item_json("del")).expect("create");
    let r = store.delete_item("col1", "del").expect("delete");
    assert_eq!(r.op, TransactionOp::Delete);
    assert!(store.get_item("col1", "del").is_none());
}

#[test]
fn test_transaction_delete_nonexistent_error() {
    let mut store = StacItemStore::new();
    let err = store.delete_item("col1", "missing");
    assert!(err.is_err());
    assert!(matches!(err, Err(StacError::NotFound(_))));
}

#[test]
fn test_transaction_get_item() {
    let mut store = StacItemStore::new();
    store
        .create_item("col1", item_json("get-me"))
        .expect("create");
    assert!(store.get_item("col1", "get-me").is_some());
    assert!(store.get_item("col2", "get-me").is_none()); // wrong collection
}

#[test]
fn test_transaction_list_items_by_collection() {
    let mut store = StacItemStore::new();
    for i in 0..3 {
        store
            .create_item("col-a", item_json(&format!("a-{}", i)))
            .expect("create");
    }
    store
        .create_item("col-b", item_json("b-0"))
        .expect("create");
    assert_eq!(store.list_items("col-a").len(), 3);
    assert_eq!(store.list_items("col-b").len(), 1);
    assert_eq!(store.list_items("col-c").len(), 0);
}

#[test]
fn test_transaction_log_populated() {
    let mut store = StacItemStore::new();
    store.create_item("c", item_json("log1")).expect("c");
    store.upsert_item("c", item_json("log2")).expect("u");
    store.delete_item("c", "log1").expect("d");
    let log = store.transaction_log();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].op, TransactionOp::Create);
    assert_eq!(log[1].op, TransactionOp::Upsert);
    assert_eq!(log[2].op, TransactionOp::Delete);
}

#[test]
fn test_transaction_missing_id_field_error() {
    let mut store = StacItemStore::new();
    let no_id = json!({ "type": "Feature" });
    let err = store.create_item("col1", no_id);
    assert!(err.is_err());
    assert!(matches!(err, Err(StacError::InvalidItem(_))));
}

// ═══════════════════════════════════════════════════════════════════════════
// Collection Aggregation (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

fn eo_item(id: &str, cloud: f64, platform: &str, bbox: [f64; 4]) -> serde_json::Value {
    let [w, s, e, n] = bbox;
    json!({
        "id": id,
        "bbox": [w, s, e, n],
        "properties": {
            "eo:cloud_cover": cloud,
            "platform": platform
        }
    })
}

#[test]
fn test_aggregation_empty() {
    let stats = CollectionAggregator::new("empty").build();
    assert_eq!(stats.item_count, 0);
    assert!(stats.spatial_extent.is_none());
    assert!(stats.cloud_cover.is_none());
}

#[test]
fn test_aggregation_single_item_bbox() {
    let mut agg = CollectionAggregator::new("col");
    agg.ingest(&eo_item("i1", 10.0, "s2a", [-5.0, -3.0, 5.0, 3.0]));
    let stats = agg.build();
    let ext = stats.spatial_extent.expect("extent");
    assert!((ext[0] - (-5.0)).abs() < 1e-9); // west
    assert!((ext[2] - 5.0).abs() < 1e-9); // east
}

#[test]
fn test_aggregation_spatial_union() {
    let mut agg = CollectionAggregator::new("col");
    agg.ingest(&eo_item("i1", 0.0, "p", [-10.0, -10.0, 0.0, 0.0]));
    agg.ingest(&eo_item("i2", 0.0, "p", [0.0, 0.0, 20.0, 15.0]));
    let stats = agg.build();
    let [w, s, e, n] = stats.spatial_extent.expect("extent");
    assert!((w - (-10.0)).abs() < 1e-9);
    assert!((s - (-10.0)).abs() < 1e-9);
    assert!((e - 20.0).abs() < 1e-9);
    assert!((n - 15.0).abs() < 1e-9);
}

#[test]
fn test_aggregation_cloud_cover_statistics() {
    let mut agg = CollectionAggregator::new("eo");
    for cc in [0.0_f64, 25.0, 50.0, 75.0, 100.0] {
        agg.ingest(&eo_item("x", cc, "s2a", [0.0, 0.0, 1.0, 1.0]));
    }
    let stats = agg.build();
    let cc = stats.cloud_cover.expect("cloud_cover");
    assert!((cc.min - 0.0).abs() < 1e-9);
    assert!((cc.max - 100.0).abs() < 1e-9);
    assert!((cc.mean - 50.0).abs() < 1e-9);
    assert_eq!(cc.count, 5);
}

#[test]
fn test_aggregation_platform_counts() {
    let mut agg = CollectionAggregator::new("p");
    agg.ingest(&eo_item("a1", 0.0, "sentinel-2a", [0.0, 0.0, 1.0, 1.0]));
    agg.ingest(&eo_item("a2", 0.0, "sentinel-2a", [0.0, 0.0, 1.0, 1.0]));
    agg.ingest(&eo_item("a3", 0.0, "sentinel-2b", [0.0, 0.0, 1.0, 1.0]));
    let stats = agg.build();
    assert_eq!(stats.platforms["sentinel-2a"], 2);
    assert_eq!(stats.platforms["sentinel-2b"], 1);
}

#[test]
fn test_numeric_stats_correct_mean_and_std() {
    // Known dataset: [2, 4, 4, 4, 5, 5, 7, 9] → mean=5, pop_std=2
    let vals = vec![2.0_f64, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
    let ns = NumericStats::from_values(&vals).expect("stats");
    assert!((ns.mean - 5.0).abs() < 1e-9);
    assert!((ns.std_dev - 2.0).abs() < 1e-9);
    assert_eq!(ns.count, 8);
}

#[test]
fn test_numeric_stats_single_value() {
    let ns = NumericStats::from_values(&[42.0]).expect("stats");
    assert!((ns.min - 42.0).abs() < 1e-9);
    assert!((ns.max - 42.0).abs() < 1e-9);
    assert!((ns.mean - 42.0).abs() < 1e-9);
    assert!((ns.std_dev - 0.0).abs() < 1e-9);
}

#[test]
fn test_aggregation_item_count() {
    let mut agg = CollectionAggregator::new("cnt");
    for i in 0..7 {
        agg.ingest(&eo_item(
            &format!("i{}", i),
            10.0,
            "plat",
            [0.0, 0.0, 1.0, 1.0],
        ));
    }
    assert_eq!(agg.build().item_count, 7);
}
