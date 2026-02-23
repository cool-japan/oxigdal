//! VRT builder API for fluent VRT creation

use crate::band::VrtBand;
use crate::dataset::VrtDataset;
use crate::error::{Result, VrtError};
use crate::source::{PixelRect, SourceWindow, VrtSource};
use crate::xml::VrtXmlWriter;
use oxigdal_core::types::{GeoTransform, RasterDataType};
use std::path::{Path, PathBuf};

/// VRT builder for creating VRT datasets
pub struct VrtBuilder {
    dataset: VrtDataset,
    auto_calculate_extent: bool,
    vrt_path: Option<PathBuf>,
}

impl VrtBuilder {
    /// Creates a new VRT builder
    pub fn new() -> Self {
        Self {
            dataset: VrtDataset::new(0, 0),
            auto_calculate_extent: true,
            vrt_path: None,
        }
    }

    /// Creates a VRT builder with specified dimensions
    pub fn with_size(width: u64, height: u64) -> Self {
        Self {
            dataset: VrtDataset::new(width, height),
            auto_calculate_extent: false,
            vrt_path: None,
        }
    }

    /// Sets the VRT file path (for resolving relative source paths)
    pub fn with_vrt_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.vrt_path = Some(path.into());
        self
    }

    /// Sets the spatial reference system
    pub fn with_srs<S: Into<String>>(mut self, srs: S) -> Self {
        self.dataset = self.dataset.with_srs(srs);
        self
    }

    /// Sets the GeoTransform
    pub fn with_geo_transform(mut self, geo_transform: GeoTransform) -> Self {
        self.dataset = self.dataset.with_geo_transform(geo_transform);
        self
    }

    /// Sets the block size
    pub fn with_block_size(mut self, width: u32, height: u32) -> Self {
        self.dataset = self.dataset.with_block_size(width, height);
        self
    }

    /// Adds a simple source to a band
    ///
    /// # Errors
    /// Returns an error if the source configuration is invalid
    pub fn add_source<P: AsRef<Path>>(
        mut self,
        path: P,
        band_num: usize,
        source_band: usize,
    ) -> Result<Self> {
        let source = VrtSource::simple(path.as_ref(), source_band);
        self.add_source_to_band(band_num, source)?;
        Ok(self)
    }

    /// Adds a source with a window to a band
    ///
    /// # Errors
    /// Returns an error if the source configuration is invalid
    pub fn add_source_with_window<P: AsRef<Path>>(
        mut self,
        path: P,
        band_num: usize,
        source_band: usize,
        src_rect: PixelRect,
        dst_rect: PixelRect,
    ) -> Result<Self> {
        let window = SourceWindow::new(src_rect, dst_rect);
        let source = VrtSource::simple(path.as_ref(), source_band).with_window(window);
        self.add_source_to_band(band_num, source)?;
        Ok(self)
    }

    /// Adds a mosaic tile at a specific position
    ///
    /// # Errors
    /// Returns an error if the tile configuration is invalid
    pub fn add_tile<P: AsRef<Path>>(
        mut self,
        path: P,
        x_off: u64,
        y_off: u64,
        width: u64,
        height: u64,
    ) -> Result<Self> {
        let src_rect = PixelRect::new(0, 0, width, height);
        let dst_rect = PixelRect::new(x_off, y_off, width, height);
        let window = SourceWindow::new(src_rect, dst_rect);

        let source = VrtSource::simple(path.as_ref(), 1).with_window(window);
        self.add_source_to_band(1, source)?;
        Ok(self)
    }

    /// Adds a mosaic tile grid (NxM tiles)
    ///
    /// # Errors
    /// Returns an error if the tile configuration is invalid
    pub fn add_tile_grid<P>(
        mut self,
        paths: &[P],
        tile_width: u64,
        tile_height: u64,
        cols: usize,
    ) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        for (idx, path) in paths.iter().enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let x_off = col as u64 * tile_width;
            let y_off = row as u64 * tile_height;

            let src_rect = PixelRect::new(0, 0, tile_width, tile_height);
            let dst_rect = PixelRect::new(x_off, y_off, tile_width, tile_height);
            let window = SourceWindow::new(src_rect, dst_rect);

            let source = VrtSource::simple(path.as_ref(), 1).with_window(window);
            self.add_source_to_band(1, source)?;
        }

        Ok(self)
    }

    /// Adds a full band configuration
    ///
    /// # Errors
    /// Returns an error if the band is invalid
    pub fn add_band(mut self, band: VrtBand) -> Result<Self> {
        band.validate()?;
        self.dataset.add_band(band);
        Ok(self)
    }

    /// Sets the dataset dimensions explicitly
    pub fn set_dimensions(mut self, width: u64, height: u64) -> Self {
        self.dataset.raster_x_size = width;
        self.dataset.raster_y_size = height;
        self.auto_calculate_extent = false;
        self
    }

    /// Builds the VRT dataset
    ///
    /// # Errors
    /// Returns an error if the dataset configuration is invalid
    pub fn build(mut self) -> Result<VrtDataset> {
        // Auto-calculate extent if needed
        if self.auto_calculate_extent && self.dataset.raster_x_size == 0 {
            self.calculate_extent()?;
        }

        // Set VRT path if provided
        if let Some(path) = self.vrt_path {
            self.dataset.vrt_path = Some(path);
        }

        // Validate the dataset
        self.dataset.validate()?;

        Ok(self.dataset)
    }

    /// Builds and writes the VRT to a file
    ///
    /// # Errors
    /// Returns an error if building or writing fails
    pub fn build_file<P: AsRef<Path>>(mut self, path: P) -> Result<VrtDataset> {
        // Set VRT path for relative source resolution
        let path = path.as_ref();
        self.vrt_path = Some(path.to_path_buf());

        let dataset = self.build()?;
        VrtXmlWriter::write_file(&dataset, path)?;
        Ok(dataset)
    }

    /// Helper to add a source to a specific band
    fn add_source_to_band(&mut self, band_num: usize, source: VrtSource) -> Result<()> {
        source.validate()?;

        // Find or create the band
        let band_idx = band_num - 1;
        while self.dataset.bands.len() <= band_idx {
            let new_band_num = self.dataset.bands.len() + 1;
            let data_type = source
                .data_type
                .or_else(|| source.properties.as_ref().map(|p| p.data_type))
                .unwrap_or(RasterDataType::UInt8);
            self.dataset
                .bands
                .push(VrtBand::new(new_band_num, data_type));
        }

        if let Some(band) = self.dataset.get_band_mut(band_idx) {
            band.add_source(source);
        }

        Ok(())
    }

    /// Calculates extent from sources
    fn calculate_extent(&mut self) -> Result<()> {
        if self.dataset.bands.is_empty() {
            return Err(VrtError::invalid_structure(
                "No bands to calculate extent from",
            ));
        }

        let mut max_x = 0u64;
        let mut max_y = 0u64;

        for band in &self.dataset.bands {
            for source in &band.sources {
                if let Some(dst_rect) = source.dst_rect() {
                    max_x = max_x.max(dst_rect.x_off + dst_rect.x_size);
                    max_y = max_y.max(dst_rect.y_off + dst_rect.y_size);
                } else if let Some(ref props) = source.properties {
                    max_x = max_x.max(props.width);
                    max_y = max_y.max(props.height);
                }
            }
        }

        if max_x == 0 || max_y == 0 {
            return Err(VrtError::invalid_structure(
                "Cannot calculate extent from sources",
            ));
        }

        self.dataset.raster_x_size = max_x;
        self.dataset.raster_y_size = max_y;

        Ok(())
    }
}

impl Default for VrtBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience builder for simple mosaic VRTs
pub struct MosaicBuilder {
    builder: VrtBuilder,
    tile_width: u64,
    tile_height: u64,
    current_x: u64,
    current_y: u64,
    max_x: u64,
    max_y: u64,
}

impl MosaicBuilder {
    /// Creates a new mosaic builder
    pub fn new(tile_width: u64, tile_height: u64) -> Self {
        Self {
            builder: VrtBuilder::new(),
            tile_width,
            tile_height,
            current_x: 0,
            current_y: 0,
            max_x: 0,
            max_y: 0,
        }
    }

    /// Adds a tile at the current position and advances
    ///
    /// # Errors
    /// Returns an error if the tile cannot be added
    pub fn add_tile<P: AsRef<Path>>(mut self, path: P) -> Result<Self> {
        let src_rect = PixelRect::new(0, 0, self.tile_width, self.tile_height);
        let dst_rect = PixelRect::new(
            self.current_x,
            self.current_y,
            self.tile_width,
            self.tile_height,
        );
        let window = SourceWindow::new(src_rect, dst_rect);

        let source = VrtSource::simple(path.as_ref(), 1).with_window(window);
        self.builder.add_source_to_band(1, source)?;

        // Update extent tracking
        self.max_x = self.max_x.max(self.current_x + self.tile_width);
        self.max_y = self.max_y.max(self.current_y + self.tile_height);

        Ok(self)
    }

    /// Adds a tile at a specific position
    ///
    /// # Errors
    /// Returns an error if the tile cannot be added
    pub fn add_tile_at<P: AsRef<Path>>(mut self, path: P, x: u64, y: u64) -> Result<Self> {
        let src_rect = PixelRect::new(0, 0, self.tile_width, self.tile_height);
        let dst_rect = PixelRect::new(x, y, self.tile_width, self.tile_height);
        let window = SourceWindow::new(src_rect, dst_rect);

        let source = VrtSource::simple(path.as_ref(), 1).with_window(window);
        self.builder.add_source_to_band(1, source)?;

        // Update extent tracking
        self.max_x = self.max_x.max(x + self.tile_width);
        self.max_y = self.max_y.max(y + self.tile_height);

        Ok(self)
    }

    /// Moves to the next column
    pub fn next_column(mut self) -> Self {
        self.current_x += self.tile_width;
        self
    }

    /// Moves to the next row
    pub fn next_row(mut self) -> Self {
        self.current_x = 0;
        self.current_y += self.tile_height;
        self
    }

    /// Sets the current position
    pub fn at(mut self, x: u64, y: u64) -> Self {
        self.current_x = x;
        self.current_y = y;
        self
    }

    /// Sets the spatial reference system
    pub fn with_srs<S: Into<String>>(mut self, srs: S) -> Self {
        self.builder = self.builder.with_srs(srs);
        self
    }

    /// Sets the GeoTransform
    pub fn with_geo_transform(mut self, geo_transform: GeoTransform) -> Self {
        self.builder = self.builder.with_geo_transform(geo_transform);
        self
    }

    /// Builds the mosaic VRT
    ///
    /// # Errors
    /// Returns an error if building fails
    pub fn build(mut self) -> Result<VrtDataset> {
        // Set dimensions based on extent
        self.builder = self.builder.set_dimensions(self.max_x, self.max_y);
        self.builder.build()
    }

    /// Builds and writes the mosaic VRT to a file
    ///
    /// # Errors
    /// Returns an error if building or writing fails
    pub fn build_file<P: AsRef<Path>>(mut self, path: P) -> Result<VrtDataset> {
        // Set dimensions based on extent
        self.builder = self.builder.set_dimensions(self.max_x, self.max_y);
        self.builder.build_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vrt_builder() {
        let builder = VrtBuilder::with_size(512, 512);
        let result = builder.add_source("/test1.tif", 1, 1);

        assert!(result.is_ok());
        let builder = result.expect("Should add source");
        let dataset = builder.build();
        assert!(dataset.is_ok());
    }

    #[test]
    fn test_mosaic_builder() {
        let builder = MosaicBuilder::new(256, 256);
        let result = builder.add_tile("/tile1.tif");

        assert!(result.is_ok());
        let builder = result.expect("Should add tile");
        let result = builder.next_column().add_tile("/tile2.tif");
        assert!(result.is_ok());

        let builder = result.expect("Should add tile");
        let dataset = builder.build();
        assert!(dataset.is_ok());
        let ds = dataset.expect("Should build");
        assert_eq!(ds.band_count(), 1);
    }

    #[test]
    fn test_tile_grid() {
        let paths = vec!["/tile1.tif", "/tile2.tif", "/tile3.tif", "/tile4.tif"];
        let builder = VrtBuilder::new();
        let result = builder.add_tile_grid(&paths, 256, 256, 2);

        assert!(result.is_ok());
        let builder = result.expect("Should add tiles");
        let result = builder.set_dimensions(512, 512).build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_geo_transform() {
        let gt = GeoTransform {
            origin_x: 0.0,
            pixel_width: 1.0,
            row_rotation: 0.0,
            origin_y: 0.0,
            col_rotation: 0.0,
            pixel_height: -1.0,
        };

        let builder = VrtBuilder::with_size(512, 512);
        let result = builder
            .with_geo_transform(gt)
            .with_srs("EPSG:4326")
            .add_source("/test.tif", 1, 1);

        assert!(result.is_ok());
        let builder = result.expect("Should configure");
        let dataset = builder.build();
        assert!(dataset.is_ok());
        let ds = dataset.expect("Should build");
        assert!(ds.geo_transform.is_some());
        assert!(ds.srs.is_some());
    }
}
