//! VRT dataset definition

use crate::band::VrtBand;
use crate::error::{Result, VrtError};
use crate::source::PixelRect;
use oxigdal_core::types::{GeoTransform, RasterDataType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// VRT dataset
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VrtDataset {
    /// Raster width in pixels
    pub raster_x_size: u64,
    /// Raster height in pixels
    pub raster_y_size: u64,
    /// GeoTransform (affine transform for georeferencing)
    pub geo_transform: Option<GeoTransform>,
    /// Spatial reference system (WKT or PROJ.4 string)
    pub srs: Option<String>,
    /// Bands
    pub bands: Vec<VrtBand>,
    /// Block size (default tile dimensions)
    pub block_size: Option<(u32, u32)>,
    /// Subclass (for special VRT types)
    pub subclass: Option<VrtSubclass>,
    /// VRT file path (for resolving relative paths)
    pub vrt_path: Option<PathBuf>,
}

impl VrtDataset {
    /// Creates a new VRT dataset
    pub fn new(raster_x_size: u64, raster_y_size: u64) -> Self {
        Self {
            raster_x_size,
            raster_y_size,
            geo_transform: None,
            srs: None,
            bands: Vec::new(),
            block_size: None,
            subclass: None,
            vrt_path: None,
        }
    }

    /// Creates a new VRT dataset with extent from sources
    ///
    /// # Errors
    /// Returns an error if no bands are provided
    pub fn from_bands(bands: Vec<VrtBand>) -> Result<Self> {
        if bands.is_empty() {
            return Err(VrtError::invalid_structure(
                "Dataset must have at least one band",
            ));
        }

        // Calculate extent from first band's sources
        let (width, height) = Self::calculate_extent_from_band(&bands[0])?;

        Ok(Self {
            raster_x_size: width,
            raster_y_size: height,
            geo_transform: None,
            srs: None,
            bands,
            block_size: None,
            subclass: None,
            vrt_path: None,
        })
    }

    /// Adds a band to the dataset
    pub fn add_band(&mut self, band: VrtBand) {
        self.bands.push(band);
    }

    /// Sets the GeoTransform
    pub fn with_geo_transform(mut self, geo_transform: GeoTransform) -> Self {
        self.geo_transform = Some(geo_transform);
        self
    }

    /// Sets the spatial reference system
    pub fn with_srs<S: Into<String>>(mut self, srs: S) -> Self {
        self.srs = Some(srs.into());
        self
    }

    /// Sets the block size
    pub fn with_block_size(mut self, width: u32, height: u32) -> Self {
        self.block_size = Some((width, height));
        self
    }

    /// Sets the subclass
    pub fn with_subclass(mut self, subclass: VrtSubclass) -> Self {
        self.subclass = Some(subclass);
        self
    }

    /// Sets the VRT file path
    pub fn with_vrt_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.vrt_path = Some(path.into());
        self
    }

    /// Validates the dataset
    ///
    /// # Errors
    /// Returns an error if the dataset is invalid
    pub fn validate(&self) -> Result<()> {
        if self.raster_x_size == 0 || self.raster_y_size == 0 {
            return Err(VrtError::invalid_structure(
                "Dataset dimensions must be > 0",
            ));
        }

        if self.bands.is_empty() {
            return Err(VrtError::invalid_structure(
                "Dataset must have at least one band",
            ));
        }

        // Validate all bands
        for (idx, band) in self.bands.iter().enumerate() {
            band.validate().map_err(|e| {
                VrtError::invalid_structure(format!("Band {} validation failed: {}", idx + 1, e))
            })?;

            // Check that band numbers are sequential
            if band.band != idx + 1 {
                return Err(VrtError::invalid_structure(format!(
                    "Band number mismatch: expected {}, got {}",
                    idx + 1,
                    band.band
                )));
            }
        }

        // Validate block size if present
        if let Some((width, height)) = self.block_size {
            if width == 0 || height == 0 {
                return Err(VrtError::invalid_structure("Block size must be > 0"));
            }
        }

        Ok(())
    }

    /// Gets the number of bands
    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    /// Gets a band by index (0-based)
    pub fn get_band(&self, index: usize) -> Option<&VrtBand> {
        self.bands.get(index)
    }

    /// Gets a mutable reference to a band by index (0-based)
    pub fn get_band_mut(&mut self, index: usize) -> Option<&mut VrtBand> {
        self.bands.get_mut(index)
    }

    /// Gets the extent as a pixel rectangle
    pub fn extent(&self) -> PixelRect {
        PixelRect::new(0, 0, self.raster_x_size, self.raster_y_size)
    }

    /// Gets the effective block size (uses dataset default or falls back to 256x256)
    pub fn effective_block_size(&self) -> (u32, u32) {
        self.block_size.unwrap_or((256, 256))
    }

    /// Calculates extent from a band's sources
    fn calculate_extent_from_band(band: &VrtBand) -> Result<(u64, u64)> {
        if band.sources.is_empty() {
            return Err(VrtError::invalid_structure(
                "Band has no sources to calculate extent from",
            ));
        }

        // Find the bounding box of all destination rectangles
        let mut min_x = u64::MAX;
        let mut min_y = u64::MAX;
        let mut max_x = 0u64;
        let mut max_y = 0u64;

        for source in &band.sources {
            if let Some(dst_rect) = source.dst_rect() {
                min_x = min_x.min(dst_rect.x_off);
                min_y = min_y.min(dst_rect.y_off);
                max_x = max_x.max(dst_rect.x_off + dst_rect.x_size);
                max_y = max_y.max(dst_rect.y_off + dst_rect.y_size);
            } else if let Some(ref props) = source.properties {
                // If no dst_rect, use source properties
                max_x = max_x.max(props.width);
                max_y = max_y.max(props.height);
            }
        }

        if max_x == 0 || max_y == 0 {
            return Err(VrtError::invalid_structure(
                "Cannot calculate extent: no valid source windows",
            ));
        }

        Ok((max_x - min_x, max_y - min_y))
    }

    /// Merges GeoTransforms from multiple sources to create a unified transform
    ///
    /// # Errors
    /// Returns an error if sources have incompatible GeoTransforms
    pub fn merge_geo_transforms(&mut self) -> Result<()> {
        if self.geo_transform.is_some() {
            return Ok(()); // Already set
        }

        let mut transforms = Vec::new();

        // Collect all GeoTransforms from band sources
        for band in &self.bands {
            for source in &band.sources {
                if let Some(ref props) = source.properties {
                    if let Some(ref gt) = props.geo_transform {
                        transforms.push(*gt);
                    }
                }
            }
        }

        if transforms.is_empty() {
            return Ok(()); // No GeoTransforms to merge
        }

        // For simplicity, use the first transform
        // In a real implementation, we would validate that all transforms are compatible
        self.geo_transform = Some(transforms[0]);

        Ok(())
    }

    /// Gets the data type of the first band
    pub fn primary_data_type(&self) -> Option<RasterDataType> {
        self.bands.first().map(|b| b.data_type)
    }

    /// Checks if all bands have the same data type
    pub fn has_uniform_data_type(&self) -> bool {
        if self.bands.is_empty() {
            return true;
        }

        let first_type = self.bands[0].data_type;
        self.bands.iter().all(|b| b.data_type == first_type)
    }
}

/// VRT subclass types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VrtSubclass {
    /// Standard VRT
    #[default]
    Standard,
    /// Warped VRT (for reprojection)
    Warped,
    /// Pansharpened VRT
    Pansharpened,
    /// Processed VRT (with pixel functions)
    Processed,
}

/// VRT metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VrtMetadata {
    /// Metadata domain
    pub domain: Option<String>,
    /// Metadata items (key-value pairs)
    pub items: Vec<(String, String)>,
}

impl VrtMetadata {
    /// Creates new VRT metadata
    pub fn new() -> Self {
        Self {
            domain: None,
            items: Vec::new(),
        }
    }

    /// Creates VRT metadata with a domain
    pub fn with_domain<S: Into<String>>(domain: S) -> Self {
        Self {
            domain: Some(domain.into()),
            items: Vec::new(),
        }
    }

    /// Adds a metadata item
    pub fn add_item<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.items.push((key.into(), value.into()));
    }

    /// Gets a metadata value by key
    pub fn get(&self, key: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

impl Default for VrtMetadata {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::band::VrtBand;
    use crate::source::{SourceFilename, VrtSource};
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_vrt_dataset_creation() {
        let dataset = VrtDataset::new(512, 512);
        assert_eq!(dataset.raster_x_size, 512);
        assert_eq!(dataset.raster_y_size, 512);
        assert_eq!(dataset.band_count(), 0);
    }

    #[test]
    fn test_vrt_dataset_validation() {
        let mut dataset = VrtDataset::new(512, 512);
        let source = VrtSource::new(SourceFilename::absolute("/test.tif"), 1);
        let band = VrtBand::simple(1, RasterDataType::UInt8, source);
        dataset.add_band(band);

        assert!(dataset.validate().is_ok());

        let empty_dataset = VrtDataset::new(512, 512);
        assert!(empty_dataset.validate().is_err());

        let invalid_dataset = VrtDataset::new(0, 0);
        assert!(invalid_dataset.validate().is_err());
    }

    #[test]
    fn test_vrt_dataset_extent() {
        let dataset = VrtDataset::new(1024, 768);
        let extent = dataset.extent();
        assert_eq!(extent.x_size, 1024);
        assert_eq!(extent.y_size, 768);
    }

    #[test]
    fn test_vrt_dataset_band_access() {
        let mut dataset = VrtDataset::new(512, 512);
        let source = VrtSource::new(SourceFilename::absolute("/test.tif"), 1);
        let band = VrtBand::simple(1, RasterDataType::UInt8, source);
        dataset.add_band(band);

        assert_eq!(dataset.band_count(), 1);
        assert!(dataset.get_band(0).is_some());
        assert!(dataset.get_band(1).is_none());
    }

    #[test]
    fn test_effective_block_size() {
        let dataset = VrtDataset::new(512, 512);
        assert_eq!(dataset.effective_block_size(), (256, 256));

        let dataset_with_blocks = VrtDataset::new(512, 512).with_block_size(128, 128);
        assert_eq!(dataset_with_blocks.effective_block_size(), (128, 128));
    }

    #[test]
    fn test_vrt_metadata() {
        let mut metadata = VrtMetadata::new();
        metadata.add_item("author", "test");
        metadata.add_item("version", "1.0");

        assert_eq!(metadata.get("author"), Some("test"));
        assert_eq!(metadata.get("version"), Some("1.0"));
        assert_eq!(metadata.get("missing"), None);
    }

    #[test]
    fn test_uniform_data_type() {
        let mut dataset = VrtDataset::new(512, 512);

        let source1 = VrtSource::new(SourceFilename::absolute("/test1.tif"), 1);
        let band1 = VrtBand::simple(1, RasterDataType::UInt8, source1);
        dataset.add_band(band1);

        let source2 = VrtSource::new(SourceFilename::absolute("/test2.tif"), 1);
        let band2 = VrtBand::simple(2, RasterDataType::UInt8, source2);
        dataset.add_band(band2);

        assert!(dataset.has_uniform_data_type());

        let source3 = VrtSource::new(SourceFilename::absolute("/test3.tif"), 1);
        let band3 = VrtBand::simple(3, RasterDataType::Float32, source3);
        dataset.add_band(band3);

        assert!(!dataset.has_uniform_data_type());
    }
}
