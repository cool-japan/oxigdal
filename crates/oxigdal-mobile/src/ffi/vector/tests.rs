//! Tests for vector FFI operations.

use super::super::types::*;
use super::feature::{FeatureHandle, FfiFeature, FieldValue};
use super::geometry::FfiGeometry;
use super::layer::{
    LayerHandle, oxigdal_dataset_create_layer, oxigdal_dataset_open_layer, oxigdal_feature_free,
    oxigdal_feature_get_fid, oxigdal_feature_get_geometry_type, oxigdal_feature_get_geometry_wkt,
    oxigdal_layer_add_feature_internal, oxigdal_layer_clear_spatial_filter, oxigdal_layer_close,
    oxigdal_layer_get_extent, oxigdal_layer_get_feature, oxigdal_layer_get_feature_count,
    oxigdal_layer_get_geom_type, oxigdal_layer_get_name, oxigdal_layer_get_next_feature,
    oxigdal_layer_get_srs_epsg, oxigdal_layer_reset_reading, oxigdal_layer_set_spatial_filter_bbox,
};
use crate::ffi::error::oxigdal_string_free;
use std::ffi::{CStr, CString};
use std::ptr;

#[test]
fn test_layer_handle_creation() {
    let handle = LayerHandle::new("test_layer".to_string(), "Point".to_string(), 4326);

    assert_eq!(handle.name(), "test_layer");
    assert_eq!(handle.feature_count(), 0);
    assert_eq!(handle.geom_type(), "Point");
    assert_eq!(handle.srs_epsg(), 4326);
}

#[test]
fn test_feature_handle_creation() {
    let mut feature = FfiFeature::new(123);
    feature.geometry = Some(FfiGeometry::Point {
        x: 10.0,
        y: 20.0,
        z: None,
    });

    let handle = FeatureHandle::new(feature);
    assert_eq!(handle.inner().fid, 123);
    assert!(handle.inner().geometry.is_some());
}

#[test]
fn test_geometry_bounds() {
    // Test Point bounds
    let point = FfiGeometry::Point {
        x: 10.0,
        y: 20.0,
        z: None,
    };
    let bounds = point.bounds();
    assert!(bounds.is_some());
    let (min_x, min_y, max_x, max_y) = bounds.expect("should have bounds");
    assert_eq!(min_x, 10.0);
    assert_eq!(min_y, 20.0);
    assert_eq!(max_x, 10.0);
    assert_eq!(max_y, 20.0);

    // Test LineString bounds
    let linestring = FfiGeometry::LineString {
        coords: vec![(0.0, 0.0, None), (10.0, 10.0, None), (20.0, 5.0, None)],
    };
    let bounds = linestring.bounds();
    assert!(bounds.is_some());
    let (min_x, min_y, max_x, max_y) = bounds.expect("should have bounds");
    assert_eq!(min_x, 0.0);
    assert_eq!(min_y, 0.0);
    assert_eq!(max_x, 20.0);
    assert_eq!(max_y, 10.0);
}

#[test]
fn test_geometry_to_wkt() {
    // Test Point WKT
    let point = FfiGeometry::Point {
        x: 10.5,
        y: 20.5,
        z: None,
    };
    assert_eq!(point.to_wkt(), "POINT (10.5 20.5)");

    // Test Point with Z
    let point_z = FfiGeometry::Point {
        x: 10.0,
        y: 20.0,
        z: Some(30.0),
    };
    assert_eq!(point_z.to_wkt(), "POINT Z (10 20 30)");

    // Test LineString WKT
    let linestring = FfiGeometry::LineString {
        coords: vec![(0.0, 0.0, None), (10.0, 10.0, None)],
    };
    assert_eq!(linestring.to_wkt(), "LINESTRING (0 0, 10 10)");

    // Test Polygon WKT
    let polygon = FfiGeometry::Polygon {
        exterior: vec![
            (0.0, 0.0, None),
            (10.0, 0.0, None),
            (10.0, 10.0, None),
            (0.0, 10.0, None),
            (0.0, 0.0, None),
        ],
        interiors: vec![],
    };
    assert!(polygon.to_wkt().starts_with("POLYGON"));
}

#[test]
fn test_layer_extent_calculation() {
    let mut layer = LayerHandle::new("test".to_string(), "Point".to_string(), 4326);

    // Empty layer should return zero extent
    let extent = layer.calculate_extent();
    assert_eq!(extent.min_x, 0.0);
    assert_eq!(extent.max_x, 0.0);

    // Add features and test extent
    let mut f1 = FfiFeature::new(1);
    f1.geometry = Some(FfiGeometry::Point {
        x: 10.0,
        y: 20.0,
        z: None,
    });
    layer.add_feature(f1);

    let mut f2 = FfiFeature::new(2);
    f2.geometry = Some(FfiGeometry::Point {
        x: 30.0,
        y: 40.0,
        z: None,
    });
    layer.add_feature(f2);

    let extent = layer.calculate_extent();
    assert_eq!(extent.min_x, 10.0);
    assert_eq!(extent.min_y, 20.0);
    assert_eq!(extent.max_x, 30.0);
    assert_eq!(extent.max_y, 40.0);
}

#[test]
fn test_layer_iteration() {
    let mut layer = LayerHandle::new("test".to_string(), "Point".to_string(), 4326);

    // Add features
    for i in 0..5 {
        let mut f = FfiFeature::new(i);
        f.geometry = Some(FfiGeometry::Point {
            x: i as f64 * 10.0,
            y: i as f64 * 10.0,
            z: None,
        });
        layer.add_feature(f);
    }

    // Iterate through features
    let mut count = 0;
    while layer.next_feature().is_some() {
        count += 1;
    }
    assert_eq!(count, 5);

    // Reset and iterate again
    layer.reset_cursor();
    count = 0;
    while layer.next_feature().is_some() {
        count += 1;
    }
    assert_eq!(count, 5);
}

#[test]
fn test_spatial_filter() {
    let mut layer = LayerHandle::new("test".to_string(), "Point".to_string(), 4326);

    // Add features at different locations
    for i in 0..10 {
        let mut f = FfiFeature::new(i);
        f.geometry = Some(FfiGeometry::Point {
            x: i as f64 * 10.0,
            y: i as f64 * 10.0,
            z: None,
        });
        layer.add_feature(f);
    }

    // Set spatial filter
    layer.set_spatial_filter(OxiGdalBbox {
        min_x: 15.0,
        min_y: 15.0,
        max_x: 55.0,
        max_y: 55.0,
    });

    // Should only get features 2, 3, 4, 5 (x=20,30,40,50)
    let mut filtered_count = 0;
    while layer.next_feature().is_some() {
        filtered_count += 1;
    }
    assert_eq!(filtered_count, 4);

    // Clear filter and iterate again
    layer.reset_cursor();
    layer.clear_spatial_filter();
    let mut total_count = 0;
    while layer.next_feature().is_some() {
        total_count += 1;
    }
    assert_eq!(total_count, 10);
}

#[test]
fn test_get_feature_by_fid() {
    let mut layer = LayerHandle::new("test".to_string(), "Point".to_string(), 4326);

    for i in 0..5 {
        let mut f = FfiFeature::new(i * 10); // FIDs: 0, 10, 20, 30, 40
        f.geometry = Some(FfiGeometry::Point {
            x: i as f64,
            y: i as f64,
            z: None,
        });
        layer.add_feature(f);
    }

    // Find existing feature
    let feature = layer.get_feature_by_fid(20);
    assert!(feature.is_some());
    let feature = feature.expect("should have feature");
    assert_eq!(feature.fid, 20);

    // Try to find non-existing feature
    let missing = layer.get_feature_by_fid(15);
    assert!(missing.is_none());
}

#[test]
fn test_field_values() {
    let mut feature = FfiFeature::new(1);
    feature
        .fields
        .insert("name".to_string(), FieldValue::String("Test".to_string()));
    feature
        .fields
        .insert("count".to_string(), FieldValue::Integer(42));
    feature.fields.insert(
        "value".to_string(),
        FieldValue::Double(std::f64::consts::PI),
    );
    feature
        .fields
        .insert("active".to_string(), FieldValue::Bool(true));

    // Test string conversion
    assert_eq!(
        feature.fields.get("name").map(|v| v.to_string_value()),
        Some("Test".to_string())
    );
    assert_eq!(
        feature.fields.get("count").map(|v| v.to_string_value()),
        Some("42".to_string())
    );

    // Test integer conversion
    assert_eq!(
        feature.fields.get("count").and_then(|v| v.as_integer()),
        Some(42)
    );
    assert_eq!(
        feature.fields.get("value").and_then(|v| v.as_integer()),
        Some(3) // truncated
    );

    // Test double conversion
    assert_eq!(
        feature.fields.get("value").and_then(|v| v.as_double()),
        Some(std::f64::consts::PI)
    );
    assert_eq!(
        feature.fields.get("count").and_then(|v| v.as_double()),
        Some(42.0)
    );
}

#[test]
fn test_ffi_layer_create_and_get_extent() {
    unsafe {
        // Create a mock dataset pointer (just needs to be non-null for this test)
        let mock_dataset = std::ptr::NonNull::<OxiGdalDataset>::dangling().as_ptr();

        // Create layer
        let layer_name = CString::new("test_layer").expect("valid string");
        let geom_type = CString::new("Point").expect("valid string");
        let mut layer_ptr: *mut OxiGdalLayer = ptr::null_mut();

        let result = oxigdal_dataset_create_layer(
            mock_dataset,
            layer_name.as_ptr(),
            geom_type.as_ptr(),
            4326,
            &mut layer_ptr,
        );
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(!layer_ptr.is_null());

        // Add some features using internal function
        let result = oxigdal_layer_add_feature_internal(layer_ptr, 1, 10.0, 20.0);
        assert_eq!(result, OxiGdalErrorCode::Success);

        let result = oxigdal_layer_add_feature_internal(layer_ptr, 2, 30.0, 40.0);
        assert_eq!(result, OxiGdalErrorCode::Success);

        // Get extent
        let mut bbox = OxiGdalBbox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        };
        let result = oxigdal_layer_get_extent(layer_ptr, &mut bbox);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert_eq!(bbox.min_x, 10.0);
        assert_eq!(bbox.min_y, 20.0);
        assert_eq!(bbox.max_x, 30.0);
        assert_eq!(bbox.max_y, 40.0);

        // Get feature count
        let mut count: i64 = 0;
        let result = oxigdal_layer_get_feature_count(layer_ptr, &mut count);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert_eq!(count, 2);

        // Clean up
        let result = oxigdal_layer_close(layer_ptr);
        assert_eq!(result, OxiGdalErrorCode::Success);
    }
}

#[test]
fn test_ffi_feature_iteration() {
    unsafe {
        let mock_dataset = std::ptr::NonNull::<OxiGdalDataset>::dangling().as_ptr();
        let layer_name = CString::new("test").expect("valid string");
        let geom_type = CString::new("Point").expect("valid string");
        let mut layer_ptr: *mut OxiGdalLayer = ptr::null_mut();

        let result = oxigdal_dataset_create_layer(
            mock_dataset,
            layer_name.as_ptr(),
            geom_type.as_ptr(),
            4326,
            &mut layer_ptr,
        );
        assert_eq!(result, OxiGdalErrorCode::Success);

        // Add features
        for i in 0..3 {
            let result =
                oxigdal_layer_add_feature_internal(layer_ptr, i, i as f64 * 10.0, i as f64 * 10.0);
            assert_eq!(result, OxiGdalErrorCode::Success);
        }

        // Reset reading
        let result = oxigdal_layer_reset_reading(layer_ptr);
        assert_eq!(result, OxiGdalErrorCode::Success);

        // Iterate through features
        let mut count = 0;
        loop {
            let mut feature_ptr: *mut OxiGdalFeature = ptr::null_mut();
            let result = oxigdal_layer_get_next_feature(layer_ptr, &mut feature_ptr);
            assert_eq!(result, OxiGdalErrorCode::Success);

            if feature_ptr.is_null() {
                break;
            }

            // Get FID
            let mut fid: i64 = 0;
            let result = oxigdal_feature_get_fid(feature_ptr, &mut fid);
            assert_eq!(result, OxiGdalErrorCode::Success);
            assert_eq!(fid, count);

            // Get WKT
            let wkt_ptr = oxigdal_feature_get_geometry_wkt(feature_ptr);
            assert!(!wkt_ptr.is_null());
            let wkt = CStr::from_ptr(wkt_ptr).to_str().expect("valid UTF-8");
            assert!(wkt.starts_with("POINT"));
            oxigdal_string_free(wkt_ptr);

            count += 1;
            let result = oxigdal_feature_free(feature_ptr);
            assert_eq!(result, OxiGdalErrorCode::Success);
        }
        assert_eq!(count, 3);

        let result = oxigdal_layer_close(layer_ptr);
        assert_eq!(result, OxiGdalErrorCode::Success);
    }
}

#[test]
fn test_ffi_get_feature_by_fid() {
    unsafe {
        let mock_dataset = std::ptr::NonNull::<OxiGdalDataset>::dangling().as_ptr();
        let layer_name = CString::new("test").expect("valid string");
        let geom_type = CString::new("Point").expect("valid string");
        let mut layer_ptr: *mut OxiGdalLayer = ptr::null_mut();

        oxigdal_dataset_create_layer(
            mock_dataset,
            layer_name.as_ptr(),
            geom_type.as_ptr(),
            4326,
            &mut layer_ptr,
        );

        // Add features with specific FIDs
        oxigdal_layer_add_feature_internal(layer_ptr, 100, 10.0, 20.0);
        oxigdal_layer_add_feature_internal(layer_ptr, 200, 30.0, 40.0);

        // Get existing feature
        let mut feature_ptr: *mut OxiGdalFeature = ptr::null_mut();
        let result = oxigdal_layer_get_feature(layer_ptr, 100, &mut feature_ptr);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(!feature_ptr.is_null());

        let mut fid: i64 = 0;
        oxigdal_feature_get_fid(feature_ptr, &mut fid);
        assert_eq!(fid, 100);
        oxigdal_feature_free(feature_ptr);

        // Try to get non-existing feature
        let mut missing_ptr: *mut OxiGdalFeature = ptr::null_mut();
        let result = oxigdal_layer_get_feature(layer_ptr, 999, &mut missing_ptr);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(missing_ptr.is_null());

        oxigdal_layer_close(layer_ptr);
    }
}

#[test]
fn test_ffi_spatial_filter() {
    unsafe {
        let mock_dataset = std::ptr::NonNull::<OxiGdalDataset>::dangling().as_ptr();
        let layer_name = CString::new("test").expect("valid string");
        let geom_type = CString::new("Point").expect("valid string");
        let mut layer_ptr: *mut OxiGdalLayer = ptr::null_mut();

        oxigdal_dataset_create_layer(
            mock_dataset,
            layer_name.as_ptr(),
            geom_type.as_ptr(),
            4326,
            &mut layer_ptr,
        );

        // Add features at different locations
        for i in 0..10 {
            oxigdal_layer_add_feature_internal(layer_ptr, i, i as f64 * 10.0, i as f64 * 10.0);
        }

        // Set spatial filter
        let bbox = OxiGdalBbox {
            min_x: 25.0,
            min_y: 25.0,
            max_x: 65.0,
            max_y: 65.0,
        };
        let result = oxigdal_layer_set_spatial_filter_bbox(layer_ptr, &bbox);
        assert_eq!(result, OxiGdalErrorCode::Success);

        // Reset and count filtered features
        oxigdal_layer_reset_reading(layer_ptr);
        let mut filtered_count = 0;
        loop {
            let mut feature_ptr: *mut OxiGdalFeature = ptr::null_mut();
            oxigdal_layer_get_next_feature(layer_ptr, &mut feature_ptr);
            if feature_ptr.is_null() {
                break;
            }
            filtered_count += 1;
            oxigdal_feature_free(feature_ptr);
        }
        // Should include points at (30,30), (40,40), (50,50), (60,60) = 4 points
        assert_eq!(filtered_count, 4);

        // Clear filter
        let result = oxigdal_layer_clear_spatial_filter(layer_ptr);
        assert_eq!(result, OxiGdalErrorCode::Success);

        // Count all features
        oxigdal_layer_reset_reading(layer_ptr);
        let mut total_count = 0;
        loop {
            let mut feature_ptr: *mut OxiGdalFeature = ptr::null_mut();
            oxigdal_layer_get_next_feature(layer_ptr, &mut feature_ptr);
            if feature_ptr.is_null() {
                break;
            }
            total_count += 1;
            oxigdal_feature_free(feature_ptr);
        }
        assert_eq!(total_count, 10);

        oxigdal_layer_close(layer_ptr);
    }
}

#[test]
fn test_multipoint_geometry() {
    let mp = FfiGeometry::MultiPoint {
        points: vec![(0.0, 0.0, None), (10.0, 10.0, None), (20.0, 5.0, None)],
    };

    assert_eq!(mp.geometry_type(), "MultiPoint");

    let bounds = mp.bounds();
    assert!(bounds.is_some());
    let (min_x, min_y, max_x, max_y) = bounds.expect("should have bounds");
    assert_eq!(min_x, 0.0);
    assert_eq!(min_y, 0.0);
    assert_eq!(max_x, 20.0);
    assert_eq!(max_y, 10.0);

    let wkt = mp.to_wkt();
    assert!(wkt.starts_with("MULTIPOINT"));
}

#[test]
fn test_polygon_geometry() {
    let polygon = FfiGeometry::Polygon {
        exterior: vec![
            (0.0, 0.0, None),
            (10.0, 0.0, None),
            (10.0, 10.0, None),
            (0.0, 10.0, None),
            (0.0, 0.0, None),
        ],
        interiors: vec![vec![
            (2.0, 2.0, None),
            (8.0, 2.0, None),
            (8.0, 8.0, None),
            (2.0, 8.0, None),
            (2.0, 2.0, None),
        ]],
    };

    assert_eq!(polygon.geometry_type(), "Polygon");

    let bounds = polygon.bounds();
    assert!(bounds.is_some());
    let (min_x, min_y, max_x, max_y) = bounds.expect("should have bounds");
    assert_eq!(min_x, 0.0);
    assert_eq!(min_y, 0.0);
    assert_eq!(max_x, 10.0);
    assert_eq!(max_y, 10.0);

    let wkt = polygon.to_wkt();
    assert!(wkt.starts_with("POLYGON"));
    assert!(wkt.contains("(0 0, 10 0, 10 10, 0 10, 0 0)"));
}

#[test]
fn test_layer_name_and_geom_type() {
    unsafe {
        let mock_dataset = std::ptr::NonNull::<OxiGdalDataset>::dangling().as_ptr();
        let layer_name = CString::new("my_layer").expect("valid string");
        let geom_type = CString::new("Polygon").expect("valid string");
        let mut layer_ptr: *mut OxiGdalLayer = ptr::null_mut();

        oxigdal_dataset_create_layer(
            mock_dataset,
            layer_name.as_ptr(),
            geom_type.as_ptr(),
            4326,
            &mut layer_ptr,
        );

        // Get layer name
        let name_ptr = oxigdal_layer_get_name(layer_ptr);
        assert!(!name_ptr.is_null());
        let name = CStr::from_ptr(name_ptr).to_str().expect("valid UTF-8");
        assert_eq!(name, "my_layer");
        oxigdal_string_free(name_ptr);

        // Get geometry type
        let type_ptr = oxigdal_layer_get_geom_type(layer_ptr);
        assert!(!type_ptr.is_null());
        let geom = CStr::from_ptr(type_ptr).to_str().expect("valid UTF-8");
        assert_eq!(geom, "Polygon");
        oxigdal_string_free(type_ptr);

        // Get EPSG
        let mut epsg: i32 = 0;
        let result = oxigdal_layer_get_srs_epsg(layer_ptr, &mut epsg);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert_eq!(epsg, 4326);

        oxigdal_layer_close(layer_ptr);
    }
}

#[test]
fn test_invalid_geometry_type() {
    unsafe {
        let mock_dataset = std::ptr::NonNull::<OxiGdalDataset>::dangling().as_ptr();
        let layer_name = CString::new("test").expect("valid string");
        let geom_type = CString::new("InvalidType").expect("valid string");
        let mut layer_ptr: *mut OxiGdalLayer = ptr::null_mut();

        let result = oxigdal_dataset_create_layer(
            mock_dataset,
            layer_name.as_ptr(),
            geom_type.as_ptr(),
            4326,
            &mut layer_ptr,
        );
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
        assert!(layer_ptr.is_null());
    }
}

#[test]
fn test_invalid_bbox() {
    unsafe {
        let mock_dataset = std::ptr::NonNull::<OxiGdalDataset>::dangling().as_ptr();
        let layer_name = CString::new("test").expect("valid string");
        let geom_type = CString::new("Point").expect("valid string");
        let mut layer_ptr: *mut OxiGdalLayer = ptr::null_mut();

        oxigdal_dataset_create_layer(
            mock_dataset,
            layer_name.as_ptr(),
            geom_type.as_ptr(),
            4326,
            &mut layer_ptr,
        );

        // Invalid bbox (min > max)
        let invalid_bbox = OxiGdalBbox {
            min_x: 100.0,
            min_y: 100.0,
            max_x: 0.0,
            max_y: 0.0,
        };
        let result = oxigdal_layer_set_spatial_filter_bbox(layer_ptr, &invalid_bbox);
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

        oxigdal_layer_close(layer_ptr);
    }
}
