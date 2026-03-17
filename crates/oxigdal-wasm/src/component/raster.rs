//! Raster dataset component interface (wasm32-wasip2 compatible).
//!
//! [`ComponentRaster`] is a self-contained, band-interleaved-by-pixel raster
//! that can be transferred across the WASM component boundary as a plain byte
//! buffer.  All pixel access is bounds-checked and returns
//! [`ComponentResult`] rather than panicking.

use crate::component::types::{
    ComponentBbox, ComponentDataType, ComponentError, ComponentResult, ImageDimensions,
};

/// A fully in-memory raster dataset suitable for transfer across the WASM
/// component boundary.
///
/// Data layout: band-sequential (BSQ), little-endian, dtype-encoded.
/// Band `b`, row `r`, column `c` starts at byte:
/// `(b * height * width + r * width + c) * dtype.byte_size()`
#[derive(Debug, Clone)]
pub struct ComponentRaster {
    /// Spatial extent in the dataset's CRS.
    pub dims: ImageDimensions,
    /// Per-pixel data type for every band (all bands share the same type).
    pub dtype: ComponentDataType,
    /// Spatial extent in the dataset's CRS.
    pub bbox: ComponentBbox,
    /// Well-Known Text representation of the coordinate reference system.
    pub crs_wkt: String,
    /// Optional no-data sentinel value.
    pub nodata: Option<f64>,
    /// Raw pixel bytes: BSQ, little-endian, `dtype`-encoded.
    pub data: Vec<u8>,
}

impl ComponentRaster {
    /// Create a zero-initialised raster with the given dimensions and extent.
    pub fn new(
        dims: ImageDimensions,
        dtype: ComponentDataType,
        bbox: ComponentBbox,
        crs_wkt: impl Into<String>,
    ) -> Self {
        let data_size = dims.total_size_bytes(&dtype) as usize;
        Self {
            data: vec![0u8; data_size],
            dims,
            dtype,
            bbox,
            crs_wkt: crs_wkt.into(),
            nodata: None,
        }
    }

    /// Builder helper to attach a no-data value.
    pub fn with_nodata(mut self, nodata: f64) -> Self {
        self.nodata = Some(nodata);
        self
    }

    /// Read the value at (`band`, `row`, `col`) and return it as `f64`.
    ///
    /// Returns [`ComponentError::invalid_input`] if any index is out of range.
    pub fn get_pixel(&self, band: u32, row: u32, col: u32) -> ComponentResult<f64> {
        if band >= self.dims.bands {
            return Err(ComponentError::invalid_input(format!(
                "Band index {band} out of range (bands={})",
                self.dims.bands
            )));
        }
        if row >= self.dims.height {
            return Err(ComponentError::invalid_input(format!(
                "Row index {row} out of range (height={})",
                self.dims.height
            )));
        }
        if col >= self.dims.width {
            return Err(ComponentError::invalid_input(format!(
                "Column index {col} out of range (width={})",
                self.dims.width
            )));
        }

        let band_pixels = self.dims.width as usize * self.dims.height as usize;
        let pixel_idx =
            band as usize * band_pixels + row as usize * self.dims.width as usize + col as usize;
        let byte_idx = pixel_idx * self.dtype.byte_size();
        self.read_value_as_f64(byte_idx)
    }

    /// Set the pixel at (`band`, `row`, `col`) from a `f64` value.
    pub fn set_pixel(&mut self, band: u32, row: u32, col: u32, value: f64) -> ComponentResult<()> {
        if band >= self.dims.bands {
            return Err(ComponentError::invalid_input(format!(
                "Band index {band} out of range"
            )));
        }
        if row >= self.dims.height {
            return Err(ComponentError::invalid_input(format!(
                "Row index {row} out of range"
            )));
        }
        if col >= self.dims.width {
            return Err(ComponentError::invalid_input(format!(
                "Column index {col} out of range"
            )));
        }
        let band_pixels = self.dims.width as usize * self.dims.height as usize;
        let pixel_idx =
            band as usize * band_pixels + row as usize * self.dims.width as usize + col as usize;
        let byte_idx = pixel_idx * self.dtype.byte_size();
        self.write_value_from_f64(byte_idx, value)
    }

    fn read_value_as_f64(&self, byte_idx: usize) -> ComponentResult<f64> {
        let ps = self.dtype.byte_size();
        let end = byte_idx + ps;
        if end > self.data.len() {
            return Err(ComponentError::internal(format!(
                "Byte index {byte_idx} out of data buffer (len={})",
                self.data.len()
            )));
        }
        let b = &self.data[byte_idx..end];
        let value = match self.dtype {
            ComponentDataType::Uint8 => f64::from(b[0]),
            ComponentDataType::Uint16 => f64::from(u16::from_le_bytes([b[0], b[1]])),
            ComponentDataType::Uint32 => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            ComponentDataType::Int8 => f64::from(b[0] as i8),
            ComponentDataType::Int16 => f64::from(i16::from_le_bytes([b[0], b[1]])),
            ComponentDataType::Int32 => f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            ComponentDataType::Float32 => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            ComponentDataType::Float64 => {
                // b has exactly 8 bytes guaranteed by `end = byte_idx + 8 <= data.len()`
                let arr: [u8; 8] = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
                f64::from_le_bytes(arr)
            }
        };
        Ok(value)
    }

    fn write_value_from_f64(&mut self, byte_idx: usize, value: f64) -> ComponentResult<()> {
        let ps = self.dtype.byte_size();
        let end = byte_idx + ps;
        if end > self.data.len() {
            return Err(ComponentError::internal("Byte index out of data buffer"));
        }
        let bytes: Vec<u8> = match self.dtype {
            ComponentDataType::Uint8 => vec![value as u8],
            ComponentDataType::Uint16 => (value as u16).to_le_bytes().to_vec(),
            ComponentDataType::Uint32 => (value as u32).to_le_bytes().to_vec(),
            ComponentDataType::Int8 => (value as i8).to_le_bytes().to_vec(),
            ComponentDataType::Int16 => (value as i16).to_le_bytes().to_vec(),
            ComponentDataType::Int32 => (value as i32).to_le_bytes().to_vec(),
            ComponentDataType::Float32 => (value as f32).to_le_bytes().to_vec(),
            ComponentDataType::Float64 => value.to_le_bytes().to_vec(),
        };
        self.data[byte_idx..end].copy_from_slice(&bytes);
        Ok(())
    }

    /// Returns `true` if `value` matches the no-data sentinel (within 1e-10).
    pub fn is_nodata(&self, value: f64) -> bool {
        self.nodata
            .map(|nd| (value - nd).abs() < 1e-10)
            .unwrap_or(false)
    }

    /// Compute basic statistics (min, max, mean) over all bands, ignoring
    /// no-data pixels and non-finite values.
    pub fn statistics(&self) -> ComponentResult<RasterStats> {
        let total_pixels = self.dims.pixel_count() as usize * self.dims.bands as usize;
        let ps = self.dtype.byte_size();

        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut sum = 0.0_f64;
        let mut count = 0u64;

        for i in 0..total_pixels {
            let byte_idx = i * ps;
            let v = self.read_value_as_f64(byte_idx)?;
            if !self.is_nodata(v) && v.is_finite() {
                if v < min {
                    min = v;
                }
                if v > max {
                    max = v;
                }
                sum += v;
                count += 1;
            }
        }

        let mean = if count > 0 { sum / count as f64 } else { 0.0 };
        Ok(RasterStats {
            min,
            max,
            mean,
            valid_pixels: count,
        })
    }
}

/// Basic per-raster statistics.
#[derive(Debug, Clone)]
pub struct RasterStats {
    /// Minimum valid pixel value.
    pub min: f64,
    /// Maximum valid pixel value.
    pub max: f64,
    /// Mean of all valid pixel values.
    pub mean: f64,
    /// Count of pixels that are not no-data and are finite.
    pub valid_pixels: u64,
}

/// Stateless helper providing common raster operations over [`ComponentRaster`].
pub struct ComponentRasterOps;

impl ComponentRasterOps {
    /// Clip a raster to a bounding box, returning a new raster covering the
    /// intersection.
    ///
    /// # Errors
    ///
    /// - [`ComponentError::invalid_input`] if `bbox` does not intersect the
    ///   raster's extent or the resulting window is empty.
    pub fn clip(
        raster: &ComponentRaster,
        bbox: &ComponentBbox,
    ) -> ComponentResult<ComponentRaster> {
        if !raster.bbox.intersects(bbox) {
            return Err(ComponentError::invalid_input(
                "Clip bbox does not intersect raster extent",
            ));
        }

        let src = &raster.bbox;
        let x_res = (src.max_x - src.min_x) / raster.dims.width as f64;
        let y_res = (src.max_y - src.min_y) / raster.dims.height as f64;

        // Clamp to raster extent.
        let col_start = ((bbox.min_x - src.min_x) / x_res).max(0.0).floor() as u32;
        let col_end = ((bbox.max_x - src.min_x) / x_res)
            .min(raster.dims.width as f64)
            .ceil() as u32;
        let row_start = ((src.max_y - bbox.max_y) / y_res).max(0.0).floor() as u32;
        let row_end = ((src.max_y - bbox.min_y) / y_res)
            .min(raster.dims.height as f64)
            .ceil() as u32;

        if col_end <= col_start || row_end <= row_start {
            return Err(ComponentError::invalid_input(
                "Computed clip window is empty after coordinate transformation",
            ));
        }

        let new_width = col_end - col_start;
        let new_height = row_end - row_start;
        let new_dims = ImageDimensions::new(new_width, new_height, raster.dims.bands);

        let new_bbox = ComponentBbox::new(
            src.min_x + col_start as f64 * x_res,
            src.max_y - row_end as f64 * y_res,
            src.min_x + col_end as f64 * x_res,
            src.max_y - row_start as f64 * y_res,
        );

        let mut out = ComponentRaster::new(
            new_dims,
            raster.dtype.clone(),
            new_bbox,
            raster.crs_wkt.clone(),
        );
        out.nodata = raster.nodata;

        let ps = raster.dtype.byte_size();
        let src_row_stride = raster.dims.width as usize * ps;
        let dst_row_stride = new_width as usize * ps;

        for band in 0..raster.dims.bands as usize {
            let src_band_off = band * raster.dims.width as usize * raster.dims.height as usize * ps;
            let dst_band_off = band * new_width as usize * new_height as usize * ps;

            for row in 0..new_height as usize {
                let src_row = row_start as usize + row;
                let src_off = src_band_off + src_row * src_row_stride + col_start as usize * ps;
                let dst_off = dst_band_off + row * dst_row_stride;
                let copy_len = new_width as usize * ps;

                if src_off + copy_len <= raster.data.len() && dst_off + copy_len <= out.data.len() {
                    out.data[dst_off..dst_off + copy_len]
                        .copy_from_slice(&raster.data[src_off..src_off + copy_len]);
                }
            }
        }

        Ok(out)
    }

    /// Resample a raster to new dimensions using nearest-neighbour interpolation.
    pub fn resample(
        raster: &ComponentRaster,
        new_width: u32,
        new_height: u32,
    ) -> ComponentResult<ComponentRaster> {
        if new_width == 0 || new_height == 0 {
            return Err(ComponentError::invalid_input(
                "Resampled dimensions must be non-zero",
            ));
        }

        let new_dims = ImageDimensions::new(new_width, new_height, raster.dims.bands);
        let mut out = ComponentRaster::new(
            new_dims,
            raster.dtype.clone(),
            raster.bbox.clone(),
            raster.crs_wkt.clone(),
        );
        out.nodata = raster.nodata;

        let x_scale = raster.dims.width as f64 / new_width as f64;
        let y_scale = raster.dims.height as f64 / new_height as f64;
        let ps = raster.dtype.byte_size();

        for band in 0..raster.dims.bands as usize {
            let src_band_off = band * raster.dims.width as usize * raster.dims.height as usize * ps;
            let dst_band_off = band * new_width as usize * new_height as usize * ps;

            for dst_row in 0..new_height as usize {
                let src_row =
                    ((dst_row as f64 * y_scale) as usize).min(raster.dims.height as usize - 1);
                for dst_col in 0..new_width as usize {
                    let src_col =
                        ((dst_col as f64 * x_scale) as usize).min(raster.dims.width as usize - 1);

                    let src_off =
                        src_band_off + src_row * raster.dims.width as usize * ps + src_col * ps;
                    let dst_off = dst_band_off + dst_row * new_width as usize * ps + dst_col * ps;

                    if src_off + ps <= raster.data.len() && dst_off + ps <= out.data.len() {
                        out.data[dst_off..dst_off + ps]
                            .copy_from_slice(&raster.data[src_off..src_off + ps]);
                    }
                }
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raster(w: u32, h: u32, bands: u32) -> ComponentRaster {
        ComponentRaster::new(
            ImageDimensions::new(w, h, bands),
            ComponentDataType::Float32,
            ComponentBbox::new(0.0, 0.0, 1.0, 1.0),
            "EPSG:4326",
        )
    }

    #[test]
    fn new_allocates_correct_size() {
        let r = make_raster(10, 10, 3);
        assert_eq!(r.data.len(), 10 * 10 * 3 * 4); // Float32 = 4 bytes
    }

    #[test]
    fn get_pixel_default_zero() {
        let r = make_raster(4, 4, 1);
        assert_eq!(r.get_pixel(0, 0, 0).expect("pixel"), 0.0);
    }

    #[test]
    fn get_pixel_out_of_bounds_band() {
        let r = make_raster(4, 4, 1);
        assert!(r.get_pixel(1, 0, 0).is_err());
    }

    #[test]
    fn get_pixel_out_of_bounds_row() {
        let r = make_raster(4, 4, 1);
        assert!(r.get_pixel(0, 10, 0).is_err());
    }

    #[test]
    fn set_and_get_pixel_roundtrip() {
        let mut r = make_raster(4, 4, 1);
        r.set_pixel(0, 2, 3, 42.5).expect("set pixel");
        let v = r.get_pixel(0, 2, 3).expect("get pixel");
        assert!((v - 42.5).abs() < 1e-5);
    }

    #[test]
    fn statistics_all_zeros() {
        let r = make_raster(4, 4, 1);
        let stats = r.statistics().expect("stats");
        assert_eq!(stats.min, 0.0);
        assert_eq!(stats.max, 0.0);
        assert_eq!(stats.valid_pixels, 16);
    }

    #[test]
    fn is_nodata_true() {
        let r = make_raster(2, 2, 1).with_nodata(-9999.0);
        assert!(r.is_nodata(-9999.0));
        assert!(!r.is_nodata(0.0));
    }

    #[test]
    fn clip_reduces_dimensions() {
        let r = ComponentRaster::new(
            ImageDimensions::new(100, 100, 1),
            ComponentDataType::Uint8,
            ComponentBbox::new(0.0, 0.0, 100.0, 100.0),
            "EPSG:4326",
        );
        let clipped =
            ComponentRasterOps::clip(&r, &ComponentBbox::new(0.0, 0.0, 50.0, 50.0)).expect("clip");
        assert!(clipped.dims.width <= 100);
        assert!(clipped.dims.height <= 100);
    }

    #[test]
    fn clip_nonoverlap_returns_error() {
        let r = ComponentRaster::new(
            ImageDimensions::new(10, 10, 1),
            ComponentDataType::Uint8,
            ComponentBbox::new(0.0, 0.0, 10.0, 10.0),
            "EPSG:4326",
        );
        let result = ComponentRasterOps::clip(&r, &ComponentBbox::new(20.0, 20.0, 30.0, 30.0));
        assert!(result.is_err());
    }

    #[test]
    fn clip_preserves_band_count() {
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
    fn clip_returned_bbox_is_subset() {
        let r = ComponentRaster::new(
            ImageDimensions::new(100, 100, 1),
            ComponentDataType::Uint8,
            ComponentBbox::new(0.0, 0.0, 100.0, 100.0),
            "EPSG:4326",
        );
        let clip_bbox = ComponentBbox::new(10.0, 10.0, 90.0, 90.0);
        let clipped = ComponentRasterOps::clip(&r, &clip_bbox).expect("clip");
        // The clipped raster bbox must be contained within the original.
        assert!(clipped.bbox.min_x >= r.bbox.min_x);
        assert!(clipped.bbox.min_y >= r.bbox.min_y);
        assert!(clipped.bbox.max_x <= r.bbox.max_x);
        assert!(clipped.bbox.max_y <= r.bbox.max_y);
    }

    #[test]
    fn resample_changes_dims() {
        let r = make_raster(100, 100, 1);
        let out = ComponentRasterOps::resample(&r, 50, 50).expect("resample");
        assert_eq!(out.dims.width, 50);
        assert_eq!(out.dims.height, 50);
    }

    #[test]
    fn resample_zero_dims_error() {
        let r = make_raster(10, 10, 1);
        assert!(ComponentRasterOps::resample(&r, 0, 10).is_err());
    }
}
