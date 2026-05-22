//! Integration tests for STAC 1.1.0 compatibility.
//!
//! Verifies that:
//! - Both "1.0.0" and "1.1.0" are accepted by validators
//! - Unknown version strings are still rejected
//! - The `assets` field on Collection round-trips through serde
//! - The `bands` shorthand on Asset round-trips through serde

use oxigdal_stac::{Asset, Band, Collection, Item, StacVersion};
use serde_json::json;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// StacVersion unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_stac_version_parse_accepts_1_0_0() {
    let v = StacVersion::parse("1.0.0").expect("1.0.0 must be accepted");
    assert_eq!(v, StacVersion::V1_0_0);
}

#[test]
fn test_stac_version_parse_accepts_1_1_0() {
    let v = StacVersion::parse("1.1.0").expect("1.1.0 must be accepted");
    assert_eq!(v, StacVersion::V1_1_0);
}

#[test]
fn test_stac_version_parse_rejects_2_0_0() {
    assert!(
        StacVersion::parse("2.0.0").is_err(),
        "2.0.0 must be rejected"
    );
}

#[test]
fn test_stac_version_as_str_roundtrip() {
    assert_eq!(StacVersion::V1_0_0.as_str(), "1.0.0");
    assert_eq!(StacVersion::V1_1_0.as_str(), "1.1.0");
    // Also validate the Display impl
    assert_eq!(StacVersion::V1_0_0.to_string(), "1.0.0");
    assert_eq!(StacVersion::V1_1_0.to_string(), "1.1.0");
}

#[test]
fn test_stac_version_default_is_v1_0_0() {
    assert_eq!(StacVersion::default(), StacVersion::V1_0_0);
}

// ---------------------------------------------------------------------------
// Collection validation with STAC 1.1.0
// ---------------------------------------------------------------------------

/// Minimal valid Collection JSON with stac_version 1.1.0.
fn minimal_collection_json(stac_version: &str) -> String {
    json!({
        "type": "Collection",
        "stac_version": stac_version,
        "id": "test-collection",
        "description": "A test collection",
        "license": "MIT",
        "extent": {
            "spatial": { "bbox": [[-180.0, -90.0, 180.0, 90.0]] },
            "temporal": { "interval": [[null, null]] }
        },
        "links": []
    })
    .to_string()
}

#[test]
fn test_collection_with_stac_version_1_1_0_validates() {
    let json_str = minimal_collection_json("1.1.0");
    let collection: Collection =
        serde_json::from_str(&json_str).expect("deserialization must succeed for 1.1.0");
    collection
        .validate()
        .expect("Collection with stac_version 1.1.0 must pass validation");
}

#[test]
fn test_collection_with_stac_version_1_0_0_still_validates() {
    let json_str = minimal_collection_json("1.0.0");
    let collection: Collection =
        serde_json::from_str(&json_str).expect("deserialization must succeed for 1.0.0");
    collection
        .validate()
        .expect("Collection with stac_version 1.0.0 must still pass validation");
}

#[test]
fn test_collection_with_unknown_version_fails_validation() {
    let json_str = minimal_collection_json("2.0.0");
    let collection: Collection =
        serde_json::from_str(&json_str).expect("deserialization must succeed");
    assert!(
        collection.validate().is_err(),
        "Collection with stac_version 2.0.0 must fail validation"
    );
}

// ---------------------------------------------------------------------------
// Collection `assets` field (STAC 1.1.0)
// ---------------------------------------------------------------------------

#[test]
fn test_collection_assets_field_serializes_when_set() {
    let mut col = Collection::new("col-with-assets", "desc", "proprietary");
    col.stac_version = "1.1.0".to_string();

    let mut assets = HashMap::new();
    assets.insert(
        "thumbnail".to_string(),
        Asset::new("https://example.com/thumb.png"),
    );
    col.assets = Some(assets);

    let json_str = serde_json::to_string(&col).expect("serialization must succeed");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("re-parse must succeed");

    assert!(
        parsed["assets"].is_object(),
        "assets must appear in serialized JSON when Some"
    );
    assert!(
        parsed["assets"]["thumbnail"].is_object(),
        "thumbnail asset must be present"
    );
}

#[test]
fn test_collection_assets_field_omitted_when_none() {
    let col = Collection::new("col-no-assets", "desc", "proprietary");
    let json_str = serde_json::to_string(&col).expect("serialization must succeed");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("re-parse must succeed");

    assert!(
        parsed.get("assets").is_none(),
        "assets must be omitted from JSON when None"
    );
}

// ---------------------------------------------------------------------------
// Item validation with STAC 1.1.0
// ---------------------------------------------------------------------------

/// Minimal valid Item JSON with stac_version 1.1.0.
fn minimal_item_json(stac_version: &str) -> String {
    json!({
        "type": "Feature",
        "stac_version": stac_version,
        "id": "test-item",
        "geometry": {
            "type": "Point",
            "coordinates": [-122.0, 37.0]
        },
        "bbox": [-122.5, 36.5, -121.5, 37.5],
        "properties": {
            "datetime": "2024-09-01T00:00:00Z"
        },
        "assets": {},
        "links": []
    })
    .to_string()
}

#[test]
fn test_item_with_stac_version_1_1_0_validates() {
    let json_str = minimal_item_json("1.1.0");
    let item: Item =
        serde_json::from_str(&json_str).expect("deserialization must succeed for 1.1.0");
    item.validate()
        .expect("Item with stac_version 1.1.0 must pass validation");
}

#[test]
fn test_item_with_stac_version_1_0_0_still_validates() {
    let json_str = minimal_item_json("1.0.0");
    let item: Item =
        serde_json::from_str(&json_str).expect("deserialization must succeed for 1.0.0");
    item.validate()
        .expect("Item with stac_version 1.0.0 must still pass validation");
}

// ---------------------------------------------------------------------------
// Asset `bands` field (STAC 1.1.0 shorthand)
// ---------------------------------------------------------------------------

#[test]
fn test_asset_bands_field_round_trips() {
    let red_band = Band::new()
        .with_name("B04")
        .with_center_wavelength(0.665)
        .with_full_width_half_max(0.038);

    let asset = Asset::new("https://example.com/B04.tif").with_bands(vec![red_band.clone()]);

    // Serialise
    let json_str = serde_json::to_string(&asset).expect("serialization must succeed");
    let parsed_val: serde_json::Value =
        serde_json::from_str(&json_str).expect("re-parse must succeed");

    assert!(
        parsed_val["bands"].is_array(),
        "bands must appear in serialized JSON"
    );
    assert_eq!(
        parsed_val["bands"][0]["center_wavelength"],
        json!(0.665),
        "center_wavelength must survive serialization"
    );

    // Deserialise back
    let deserialized: Asset =
        serde_json::from_str(&json_str).expect("deserialization must succeed");
    let bands = deserialized
        .bands
        .expect("bands must be Some after round-trip");
    assert_eq!(bands.len(), 1);
    assert_eq!(bands[0].name, Some("B04".to_string()));
    assert_eq!(bands[0].center_wavelength, Some(0.665));
}

#[test]
fn test_asset_bands_omitted_when_none() {
    let asset = Asset::new("https://example.com/image.tif");
    let json_str = serde_json::to_string(&asset).expect("serialization must succeed");
    let parsed_val: serde_json::Value =
        serde_json::from_str(&json_str).expect("re-parse must succeed");

    assert!(
        parsed_val.get("bands").is_none(),
        "bands must be omitted when None"
    );
}

#[test]
fn test_asset_add_band_builder() {
    let asset = Asset::new("https://example.com/multi.tif")
        .add_band(Band::new().with_name("red"))
        .add_band(Band::new().with_name("green"))
        .add_band(Band::new().with_name("blue"));

    let bands = asset.bands.expect("bands must be Some");
    assert_eq!(bands.len(), 3);
    assert_eq!(bands[0].name.as_deref(), Some("red"));
    assert_eq!(bands[1].name.as_deref(), Some("green"));
    assert_eq!(bands[2].name.as_deref(), Some("blue"));
}
