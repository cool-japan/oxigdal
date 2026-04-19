//! Band iteration for multi-band raster datasets.
//!
//! Provides [`BandIterator`] for lazy per-band iteration over raster data,
//! supporting all [`PixelLayout`] memory organizations (BSQ, BIL, BIP, Tiled).
//!
//! # Examples
//!
//! ```
//! use oxigdal_core::buffer::{MultiBandBuffer, RasterBuffer};
//! use oxigdal_core::types::{RasterDataType, ColorInterpretation, NoDataValue, PixelLayout};
//!
//! // Create an RGB image (3 bands, 100×100)
//! let band_r = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
//! let band_g = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
//! let band_b = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
//!
//! let multi = MultiBandBuffer::from_bands(
//!     vec![band_r, band_g, band_b],
//!     vec![ColorInterpretation::Red, ColorInterpretation::Green, ColorInterpretation::Blue],
//! ).expect("should create multi-band buffer");
//!
//! for band in multi.bands() {
//!     println!("Band {}: {} ({:?})", band.index(), band.buffer().width(), band.color());
//! }
//! ```

use crate::error::{OxiGdalError, Result};
use crate::types::{ColorInterpretation, NoDataValue, PixelLayout, RasterDataType};

use super::{BufferStatistics, RasterBuffer};

/// A multi-band raster buffer holding multiple single-band buffers.
///
/// All bands must share identical dimensions and data type.
#[derive(Debug, Clone)]
pub struct MultiBandBuffer {
    /// Per-band pixel data (one `RasterBuffer` per band).
    bands: Vec<RasterBuffer>,
    /// Per-band color interpretation.
    colors: Vec<ColorInterpretation>,
    /// Per-band nodata values (if different from the buffer's own nodata).
    nodata_overrides: Vec<Option<NoDataValue>>,
    /// Pixel layout describing the original memory organization.
    layout: PixelLayout,
}

impl MultiBandBuffer {
    /// Create a `MultiBandBuffer` from individual band buffers and color interpretations.
    ///
    /// All bands must have the same width, height, and data type.
    ///
    /// # Errors
    ///
    /// Returns an error if bands have mismatched dimensions or data types, or if
    /// the color interpretation count doesn't match the band count.
    pub fn from_bands(bands: Vec<RasterBuffer>, colors: Vec<ColorInterpretation>) -> Result<Self> {
        if bands.is_empty() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "bands",
                message: "At least one band is required".to_string(),
            });
        }
        if colors.len() != bands.len() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "colors",
                message: format!(
                    "Color interpretation count ({}) must match band count ({})",
                    colors.len(),
                    bands.len()
                ),
            });
        }

        let w = bands[0].width();
        let h = bands[0].height();
        let dt = bands[0].data_type();

        for (i, band) in bands.iter().enumerate().skip(1) {
            if band.width() != w || band.height() != h {
                return Err(OxiGdalError::InvalidParameter {
                    parameter: "bands",
                    message: format!(
                        "Band {} dimensions {}x{} differ from band 0 dimensions {}x{}",
                        i,
                        band.width(),
                        band.height(),
                        w,
                        h
                    ),
                });
            }
            if band.data_type() != dt {
                return Err(OxiGdalError::InvalidParameter {
                    parameter: "bands",
                    message: format!(
                        "Band {} data type {:?} differs from band 0 data type {:?}",
                        i,
                        band.data_type(),
                        dt
                    ),
                });
            }
        }

        let nodata_overrides = vec![None; bands.len()];

        Ok(Self {
            bands,
            colors,
            nodata_overrides,
            layout: PixelLayout::BandSequential,
        })
    }

    /// Create a `MultiBandBuffer` from interleaved (BIP) data.
    ///
    /// Deinterleaves band-interleaved-by-pixel data into separate band buffers.
    ///
    /// # Errors
    ///
    /// Returns an error if the data size doesn't match dimensions × band_count × type_size.
    pub fn from_interleaved(
        data: &[u8],
        width: u64,
        height: u64,
        band_count: u32,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> Result<Self> {
        let pixel_size = data_type.size_bytes();
        let expected = (width * height * band_count as u64 * pixel_size as u64) as usize;
        if data.len() != expected {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Interleaved data size {} differs from expected {} ({}x{}x{}x{})",
                    data.len(),
                    expected,
                    width,
                    height,
                    band_count,
                    pixel_size
                ),
            });
        }

        let pixels = width * height;
        let band_size = (pixels * pixel_size as u64) as usize;
        let bc = band_count as usize;
        let ps = pixel_size;

        let mut band_buffers = Vec::with_capacity(bc);
        for b in 0..bc {
            let mut band_data = vec![0u8; band_size];
            for pixel_idx in 0..(pixels as usize) {
                let src_offset = pixel_idx * bc * ps + b * ps;
                let dst_offset = pixel_idx * ps;
                band_data[dst_offset..dst_offset + ps]
                    .copy_from_slice(&data[src_offset..src_offset + ps]);
            }
            band_buffers.push(RasterBuffer::new(
                band_data, width, height, data_type, nodata,
            )?);
        }

        let colors = default_color_interpretation(bc);

        Ok(Self {
            bands: band_buffers,
            colors,
            nodata_overrides: vec![None; bc],
            layout: PixelLayout::BandInterleavedByPixel,
        })
    }

    /// Create a `MultiBandBuffer` from band-sequential (BSQ) data.
    ///
    /// # Errors
    ///
    /// Returns an error if the data size doesn't match dimensions × band_count × type_size.
    pub fn from_bsq(
        data: &[u8],
        width: u64,
        height: u64,
        band_count: u32,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> Result<Self> {
        let pixel_size = data_type.size_bytes();
        let expected = (width * height * band_count as u64 * pixel_size as u64) as usize;
        if data.len() != expected {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "BSQ data size {} differs from expected {}",
                    data.len(),
                    expected
                ),
            });
        }

        let band_size = (width * height * pixel_size as u64) as usize;
        let bc = band_count as usize;
        let mut band_buffers = Vec::with_capacity(bc);

        for b in 0..bc {
            let start = b * band_size;
            let band_data = data[start..start + band_size].to_vec();
            band_buffers.push(RasterBuffer::new(
                band_data, width, height, data_type, nodata,
            )?);
        }

        let colors = default_color_interpretation(bc);

        Ok(Self {
            bands: band_buffers,
            colors,
            nodata_overrides: vec![None; bc],
            layout: PixelLayout::BandSequential,
        })
    }

    /// Create a `MultiBandBuffer` from Band-Interleaved-by-Line (BIL) data.
    ///
    /// Rearranges BIL-layout bytes into the internal BSQ (band-sequential) representation.
    ///
    /// BIL layout: for each row `r` and then each band `b`, the `width * pixel_size` bytes
    /// for that (row, band) slice are contiguous:
    ///
    /// ```text
    /// [row0_band0_col0..colN, row0_band1_col0..colN, …, row0_bandN_col0..colN,
    ///  row1_band0_col0..colN, row1_band1_col0..colN, …, rowH_bandN_col0..colN]
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the data size doesn't match `width × height × band_count × pixel_size`.
    pub fn from_bil(
        data: &[u8],
        width: u64,
        height: u64,
        band_count: u32,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> Result<Self> {
        let pixel_size = data_type.size_bytes();
        let bc = band_count as usize;
        let w = width as usize;
        let h = height as usize;
        let expected = bc * h * w * pixel_size;
        if data.len() != expected {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "BIL data size {} differs from expected {} ({}x{}x{}x{})",
                    data.len(),
                    expected,
                    width,
                    height,
                    band_count,
                    pixel_size
                ),
            });
        }

        // BSQ band buffer size: one contiguous block per band
        let band_byte_size = h * w * pixel_size;
        let mut band_buffers: Vec<RasterBuffer> = Vec::with_capacity(bc);

        for b in 0..bc {
            let mut bsq_data = vec![0u8; band_byte_size];
            for r in 0..h {
                // BIL source offset: row r, band b
                let bil_offset = (r * bc + b) * w * pixel_size;
                // BSQ destination offset: band b, row r
                let bsq_offset = r * w * pixel_size;
                let row_bytes = w * pixel_size;
                bsq_data[bsq_offset..bsq_offset + row_bytes]
                    .copy_from_slice(&data[bil_offset..bil_offset + row_bytes]);
            }
            band_buffers.push(RasterBuffer::new(
                bsq_data, width, height, data_type, nodata,
            )?);
        }

        let colors = default_color_interpretation(bc);

        Ok(Self {
            bands: band_buffers,
            colors,
            nodata_overrides: vec![None; bc],
            layout: PixelLayout::BandInterleavedByLine,
        })
    }

    /// Returns the number of bands.
    #[must_use]
    pub fn band_count(&self) -> u32 {
        self.bands.len() as u32
    }

    /// Returns the width in pixels (same for all bands).
    #[must_use]
    pub fn width(&self) -> u64 {
        self.bands.first().map_or(0, |b| b.width())
    }

    /// Returns the height in pixels (same for all bands).
    #[must_use]
    pub fn height(&self) -> u64 {
        self.bands.first().map_or(0, |b| b.height())
    }

    /// Returns the data type (same for all bands).
    #[must_use]
    pub fn data_type(&self) -> RasterDataType {
        self.bands
            .first()
            .map_or(RasterDataType::UInt8, |b| b.data_type())
    }

    /// Returns the pixel layout.
    #[must_use]
    pub fn layout(&self) -> PixelLayout {
        self.layout
    }

    /// Set per-band nodata override value.
    pub fn set_band_nodata(&mut self, band: u32, nodata: NoDataValue) -> Result<()> {
        let idx = band as usize;
        if idx >= self.bands.len() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "band",
                message: format!("Band index {} out of range (0..{})", band, self.bands.len()),
            });
        }
        self.nodata_overrides[idx] = Some(nodata);
        Ok(())
    }

    /// Get a reference to a specific band buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if `band` is out of range.
    pub fn band(&self, band: u32) -> Result<BandRef<'_>> {
        let idx = band as usize;
        if idx >= self.bands.len() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "band",
                message: format!("Band index {} out of range (0..{})", band, self.bands.len()),
            });
        }
        Ok(BandRef {
            index: band,
            buffer: &self.bands[idx],
            color: self.colors[idx],
            nodata_override: self.nodata_overrides[idx],
        })
    }

    /// Get a mutable reference to a specific band buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if `band` is out of range.
    pub fn band_mut(&mut self, band: u32) -> Result<&mut RasterBuffer> {
        let idx = band as usize;
        if idx >= self.bands.len() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "band",
                message: format!("Band index {} out of range (0..{})", band, self.bands.len()),
            });
        }
        Ok(&mut self.bands[idx])
    }

    /// Returns a lazy iterator over all bands.
    #[must_use]
    pub fn bands(&self) -> BandIterator<'_> {
        BandIterator {
            multi: self,
            current: 0,
        }
    }

    /// Interleave all bands into a single BIP byte buffer.
    ///
    /// Returns pixels in band-interleaved-by-pixel order: R₁G₁B₁ R₂G₂B₂ ...
    #[must_use]
    pub fn to_interleaved(&self) -> Vec<u8> {
        let ps = self.data_type().size_bytes();
        let bc = self.bands.len();
        let pixels = (self.width() * self.height()) as usize;
        let mut out = vec![0u8; pixels * bc * ps];

        for (b, band) in self.bands.iter().enumerate() {
            let src = band.as_bytes();
            for pixel_idx in 0..pixels {
                let dst_off = pixel_idx * bc * ps + b * ps;
                let src_off = pixel_idx * ps;
                out[dst_off..dst_off + ps].copy_from_slice(&src[src_off..src_off + ps]);
            }
        }

        out
    }

    /// Flatten all bands into BSQ byte buffer.
    ///
    /// Returns band-sequential data: all of band 0, then all of band 1, etc.
    #[must_use]
    pub fn to_bsq(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bands.iter().map(|b| b.as_bytes().len()).sum());
        for band in &self.bands {
            out.extend_from_slice(band.as_bytes());
        }
        out
    }

    /// Emits the buffer data in Band-Interleaved-by-Line (BIL) layout.
    ///
    /// BIL layout: for each row `r` and then each band `b`, all `width * pixel_size` bytes
    /// for that (row, band) slice are written contiguously:
    ///
    /// ```text
    /// [row0_band0, row0_band1, …, row0_bandN, row1_band0, …, rowH_bandN]
    /// ```
    #[must_use]
    pub fn to_bil(&self) -> Vec<u8> {
        let bc = self.bands.len();
        let pixel_size = self.data_type().size_bytes();
        let h = self.height() as usize;
        let w = self.width() as usize;
        let total = bc * h * w * pixel_size;
        let mut out = vec![0u8; total];

        for (b, band) in self.bands.iter().enumerate() {
            let src = band.as_bytes();
            for r in 0..h {
                // BIL destination offset for (row r, band b)
                let bil_offset = (r * bc + b) * w * pixel_size;
                // BSQ source offset for (band b, row r)
                let bsq_offset = r * w * pixel_size;
                let row_bytes = w * pixel_size;
                out[bil_offset..bil_offset + row_bytes]
                    .copy_from_slice(&src[bsq_offset..bsq_offset + row_bytes]);
            }
        }

        out
    }

    /// Compute statistics for all bands.
    ///
    /// # Errors
    ///
    /// Returns an error if statistics computation fails for any band.
    pub fn compute_all_statistics(&self) -> Result<Vec<BufferStatistics>> {
        self.bands.iter().map(|b| b.compute_statistics()).collect()
    }

    /// Consume and return the inner band buffers.
    #[must_use]
    pub fn into_bands(self) -> Vec<RasterBuffer> {
        self.bands
    }
}

/// A borrowed reference to a single band within a [`MultiBandBuffer`].
#[derive(Debug, Clone, Copy)]
pub struct BandRef<'a> {
    /// 0-based band index.
    index: u32,
    /// Reference to the band's pixel buffer.
    buffer: &'a RasterBuffer,
    /// Color interpretation.
    color: ColorInterpretation,
    /// Per-band nodata override (if set).
    nodata_override: Option<NoDataValue>,
}

impl<'a> BandRef<'a> {
    /// Returns the 0-based band index.
    #[must_use]
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Returns the 1-based band index (GDAL convention).
    #[must_use]
    pub fn gdal_index(&self) -> u32 {
        self.index + 1
    }

    /// Returns the band's pixel buffer.
    #[must_use]
    pub fn buffer(&self) -> &'a RasterBuffer {
        self.buffer
    }

    /// Returns the color interpretation.
    #[must_use]
    pub fn color(&self) -> ColorInterpretation {
        self.color
    }

    /// Returns the effective nodata value (override or buffer default).
    #[must_use]
    pub fn nodata(&self) -> NoDataValue {
        self.nodata_override.unwrap_or(self.buffer.nodata())
    }

    /// Get a typed slice of the band's pixel data.
    ///
    /// # Errors
    ///
    /// Returns an error if `T`'s size doesn't match the band's data type.
    pub fn as_slice<T: Copy + 'static>(&self) -> Result<&[T]> {
        self.buffer.as_slice::<T>()
    }
}

/// Lazy iterator over bands of a [`MultiBandBuffer`].
///
/// Yields [`BandRef`] for each band in order (band 0, 1, 2, ...).
/// Does not copy pixel data — references the underlying buffers.
pub struct BandIterator<'a> {
    multi: &'a MultiBandBuffer,
    current: u32,
}

impl<'a> Iterator for BandIterator<'a> {
    type Item = BandRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.multi.band_count() {
            return None;
        }
        let idx = self.current as usize;
        let band = BandRef {
            index: self.current,
            buffer: &self.multi.bands[idx],
            color: self.multi.colors[idx],
            nodata_override: self.multi.nodata_overrides[idx],
        };
        self.current += 1;
        Some(band)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.multi.band_count() - self.current) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for BandIterator<'_> {}

/// Default color interpretation based on band count.
fn default_color_interpretation(band_count: usize) -> Vec<ColorInterpretation> {
    match band_count {
        1 => vec![ColorInterpretation::Gray],
        3 => vec![
            ColorInterpretation::Red,
            ColorInterpretation::Green,
            ColorInterpretation::Blue,
        ],
        4 => vec![
            ColorInterpretation::Red,
            ColorInterpretation::Green,
            ColorInterpretation::Blue,
            ColorInterpretation::Alpha,
        ],
        _ => vec![ColorInterpretation::Undefined; band_count],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_band_from_bands() {
        let r = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
        let g = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
        let b = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);

        let multi = MultiBandBuffer::from_bands(
            vec![r, g, b],
            vec![
                ColorInterpretation::Red,
                ColorInterpretation::Green,
                ColorInterpretation::Blue,
            ],
        )
        .expect("should create multi-band buffer");

        assert_eq!(multi.band_count(), 3);
        assert_eq!(multi.width(), 100);
        assert_eq!(multi.height(), 100);
        assert_eq!(multi.data_type(), RasterDataType::UInt8);
    }

    #[test]
    fn test_band_iterator() {
        let bands: Vec<_> = (0..4)
            .map(|_| RasterBuffer::zeros(50, 50, RasterDataType::Float32))
            .collect();
        let colors = vec![
            ColorInterpretation::Red,
            ColorInterpretation::Green,
            ColorInterpretation::Blue,
            ColorInterpretation::Alpha,
        ];

        let multi = MultiBandBuffer::from_bands(bands, colors).expect("should create");

        let collected: Vec<_> = multi.bands().collect();
        assert_eq!(collected.len(), 4);
        assert_eq!(collected[0].index(), 0);
        assert_eq!(collected[0].gdal_index(), 1);
        assert_eq!(collected[0].color(), ColorInterpretation::Red);
        assert_eq!(collected[3].color(), ColorInterpretation::Alpha);
    }

    #[test]
    fn test_band_iterator_exact_size() {
        let multi = MultiBandBuffer::from_bands(
            vec![
                RasterBuffer::zeros(10, 10, RasterDataType::UInt8),
                RasterBuffer::zeros(10, 10, RasterDataType::UInt8),
            ],
            vec![ColorInterpretation::Gray, ColorInterpretation::Alpha],
        )
        .expect("should create");

        let iter = multi.bands();
        assert_eq!(iter.len(), 2);
    }

    #[test]
    fn test_multi_band_empty_error() {
        let result = MultiBandBuffer::from_bands(vec![], vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_multi_band_dimension_mismatch() {
        let a = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
        let b = RasterBuffer::zeros(50, 50, RasterDataType::UInt8);

        let result = MultiBandBuffer::from_bands(
            vec![a, b],
            vec![ColorInterpretation::Gray, ColorInterpretation::Alpha],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_multi_band_type_mismatch() {
        let a = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
        let b = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        let result = MultiBandBuffer::from_bands(
            vec![a, b],
            vec![ColorInterpretation::Gray, ColorInterpretation::Alpha],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_multi_band_color_count_mismatch() {
        let a = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);

        let result = MultiBandBuffer::from_bands(
            vec![a],
            vec![ColorInterpretation::Red, ColorInterpretation::Green],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_band_access() {
        let mut buf = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);
        buf.set_pixel(5, 5, 42.0).expect("should set");

        let multi = MultiBandBuffer::from_bands(vec![buf], vec![ColorInterpretation::Gray])
            .expect("should create");

        let band = multi.band(0).expect("should get band");
        assert_eq!(band.buffer().get_pixel(5, 5).expect("should get"), 42.0);
    }

    #[test]
    fn test_band_out_of_range() {
        let multi = MultiBandBuffer::from_bands(
            vec![RasterBuffer::zeros(10, 10, RasterDataType::UInt8)],
            vec![ColorInterpretation::Gray],
        )
        .expect("should create");

        assert!(multi.band(1).is_err());
    }

    #[test]
    fn test_from_interleaved_roundtrip() {
        // Create 3-band 2x2 UInt8 interleaved data: R₁G₁B₁ R₂G₁B₂ ...
        let data = vec![
            10, 20, 30, // pixel (0,0): R=10, G=20, B=30
            40, 50, 60, // pixel (1,0)
            70, 80, 90, // pixel (0,1)
            100, 110, 120, // pixel (1,1)
        ];

        let multi = MultiBandBuffer::from_interleaved(
            &data,
            2,
            2,
            3,
            RasterDataType::UInt8,
            NoDataValue::None,
        )
        .expect("should create from interleaved");

        assert_eq!(multi.band_count(), 3);
        assert_eq!(multi.width(), 2);
        assert_eq!(multi.height(), 2);
        assert_eq!(multi.layout(), PixelLayout::BandInterleavedByPixel);

        // Verify band 0 (Red): 10, 40, 70, 100
        let r_band = multi.band(0).expect("should get");
        assert_eq!(r_band.as_slice::<u8>().expect("slice"), &[10, 40, 70, 100]);

        // Verify band 1 (Green): 20, 50, 80, 110
        let g_band = multi.band(1).expect("should get");
        assert_eq!(g_band.as_slice::<u8>().expect("slice"), &[20, 50, 80, 110]);

        // Verify band 2 (Blue): 30, 60, 90, 120
        let b_band = multi.band(2).expect("should get");
        assert_eq!(b_band.as_slice::<u8>().expect("slice"), &[30, 60, 90, 120]);

        // Roundtrip back to interleaved
        let interleaved = multi.to_interleaved();
        assert_eq!(interleaved, data);
    }

    #[test]
    fn test_from_bsq_roundtrip() {
        // BSQ: all of R, then all of G, then all of B
        let data = vec![
            10, 40, 70, 100, // band 0 (Red)
            20, 50, 80, 110, // band 1 (Green)
            30, 60, 90, 120, // band 2 (Blue)
        ];

        let multi =
            MultiBandBuffer::from_bsq(&data, 2, 2, 3, RasterDataType::UInt8, NoDataValue::None)
                .expect("should create from BSQ");

        assert_eq!(multi.band_count(), 3);
        assert_eq!(multi.layout(), PixelLayout::BandSequential);

        let r_band = multi.band(0).expect("should get");
        assert_eq!(r_band.as_slice::<u8>().expect("slice"), &[10, 40, 70, 100]);

        let bsq = multi.to_bsq();
        assert_eq!(bsq, data);
    }

    #[test]
    fn test_compute_all_statistics() {
        let mut r = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        r.set_pixel(0, 0, 1.0).expect("set");
        r.set_pixel(1, 0, 2.0).expect("set");
        r.set_pixel(0, 1, 3.0).expect("set");
        r.set_pixel(1, 1, 4.0).expect("set");

        let multi = MultiBandBuffer::from_bands(vec![r], vec![ColorInterpretation::Gray])
            .expect("should create");

        let stats = multi.compute_all_statistics().expect("should compute");
        assert_eq!(stats.len(), 1);
        assert!((stats[0].min - 1.0).abs() < 1e-6);
        assert!((stats[0].max - 4.0).abs() < 1e-6);
        assert!((stats[0].mean - 2.5).abs() < 1e-6);
    }

    #[test]
    fn test_band_nodata_override() {
        let buf = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut multi = MultiBandBuffer::from_bands(vec![buf], vec![ColorInterpretation::Gray])
            .expect("should create");

        // Default: no override, uses buffer's nodata
        let band = multi.band(0).expect("get");
        assert_eq!(band.nodata(), NoDataValue::None);

        // Set override
        multi
            .set_band_nodata(0, NoDataValue::Float(-9999.0))
            .expect("should set");

        let band = multi.band(0).expect("get");
        assert_eq!(band.nodata(), NoDataValue::Float(-9999.0));
    }

    #[test]
    fn test_band_mut() {
        let buf = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);
        let mut multi = MultiBandBuffer::from_bands(vec![buf], vec![ColorInterpretation::Gray])
            .expect("should create");

        let band = multi.band_mut(0).expect("should get mut");
        band.set_pixel(0, 0, 99.0).expect("should set");

        let band = multi.band(0).expect("should get");
        assert_eq!(band.buffer().get_pixel(0, 0).expect("should get"), 99.0);
    }

    #[test]
    fn test_into_bands() {
        let r = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);
        let g = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);

        let multi = MultiBandBuffer::from_bands(
            vec![r, g],
            vec![ColorInterpretation::Gray, ColorInterpretation::Alpha],
        )
        .expect("should create");

        let bands = multi.into_bands();
        assert_eq!(bands.len(), 2);
    }

    #[test]
    fn test_from_bil_roundtrip() {
        // 2 bands, 3 rows, 4 cols of u8
        // BIL layout: [row0_b0: 4 bytes, row0_b1: 4 bytes, row1_b0: 4 bytes, ...]
        let bands = 2usize;
        let height = 3usize;
        let width = 4usize;
        // Manually build BIL bytes
        let mut bil: Vec<u8> = Vec::new();
        for row in 0..height {
            for band in 0..bands {
                for col in 0..width {
                    bil.push(((band * 100 + row * 10 + col) & 0xFF) as u8);
                }
            }
        }
        let buf = MultiBandBuffer::from_bil(
            &bil,
            width as u64,
            height as u64,
            bands as u32,
            RasterDataType::UInt8,
            NoDataValue::None,
        )
        .expect("should create from BIL");
        let back = buf.to_bil();
        assert_eq!(bil, back, "BIL roundtrip failed");
    }

    #[test]
    fn test_from_bil_to_bsq_equivalence() {
        // Build the same logical data via BIL and BSQ, verify identical BSQ output
        let (bands, height, width) = (2usize, 2usize, 3usize);
        let mut bsq_data: Vec<u8> = Vec::new();
        for band in 0..bands {
            for row in 0..height {
                for col in 0..width {
                    bsq_data.push((band * 10 + row * 3 + col) as u8);
                }
            }
        }
        let mut bil_data: Vec<u8> = vec![0; bands * height * width];
        for band in 0..bands {
            for row in 0..height {
                for col in 0..width {
                    let bsq_idx = band * height * width + row * width + col;
                    let bil_idx = row * bands * width + band * width + col;
                    bil_data[bil_idx] = bsq_data[bsq_idx];
                }
            }
        }
        let from_bil = MultiBandBuffer::from_bil(
            &bil_data,
            width as u64,
            height as u64,
            bands as u32,
            RasterDataType::UInt8,
            NoDataValue::None,
        )
        .expect("should create from BIL");
        let bsq_out = from_bil.to_bsq();
        assert_eq!(bsq_data, bsq_out, "BSQ equivalence failed");
    }

    #[test]
    fn test_from_bil_mismatched_size() {
        // Wrong size for 2x3x4 layout
        let data: Vec<u8> = vec![0; 5];
        let result =
            MultiBandBuffer::from_bil(&data, 4, 3, 2, RasterDataType::UInt8, NoDataValue::None);
        assert!(result.is_err(), "expected error for size mismatch");
    }

    #[test]
    fn test_from_bil_various_types() {
        // u8 roundtrip for different dimensions
        for &(bands, height, width) in &[(2usize, 3usize, 4usize), (3usize, 2usize, 5usize)] {
            // u8 roundtrip
            let mut bil_u8: Vec<u8> = Vec::new();
            for row in 0..height {
                for band in 0..bands {
                    for col in 0..width {
                        bil_u8.push(((band * 100 + row * 10 + col) & 0xFF) as u8);
                    }
                }
            }
            let buf = MultiBandBuffer::from_bil(
                &bil_u8,
                width as u64,
                height as u64,
                bands as u32,
                RasterDataType::UInt8,
                NoDataValue::None,
            )
            .expect("u8 from_bil should succeed");
            let back = buf.to_bil();
            assert_eq!(
                bil_u8, back,
                "u8 BIL roundtrip failed for {}x{}x{}",
                bands, height, width
            );

            // u16 roundtrip — encode as raw bytes (little-endian)
            let mut bil_u16_raw: Vec<u8> = Vec::new();
            for row in 0..height {
                for band in 0..bands {
                    for col in 0..width {
                        let v: u16 = (band * 1000 + row * 10 + col) as u16;
                        bil_u16_raw.extend_from_slice(&v.to_ne_bytes());
                    }
                }
            }
            let buf_u16 = MultiBandBuffer::from_bil(
                &bil_u16_raw,
                width as u64,
                height as u64,
                bands as u32,
                RasterDataType::UInt16,
                NoDataValue::None,
            )
            .expect("u16 from_bil should succeed");
            let back_u16 = buf_u16.to_bil();
            assert_eq!(
                bil_u16_raw, back_u16,
                "u16 BIL roundtrip failed for {}x{}x{}",
                bands, height, width
            );

            // f32 roundtrip — encode as raw bytes (native-endian)
            let mut bil_f32_raw: Vec<u8> = Vec::new();
            for row in 0..height {
                for band in 0..bands {
                    for col in 0..width {
                        let v: f32 = (band * 100 + row * 10 + col) as f32 + 0.5;
                        bil_f32_raw.extend_from_slice(&v.to_ne_bytes());
                    }
                }
            }
            let buf_f32 = MultiBandBuffer::from_bil(
                &bil_f32_raw,
                width as u64,
                height as u64,
                bands as u32,
                RasterDataType::Float32,
                NoDataValue::None,
            )
            .expect("f32 from_bil should succeed");
            let back_f32 = buf_f32.to_bil();
            assert_eq!(
                bil_f32_raw, back_f32,
                "f32 BIL roundtrip failed for {}x{}x{}",
                bands, height, width
            );
        }
    }

    #[test]
    fn test_default_color_interpretation() {
        let colors_1 = default_color_interpretation(1);
        assert_eq!(colors_1, vec![ColorInterpretation::Gray]);

        let colors_3 = default_color_interpretation(3);
        assert_eq!(
            colors_3,
            vec![
                ColorInterpretation::Red,
                ColorInterpretation::Green,
                ColorInterpretation::Blue,
            ]
        );

        let colors_4 = default_color_interpretation(4);
        assert_eq!(
            colors_4,
            vec![
                ColorInterpretation::Red,
                ColorInterpretation::Green,
                ColorInterpretation::Blue,
                ColorInterpretation::Alpha,
            ]
        );

        let colors_5 = default_color_interpretation(5);
        assert!(
            colors_5
                .iter()
                .all(|c| *c == ColorInterpretation::Undefined)
        );
    }
}
