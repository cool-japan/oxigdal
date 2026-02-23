//! Layer operations and FFI functions for vector data.
//!
//! Provides C-compatible functions for working with vector layers and features.

use super::super::types::*;
use super::feature::{FeatureHandle, FfiFeature, FieldValue};
use super::geometry::{FfiGeometry, merge_bounds};
use crate::{check_null, deref_ptr, deref_ptr_mut};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

/// Internal layer handle with feature storage and iteration state.
pub struct LayerHandle {
    /// Layer name
    name: String,
    /// Geometry type for this layer
    geom_type: String,
    /// EPSG code for spatial reference
    srs_epsg: i32,
    /// Features in the layer
    features: Vec<FfiFeature>,
    /// Current cursor position for iteration
    cursor: usize,
    /// Spatial filter (if set)
    spatial_filter: Option<OxiGdalBbox>,
    /// Cached extent
    extent_cache: Option<OxiGdalBbox>,
}

impl LayerHandle {
    /// Creates a new empty layer.
    #[must_use]
    pub fn new(name: String, geom_type: String, srs_epsg: i32) -> Self {
        Self {
            name,
            geom_type,
            srs_epsg,
            features: Vec::new(),
            cursor: 0,
            spatial_filter: None,
            extent_cache: None,
        }
    }

    /// Returns the number of features in the layer.
    #[must_use]
    pub fn feature_count(&self) -> i64 {
        self.features.len() as i64
    }

    /// Calculates the extent from all features.
    #[must_use]
    pub fn calculate_extent(&self) -> OxiGdalBbox {
        if let Some(cached) = &self.extent_cache {
            return *cached;
        }

        let bounds = merge_bounds(self.features.iter().filter_map(FfiFeature::bounds));

        if let Some((min_x, min_y, max_x, max_y)) = bounds {
            OxiGdalBbox {
                min_x,
                min_y,
                max_x,
                max_y,
            }
        } else {
            OxiGdalBbox {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 0.0,
                max_y: 0.0,
            }
        }
    }

    /// Resets the cursor to the beginning.
    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
    }

    /// Gets the next feature that matches the current filter.
    pub fn next_feature(&mut self) -> Option<&FfiFeature> {
        while self.cursor < self.features.len() {
            let feature = &self.features[self.cursor];
            self.cursor += 1;

            // Check spatial filter if set
            if let Some(ref filter) = self.spatial_filter {
                if !feature.intersects_bbox(filter) {
                    continue;
                }
            }

            return Some(feature);
        }
        None
    }

    /// Gets a feature by FID.
    #[must_use]
    pub fn get_feature_by_fid(&self, fid: i64) -> Option<&FfiFeature> {
        self.features.iter().find(|f| f.fid == fid)
    }

    /// Adds a feature to the layer.
    pub fn add_feature(&mut self, feature: FfiFeature) {
        self.features.push(feature);
        self.extent_cache = None; // Invalidate cache
    }

    /// Sets the spatial filter.
    pub fn set_spatial_filter(&mut self, bbox: OxiGdalBbox) {
        self.spatial_filter = Some(bbox);
    }

    /// Clears the spatial filter.
    pub fn clear_spatial_filter(&mut self) {
        self.spatial_filter = None;
    }

    /// Returns the layer name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the geometry type.
    #[must_use]
    pub fn geom_type(&self) -> &str {
        &self.geom_type
    }

    /// Returns the EPSG code.
    #[must_use]
    pub fn srs_epsg(&self) -> i32 {
        self.srs_epsg
    }
}

/// Opens a vector layer from a dataset.
///
/// # Parameters
/// - `dataset`: Dataset handle
/// - `layer_name`: Layer name (null-terminated string)
/// - `out_layer`: Output layer handle
///
/// # Safety
/// - All pointers must be valid
/// - Caller must call `oxigdal_layer_close` when done
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_open_layer(
    dataset: *const OxiGdalDataset,
    layer_name: *const c_char,
    out_layer: *mut *mut OxiGdalLayer,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(out_layer, "out_layer");

    let name = if layer_name.is_null() {
        String::new()
    } else {
        unsafe {
            match CStr::from_ptr(layer_name).to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    crate::ffi::error::set_last_error("Invalid UTF-8 in layer name".to_string());
                    return OxiGdalErrorCode::InvalidUtf8;
                }
            }
        }
    };

    let handle = Box::new(LayerHandle::new(name, "Unknown".to_string(), 0));

    unsafe {
        *out_layer = Box::into_raw(handle) as *mut OxiGdalLayer;
    }
    OxiGdalErrorCode::Success
}

/// Closes a layer and frees resources.
///
/// # Safety
/// - layer must be a valid handle from oxigdal_dataset_open_layer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_close(layer: *mut OxiGdalLayer) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    unsafe {
        drop(Box::from_raw(layer as *mut LayerHandle));
    }
    OxiGdalErrorCode::Success
}

/// Gets the number of features in a layer.
///
/// # Safety
/// - layer must be a valid handle
/// - out_count must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_get_feature_count(
    layer: *const OxiGdalLayer,
    out_count: *mut i64,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(out_count, "out_count");

    unsafe {
        let handle = deref_ptr!(layer, LayerHandle, "layer");
        *out_count = handle.feature_count();
    }

    OxiGdalErrorCode::Success
}

/// Gets the spatial extent of a layer.
///
/// Calculates the bounding box that encompasses all features in the layer.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_get_extent(
    layer: *const OxiGdalLayer,
    out_bbox: *mut OxiGdalBbox,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(out_bbox, "out_bbox");

    unsafe {
        let handle = deref_ptr!(layer, LayerHandle, "layer");
        *out_bbox = handle.calculate_extent();
    }

    OxiGdalErrorCode::Success
}

/// Resets the layer read cursor to the beginning.
///
/// After calling this function, the next call to `oxigdal_layer_get_next_feature`
/// will return the first feature in the layer.
///
/// # Safety
/// - layer must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_reset_reading(layer: *mut OxiGdalLayer) -> OxiGdalErrorCode {
    check_null!(layer, "layer");

    unsafe {
        let handle = &mut *(layer as *mut LayerHandle);
        handle.reset_cursor();
    }

    OxiGdalErrorCode::Success
}

/// Reads the next feature from a layer.
///
/// This function advances the internal cursor and returns the next feature.
/// If a spatial filter is set, only features that intersect the filter are returned.
///
/// # Parameters
/// - `layer`: Layer handle
/// - `out_feature`: Output feature handle (null if no more features)
///
/// # Safety
/// - All pointers must be valid
/// - Caller must call `oxigdal_feature_free` when done with each feature
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_get_next_feature(
    layer: *mut OxiGdalLayer,
    out_feature: *mut *mut OxiGdalFeature,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(out_feature, "out_feature");

    unsafe {
        let handle = &mut *(layer as *mut LayerHandle);

        if let Some(feature) = handle.next_feature() {
            // Clone the feature data for the handle
            let feature_handle = Box::new(FeatureHandle::new(feature.clone()));
            *out_feature = Box::into_raw(feature_handle) as *mut OxiGdalFeature;
        } else {
            *out_feature = ptr::null_mut();
        }
    }

    OxiGdalErrorCode::Success
}

/// Gets a feature by its feature ID.
///
/// # Safety
/// - All pointers must be valid
/// - Caller must call `oxigdal_feature_free` when done
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_get_feature(
    layer: *const OxiGdalLayer,
    fid: i64,
    out_feature: *mut *mut OxiGdalFeature,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(out_feature, "out_feature");

    if fid < 0 {
        crate::ffi::error::set_last_error("Invalid feature ID".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    unsafe {
        let handle = deref_ptr!(layer, LayerHandle, "layer");

        if let Some(feature) = handle.get_feature_by_fid(fid) {
            let feature_handle = Box::new(FeatureHandle::new(feature.clone()));
            *out_feature = Box::into_raw(feature_handle) as *mut OxiGdalFeature;
        } else {
            *out_feature = ptr::null_mut();
        }
    }

    OxiGdalErrorCode::Success
}

/// Frees a feature handle.
///
/// # Safety
/// - feature must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_feature_free(feature: *mut OxiGdalFeature) -> OxiGdalErrorCode {
    check_null!(feature, "feature");
    unsafe {
        drop(Box::from_raw(feature as *mut FeatureHandle));
    }
    OxiGdalErrorCode::Success
}

/// Gets the FID of a feature.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_feature_get_fid(
    feature: *const OxiGdalFeature,
    out_fid: *mut i64,
) -> OxiGdalErrorCode {
    check_null!(feature, "feature");
    check_null!(out_fid, "out_fid");

    unsafe {
        let handle = deref_ptr!(feature, FeatureHandle, "feature");
        *out_fid = handle.inner().fid;
    }

    OxiGdalErrorCode::Success
}

/// Gets the geometry of a feature as WKT (Well-Known Text).
///
/// Converts the feature's geometry to a WKT string representation.
/// Supports all geometry types: Point, LineString, Polygon, MultiPoint,
/// MultiLineString, MultiPolygon, and GeometryCollection.
///
/// # Returns
/// WKT string (caller must free with oxigdal_string_free)
/// Returns NULL if the feature has no geometry.
///
/// # Safety
/// - feature must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_feature_get_geometry_wkt(
    feature: *const OxiGdalFeature,
) -> *mut c_char {
    if feature.is_null() {
        crate::ffi::error::set_last_error("Null feature pointer".to_string());
        return ptr::null_mut();
    }

    unsafe {
        let handle = &*(feature as *const FeatureHandle);

        let wkt = match &handle.inner().geometry {
            Some(geom) => geom.to_wkt(),
            None => {
                // Return empty geometry indicator
                "GEOMETRYCOLLECTION EMPTY".to_string()
            }
        };

        match CString::new(wkt) {
            Ok(s) => s.into_raw(),
            Err(_) => {
                crate::ffi::error::set_last_error("Failed to create WKT string".to_string());
                ptr::null_mut()
            }
        }
    }
}

/// Gets a field value from a feature as a string.
///
/// Any field type can be retrieved as a string - integers and doubles
/// will be converted to their string representation.
///
/// # Parameters
/// - `feature`: Feature handle
/// - `field_name`: Field name
///
/// # Returns
/// Field value as string (caller must free with oxigdal_string_free)
/// Returns empty string if field doesn't exist.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_feature_get_field_as_string(
    feature: *const OxiGdalFeature,
    field_name: *const c_char,
) -> *mut c_char {
    if feature.is_null() || field_name.is_null() {
        crate::ffi::error::set_last_error("Null pointer provided".to_string());
        return ptr::null_mut();
    }

    let field = unsafe {
        match CStr::from_ptr(field_name).to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid UTF-8 in field name".to_string());
                return ptr::null_mut();
            }
        }
    };

    unsafe {
        let handle = &*(feature as *const FeatureHandle);

        let value_str = match handle.inner().fields.get(field) {
            Some(value) => value.to_string_value(),
            None => String::new(),
        };

        match CString::new(value_str) {
            Ok(s) => s.into_raw(),
            Err(_) => {
                crate::ffi::error::set_last_error("Failed to create string".to_string());
                ptr::null_mut()
            }
        }
    }
}

/// Gets a field value as an integer.
///
/// If the field contains a string, it will attempt to parse it as an integer.
/// Doubles will be truncated to integers.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_feature_get_field_as_integer(
    feature: *const OxiGdalFeature,
    field_name: *const c_char,
    out_value: *mut i64,
) -> OxiGdalErrorCode {
    check_null!(feature, "feature");
    check_null!(field_name, "field_name");
    check_null!(out_value, "out_value");

    let field = unsafe {
        match CStr::from_ptr(field_name).to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid UTF-8 in field name".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    unsafe {
        let handle = deref_ptr!(feature, FeatureHandle, "feature");

        let value = match handle.inner().fields.get(field) {
            Some(field_value) => field_value.as_integer().unwrap_or(0),
            None => 0,
        };

        *out_value = value;
    }

    OxiGdalErrorCode::Success
}

/// Gets a field value as a double.
///
/// If the field contains a string, it will attempt to parse it as a double.
/// Integers will be converted to doubles.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_feature_get_field_as_double(
    feature: *const OxiGdalFeature,
    field_name: *const c_char,
    out_value: *mut f64,
) -> OxiGdalErrorCode {
    check_null!(feature, "feature");
    check_null!(field_name, "field_name");
    check_null!(out_value, "out_value");

    let field = unsafe {
        match CStr::from_ptr(field_name).to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid UTF-8 in field name".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    unsafe {
        let handle = deref_ptr!(feature, FeatureHandle, "feature");

        let value = match handle.inner().fields.get(field) {
            Some(field_value) => field_value.as_double().unwrap_or(0.0),
            None => 0.0,
        };

        *out_value = value;
    }

    OxiGdalErrorCode::Success
}

/// Creates a new layer in a dataset.
///
/// The layer is created with the specified geometry type and spatial reference.
///
/// # Parameters
/// - `dataset`: Dataset handle
/// - `layer_name`: Name for the new layer
/// - `geom_type`: Geometry type name (e.g., "Point", "LineString", "Polygon")
/// - `srs_epsg`: EPSG code for spatial reference (0 for none)
/// - `out_layer`: Output layer handle
///
/// # Safety
/// - All pointers must be valid
/// - Caller must call `oxigdal_layer_close` when done
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_create_layer(
    dataset: *mut OxiGdalDataset,
    layer_name: *const c_char,
    geom_type: *const c_char,
    srs_epsg: i32,
    out_layer: *mut *mut OxiGdalLayer,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(layer_name, "layer_name");
    check_null!(out_layer, "out_layer");

    let name = unsafe {
        match CStr::from_ptr(layer_name).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid UTF-8 in layer name".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    let geom = if geom_type.is_null() {
        "Unknown".to_string()
    } else {
        unsafe {
            match CStr::from_ptr(geom_type).to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    crate::ffi::error::set_last_error("Invalid UTF-8 in geometry type".to_string());
                    return OxiGdalErrorCode::InvalidUtf8;
                }
            }
        }
    };

    // Validate geometry type
    let valid_geom_types = [
        "Unknown",
        "Point",
        "LineString",
        "Polygon",
        "MultiPoint",
        "MultiLineString",
        "MultiPolygon",
        "GeometryCollection",
    ];

    if !valid_geom_types.contains(&geom.as_str()) {
        crate::ffi::error::set_last_error(format!("Invalid geometry type: {}", geom));
        return OxiGdalErrorCode::InvalidArgument;
    }

    if srs_epsg < 0 {
        crate::ffi::error::set_last_error("Invalid EPSG code".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let handle = Box::new(LayerHandle::new(name, geom, srs_epsg));

    unsafe {
        *out_layer = Box::into_raw(handle) as *mut OxiGdalLayer;
    }
    OxiGdalErrorCode::Success
}

/// Sets a spatial filter on a layer using a bounding box.
///
/// When a spatial filter is set, only features whose geometry intersects
/// the bounding box will be returned by `oxigdal_layer_get_next_feature`.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_set_spatial_filter_bbox(
    layer: *mut OxiGdalLayer,
    bbox: *const OxiGdalBbox,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(bbox, "bbox");

    unsafe {
        let handle = &mut *(layer as *mut LayerHandle);
        let bounds = &*(bbox);

        // Validate bbox
        if bounds.min_x > bounds.max_x || bounds.min_y > bounds.max_y {
            crate::ffi::error::set_last_error("Invalid bounding box: min > max".to_string());
            return OxiGdalErrorCode::InvalidArgument;
        }

        handle.set_spatial_filter(*bounds);
    }

    OxiGdalErrorCode::Success
}

/// Clears any spatial filter on a layer.
///
/// After calling this, all features will be returned by iteration.
///
/// # Safety
/// - layer must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_clear_spatial_filter(
    layer: *mut OxiGdalLayer,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");

    unsafe {
        let handle = &mut *(layer as *mut LayerHandle);
        handle.clear_spatial_filter();
    }

    OxiGdalErrorCode::Success
}

/// Gets the geometry type of a layer.
///
/// # Returns
/// Geometry type string (caller must free with oxigdal_string_free)
///
/// # Safety
/// - layer must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_get_geom_type(layer: *const OxiGdalLayer) -> *mut c_char {
    if layer.is_null() {
        crate::ffi::error::set_last_error("Null layer pointer".to_string());
        return ptr::null_mut();
    }

    unsafe {
        let handle = &*(layer as *const LayerHandle);

        match CString::new(handle.geom_type()) {
            Ok(s) => s.into_raw(),
            Err(_) => {
                crate::ffi::error::set_last_error(
                    "Failed to create geometry type string".to_string(),
                );
                ptr::null_mut()
            }
        }
    }
}

/// Gets the EPSG code of a layer's spatial reference.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_get_srs_epsg(
    layer: *const OxiGdalLayer,
    out_epsg: *mut i32,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(out_epsg, "out_epsg");

    unsafe {
        let handle = deref_ptr!(layer, LayerHandle, "layer");
        *out_epsg = handle.srs_epsg();
    }

    OxiGdalErrorCode::Success
}

/// Gets the name of a layer.
///
/// # Returns
/// Layer name string (caller must free with oxigdal_string_free)
///
/// # Safety
/// - layer must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_get_name(layer: *const OxiGdalLayer) -> *mut c_char {
    if layer.is_null() {
        crate::ffi::error::set_last_error("Null layer pointer".to_string());
        return ptr::null_mut();
    }

    unsafe {
        let handle = &*(layer as *const LayerHandle);

        match CString::new(handle.name()) {
            Ok(s) => s.into_raw(),
            Err(_) => {
                crate::ffi::error::set_last_error("Failed to create layer name string".to_string());
                ptr::null_mut()
            }
        }
    }
}

/// Adds a feature to a layer (for testing/internal use).
///
/// # Safety
/// - layer must be a valid handle
/// - This is an internal function for testing
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_layer_add_feature_internal(
    layer: *mut OxiGdalLayer,
    fid: i64,
    x: f64,
    y: f64,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");

    unsafe {
        let handle = &mut *(layer as *mut LayerHandle);

        let mut feature = FfiFeature::new(fid);
        feature.geometry = Some(FfiGeometry::Point { x, y, z: None });
        handle.add_feature(feature);
    }

    OxiGdalErrorCode::Success
}

/// Gets the geometry type of a feature.
///
/// # Returns
/// Geometry type string (caller must free with oxigdal_string_free)
///
/// # Safety
/// - feature must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_feature_get_geometry_type(
    feature: *const OxiGdalFeature,
) -> *mut c_char {
    if feature.is_null() {
        crate::ffi::error::set_last_error("Null feature pointer".to_string());
        return ptr::null_mut();
    }

    unsafe {
        let handle = &*(feature as *const FeatureHandle);

        let type_str = match &handle.inner().geometry {
            Some(geom) => geom.geometry_type(),
            None => "None",
        };

        match CString::new(type_str) {
            Ok(s) => s.into_raw(),
            Err(_) => {
                crate::ffi::error::set_last_error(
                    "Failed to create geometry type string".to_string(),
                );
                ptr::null_mut()
            }
        }
    }
}
