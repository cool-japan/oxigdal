//! Dataset traits for unified geospatial data access
//!
//! This module provides object-safe traits for reading raster and vector datasets,
//! enabling polymorphic access to diverse geospatial data sources.
//!
//! # Design Principles
//!
//! All traits are **object-safe**: no generic methods, no `impl Trait` returns.
//! Consumers can hold `Box<dyn Dataset>`, `Box<dyn RasterDataset>`, or
//! `Box<dyn VectorDataset>` without knowing the concrete driver at compile time.
//!
//! # Example
//!
//! ```rust
//! use oxigeo_core::io::dataset::{Dataset, RasterDataset, VectorDataset, FieldType};
//!
//! fn describe(ds: &dyn Dataset) -> String {
//!     ds.description()
//! }
//! ```

use core::fmt;
use std::path::Path;
use std::string::String;

use crate::buffer::{RasterBuffer, RasterWindow};
use crate::error::Result;
use crate::types::{BoundingBox, GeoTransform, NoDataValue, RasterDataType};
use crate::vector::feature::Feature;

/// Describes the type of a vector field for schema introspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    /// Null / unknown type
    Null,
    /// Boolean
    Bool,
    /// Signed integer
    Integer,
    /// Unsigned integer
    UInteger,
    /// Floating-point
    Real,
    /// UTF-8 string
    String,
    /// Raw binary blob
    Blob,
    /// Calendar date
    Date,
    /// JSON object
    Object,
    /// Array of values
    Array,
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Null => "Null",
            Self::Bool => "Bool",
            Self::Integer => "Integer",
            Self::UInteger => "UInteger",
            Self::Real => "Real",
            Self::String => "String",
            Self::Blob => "Blob",
            Self::Date => "Date",
            Self::Object => "Object",
            Self::Array => "Array",
        };
        f.write_str(s)
    }
}

/// Base trait shared by all dataset types.
///
/// Provides metadata access common to raster and vector datasets.
/// This trait is object-safe: implementations may be held as `Box<dyn Dataset>`.
pub trait Dataset: Send + Sync {
    /// Returns the short name of the driver that opened this dataset.
    fn driver_name(&self) -> &'static str;

    /// Returns the filesystem path of this dataset, if it has one.
    fn path(&self) -> Option<&Path>;

    /// Returns the coordinate reference system as a WKT string, if known.
    fn crs(&self) -> Option<&str>;

    /// Returns a reference to the affine geotransform, if defined.
    fn geotransform(&self) -> Option<&GeoTransform>;

    /// Returns the spatial bounding box of the dataset, if determinable.
    fn bounds(&self) -> Option<BoundingBox>;

    /// Returns a human-readable description of the dataset.
    ///
    /// Defaults to `"<driver>: <path>"` when a path is available,
    /// or just `"<driver>"` otherwise.
    fn description(&self) -> String {
        match self.path() {
            Some(p) => format!("{}: {}", self.driver_name(), p.display()),
            None => self.driver_name().to_string(),
        }
    }
}

/// Trait for raster datasets — datasets composed of one or more pixel bands.
///
/// Extends [`Dataset`] with raster-specific access methods.
/// All implementations must also implement `Dataset`.
pub trait RasterDataset: Dataset {
    /// Returns the width of the raster in pixels.
    fn width(&self) -> u64;

    /// Returns the height of the raster in pixels.
    fn height(&self) -> u64;

    /// Returns the number of raster bands.
    fn band_count(&self) -> u32;

    /// Returns the pixel data type for the given band (1-based index).
    ///
    /// # Errors
    ///
    /// Returns an error if the band index is out of range.
    fn data_type(&self, band: u32) -> Result<RasterDataType>;

    /// Reads an entire band into a [`RasterBuffer`].
    ///
    /// # Arguments
    ///
    /// * `band` - 1-based band index.
    ///
    /// # Errors
    ///
    /// Returns an error if the band cannot be read.
    fn read_band(&mut self, band: u32) -> Result<RasterBuffer>;

    /// Reads a rectangular sub-window of a band into a [`RasterBuffer`].
    ///
    /// # Arguments
    ///
    /// * `band`   - 1-based band index.
    /// * `window` - Sub-region to read.
    ///
    /// # Errors
    ///
    /// Returns an error if the window is out of bounds or the read fails.
    fn read_window(&mut self, band: u32, window: RasterWindow) -> Result<RasterBuffer>;

    /// Returns the number of overview levels available for the given band.
    ///
    /// Returns `0` if no overviews are present (the default).
    fn overview_count(&self, _band: u32) -> u32 {
        0
    }

    /// Returns the `NoData` sentinel value for the given band.
    ///
    /// Returns [`NoDataValue::None`] if no `NoData` value is defined (the default).
    fn nodata(&self, _band: u32) -> NoDataValue {
        NoDataValue::None
    }
}

/// Trait for vector datasets — datasets composed of layers of geographic features.
///
/// Extends [`Dataset`] with vector-specific access methods.
/// All implementations must also implement `Dataset`.
///
/// # Object safety
///
/// The `features` and `features_in_bbox` methods return
/// `Box<dyn Iterator<…> + '_>`, preserving object safety while
/// allowing the iterator lifetime to be tied to the mutable borrow
/// of `self`.
pub trait VectorDataset: Dataset {
    /// Returns the number of layers in this dataset.
    fn layer_count(&self) -> u32;

    /// Returns the name of the layer at the given index, or `None` if out of range.
    fn layer_name(&self, idx: u32) -> Option<&str>;

    /// Returns the total feature count for the layer, or `None` if unknown.
    fn feature_count(&self, layer: u32) -> Option<u64>;

    /// Returns a boxed iterator over all features in the specified layer.
    ///
    /// # Arguments
    ///
    /// * `layer` - 0-based layer index.
    ///
    /// # Errors
    ///
    /// Returns an error if the layer index is out of range or iteration
    /// cannot begin.
    fn features(&mut self, layer: u32) -> Result<Box<dyn Iterator<Item = Result<Feature>> + '_>>;

    /// Returns a boxed iterator over features whose bounding box intersects `bbox`.
    ///
    /// # Arguments
    ///
    /// * `layer` - 0-based layer index.
    /// * `bbox`  - The spatial filter bounding box.
    ///
    /// # Errors
    ///
    /// Returns an error if the layer index is out of range or the
    /// spatial filter cannot be applied.
    fn features_in_bbox(
        &mut self,
        layer: u32,
        bbox: BoundingBox,
    ) -> Result<Box<dyn Iterator<Item = Result<Feature>> + '_>>;

    /// Returns the field schema for the given layer as a list of `(name, type)` pairs.
    ///
    /// # Errors
    ///
    /// Returns an error if the layer index is out of range or schema information
    /// is unavailable.
    fn schema(&self, layer: u32) -> Result<Vec<(String, FieldType)>>;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    // Compile-time object-safety checks — these lines must compile.
    fn _check_object_safety() {
        let _: Option<Box<dyn Dataset>> = None;
        let _: Option<Box<dyn RasterDataset>> = None;
        let _: Option<Box<dyn VectorDataset>> = None;
    }

    // ----------------------------------------------------------------
    // Minimal concrete implementations for testing
    // ----------------------------------------------------------------

    struct FakeDataset;

    impl Dataset for FakeDataset {
        fn driver_name(&self) -> &'static str {
            "fake"
        }
        fn path(&self) -> Option<&Path> {
            None
        }
        fn crs(&self) -> Option<&str> {
            None
        }
        fn geotransform(&self) -> Option<&GeoTransform> {
            None
        }
        fn bounds(&self) -> Option<BoundingBox> {
            None
        }
    }

    #[test]
    fn test_default_description_no_path() {
        let d = FakeDataset;
        assert_eq!(d.description(), "fake");
    }

    struct FakeDatasetWithPath {
        gt: GeoTransform,
        path: std::path::PathBuf,
    }

    impl Dataset for FakeDatasetWithPath {
        fn driver_name(&self) -> &'static str {
            "fake-driver"
        }
        fn path(&self) -> Option<&Path> {
            Some(self.path.as_path())
        }
        fn crs(&self) -> Option<&str> {
            Some("EPSG:4326")
        }
        fn geotransform(&self) -> Option<&GeoTransform> {
            Some(&self.gt)
        }
        fn bounds(&self) -> Option<BoundingBox> {
            BoundingBox::new(-180.0, -90.0, 180.0, 90.0).ok()
        }
    }

    #[test]
    fn test_default_description_with_path() {
        let test_path = std::env::temp_dir().join("oxigeo_dataset_test.tif");
        let d = FakeDatasetWithPath {
            gt: GeoTransform::north_up(-180.0, 90.0, 1.0, -1.0),
            path: test_path.clone(),
        };
        let desc = d.description();
        assert!(
            desc.contains("fake-driver"),
            "expected driver name in description: {desc}"
        );
        let expected_path_str = test_path.to_string_lossy();
        assert!(
            desc.contains(expected_path_str.as_ref()),
            "expected path in description: {desc}"
        );
    }

    // ----------------------------------------------------------------
    // FakeRaster
    // ----------------------------------------------------------------

    struct FakeRaster;

    impl Dataset for FakeRaster {
        fn driver_name(&self) -> &'static str {
            "fake-raster"
        }
        fn path(&self) -> Option<&Path> {
            None
        }
        fn crs(&self) -> Option<&str> {
            None
        }
        fn geotransform(&self) -> Option<&GeoTransform> {
            None
        }
        fn bounds(&self) -> Option<BoundingBox> {
            None
        }
    }

    impl RasterDataset for FakeRaster {
        fn width(&self) -> u64 {
            256
        }
        fn height(&self) -> u64 {
            256
        }
        fn band_count(&self) -> u32 {
            3
        }
        fn data_type(&self, _band: u32) -> Result<RasterDataType> {
            Ok(RasterDataType::UInt8)
        }
        fn read_band(&mut self, _band: u32) -> Result<RasterBuffer> {
            Ok(RasterBuffer::zeros(256, 256, RasterDataType::UInt8))
        }
        fn read_window(&mut self, _band: u32, _window: RasterWindow) -> Result<RasterBuffer> {
            Ok(RasterBuffer::zeros(1, 1, RasterDataType::UInt8))
        }
    }

    #[test]
    fn test_raster_dataset_defaults() {
        let r = FakeRaster;
        assert_eq!(r.overview_count(1), 0);
        assert_eq!(r.nodata(1), NoDataValue::None);
        assert_eq!(r.width(), 256);
        assert_eq!(r.height(), 256);
        assert_eq!(r.band_count(), 3);
    }

    // ----------------------------------------------------------------
    // FakeVector
    // ----------------------------------------------------------------

    struct FakeVector;

    impl Dataset for FakeVector {
        fn driver_name(&self) -> &'static str {
            "fake-vector"
        }
        fn path(&self) -> Option<&Path> {
            None
        }
        fn crs(&self) -> Option<&str> {
            None
        }
        fn geotransform(&self) -> Option<&GeoTransform> {
            None
        }
        fn bounds(&self) -> Option<BoundingBox> {
            None
        }
    }

    impl VectorDataset for FakeVector {
        fn layer_count(&self) -> u32 {
            1
        }
        fn layer_name(&self, idx: u32) -> Option<&str> {
            if idx == 0 { Some("layer0") } else { None }
        }
        fn feature_count(&self, _layer: u32) -> Option<u64> {
            Some(0)
        }
        fn features(
            &mut self,
            _layer: u32,
        ) -> Result<Box<dyn Iterator<Item = Result<Feature>> + '_>> {
            Ok(Box::new(std::iter::empty()))
        }
        fn features_in_bbox(
            &mut self,
            _layer: u32,
            _bbox: BoundingBox,
        ) -> Result<Box<dyn Iterator<Item = Result<Feature>> + '_>> {
            Ok(Box::new(std::iter::empty()))
        }
        fn schema(&self, layer: u32) -> Result<Vec<(String, FieldType)>> {
            if layer == 0 {
                Ok(vec![("name".to_string(), FieldType::String)])
            } else {
                Err(crate::error::OxiGeoError::OutOfBounds {
                    message: format!("layer {layer} out of range"),
                })
            }
        }
    }

    #[test]
    fn test_vector_dataset_layer_metadata() {
        let v = FakeVector;
        assert_eq!(v.layer_count(), 1);
        assert_eq!(v.layer_name(0), Some("layer0"));
        assert_eq!(v.layer_name(1), None);
        assert_eq!(v.feature_count(0), Some(0));
    }

    #[test]
    fn test_vector_dataset_schema() {
        let v = FakeVector;
        let schema = v.schema(0).expect("schema for layer 0");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].0, "name");
        assert_eq!(schema[0].1, FieldType::String);
    }

    #[test]
    fn test_display_fieldtype_each_variant() {
        assert_eq!(FieldType::Null.to_string(), "Null");
        assert_eq!(FieldType::Bool.to_string(), "Bool");
        assert_eq!(FieldType::Integer.to_string(), "Integer");
        assert_eq!(FieldType::UInteger.to_string(), "UInteger");
        assert_eq!(FieldType::Real.to_string(), "Real");
        assert_eq!(FieldType::String.to_string(), "String");
        assert_eq!(FieldType::Blob.to_string(), "Blob");
        assert_eq!(FieldType::Date.to_string(), "Date");
        assert_eq!(FieldType::Object.to_string(), "Object");
        assert_eq!(FieldType::Array.to_string(), "Array");
    }
}
