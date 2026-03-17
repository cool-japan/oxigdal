//! Integration tests for the OxiGDAL WASM Component Model interface.
//!
//! Covers:
//! - [`ComponentBbox`]          — 12 tests
//! - [`ComponentDataType`]      — 10 tests
//! - [`ComponentError`]         — 7 tests
//! - [`ImageDimensions`]        — 6 tests
//! - [`ComponentRaster`]        — 14 tests
//! - [`ComponentFeature`]       — 9 tests
//! - [`ComponentFeatureCollection`] — 8 tests
//! - [`WasmBumpAllocator`]      — 6 tests
//! - [`ComponentProjection`]    — 6 tests
//! - [`ComponentTransform`]     — 5 tests
//! - [`ComponentCoord`]         — 3 tests
//!
//! Total: 86 tests

use oxigdal_wasm::component::{
    ComponentBbox, ComponentDataType, ComponentError, ComponentFeature, ComponentFeatureCollection,
    ComponentProjection, ComponentRaster, ComponentRasterOps, ComponentTransform, ErrorCategory,
    ImageDimensions, PropertyValue,
};
use oxigdal_wasm::wasm_memory::WasmBumpAllocator;

// ---------------------------------------------------------------------------
// ComponentBbox  (12 tests)
// ---------------------------------------------------------------------------

#[test]
fn bbox_new_stores_values() {
    let b = ComponentBbox::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(b.min_x, 1.0);
    assert_eq!(b.min_y, 2.0);
    assert_eq!(b.max_x, 3.0);
    assert_eq!(b.max_y, 4.0);
}

#[test]
fn bbox_width() {
    let b = ComponentBbox::new(0.0, 0.0, 7.5, 5.0);
    assert!((b.width() - 7.5).abs() < 1e-12);
}

#[test]
fn bbox_height() {
    let b = ComponentBbox::new(0.0, 0.0, 7.5, 5.0);
    assert!((b.height() - 5.0).abs() < 1e-12);
}

#[test]
fn bbox_area() {
    let b = ComponentBbox::new(0.0, 0.0, 4.0, 3.0);
    assert!((b.area() - 12.0).abs() < 1e-12);
}

#[test]
fn bbox_center() {
    let b = ComponentBbox::new(0.0, 0.0, 10.0, 6.0);
    assert_eq!(b.center(), (5.0, 3.0));
}

#[test]
fn bbox_contains_inside() {
    let b = ComponentBbox::new(0.0, 0.0, 10.0, 10.0);
    assert!(b.contains(5.0, 5.0));
}

#[test]
fn bbox_contains_on_edge() {
    let b = ComponentBbox::new(0.0, 0.0, 10.0, 10.0);
    assert!(b.contains(0.0, 5.0));
    assert!(b.contains(10.0, 5.0));
}

#[test]
fn bbox_contains_outside() {
    let b = ComponentBbox::new(0.0, 0.0, 10.0, 10.0);
    assert!(!b.contains(11.0, 5.0));
    assert!(!b.contains(5.0, -1.0));
}

#[test]
fn bbox_intersects_overlapping() {
    let a = ComponentBbox::new(0.0, 0.0, 5.0, 5.0);
    let b = ComponentBbox::new(3.0, 3.0, 8.0, 8.0);
    assert!(a.intersects(&b));
}

#[test]
fn bbox_intersects_non_overlapping() {
    let a = ComponentBbox::new(0.0, 0.0, 2.0, 2.0);
    let b = ComponentBbox::new(5.0, 5.0, 10.0, 10.0);
    assert!(!a.intersects(&b));
}

#[test]
fn bbox_intersects_touching_edge() {
    let a = ComponentBbox::new(0.0, 0.0, 5.0, 5.0);
    let b = ComponentBbox::new(5.0, 0.0, 10.0, 5.0);
    assert!(a.intersects(&b)); // touching edge counts as intersecting
}

#[test]
fn bbox_union_expands() {
    let a = ComponentBbox::new(0.0, 0.0, 5.0, 5.0);
    let b = ComponentBbox::new(3.0, 3.0, 10.0, 10.0);
    let u = a.union(&b);
    assert_eq!(u.min_x, 0.0);
    assert_eq!(u.min_y, 0.0);
    assert_eq!(u.max_x, 10.0);
    assert_eq!(u.max_y, 10.0);
}

// ---------------------------------------------------------------------------
// ComponentDataType  (10 tests)
// ---------------------------------------------------------------------------

#[test]
fn dtype_byte_size_u8() {
    assert_eq!(ComponentDataType::Uint8.byte_size(), 1);
}

#[test]
fn dtype_byte_size_u16() {
    assert_eq!(ComponentDataType::Uint16.byte_size(), 2);
}

#[test]
fn dtype_byte_size_u32() {
    assert_eq!(ComponentDataType::Uint32.byte_size(), 4);
}

#[test]
fn dtype_byte_size_f32() {
    assert_eq!(ComponentDataType::Float32.byte_size(), 4);
}

#[test]
fn dtype_byte_size_f64() {
    assert_eq!(ComponentDataType::Float64.byte_size(), 8);
}

#[test]
fn dtype_from_u8_all_valid() {
    for i in 0u8..=7 {
        assert!(
            ComponentDataType::from_u8(i).is_some(),
            "Expected Some for discriminant {i}"
        );
    }
}

#[test]
fn dtype_from_u8_invalid() {
    assert!(ComponentDataType::from_u8(8).is_none());
    assert!(ComponentDataType::from_u8(200).is_none());
    assert!(ComponentDataType::from_u8(255).is_none());
}

#[test]
fn dtype_is_floating_point() {
    assert!(ComponentDataType::Float32.is_floating_point());
    assert!(ComponentDataType::Float64.is_floating_point());
    assert!(!ComponentDataType::Uint8.is_floating_point());
    assert!(!ComponentDataType::Int32.is_floating_point());
}

#[test]
fn dtype_is_integer() {
    assert!(ComponentDataType::Uint8.is_integer());
    assert!(ComponentDataType::Int16.is_integer());
    assert!(!ComponentDataType::Float64.is_integer());
}

#[test]
fn dtype_is_signed() {
    assert!(ComponentDataType::Int8.is_signed());
    assert!(ComponentDataType::Int16.is_signed());
    assert!(ComponentDataType::Int32.is_signed());
    assert!(ComponentDataType::Float32.is_signed());
    assert!(ComponentDataType::Float64.is_signed());
    assert!(!ComponentDataType::Uint8.is_signed());
    assert!(!ComponentDataType::Uint16.is_signed());
    assert!(!ComponentDataType::Uint32.is_signed());
}

// ---------------------------------------------------------------------------
// ComponentError  (7 tests)
// ---------------------------------------------------------------------------

#[test]
fn error_invalid_input_constructor() {
    let e = ComponentError::invalid_input("bad param");
    assert_eq!(e.category, ErrorCategory::InvalidInput);
    assert!(e.message.contains("bad param"));
}

#[test]
fn error_unsupported_constructor() {
    let e = ComponentError::unsupported("no JPEG2000");
    assert_eq!(e.category, ErrorCategory::UnsupportedFormat);
}

#[test]
fn error_io_constructor() {
    let e = ComponentError::io("file not found");
    assert_eq!(e.category, ErrorCategory::Io);
}

#[test]
fn error_message_preserved() {
    let msg = "detailed error description with context";
    let e = ComponentError::internal(msg);
    assert!(e.message.contains(msg));
}

#[test]
fn error_category_correct_for_projection() {
    let e = ComponentError::projection("unknown CRS");
    assert_eq!(e.category, ErrorCategory::Projection);
}

#[test]
fn error_display_includes_code_and_message() {
    let e = ComponentError::new(42, "test error", ErrorCategory::Internal);
    let s = format!("{e}");
    assert!(s.contains("42"));
    assert!(s.contains("test error"));
}

#[test]
fn error_out_of_memory_constructor() {
    let e = ComponentError::out_of_memory("no more ram");
    assert_eq!(e.category, ErrorCategory::OutOfMemory);
}

// ---------------------------------------------------------------------------
// ImageDimensions  (6 tests)
// ---------------------------------------------------------------------------

#[test]
fn dims_pixel_count_single_band() {
    let d = ImageDimensions::new(100, 200, 1);
    assert_eq!(d.pixel_count(), 20_000);
}

#[test]
fn dims_pixel_count_matches_w_times_h() {
    let d = ImageDimensions::new(13, 17, 1);
    assert_eq!(d.pixel_count(), 13 * 17);
}

#[test]
fn dims_band_size_bytes_u8() {
    let d = ImageDimensions::new(10, 10, 1);
    assert_eq!(d.band_size_bytes(&ComponentDataType::Uint8), 100);
}

#[test]
fn dims_band_size_bytes_f64() {
    let d = ImageDimensions::new(10, 10, 1);
    assert_eq!(d.band_size_bytes(&ComponentDataType::Float64), 800);
}

#[test]
fn dims_total_size_bytes_multi_band() {
    let d = ImageDimensions::new(4, 4, 3);
    // 4*4 = 16 pixels, Float32 = 4 bytes, 3 bands => 192 bytes
    assert_eq!(d.total_size_bytes(&ComponentDataType::Float32), 192);
}

#[test]
fn dims_total_size_single_band() {
    let d = ImageDimensions::new(5, 5, 1);
    assert_eq!(d.total_size_bytes(&ComponentDataType::Uint16), 50);
}

// ---------------------------------------------------------------------------
// ComponentRaster  (14 tests)
// ---------------------------------------------------------------------------

fn make_f32_raster(w: u32, h: u32, bands: u32) -> ComponentRaster {
    ComponentRaster::new(
        ImageDimensions::new(w, h, bands),
        ComponentDataType::Float32,
        ComponentBbox::new(0.0, 0.0, 1.0, 1.0),
        "EPSG:4326",
    )
}

#[test]
fn raster_new_allocates_correct_data_size() {
    let r = make_f32_raster(10, 10, 3);
    assert_eq!(r.data.len(), 10 * 10 * 3 * 4);
}

#[test]
fn raster_get_pixel_default_zero() {
    let r = make_f32_raster(4, 4, 1);
    assert_eq!(r.get_pixel(0, 0, 0).expect("pixel (0,0,0)"), 0.0);
    assert_eq!(r.get_pixel(0, 3, 3).expect("pixel (0,3,3)"), 0.0);
}

#[test]
fn raster_get_pixel_out_of_bounds_band() {
    let r = make_f32_raster(4, 4, 1);
    assert!(r.get_pixel(1, 0, 0).is_err());
}

#[test]
fn raster_get_pixel_out_of_bounds_row() {
    let r = make_f32_raster(4, 4, 1);
    assert!(r.get_pixel(0, 10, 0).is_err());
}

#[test]
fn raster_get_pixel_out_of_bounds_col() {
    let r = make_f32_raster(4, 4, 1);
    assert!(r.get_pixel(0, 0, 10).is_err());
}

#[test]
fn raster_set_and_get_pixel_roundtrip_f32() {
    let mut r = make_f32_raster(4, 4, 1);
    r.set_pixel(0, 1, 2, 123.456).expect("set");
    let v = r.get_pixel(0, 1, 2).expect("get");
    // Float32 precision
    assert!((v - 123.456).abs() < 0.001);
}

#[test]
fn raster_set_and_get_pixel_roundtrip_u8() {
    let mut r = ComponentRaster::new(
        ImageDimensions::new(4, 4, 1),
        ComponentDataType::Uint8,
        ComponentBbox::new(0.0, 0.0, 1.0, 1.0),
        "EPSG:4326",
    );
    r.set_pixel(0, 0, 0, 200.0).expect("set");
    let v = r.get_pixel(0, 0, 0).expect("get");
    assert!((v - 200.0).abs() < 1e-9);
}

#[test]
fn raster_statistics_all_zeros() {
    let r = make_f32_raster(4, 4, 1);
    let stats = r.statistics().expect("statistics");
    assert_eq!(stats.min, 0.0);
    assert_eq!(stats.max, 0.0);
    assert_eq!(stats.mean, 0.0);
    assert_eq!(stats.valid_pixels, 16);
}

#[test]
fn raster_statistics_excludes_nodata() {
    let mut r = make_f32_raster(2, 2, 1).with_nodata(-9999.0);
    // Fill all pixels with nodata
    for row in 0..2 {
        for col in 0..2 {
            r.set_pixel(0, row, col, -9999.0).expect("set nodata");
        }
    }
    let stats = r.statistics().expect("stats");
    assert_eq!(stats.valid_pixels, 0);
}

#[test]
fn raster_is_nodata_true_and_false() {
    let r = make_f32_raster(2, 2, 1).with_nodata(-9999.0);
    assert!(r.is_nodata(-9999.0));
    assert!(!r.is_nodata(0.0));
    assert!(!r.is_nodata(-9998.99));
}

#[test]
fn raster_clip_reduces_dimensions() {
    let r = ComponentRaster::new(
        ImageDimensions::new(100, 100, 1),
        ComponentDataType::Uint8,
        ComponentBbox::new(0.0, 0.0, 100.0, 100.0),
        "EPSG:4326",
    );
    let clip_bbox = ComponentBbox::new(10.0, 10.0, 60.0, 60.0);
    let clipped = ComponentRasterOps::clip(&r, &clip_bbox).expect("clip");
    assert!(clipped.dims.width < r.dims.width);
    assert!(clipped.dims.height < r.dims.height);
}

#[test]
fn raster_clip_non_overlapping_returns_error() {
    let r = ComponentRaster::new(
        ImageDimensions::new(10, 10, 1),
        ComponentDataType::Uint8,
        ComponentBbox::new(0.0, 0.0, 10.0, 10.0),
        "EPSG:4326",
    );
    let outside = ComponentBbox::new(20.0, 20.0, 30.0, 30.0);
    assert!(ComponentRasterOps::clip(&r, &outside).is_err());
}

#[test]
fn raster_clip_preserves_band_count() {
    let r = ComponentRaster::new(
        ImageDimensions::new(10, 10, 4),
        ComponentDataType::Uint8,
        ComponentBbox::new(0.0, 0.0, 10.0, 10.0),
        "EPSG:4326",
    );
    let clipped =
        ComponentRasterOps::clip(&r, &ComponentBbox::new(1.0, 1.0, 9.0, 9.0)).expect("clip");
    assert_eq!(clipped.dims.bands, 4);
}

#[test]
fn raster_clip_returns_correct_bbox_subset() {
    let r = ComponentRaster::new(
        ImageDimensions::new(200, 200, 1),
        ComponentDataType::Uint8,
        ComponentBbox::new(0.0, 0.0, 200.0, 200.0),
        "EPSG:4326",
    );
    let clip_bbox = ComponentBbox::new(50.0, 50.0, 150.0, 150.0);
    let clipped = ComponentRasterOps::clip(&r, &clip_bbox).expect("clip");
    // Clipped bbox must lie inside the original extent.
    assert!(clipped.bbox.min_x >= r.bbox.min_x - 1e-6);
    assert!(clipped.bbox.min_y >= r.bbox.min_y - 1e-6);
    assert!(clipped.bbox.max_x <= r.bbox.max_x + 1e-6);
    assert!(clipped.bbox.max_y <= r.bbox.max_y + 1e-6);
}

// ---------------------------------------------------------------------------
// ComponentFeature  (9 tests)
// ---------------------------------------------------------------------------

#[test]
fn feature_new_is_empty() {
    let f = ComponentFeature::new();
    assert!(f.id.is_none());
    assert!(!f.has_geometry());
    assert_eq!(f.property_count(), 0);
    assert!(f.bbox.is_none());
}

#[test]
fn feature_with_id_sets_id() {
    let f = ComponentFeature::new().with_id("feature-42");
    assert_eq!(f.id.as_deref(), Some("feature-42"));
}

#[test]
fn feature_with_geometry_sets_wkb() {
    let wkb: Vec<u8> = vec![
        1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let f = ComponentFeature::new().with_geometry(wkb.clone());
    assert!(f.has_geometry());
    assert_eq!(f.geometry_wkb.as_ref().map(|v| v.len()), Some(wkb.len()));
}

#[test]
fn feature_set_get_property_string() {
    let mut f = ComponentFeature::new();
    f.set_property("name", PropertyValue::String("river".into()));
    let val = f.get_property("name").expect("property exists");
    assert_eq!(val.as_str(), Some("river"));
}

#[test]
fn feature_set_get_property_int() {
    let mut f = ComponentFeature::new();
    f.set_property("pop", PropertyValue::Int(1_000_000));
    let val = f.get_property("pop").expect("property exists");
    assert_eq!(val.as_f64(), Some(1_000_000.0));
}

#[test]
fn feature_get_missing_property_returns_none() {
    let f = ComponentFeature::new();
    assert!(f.get_property("nonexistent").is_none());
}

#[test]
fn feature_has_geometry_false_when_missing() {
    let f = ComponentFeature::new();
    assert!(!f.has_geometry());
}

#[test]
fn property_value_as_str_non_string_returns_none() {
    assert!(PropertyValue::Int(1).as_str().is_none());
    assert!(PropertyValue::Float(1.0).as_str().is_none());
    assert!(PropertyValue::Null.as_str().is_none());
}

#[test]
fn property_value_is_null_only_for_null() {
    assert!(PropertyValue::Null.is_null());
    assert!(!PropertyValue::Bool(false).is_null());
    assert!(!PropertyValue::Bytes(vec![]).is_null());
}

// ---------------------------------------------------------------------------
// ComponentFeatureCollection  (8 tests)
// ---------------------------------------------------------------------------

fn make_collection_with_bboxed_features() -> ComponentFeatureCollection {
    let mut col = ComponentFeatureCollection::new();
    col.add_feature(ComponentFeature::new().with_bbox(ComponentBbox::new(0.0, 0.0, 1.0, 1.0)));
    col.add_feature(ComponentFeature::new().with_bbox(ComponentBbox::new(10.0, 10.0, 20.0, 20.0)));
    col.add_feature(
        ComponentFeature::new().with_bbox(ComponentBbox::new(100.0, 100.0, 200.0, 200.0)),
    );
    col
}

#[test]
fn collection_new_is_empty() {
    let col = ComponentFeatureCollection::new();
    assert!(col.is_empty());
    assert_eq!(col.len(), 0);
}

#[test]
fn collection_add_feature_increments_len() {
    let mut col = ComponentFeatureCollection::new();
    col.add_feature(ComponentFeature::new());
    assert_eq!(col.len(), 1);
    col.add_feature(ComponentFeature::new());
    assert_eq!(col.len(), 2);
    assert!(!col.is_empty());
}

#[test]
fn collection_filter_by_bbox_returns_intersecting() {
    let col = make_collection_with_bboxed_features();
    let filter = ComponentBbox::new(0.0, 0.0, 15.0, 15.0);
    let result = col.filter_by_bbox(&filter);
    // features at [0,0]→[1,1] and [10,10]→[20,20] both intersect
    assert_eq!(result.len(), 2);
}

#[test]
fn collection_filter_by_bbox_no_match_returns_empty() {
    let col = make_collection_with_bboxed_features();
    let filter = ComponentBbox::new(500.0, 500.0, 600.0, 600.0);
    let result = col.filter_by_bbox_strict(&filter);
    assert!(result.is_empty());
}

#[test]
fn collection_filter_by_bbox_includes_no_bbox_features() {
    let mut col = ComponentFeatureCollection::new();
    col.add_feature(ComponentFeature::new()); // no bbox — conservatively included
    let filter = ComponentBbox::new(0.0, 0.0, 1.0, 1.0);
    let result = col.filter_by_bbox(&filter);
    assert_eq!(result.len(), 1);
}

#[test]
fn collection_is_empty_after_all_filtered_out() {
    let col = make_collection_with_bboxed_features();
    let filter = ComponentBbox::new(500.0, 500.0, 600.0, 600.0);
    let result = col.filter_by_bbox_strict(&filter);
    assert!(result.is_empty());
}

#[test]
fn collection_compute_bbox_union() {
    let col = make_collection_with_bboxed_features();
    let bbox = col.compute_bbox().expect("should have bbox");
    assert_eq!(bbox.min_x, 0.0);
    assert_eq!(bbox.min_y, 0.0);
    assert_eq!(bbox.max_x, 200.0);
    assert_eq!(bbox.max_y, 200.0);
}

#[test]
fn collection_compute_bbox_none_when_no_feature_bboxes() {
    let mut col = ComponentFeatureCollection::new();
    col.add_feature(ComponentFeature::new()); // no bbox
    assert!(col.compute_bbox().is_none());
}

// ---------------------------------------------------------------------------
// WasmBumpAllocator  (6 tests)
// ---------------------------------------------------------------------------

#[test]
fn bump_new_correct_capacity() {
    let a = WasmBumpAllocator::new(1024);
    assert_eq!(a.capacity(), 1024);
    assert_eq!(a.used(), 0);
    assert_eq!(a.remaining(), 1024);
    assert!(a.is_empty());
}

#[test]
fn bump_alloc_succeeds_when_space_available() {
    let mut a = WasmBumpAllocator::new(256);
    let slice = a.alloc(64, 8).expect("first alloc should succeed");
    assert_eq!(slice.len(), 64);
    assert_eq!(a.used(), 64);
    assert_eq!(a.remaining(), 192);
}

#[test]
fn bump_alloc_fails_when_exhausted() {
    let mut a = WasmBumpAllocator::new(8);
    assert!(a.alloc(16, 1).is_none());
}

#[test]
fn bump_reset_allows_reuse() {
    let mut a = WasmBumpAllocator::new(64);
    a.alloc(32, 4).expect("first alloc");
    a.reset();
    assert_eq!(a.used(), 0);
    assert!(a.is_empty());
    // After reset the full capacity is available again.
    a.alloc(64, 1).expect("alloc after reset");
    assert_eq!(a.used(), 64);
}

#[test]
fn bump_used_and_remaining_track_correctly() {
    let mut a = WasmBumpAllocator::new(100);
    a.alloc(20, 1).expect("20");
    assert_eq!(a.used(), 20);
    assert_eq!(a.remaining(), 80);
    a.alloc(30, 1).expect("30");
    assert_eq!(a.used(), 50);
    assert_eq!(a.remaining(), 50);
}

#[test]
fn bump_alloc_zero_size_returns_none() {
    let mut a = WasmBumpAllocator::new(64);
    assert!(a.alloc(0, 1).is_none());
}

// ---------------------------------------------------------------------------
// ComponentProjection  (6 tests)
// ---------------------------------------------------------------------------

#[test]
fn projection_wgs84_descriptor() {
    let p = ComponentProjection::wgs84();
    assert_eq!(p.epsg_code, Some(4326));
    assert!(p.is_geographic);
    assert!(!p.is_projected);
    assert_eq!(p.units, "degree");
}

#[test]
fn projection_web_mercator_descriptor() {
    let p = ComponentProjection::web_mercator();
    assert_eq!(p.epsg_code, Some(3857));
    assert!(p.is_projected);
    assert!(!p.is_geographic);
    assert_eq!(p.units, "metre");
}

#[test]
fn projection_from_epsg_utm_north() {
    let p = ComponentProjection::from_epsg(32632);
    assert_eq!(p.epsg_code, Some(32632));
    assert!(p.is_projected);
    assert!(p.name.contains("32N"));
}

#[test]
fn projection_from_epsg_utm_south() {
    let p = ComponentProjection::from_epsg(32733);
    assert_eq!(p.epsg_code, Some(32733));
    assert!(p.is_projected);
    assert!(p.name.contains("33S"));
}

#[test]
fn projection_same_crs_true_for_equal_epsg() {
    let a = ComponentProjection::wgs84();
    let b = ComponentProjection::wgs84();
    assert!(a.same_crs(&b));
}

#[test]
fn projection_same_crs_false_for_different_epsg() {
    let a = ComponentProjection::wgs84();
    let b = ComponentProjection::web_mercator();
    assert!(!a.same_crs(&b));
}

// ---------------------------------------------------------------------------
// ComponentTransform  (5 tests)
// ---------------------------------------------------------------------------

#[test]
fn transform_wgs84_to_webmercator_origin_maps_to_zero() {
    let coords = [oxigdal_wasm::component::ComponentCoord::new(0.0, 0.0)];
    let out = ComponentTransform::wgs84_to_web_mercator(&coords).expect("transform");
    assert!((out[0].x).abs() < 1e-6);
    assert!((out[0].y).abs() < 1e-6);
}

#[test]
fn transform_wgs84_to_webmercator_longitude_positive() {
    let coords = [oxigdal_wasm::component::ComponentCoord::new(10.0, 0.0)];
    let out = ComponentTransform::wgs84_to_web_mercator(&coords).expect("transform");
    assert!(out[0].x > 0.0);
}

#[test]
fn transform_web_mercator_to_wgs84_roundtrip() {
    use oxigdal_wasm::component::ComponentCoord;
    let input = ComponentCoord::new(13.405, 52.52); // Berlin approx.
    let fwd =
        ComponentTransform::wgs84_to_web_mercator(std::slice::from_ref(&input)).expect("forward");
    let bwd = ComponentTransform::web_mercator_to_wgs84(&fwd).expect("backward");
    assert!((bwd[0].x - input.x).abs() < 1e-7);
    assert!((bwd[0].y - input.y).abs() < 1e-7);
}

#[test]
fn transform_invalid_longitude_returns_error() {
    let coords = [oxigdal_wasm::component::ComponentCoord::new(200.0, 0.0)];
    assert!(ComponentTransform::wgs84_to_web_mercator(&coords).is_err());
}

#[test]
fn transform_latitude_beyond_max_returns_error() {
    let coords = [oxigdal_wasm::component::ComponentCoord::new(0.0, 90.0)];
    assert!(ComponentTransform::wgs84_to_web_mercator(&coords).is_err());
}

// ---------------------------------------------------------------------------
// ComponentCoord  (3 tests)
// ---------------------------------------------------------------------------

#[test]
fn coord_distance_pythagorean_triple() {
    use oxigdal_wasm::component::ComponentCoord;
    let a = ComponentCoord::new(0.0, 0.0);
    let b = ComponentCoord::new(3.0, 4.0);
    assert!((a.distance_to(&b) - 5.0).abs() < 1e-10);
}

#[test]
fn coord_distance_zero_when_equal() {
    use oxigdal_wasm::component::ComponentCoord;
    let a = ComponentCoord::new(1.0, 2.0);
    assert!((a.distance_to(&a.clone())).abs() < 1e-10);
}

#[test]
fn coord_midpoint_correct() {
    use oxigdal_wasm::component::ComponentCoord;
    let a = ComponentCoord::new(0.0, 0.0);
    let b = ComponentCoord::new(10.0, 6.0);
    let m = a.midpoint(&b);
    assert_eq!(m, ComponentCoord::new(5.0, 3.0));
}
