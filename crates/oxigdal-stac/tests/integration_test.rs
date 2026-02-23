//! Integration tests for oxigdal-stac
#![allow(clippy::panic)]

use chrono::Utc;
use oxigdal_stac::{
    Asset, Band, CatalogBuilder, CollectionBuilder, CommonBandName, EoExtension, Item, ItemBuilder,
    ProjectionExtension,
};

#[test]
fn test_catalog_roundtrip() {
    let catalog = CatalogBuilder::new("test-catalog", "A test catalog for integration testing")
        .title("Integration Test Catalog")
        .self_link("https://example.com/catalog.json")
        .root_link("https://example.com/catalog.json")
        .child_link("https://example.com/collection1.json")
        .build()
        .expect("Failed to build catalog");

    // Serialize to JSON
    let json = serde_json::to_string(&catalog).expect("Failed to serialize catalog");

    // Deserialize back
    let deserialized: oxigdal_stac::Catalog =
        serde_json::from_str(&json).expect("Failed to deserialize catalog");

    assert_eq!(catalog, deserialized);
    assert_eq!(deserialized.id, "test-catalog");
    assert_eq!(deserialized.links.len(), 3);
}

#[test]
fn test_collection_with_extent() {
    let start = Utc::now();
    let end = start + chrono::Duration::days(30);

    let collection = CollectionBuilder::new(
        "test-collection",
        "A test collection with spatial and temporal extent",
        "CC-BY-4.0",
    )
    .title("Test Collection")
    .keywords(vec!["test".to_string(), "integration".to_string()])
    .provider("Test Organization")
    .spatial_extent(-180.0, -90.0, 180.0, 90.0)
    .temporal_extent(Some(start), Some(end))
    .self_link("https://example.com/collection.json")
    .build()
    .expect("Failed to build collection");

    // Validate
    assert!(collection.validate().is_ok());

    // Check extent
    assert_eq!(collection.extent.spatial.bbox.len(), 1);
    assert_eq!(
        collection.extent.spatial.bbox[0],
        vec![-180.0, -90.0, 180.0, 90.0]
    );
    assert_eq!(collection.extent.temporal.interval.len(), 1);
    assert_eq!(
        collection.extent.temporal.interval[0],
        vec![Some(start), Some(end)]
    );
}

#[test]
fn test_item_with_assets() {
    let datetime = Utc::now();
    let geometry = geojson::Geometry::new(geojson::Value::Point(vec![-122.4194, 37.7749]));

    let visual_asset = Asset::new("https://example.com/visual.tif")
        .with_title("Visual Asset")
        .with_type("image/tiff; application=geotiff; profile=cloud-optimized")
        .with_role("visual");

    let metadata_asset = Asset::new("https://example.com/metadata.xml")
        .with_type("application/xml")
        .with_role("metadata");

    let item = ItemBuilder::new("test-item-001")
        .geometry(geometry)
        .bbox(-122.5, 37.7, -122.3, 37.8)
        .datetime(datetime)
        .asset("visual", visual_asset)
        .asset("metadata", metadata_asset)
        .collection("test-collection")
        .link("https://example.com/item.json", "self")
        .build()
        .expect("Failed to build item");

    // Validate
    assert!(item.validate().is_ok());

    // Check assets
    assert_eq!(item.assets.len(), 2);
    assert!(item.get_asset("visual").is_some());
    assert!(item.get_asset("metadata").is_some());

    // Check geometry
    assert!(item.geometry.is_some());

    // Serialize and deserialize
    let json = serde_json::to_string(&item).expect("Failed to serialize item");
    let deserialized: Item = serde_json::from_str(&json).expect("Failed to deserialize item");
    assert_eq!(item, deserialized);
}

#[test]
fn test_item_with_eo_extension() {
    let datetime = Utc::now();

    let red_band = Band::new()
        .with_name("B04")
        .with_common_name(CommonBandName::Red)
        .with_center_wavelength(0.665)
        .with_full_width_half_max(0.038);

    let nir_band = Band::new()
        .with_name("B08")
        .with_common_name(CommonBandName::Nir)
        .with_center_wavelength(0.842)
        .with_full_width_half_max(0.145);

    let eo = EoExtension::new()
        .with_cloud_cover(15.5)
        .add_band(red_band)
        .add_band(nir_band);

    // Validate EO extension
    assert!(eo.validate().is_ok());

    let item = ItemBuilder::new("sentinel2-item")
        .datetime(datetime)
        .bbox(-122.5, 37.5, -122.0, 38.0)
        .simple_asset("data", "https://example.com/sentinel2.tif")
        .extension("https://stac-extensions.github.io/eo/v1.1.0/schema.json")
        .property("eo:cloud_cover", serde_json::json!(15.5))
        .build()
        .expect("Failed to build item with EO extension");

    assert!(item.validate().is_ok());
    assert_eq!(
        item.properties.additional_fields.get("eo:cloud_cover"),
        Some(&serde_json::json!(15.5))
    );
}

#[test]
fn test_item_with_projection_extension() {
    let datetime = Utc::now();

    let proj = ProjectionExtension::new()
        .with_epsg(32610) // UTM Zone 10N
        .with_shape(10980, 10980)
        .with_bbox(vec![600000.0, 4190220.0, 709800.0, 4300020.0])
        .with_transform(vec![10.0, 0.0, 600000.0, 0.0, -10.0, 4300020.0]);

    // Validate projection extension
    assert!(proj.validate().is_ok());

    let item = ItemBuilder::new("utm-item")
        .datetime(datetime)
        .bbox(-122.5, 37.5, -122.0, 38.0)
        .simple_asset("data", "https://example.com/utm.tif")
        .extension("https://stac-extensions.github.io/projection/v1.1.0/schema.json")
        .property("proj:epsg", serde_json::json!(32610))
        .property("proj:shape", serde_json::json!([10980, 10980]))
        .build()
        .expect("Failed to build item with projection extension");

    assert!(item.validate().is_ok());
}

#[test]
fn test_item_validation_failures() {
    // Item without datetime should fail
    let invalid_item = ItemBuilder::new("invalid-item")
        .bbox(-122.5, 37.5, -122.0, 38.0)
        .simple_asset("data", "https://example.com/data.tif")
        .build();

    assert!(invalid_item.is_err());

    // Item with invalid bbox should fail validation
    let mut item = ItemBuilder::new("test-item")
        .datetime(Utc::now())
        .build()
        .expect("Failed to build item");

    item.bbox = Some(vec![-122.5, 37.5]); // Invalid: only 2 elements
    assert!(item.validate().is_err());
}

#[test]
fn test_asset_validation() {
    // Valid asset
    let valid_asset = Asset::new("https://example.com/data.tif");
    assert!(valid_asset.validate().is_ok());

    // Empty href should fail
    let invalid_asset = Asset::new("");
    assert!(invalid_asset.validate().is_err());

    // Relative paths should be allowed
    let relative_asset = Asset::new("./data.tif");
    assert!(relative_asset.validate().is_ok());
}

#[test]
fn test_eo_extension_validation() {
    // Valid cloud cover
    let valid = EoExtension::new().with_cloud_cover(50.0);
    assert!(valid.validate().is_ok());

    // Invalid cloud cover (> 100)
    let invalid = EoExtension::new().with_cloud_cover(150.0);
    assert!(invalid.validate().is_err());

    // Invalid band wavelength
    let invalid_band = Band::new().with_center_wavelength(-0.1);
    assert!(invalid_band.validate().is_err());
}

#[test]
fn test_projection_extension_validation() {
    // Valid projection
    let valid = ProjectionExtension::new()
        .with_epsg(4326)
        .with_bbox(vec![-180.0, -90.0, 180.0, 90.0])
        .with_shape(1024, 2048);
    assert!(valid.validate().is_ok());

    // Invalid bbox (wrong number of elements)
    let invalid_bbox = ProjectionExtension::new().with_bbox(vec![-180.0, -90.0]);
    assert!(invalid_bbox.validate().is_err());

    // Invalid shape (wrong number of elements)
    let mut invalid_shape = ProjectionExtension::new();
    invalid_shape.shape = Some(vec![1024]);
    assert!(invalid_shape.validate().is_err());
}

#[test]
fn test_complex_catalog_structure() {
    // Create a complete STAC structure with catalog, collection, and items
    let catalog = CatalogBuilder::new("root-catalog", "Root catalog for testing")
        .title("Test Root Catalog")
        .self_link("https://example.com/catalog.json")
        .child_link("https://example.com/collection1.json")
        .build()
        .expect("Failed to build catalog");

    let collection = CollectionBuilder::new("collection1", "First test collection", "Apache-2.0")
        .title("Collection 1")
        .spatial_extent(-180.0, -90.0, 180.0, 90.0)
        .temporal_extent(None, None) // Open-ended
        .self_link("https://example.com/collection1.json")
        .link("https://example.com/catalog.json", "parent")
        .link("https://example.com/catalog.json", "root")
        .build()
        .expect("Failed to build collection");

    let item = ItemBuilder::new("item1")
        .datetime(Utc::now())
        .bbox(-122.5, 37.5, -122.0, 38.0)
        .simple_asset("data", "https://example.com/item1.tif")
        .collection("collection1")
        .link("https://example.com/item1.json", "self")
        .link("https://example.com/collection1.json", "parent")
        .link("https://example.com/catalog.json", "root")
        .build()
        .expect("Failed to build item");

    // All should validate
    assert!(catalog.validate().is_ok());
    assert!(collection.validate().is_ok());
    assert!(item.validate().is_ok());

    // Verify structure
    assert_eq!(
        catalog.get_self_link().map(|l| &l.href),
        Some(&"https://example.com/catalog.json".to_string())
    );
    assert_eq!(collection.id, "collection1");
    assert_eq!(item.collection, Some("collection1".to_string()));
}

#[test]
fn test_link_relationships() {
    let item = ItemBuilder::new("test-item")
        .datetime(Utc::now())
        .link("https://example.com/item.json", "self")
        .link("https://example.com/collection.json", "collection")
        .link("https://example.com/derived.json", "derived_from")
        .build()
        .expect("Failed to build item");

    // Find links by rel
    let self_links: Vec<_> = item.find_links("self").collect();
    assert_eq!(self_links.len(), 1);
    assert_eq!(self_links[0].href, "https://example.com/item.json");

    let collection_links: Vec<_> = item.find_links("collection").collect();
    assert_eq!(collection_links.len(), 1);
}

#[test]
fn test_helper_functions() {
    // Test bbox helper
    let bbox = oxigdal_stac::bbox(-122.5, 37.5, -122.0, 38.0);
    assert_eq!(bbox, vec![-122.5, 37.5, -122.0, 38.0]);

    // Test point_geometry helper
    let point = oxigdal_stac::point_geometry(-122.0, 37.0);
    assert_eq!(point.value, geojson::Value::Point(vec![-122.0, 37.0]));

    // Test bbox_to_polygon helper
    let polygon = oxigdal_stac::bbox_to_polygon(-122.5, 37.5, -122.0, 38.0);
    match polygon.value {
        geojson::Value::Polygon(coords) => {
            assert_eq!(coords.len(), 1);
            assert_eq!(coords[0].len(), 5);
            assert_eq!(coords[0][0], vec![-122.5, 37.5]);
            assert_eq!(coords[0][4], vec![-122.5, 37.5]); // Closed ring
        }
        _ => panic!("Expected Polygon"),
    }
}

#[test]
fn test_json_serialization_format() {
    let item = ItemBuilder::new("test-item")
        .datetime(Utc::now())
        .bbox(-122.5, 37.5, -122.0, 38.0)
        .simple_asset("visual", "https://example.com/image.tif")
        .build()
        .expect("Failed to build item");

    let json = serde_json::to_string_pretty(&item).expect("Failed to serialize");

    // Verify it contains expected fields (accounting for pretty-print formatting)
    assert!(json.contains("\"type\"") && json.contains("\"Feature\""));
    assert!(json.contains("\"stac_version\"") && json.contains("\"1.0.0\""));
    assert!(json.contains("\"id\"") && json.contains("\"test-item\""));
    assert!(json.contains("\"assets\""));
    assert!(json.contains("\"visual\""));
}

#[test]
fn test_real_world_sentinel2_example() {
    let datetime = chrono::DateTime::parse_from_rfc3339("2023-06-15T10:30:00Z")
        .expect("Failed to parse datetime")
        .with_timezone(&Utc);

    let geometry = geojson::Geometry::new(geojson::Value::Polygon(vec![vec![
        vec![-122.5, 37.5],
        vec![-122.0, 37.5],
        vec![-122.0, 38.0],
        vec![-122.5, 38.0],
        vec![-122.5, 37.5],
    ]]));

    let visual_asset = Asset::new("s3://sentinel-s2-l2a/tiles/10/S/FH/2023/6/15/0/TCI.jp2")
        .with_title("True color image")
        .with_type("image/jp2")
        .with_role("visual");

    let item = ItemBuilder::new("S2A_10SFH_20230615_0_L2A")
        .geometry(geometry)
        .bbox(-122.5, 37.5, -122.0, 38.0)
        .datetime(datetime)
        .asset("visual", visual_asset)
        .collection("sentinel-2-l2a")
        .property("platform", serde_json::json!("sentinel-2a"))
        .property("instruments", serde_json::json!(["msi"]))
        .property("eo:cloud_cover", serde_json::json!(5.2))
        .extension("https://stac-extensions.github.io/eo/v1.1.0/schema.json")
        .build()
        .expect("Failed to build Sentinel-2 item");

    assert!(item.validate().is_ok());
    assert_eq!(item.id, "S2A_10SFH_20230615_0_L2A");
    assert_eq!(item.collection, Some("sentinel-2-l2a".to_string()));
    assert_eq!(
        item.properties.additional_fields.get("platform"),
        Some(&serde_json::json!("sentinel-2a"))
    );
}
