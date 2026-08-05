//! Opening and reading the datasets a VRT references.
//!
//! A VRT source is not always a plain raster: `gdalwarp -of VRT` writes a
//! warped VRT whose `<SourceDataset>` is itself a VRT mosaic, and nothing stops
//! a `<SimpleSource>` naming one either. This layer therefore dispatches on the
//! file's content and recurses into nested VRTs, with a depth bound so a VRT
//! that references itself fails cleanly instead of exhausting the stack.

use crate::error::{Result, VrtError};
use crate::reader::VrtReader;
use crate::source::PixelRect;
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigeo_geotiff::GeoTiffReader;
use std::path::Path;

/// How deeply VRTs may reference other VRTs.
pub(crate) const MAX_VRT_NESTING: usize = 16;

/// A dataset referenced by a VRT.
pub struct SourceDataset {
    kind: SourceKind,
}

enum SourceKind {
    /// A GeoTIFF leaf.
    GeoTiff(Box<GeoTiffReader<FileDataSource>>),
    /// A nested VRT, read through its own reader.
    Vrt(Box<VrtReader>),
}

impl SourceDataset {
    /// Opens a source dataset.
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened or is not a raster format
    /// the VRT driver can read.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_nested(path, 0)
    }

    /// Opens a source dataset that is itself `depth` levels below a VRT.
    ///
    /// # Errors
    /// Returns an error if the nesting bound is exceeded or the file cannot be
    /// opened.
    pub(crate) fn open_nested<P: AsRef<Path>>(path: P, depth: usize) -> Result<Self> {
        let path = path.as_ref();

        if depth > MAX_VRT_NESTING {
            return Err(VrtError::source_error(
                path.display().to_string(),
                format!("VRT nesting exceeds {MAX_VRT_NESTING} levels (cyclic reference?)"),
            ));
        }

        if looks_like_vrt(path) {
            let reader = VrtReader::open_nested(path, depth + 1)?;
            return Ok(Self {
                kind: SourceKind::Vrt(Box::new(reader)),
            });
        }

        let source = FileDataSource::open(path).map_err(|e| {
            VrtError::source_error(
                path.display().to_string(),
                format!("Failed to open file: {}", e),
            )
        })?;
        let reader = GeoTiffReader::open(source).map_err(|e| {
            VrtError::source_error(
                path.display().to_string(),
                format!("Failed to open as GeoTIFF: {}", e),
            )
        })?;

        Ok(Self {
            kind: SourceKind::GeoTiff(Box::new(reader)),
        })
    }

    /// Raster width in pixels.
    #[must_use]
    pub fn width(&self) -> u64 {
        match &self.kind {
            SourceKind::GeoTiff(r) => r.width(),
            SourceKind::Vrt(r) => r.width(),
        }
    }

    /// Raster height in pixels.
    #[must_use]
    pub fn height(&self) -> u64 {
        match &self.kind {
            SourceKind::GeoTiff(r) => r.height(),
            SourceKind::Vrt(r) => r.height(),
        }
    }

    /// Number of raster bands.
    #[must_use]
    pub fn band_count(&self) -> usize {
        match &self.kind {
            SourceKind::GeoTiff(r) => r.band_count() as usize,
            SourceKind::Vrt(r) => r.band_count(),
        }
    }

    /// Element type of the raster bands.
    #[must_use]
    pub fn data_type(&self) -> RasterDataType {
        match &self.kind {
            SourceKind::GeoTiff(r) => r.data_type().unwrap_or(RasterDataType::UInt8),
            SourceKind::Vrt(r) => r.primary_data_type().unwrap_or(RasterDataType::UInt8),
        }
    }

    /// Georeferencing transform, if the source declares one.
    #[must_use]
    pub fn geo_transform(&self) -> Option<GeoTransform> {
        match &self.kind {
            SourceKind::GeoTiff(r) => r.geo_transform().copied(),
            SourceKind::Vrt(r) => r.geo_transform().copied(),
        }
    }

    /// NoData value declared by the source.
    #[must_use]
    pub fn nodata(&self) -> NoDataValue {
        match &self.kind {
            SourceKind::GeoTiff(r) => r.nodata(),
            SourceKind::Vrt(r) => r.band_nodata(1),
        }
    }

    /// Reads a rectangular window of one band (1-based).
    ///
    /// The window may extend past the raster: the part that lies outside is
    /// left at zero rather than shrinking the returned buffer, so the caller
    /// always receives exactly `window.x_size × window.y_size` samples.
    ///
    /// # Errors
    /// Returns an error if the band is out of range or a block cannot be read.
    pub fn read_window(&self, band: usize, window: PixelRect) -> Result<RasterBuffer> {
        match &self.kind {
            SourceKind::GeoTiff(reader) => self.read_geotiff_window(reader, band, window),
            SourceKind::Vrt(reader) => reader.read_window(band, window),
        }
    }

    /// As [`Self::read_window`], but a window that no source covers yields a
    /// NoData-filled buffer instead of an error.
    ///
    /// This is what a warped read needs: a warp over a sparse mosaic routinely
    /// asks for a rectangle that happens to fall in a gap, which GDAL treats as
    /// "nothing to composite" rather than as a failure (the
    /// `ERROR_OUT_IF_EMPTY_SOURCE_WINDOW=FALSE` warp option).
    ///
    /// # Errors
    /// Returns an error for genuine read failures.
    pub(crate) fn read_window_lenient(
        &self,
        band: usize,
        window: PixelRect,
    ) -> Result<RasterBuffer> {
        match self.read_window(band, window) {
            Err(VrtError::EmptyWindow { .. }) => {
                let data_type = self.data_type();
                let nodata = self.nodata();
                let len = window
                    .x_size
                    .saturating_mul(window.y_size)
                    .saturating_mul(data_type.size_bytes() as u64);
                let len = usize::try_from(len).map_err(|_| {
                    VrtError::invalid_window("Window is too large to allocate on this platform")
                })?;
                let mut data = vec![0u8; len];
                fill_with_nodata(&mut data, data_type, nodata);
                RasterBuffer::new(data, window.x_size, window.y_size, data_type, nodata)
                    .map_err(Into::into)
            }
            other => other,
        }
    }

    /// Reads a GeoTIFF window, touching only the blocks it overlaps.
    ///
    /// The window is clamped to the raster before being handed to the driver
    /// (which rejects out-of-range windows) and the result is blitted into a
    /// full-size buffer. Reading only the overlapping blocks is what makes a
    /// windowed read of a large source affordable — decoding the whole band and
    /// slicing it, as this did before, costs the full band on every call.
    fn read_geotiff_window(
        &self,
        reader: &GeoTiffReader<FileDataSource>,
        band: usize,
        window: PixelRect,
    ) -> Result<RasterBuffer> {
        if band == 0 {
            return Err(VrtError::band_out_of_range(band, self.band_count()));
        }

        let data_type = reader.data_type().unwrap_or(RasterDataType::UInt8);
        let bytes_per_pixel = data_type.size_bytes();
        let nodata = reader.nodata();

        let total = window
            .x_size
            .saturating_mul(window.y_size)
            .saturating_mul(bytes_per_pixel as u64);
        let total = usize::try_from(total).map_err(|_| {
            VrtError::invalid_window("Window is too large to allocate on this platform")
        })?;
        let mut data = vec![0u8; total];

        let (width, height) = (reader.width(), reader.height());
        let clamped = clamp_to_extent(&window, width, height);

        if let Some(rect) = clamped {
            let tile = reader
                .read_window(
                    0,
                    band - 1,
                    rect.x_off,
                    rect.y_off,
                    rect.x_size,
                    rect.y_size,
                )
                .map_err(|e| {
                    VrtError::source_error("source", format!("Failed to read window: {}", e))
                })?;

            blit(&tile, &mut data, &rect, &window, bytes_per_pixel);
        }

        RasterBuffer::new(data, window.x_size, window.y_size, data_type, nodata).map_err(Into::into)
    }
}

/// Intersects a window with the raster extent.
fn clamp_to_extent(window: &PixelRect, width: u64, height: u64) -> Option<PixelRect> {
    PixelRect::new(0, 0, width, height).intersect(window)
}

/// Copies `src` (covering `src_rect`) into `dst` (covering `dst_rect`).
fn blit(src: &[u8], dst: &mut [u8], src_rect: &PixelRect, dst_rect: &PixelRect, pixel_size: usize) {
    let row_bytes = match usize::try_from(src_rect.x_size) {
        Ok(w) => w.saturating_mul(pixel_size),
        Err(_) => return,
    };
    if row_bytes == 0 {
        return;
    }

    let x_shift = src_rect.x_off.saturating_sub(dst_rect.x_off);
    let y_shift = src_rect.y_off.saturating_sub(dst_rect.y_off);

    for row in 0..src_rect.y_size {
        let src_start = match usize::try_from(row) {
            Ok(r) => r.saturating_mul(row_bytes),
            Err(_) => return,
        };
        let dst_row = y_shift.saturating_add(row);
        let dst_start = match (usize::try_from(dst_row), usize::try_from(dst_rect.x_size)) {
            (Ok(r), Ok(w)) => r
                .saturating_mul(w)
                .saturating_add(x_shift as usize)
                .saturating_mul(pixel_size),
            _ => return,
        };

        let (Some(src_row), Some(dst_slice)) = (
            src.get(src_start..src_start + row_bytes),
            dst.get_mut(dst_start..dst_start + row_bytes),
        ) else {
            return;
        };
        dst_slice.copy_from_slice(src_row);
    }
}

/// Fills a raw sample buffer with a NoData value.
pub(crate) fn fill_with_nodata(data: &mut [u8], data_type: RasterDataType, nodata: NoDataValue) {
    let value = match nodata {
        NoDataValue::None => return,
        NoDataValue::Integer(v) => v as f64,
        NoDataValue::Float(v) => v,
    };
    fill_with_value(data, data_type, value);
}

/// Fills a raw sample buffer with a constant value.
pub(crate) fn fill_with_value(data: &mut [u8], data_type: RasterDataType, value: f64) {
    let size = data_type.size_bytes();
    if size == 0 {
        return;
    }

    let mut pattern = vec![0u8; size];
    if crate::warped::write_sample(&mut pattern, 0, value, data_type).is_err() {
        return;
    }

    if pattern.iter().all(|b| *b == 0) {
        // Already zero-filled; nothing to do.
        return;
    }

    for chunk in data.chunks_mut(size) {
        if chunk.len() == size {
            chunk.copy_from_slice(&pattern);
        }
    }
}

/// Whether a path names a VRT, judged by content first and extension second.
fn looks_like_vrt(path: &Path) -> bool {
    use std::io::Read;

    if let Ok(mut file) = std::fs::File::open(path) {
        let mut head = [0u8; 256];
        if let Ok(n) = file.read(&mut head) {
            return crate::is_vrt(&head[..n]);
        }
    }

    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("vrt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_to_extent() {
        let w = PixelRect::new(5, 5, 20, 20);
        let clamped = clamp_to_extent(&w, 10, 10).expect("overlap");
        assert_eq!((clamped.x_off, clamped.y_off), (5, 5));
        assert_eq!((clamped.x_size, clamped.y_size), (5, 5));

        assert!(clamp_to_extent(&PixelRect::new(50, 50, 4, 4), 10, 10).is_none());
    }

    #[test]
    fn test_blit_places_partial_window() {
        // A 2x2 source rect landing at offset (1,1) of a 3x3 destination.
        let src = vec![1u8, 2, 3, 4];
        let mut dst = vec![0u8; 9];
        blit(
            &src,
            &mut dst,
            &PixelRect::new(1, 1, 2, 2),
            &PixelRect::new(0, 0, 3, 3),
            1,
        );
        assert_eq!(dst, vec![0, 0, 0, 0, 1, 2, 0, 3, 4]);
    }

    #[test]
    fn test_fill_with_value() {
        let mut data = vec![0u8; 4];
        fill_with_value(&mut data, RasterDataType::UInt8, 7.0);
        assert_eq!(data, vec![7, 7, 7, 7]);
    }
}
