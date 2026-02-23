//! VRT source raster references and windowing

use crate::error::{Result, VrtError};
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Source filename with path resolution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceFilename {
    /// Path to source file (can be relative or absolute)
    pub path: PathBuf,
    /// Whether the path is relative to the VRT file
    pub relative_to_vrt: bool,
    /// Shared flag (for optimization hints)
    pub shared: bool,
}

impl SourceFilename {
    /// Creates a new source filename
    pub fn new<P: AsRef<Path>>(path: P, relative_to_vrt: bool) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            relative_to_vrt,
            shared: false,
        }
    }

    /// Creates a new absolute source filename
    pub fn absolute<P: AsRef<Path>>(path: P) -> Self {
        Self::new(path, false)
    }

    /// Creates a new relative source filename
    pub fn relative<P: AsRef<Path>>(path: P) -> Self {
        Self::new(path, true)
    }

    /// Sets the shared flag
    pub fn with_shared(mut self, shared: bool) -> Self {
        self.shared = shared;
        self
    }

    /// Resolves the path relative to a VRT file
    ///
    /// # Errors
    /// Returns an error if path resolution fails
    pub fn resolve<P: AsRef<Path>>(&self, vrt_path: P) -> Result<PathBuf> {
        if self.relative_to_vrt {
            let vrt_dir = vrt_path.as_ref().parent().ok_or_else(|| {
                VrtError::path_resolution(
                    self.path.display().to_string(),
                    "VRT path has no parent directory",
                )
            })?;
            Ok(vrt_dir.join(&self.path))
        } else {
            Ok(self.path.clone())
        }
    }
}

/// Rectangle in pixel space
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PixelRect {
    /// X offset in pixels
    pub x_off: u64,
    /// Y offset in pixels
    pub y_off: u64,
    /// Width in pixels
    pub x_size: u64,
    /// Height in pixels
    pub y_size: u64,
}

impl PixelRect {
    /// Creates a new pixel rectangle
    pub fn new(x_off: u64, y_off: u64, x_size: u64, y_size: u64) -> Self {
        Self {
            x_off,
            y_off,
            x_size,
            y_size,
        }
    }

    /// Checks if the rectangle is valid
    pub fn is_valid(&self) -> bool {
        self.x_size > 0 && self.y_size > 0
    }

    /// Checks if a point is contained in this rectangle
    pub fn contains(&self, x: u64, y: u64) -> bool {
        x >= self.x_off
            && x < self.x_off.saturating_add(self.x_size)
            && y >= self.y_off
            && y < self.y_off.saturating_add(self.y_size)
    }

    /// Computes the intersection with another rectangle
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let x1 = self.x_off.max(other.x_off);
        let y1 = self.y_off.max(other.y_off);
        let x2 = (self.x_off + self.x_size).min(other.x_off + other.x_size);
        let y2 = (self.y_off + self.y_size).min(other.y_off + other.y_size);

        if x2 > x1 && y2 > y1 {
            Some(Self::new(x1, y1, x2 - x1, y2 - y1))
        } else {
            None
        }
    }

    /// Checks if this rectangle intersects with another
    pub fn intersects(&self, other: &Self) -> bool {
        self.intersect(other).is_some()
    }
}

/// Source window configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceWindow {
    /// Source rectangle (in source pixel space)
    pub src_rect: PixelRect,
    /// Destination rectangle (in VRT pixel space)
    pub dst_rect: PixelRect,
}

impl SourceWindow {
    /// Creates a new source window
    pub fn new(src_rect: PixelRect, dst_rect: PixelRect) -> Self {
        Self { src_rect, dst_rect }
    }

    /// Creates a simple identity window
    pub fn identity(width: u64, height: u64) -> Self {
        let rect = PixelRect::new(0, 0, width, height);
        Self::new(rect, rect)
    }

    /// Validates the window configuration
    ///
    /// # Errors
    /// Returns an error if the window is invalid
    pub fn validate(&self) -> Result<()> {
        if !self.src_rect.is_valid() {
            return Err(VrtError::invalid_window("Source rectangle is invalid"));
        }
        if !self.dst_rect.is_valid() {
            return Err(VrtError::invalid_window("Destination rectangle is invalid"));
        }
        Ok(())
    }
}

/// VRT source configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VrtSource {
    /// Source filename
    pub filename: SourceFilename,
    /// Source band (1-based index in source file)
    pub source_band: usize,
    /// Source window (optional, defaults to full extent)
    pub window: Option<SourceWindow>,
    /// NoData value override
    pub nodata: Option<NoDataValue>,
    /// Data type override
    pub data_type: Option<RasterDataType>,
    /// Source properties (cached metadata)
    pub properties: Option<SourceProperties>,
}

impl VrtSource {
    /// Creates a new VRT source
    pub fn new(filename: SourceFilename, source_band: usize) -> Self {
        Self {
            filename,
            source_band,
            window: None,
            nodata: None,
            data_type: None,
            properties: None,
        }
    }

    /// Creates a simple VRT source with default settings
    pub fn simple<P: AsRef<Path>>(path: P, band: usize) -> Self {
        Self::new(SourceFilename::absolute(path), band)
    }

    /// Sets the source window
    pub fn with_window(mut self, window: SourceWindow) -> Self {
        self.window = Some(window);
        self
    }

    /// Sets the NoData value override
    pub fn with_nodata(mut self, nodata: NoDataValue) -> Self {
        self.nodata = Some(nodata);
        self
    }

    /// Sets the data type override
    pub fn with_data_type(mut self, data_type: RasterDataType) -> Self {
        self.data_type = Some(data_type);
        self
    }

    /// Sets the source properties
    pub fn with_properties(mut self, properties: SourceProperties) -> Self {
        self.properties = Some(properties);
        self
    }

    /// Validates the source configuration
    ///
    /// # Errors
    /// Returns an error if the source is invalid
    pub fn validate(&self) -> Result<()> {
        if self.source_band == 0 {
            return Err(VrtError::invalid_source("Source band must be >= 1"));
        }

        if let Some(ref window) = self.window {
            window.validate()?;
        }

        Ok(())
    }

    /// Gets the destination rectangle in VRT pixel space
    pub fn dst_rect(&self) -> Option<PixelRect> {
        self.window.as_ref().map(|w| w.dst_rect)
    }

    /// Gets the source rectangle in source pixel space
    pub fn src_rect(&self) -> Option<PixelRect> {
        self.window.as_ref().map(|w| w.src_rect)
    }
}

/// Cached source properties
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceProperties {
    /// Raster width
    pub width: u64,
    /// Raster height
    pub height: u64,
    /// Number of bands
    pub band_count: usize,
    /// Data type
    pub data_type: RasterDataType,
    /// GeoTransform
    pub geo_transform: Option<GeoTransform>,
    /// NoData value
    pub nodata: NoDataValue,
}

impl SourceProperties {
    /// Creates new source properties
    pub fn new(width: u64, height: u64, band_count: usize, data_type: RasterDataType) -> Self {
        Self {
            width,
            height,
            band_count,
            data_type,
            geo_transform: None,
            nodata: NoDataValue::None,
        }
    }

    /// Sets the GeoTransform
    pub fn with_geo_transform(mut self, geo_transform: GeoTransform) -> Self {
        self.geo_transform = Some(geo_transform);
        self
    }

    /// Sets the NoData value
    pub fn with_nodata(mut self, nodata: NoDataValue) -> Self {
        self.nodata = nodata;
        self
    }

    /// Validates that the source band exists
    ///
    /// # Errors
    /// Returns an error if the band is out of range
    pub fn validate_band(&self, band: usize) -> Result<()> {
        if band == 0 || band > self.band_count {
            return Err(VrtError::band_out_of_range(band, self.band_count));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_filename() {
        let filename = SourceFilename::absolute("/path/to/file.tif");
        assert_eq!(filename.path, PathBuf::from("/path/to/file.tif"));
        assert!(!filename.relative_to_vrt);

        let filename = SourceFilename::relative("data/file.tif");
        assert!(filename.relative_to_vrt);
    }

    #[test]
    fn test_pixel_rect() {
        let rect = PixelRect::new(10, 20, 100, 200);
        assert!(rect.is_valid());
        assert!(rect.contains(10, 20));
        assert!(rect.contains(50, 100));
        assert!(!rect.contains(5, 20));
        assert!(!rect.contains(200, 20));
    }

    #[test]
    fn test_rect_intersection() {
        let rect1 = PixelRect::new(0, 0, 100, 100);
        let rect2 = PixelRect::new(50, 50, 100, 100);

        let intersection = rect1.intersect(&rect2);
        assert!(intersection.is_some());
        let inter = intersection.expect("Should have intersection");
        assert_eq!(inter.x_off, 50);
        assert_eq!(inter.y_off, 50);
        assert_eq!(inter.x_size, 50);
        assert_eq!(inter.y_size, 50);

        let rect3 = PixelRect::new(200, 200, 100, 100);
        assert!(rect1.intersect(&rect3).is_none());
    }

    #[test]
    fn test_source_window() {
        let src_rect = PixelRect::new(0, 0, 512, 512);
        let dst_rect = PixelRect::new(100, 100, 512, 512);
        let window = SourceWindow::new(src_rect, dst_rect);

        assert!(window.validate().is_ok());
    }

    #[test]
    fn test_vrt_source() {
        let source = VrtSource::simple("/path/to/file.tif", 1);
        assert_eq!(source.source_band, 1);
        assert!(source.validate().is_ok());

        let invalid_source = VrtSource::simple("/path/to/file.tif", 0);
        assert!(invalid_source.validate().is_err());
    }

    #[test]
    fn test_source_properties() {
        let props = SourceProperties::new(512, 512, 3, RasterDataType::UInt8);
        assert_eq!(props.width, 512);
        assert_eq!(props.band_count, 3);

        assert!(props.validate_band(1).is_ok());
        assert!(props.validate_band(3).is_ok());
        assert!(props.validate_band(0).is_err());
        assert!(props.validate_band(4).is_err());
    }
}
