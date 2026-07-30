//! [`DatasetInfo`] — the format-agnostic metadata descriptor.
//!
//! Every opened dataset carries one: [`Dataset::info`](crate::Dataset::info)
//! returns a reference to it, and each
//! [`OpenedDataset`](crate::open::OpenedDataset) variant wraps one.  It is the
//! single place where "what is this file?" is answered without touching pixels
//! or features.

use crate::{BoundingBox, DatasetFormat, GeoTransform, RasterDataType};

/// Basic dataset metadata — analogous to `GDALDataset` info.
///
/// # Constructing a `DatasetInfo`
///
/// This struct is `#[non_exhaustive]`, so new fields can be added in future
/// releases without breaking downstream code.  The cost of that guarantee is
/// that struct-expression construction is unavailable outside this crate —
/// **including** the functional-update form:
///
/// ```rust,compile_fail
/// use oxigeo::{DatasetFormat, DatasetInfo};
///
/// // error[E0639]: cannot create non-exhaustive struct using struct expression
/// let info = DatasetInfo {
///     format: DatasetFormat::GeoTiff,
///     ..DatasetInfo::default()
/// };
/// ```
///
/// Start from [`DatasetInfo::default()`](Default::default) — an
/// "everything unknown" descriptor — and assign the fields you care about.
/// Fields you do not touch keep their default, so adding a field upstream can
/// never break this pattern:
///
/// ```rust
/// use oxigeo::{DatasetFormat, DatasetInfo, RasterDataType};
///
/// let mut info = DatasetInfo::default();
/// info.format = DatasetFormat::GeoTiff;
/// info.width = Some(1024);
/// info.height = Some(768);
/// info.band_count = 3;
/// info.data_type = Some(RasterDataType::UInt16);
///
/// assert_eq!(info.layer_count, 0); // untouched fields keep their default
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DatasetInfo {
    /// Detected format
    pub format: DatasetFormat,
    /// Filesystem path this dataset was opened from, if known.
    ///
    /// `None` for cloud/remote datasets and programmatically-created datasets.
    pub path: Option<String>,
    /// Width in pixels (raster) or `None` (vector-only)
    pub width: Option<u32>,
    /// Height in pixels (raster) or `None` (vector-only)
    pub height: Option<u32>,
    /// Number of raster bands
    pub band_count: u32,
    /// Number of vector layers
    pub layer_count: u32,
    /// Coordinate reference system (WKT, EPSG code, or PROJ string)
    pub crs: Option<String>,
    /// Geotransform: `[origin_x, pixel_width, rotation_x, origin_y, rotation_y, pixel_height]`
    pub geotransform: Option<GeoTransform>,
    /// Number of features in the primary vector layer.
    ///
    /// `None` when the format does not support cheap feature counting (e.g. streaming formats).
    pub feature_count: Option<u64>,
    /// Spatial extent of the dataset in the dataset's native CRS.
    ///
    /// Computed from the geotransform for raster datasets, or from the GeoJSON `bbox`
    /// field for vector datasets.  `None` when extent information is unavailable.
    pub bounds: Option<BoundingBox>,
    /// Element type of the raster bands, read from the file header at open time.
    ///
    /// `None` for vector datasets (which have no pixels at all), and for raster
    /// formats whose header probe is not wired up yet or whose declared sample
    /// layout has no [`RasterDataType`] equivalent.
    ///
    /// Knowing the pixel type *before* reading is what lets a caller size a
    /// destination buffer correctly — the alternative, reading a whole band just
    /// to call `RasterBuffer::data_type()`, defeats the purpose of a typed,
    /// pre-allocated read (cool-japan/oxigeo#14).
    pub data_type: Option<RasterDataType>,
}

impl Default for DatasetInfo {
    /// An "everything unknown" descriptor: format [`DatasetFormat::Unknown`] and
    /// every other field empty.
    ///
    /// This is the entry point for building a `DatasetInfo` outside this crate:
    /// take the default and assign the fields you know (see the type-level
    /// documentation).  Because the struct is `#[non_exhaustive]`, later field
    /// additions extend the default rather than breaking call sites.
    fn default() -> Self {
        Self {
            format: DatasetFormat::Unknown,
            path: None,
            width: None,
            height: None,
            band_count: 0,
            layer_count: 0,
            crs: None,
            geotransform: None,
            feature_count: None,
            bounds: None,
            data_type: None,
        }
    }
}
