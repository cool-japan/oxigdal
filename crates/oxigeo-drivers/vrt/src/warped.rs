//! Executing the warp described by a `<GDALWarpOptions>` block.
//!
//! The warp runs backwards, as GDAL's does: for every pixel of the requested
//! destination window it computes the destination geo-coordinate, reprojects
//! that into the source CRS, converts to a source pixel coordinate, and
//! resamples. Running backwards is what guarantees every destination pixel gets
//! a value; a forward scatter would leave holes wherever the projection
//! stretches.
//!
//! The `<MaxError>` of an `<ApproxTransformer>` is deliberately not honoured:
//! GDAL uses it to substitute a polynomial approximation of the reprojection
//! for speed, and transforming every pixel exactly is slower but strictly more
//! accurate, so ignoring the bound cannot make the output worse.

use crate::error::{Result, VrtError};
use crate::source::PixelRect;
use crate::source_dataset::SourceDataset;
use crate::srs::resolve_crs;
use crate::warp::{InitDest, WarpKernel, WarpOptions};
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{GeoTransform, RasterDataType};
use oxigeo_proj::{Coordinate, Transformer};

/// Number of probe points per axis used to bound the source window before
/// reading it. The bound is refined exactly afterwards, so this only decides
/// how often the refinement pass is needed, never whether the result is right.
const PROBE_STEPS: u64 = 32;

/// The transformation chain of a warped VRT, resolved and ready to run.
pub(crate) struct WarpEngine {
    /// Warped pixel grid → target CRS.
    dst_geo_transform: GeoTransform,
    /// Source CRS → source pixel grid.
    src_inverse: AffineInverse,
    /// Target CRS → source CRS. `None` when both sides share a CRS.
    transformer: Option<Transformer>,
    /// The interpolation kernel to apply.
    kernel: WarpKernel,
    /// Source raster extent, for clamping.
    src_width: u64,
    /// Source raster extent, for clamping.
    src_height: u64,
}

impl WarpEngine {
    /// Builds the engine from a warp block and the source it warps.
    ///
    /// `dataset_geo_transform` is the warped VRT's own `<GeoTransform>`, used
    /// when the transformer omits `<DstGeoTransform>` (hand-written warped VRTs
    /// commonly do).
    ///
    /// # Errors
    /// Returns an error if either geotransform is missing or degenerate, or if
    /// an SRS cannot be resolved.
    pub(crate) fn new(
        warp: &WarpOptions,
        source: &SourceDataset,
        dataset_geo_transform: Option<GeoTransform>,
    ) -> Result<Self> {
        let transformer_node = warp.transformer.as_ref();

        let dst_geo_transform = transformer_node
            .and_then(|t| t.dst_geo_transform)
            .or(dataset_geo_transform)
            .ok_or_else(|| {
                VrtError::invalid_structure(
                    "Warped VRT has neither <DstGeoTransform> nor a dataset <GeoTransform>",
                )
            })?;

        let src_geo_transform = transformer_node
            .and_then(|t| t.src_geo_transform)
            .or_else(|| source.geo_transform())
            .ok_or_else(|| {
                VrtError::invalid_structure(
                    "Warped VRT has neither <SrcGeoTransform> nor a georeferenced source",
                )
            })?;

        let src_inverse = AffineInverse::new(&src_geo_transform).ok_or_else(|| {
            VrtError::invalid_structure("Warped VRT <SrcGeoTransform> is not invertible")
        })?;

        let transformer = Self::build_transformer(warp)?;

        Ok(Self {
            dst_geo_transform,
            src_inverse,
            transformer,
            kernel: warp.resample_alg.kernel(),
            src_width: source.width(),
            src_height: source.height(),
        })
    }

    /// Builds the target-CRS → source-CRS transformer, or `None` when the warp
    /// stays inside one CRS (a pure resample/regrid, which is a legitimate and
    /// common warped VRT).
    fn build_transformer(warp: &WarpOptions) -> Result<Option<Transformer>> {
        let Some(reprojection) = warp
            .transformer
            .as_ref()
            .and_then(|t| t.reprojection.as_ref())
        else {
            return Ok(None);
        };

        let (Some(source_srs), Some(target_srs)) =
            (&reprojection.source_srs, &reprojection.target_srs)
        else {
            return Ok(None);
        };

        let source_crs = resolve_crs(source_srs)?;
        let target_crs = resolve_crs(target_srs)?;

        if source_crs.is_equivalent(&target_crs) {
            return Ok(None);
        }

        // Destination → source: the warp samples backwards.
        Transformer::new(target_crs, source_crs)
            .map(Some)
            .map_err(|e| {
                VrtError::invalid_structure(format!(
                    "Cannot build warp transformer between the VRT's SRS pair: {}",
                    e
                ))
            })
    }

    /// Maps a destination pixel coordinate to a source pixel coordinate.
    ///
    /// Both are continuous coordinates in GDAL's convention: integer values sit
    /// on pixel *corners*, so the centre of pixel `(i, j)` is `(i + 0.5, j +
    /// 0.5)`.
    fn dst_to_src(&self, dst_x: f64, dst_y: f64) -> Option<(f64, f64)> {
        let gt = &self.dst_geo_transform;
        let geo_x = gt.origin_x + dst_x * gt.pixel_width + dst_y * gt.row_rotation;
        let geo_y = gt.origin_y + dst_x * gt.col_rotation + dst_y * gt.pixel_height;

        let (src_geo_x, src_geo_y) = match &self.transformer {
            Some(transformer) => {
                let coord = transformer.transform(&Coordinate::new(geo_x, geo_y)).ok()?;
                (coord.x, coord.y)
            }
            None => (geo_x, geo_y),
        };

        if !src_geo_x.is_finite() || !src_geo_y.is_finite() {
            return None;
        }

        let (px, py) = self.src_inverse.apply(src_geo_x, src_geo_y);
        if px.is_finite() && py.is_finite() {
            Some((px, py))
        } else {
            None
        }
    }

    /// The integer source-pixel footprint the kernel reads for a sample point.
    fn tap_range(&self, px: f64, py: f64) -> Option<(i64, i64, i64, i64)> {
        let (x0, y0, x1, y1) = match self.kernel {
            WarpKernel::Nearest => {
                let x = px.floor();
                let y = py.floor();
                (x, y, x, y)
            }
            WarpKernel::Bilinear => {
                let x = (px - 0.5).floor();
                let y = (py - 0.5).floor();
                (x, y, x + 1.0, y + 1.0)
            }
        };

        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return None;
        }
        // Reject coordinates that cannot be represented as pixel indices at all
        // rather than saturating them into a bogus in-range footprint.
        const LIMIT: f64 = 1.0e15;
        if x0.abs() > LIMIT || y0.abs() > LIMIT || x1.abs() > LIMIT || y1.abs() > LIMIT {
            return None;
        }

        Some((x0 as i64, y0 as i64, x1 as i64, y1 as i64))
    }

    /// Estimates the source window a destination window needs, by probing a
    /// grid over it.
    fn probe_source_rect(&self, window: &PixelRect) -> Option<PixelRect> {
        let mut bounds = PixelBounds::default();

        let steps_x = PROBE_STEPS.min(window.x_size).max(1);
        let steps_y = PROBE_STEPS.min(window.y_size).max(1);

        for sy in 0..=steps_y {
            let dst_y =
                window.y_off as f64 + (window.y_size as f64) * (sy as f64) / (steps_y as f64);
            for sx in 0..=steps_x {
                let dst_x =
                    window.x_off as f64 + (window.x_size as f64) * (sx as f64) / (steps_x as f64);
                if let Some((px, py)) = self.dst_to_src(dst_x, dst_y)
                    && let Some(taps) = self.tap_range(px, py)
                {
                    bounds.include(taps);
                }
            }
        }

        // A probe grid samples the boundary at finite resolution, so pad by a
        // margin before reading; the exact footprint is verified afterwards.
        bounds.to_rect(self.src_width, self.src_height, 2)
    }

    /// Warps one band into a freshly allocated destination buffer.
    ///
    /// # Errors
    /// Returns an error if the source cannot be read or the destination window
    /// is too large to allocate.
    pub(crate) fn warp_band(
        &self,
        source: &SourceDataset,
        src_band: usize,
        window: &PixelRect,
        params: &WarpBandParams,
    ) -> Result<Vec<u8>> {
        let pixel_size = params.data_type.size_bytes();
        let len = window
            .x_size
            .saturating_mul(window.y_size)
            .saturating_mul(pixel_size as u64);
        let len = usize::try_from(len).map_err(|_| {
            VrtError::invalid_window("Warped window is too large to allocate on this platform")
        })?;

        let mut output = vec![0u8; len];
        self.initialise(&mut output, params);

        let Some(mut read_rect) = self.probe_source_rect(window) else {
            // Nothing of the source projects into this window.
            return Ok(output);
        };

        // At most two passes: the first uses the probed rectangle, and if any
        // destination pixel turned out to need a source pixel outside it, the
        // second uses the exact footprint measured during the first. The second
        // pass is therefore guaranteed to be sufficient — the warp never
        // silently substitutes NoData for a source pixel that exists.
        for pass in 0..2 {
            let buffer = source.read_window_lenient(src_band, read_rect)?;
            let mut missed = PixelBounds::default();

            self.render(
                window,
                &read_rect,
                &buffer,
                params,
                &mut output,
                &mut missed,
            );

            if pass == 1 {
                break;
            }
            match missed.to_rect(self.src_width, self.src_height, 0) {
                Some(needed) => {
                    read_rect = union(&read_rect, &needed);
                    // Re-initialise: the next pass rewrites every pixel.
                    self.initialise(&mut output, params);
                }
                None => break,
            }
        }

        Ok(output)
    }

    /// Fills the destination buffer according to `INIT_DEST`.
    fn initialise(&self, output: &mut [u8], params: &WarpBandParams) {
        match params.init_dest {
            InitDest::None => output.fill(0),
            InitDest::NoData => {
                output.fill(0);
                if let Some(nodata) = params.dst_nodata {
                    crate::source_dataset::fill_with_value(output, params.data_type, nodata);
                }
            }
            InitDest::Value(value) => {
                output.fill(0);
                crate::source_dataset::fill_with_value(output, params.data_type, value);
            }
        }
    }

    /// Resamples every destination pixel of `window`, recording any source
    /// pixel the read rectangle did not cover.
    fn render(
        &self,
        window: &PixelRect,
        read_rect: &PixelRect,
        buffer: &RasterBuffer,
        params: &WarpBandParams,
        output: &mut [u8],
        missed: &mut PixelBounds,
    ) {
        let bytes = buffer.as_bytes();
        let src_type = buffer.data_type();

        for row in 0..window.y_size {
            let dst_y = (window.y_off + row) as f64 + 0.5;
            for col in 0..window.x_size {
                let dst_x = (window.x_off + col) as f64 + 0.5;

                let Some((px, py)) = self.dst_to_src(dst_x, dst_y) else {
                    continue;
                };
                let Some(taps) = self.tap_range(px, py) else {
                    continue;
                };

                // Entirely outside the source: legitimately NoData.
                if taps.2 < 0
                    || taps.3 < 0
                    || taps.0 >= self.src_width as i64
                    || taps.1 >= self.src_height as i64
                {
                    continue;
                }

                if !covers(read_rect, taps, self.src_width, self.src_height) {
                    missed.include(taps);
                    continue;
                }

                let Some(value) = self.sample(bytes, src_type, read_rect, px, py, params) else {
                    continue;
                };

                let idx = (row * window.x_size + col) as usize;
                let _ = write_sample(output, idx, value, params.data_type);
            }
        }
    }

    /// Resamples one point out of the source buffer.
    fn sample(
        &self,
        bytes: &[u8],
        src_type: RasterDataType,
        rect: &PixelRect,
        px: f64,
        py: f64,
        params: &WarpBandParams,
    ) -> Option<f64> {
        match self.kernel {
            WarpKernel::Nearest => {
                let x = px.floor() as i64;
                let y = py.floor() as i64;
                self.fetch(bytes, src_type, rect, x, y, params)
            }
            WarpKernel::Bilinear => {
                let fx = px - 0.5;
                let fy = py - 0.5;
                let x0 = fx.floor();
                let y0 = fy.floor();
                let tx = fx - x0;
                let ty = fy - y0;
                let (x0, y0) = (x0 as i64, y0 as i64);

                let weights = [
                    ((x0, y0), (1.0 - tx) * (1.0 - ty)),
                    ((x0 + 1, y0), tx * (1.0 - ty)),
                    ((x0, y0 + 1), (1.0 - tx) * ty),
                    ((x0 + 1, y0 + 1), tx * ty),
                ];

                // NoData samples are dropped and the remaining weights
                // renormalised, so a pixel next to a NoData gap keeps a real
                // value instead of being dragged toward the fill value.
                let mut total = 0.0f64;
                let mut acc = 0.0f64;
                for ((x, y), weight) in weights {
                    if weight <= 0.0 {
                        continue;
                    }
                    // Clamp to the source edge, matching GDAL's edge handling.
                    let cx = x.clamp(0, self.src_width.saturating_sub(1) as i64);
                    let cy = y.clamp(0, self.src_height.saturating_sub(1) as i64);
                    if let Some(value) = self.fetch(bytes, src_type, rect, cx, cy, params) {
                        acc += value * weight;
                        total += weight;
                    }
                }

                if total > 0.0 { Some(acc / total) } else { None }
            }
        }
    }

    /// Reads one source pixel, honouring the source NoData value.
    fn fetch(
        &self,
        bytes: &[u8],
        src_type: RasterDataType,
        rect: &PixelRect,
        x: i64,
        y: i64,
        params: &WarpBandParams,
    ) -> Option<f64> {
        if x < 0 || y < 0 || x as u64 >= self.src_width || y as u64 >= self.src_height {
            return None;
        }

        let local_x = (x as u64).checked_sub(rect.x_off)?;
        let local_y = (y as u64).checked_sub(rect.y_off)?;
        if local_x >= rect.x_size || local_y >= rect.y_size {
            return None;
        }

        let idx = usize::try_from(local_y * rect.x_size + local_x).ok()?;
        let value = read_sample(bytes, idx, src_type)?;

        if let Some(nodata) = params.src_nodata
            && (value - nodata).abs() <= f64::EPSILON
        {
            return None;
        }

        Some(value)
    }
}

/// Per-band parameters of a warped read.
pub(crate) struct WarpBandParams {
    /// Element type of the destination band.
    pub(crate) data_type: RasterDataType,
    /// Source NoData value, if the warp declares one.
    pub(crate) src_nodata: Option<f64>,
    /// Destination NoData value, if the warp declares one.
    pub(crate) dst_nodata: Option<f64>,
    /// Destination initialisation policy.
    pub(crate) init_dest: InitDest,
}

/// Whether `rect` covers every tap of `taps` that exists in the source.
fn covers(rect: &PixelRect, taps: (i64, i64, i64, i64), width: u64, height: u64) -> bool {
    let (x0, y0, x1, y1) = taps;
    let lo_x = x0.max(0);
    let lo_y = y0.max(0);
    let hi_x = x1.min(width.saturating_sub(1) as i64);
    let hi_y = y1.min(height.saturating_sub(1) as i64);

    if lo_x > hi_x || lo_y > hi_y {
        return true; // nothing in range to cover
    }

    lo_x as u64 >= rect.x_off
        && lo_y as u64 >= rect.y_off
        && (hi_x as u64) < rect.x_off + rect.x_size
        && (hi_y as u64) < rect.y_off + rect.y_size
}

/// Smallest rectangle containing both inputs.
fn union(a: &PixelRect, b: &PixelRect) -> PixelRect {
    let x0 = a.x_off.min(b.x_off);
    let y0 = a.y_off.min(b.y_off);
    let x1 = (a.x_off + a.x_size).max(b.x_off + b.x_size);
    let y1 = (a.y_off + a.y_size).max(b.y_off + b.y_size);
    PixelRect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Accumulates an integer pixel bounding box.
#[derive(Default)]
struct PixelBounds {
    bounds: Option<(i64, i64, i64, i64)>,
}

impl PixelBounds {
    fn include(&mut self, taps: (i64, i64, i64, i64)) {
        self.bounds = Some(match self.bounds {
            None => taps,
            Some((x0, y0, x1, y1)) => (
                x0.min(taps.0),
                y0.min(taps.1),
                x1.max(taps.2),
                y1.max(taps.3),
            ),
        });
    }

    /// Clamps the accumulated box to a raster extent, padded by `pad` pixels.
    fn to_rect(&self, width: u64, height: u64, pad: i64) -> Option<PixelRect> {
        let (x0, y0, x1, y1) = self.bounds?;
        if width == 0 || height == 0 {
            return None;
        }

        let x0 = (x0 - pad).max(0);
        let y0 = (y0 - pad).max(0);
        let x1 = (x1 + pad).min(width as i64 - 1);
        let y1 = (y1 + pad).min(height as i64 - 1);

        if x0 > x1 || y0 > y1 {
            return None;
        }

        Some(PixelRect::new(
            x0 as u64,
            y0 as u64,
            (x1 - x0 + 1) as u64,
            (y1 - y0 + 1) as u64,
        ))
    }
}

/// The inverse of an affine geotransform.
struct AffineInverse {
    origin_x: f64,
    origin_y: f64,
    /// Row of the inverse matrix producing the pixel column.
    px_from_x: f64,
    px_from_y: f64,
    /// Row of the inverse matrix producing the pixel row.
    py_from_x: f64,
    py_from_y: f64,
}

impl AffineInverse {
    fn new(gt: &GeoTransform) -> Option<Self> {
        let det = gt.pixel_width * gt.pixel_height - gt.row_rotation * gt.col_rotation;
        if !det.is_finite() || det.abs() < f64::MIN_POSITIVE {
            return None;
        }

        Some(Self {
            origin_x: gt.origin_x,
            origin_y: gt.origin_y,
            px_from_x: gt.pixel_height / det,
            px_from_y: -gt.row_rotation / det,
            py_from_x: -gt.col_rotation / det,
            py_from_y: gt.pixel_width / det,
        })
    }

    fn apply(&self, geo_x: f64, geo_y: f64) -> (f64, f64) {
        let dx = geo_x - self.origin_x;
        let dy = geo_y - self.origin_y;
        (
            dx * self.px_from_x + dy * self.px_from_y,
            dx * self.py_from_x + dy * self.py_from_y,
        )
    }
}

/// Reads sample `index` from a host-native sample buffer.
pub(crate) fn read_sample(buffer: &[u8], index: usize, data_type: RasterDataType) -> Option<f64> {
    let size = data_type.size_bytes();
    let offset = index.checked_mul(size)?;
    let bytes = buffer.get(offset..offset.checked_add(size)?)?;

    let value = match data_type {
        RasterDataType::UInt8 => f64::from(bytes[0]),
        RasterDataType::Int8 => f64::from(bytes[0] as i8),
        RasterDataType::UInt16 => f64::from(u16::from_ne_bytes(bytes.try_into().ok()?)),
        RasterDataType::Int16 => f64::from(i16::from_ne_bytes(bytes.try_into().ok()?)),
        RasterDataType::UInt32 => f64::from(u32::from_ne_bytes(bytes.try_into().ok()?)),
        RasterDataType::Int32 => f64::from(i32::from_ne_bytes(bytes.try_into().ok()?)),
        RasterDataType::Float32 => f64::from(f32::from_ne_bytes(bytes.try_into().ok()?)),
        RasterDataType::Float64 => f64::from_ne_bytes(bytes.try_into().ok()?),
        _ => return None,
    };

    Some(value)
}

/// Writes `value` as sample `index` of a host-native sample buffer.
///
/// # Errors
/// Returns an error if the index is out of bounds or the type is unsupported.
pub(crate) fn write_sample(
    buffer: &mut [u8],
    index: usize,
    value: f64,
    data_type: RasterDataType,
) -> Result<()> {
    let size = data_type.size_bytes();
    let offset = index
        .checked_mul(size)
        .ok_or_else(|| VrtError::invalid_window("Sample index overflows"))?;
    let end = offset
        .checked_add(size)
        .ok_or_else(|| VrtError::invalid_window("Sample index overflows"))?;
    let slot = buffer
        .get_mut(offset..end)
        .ok_or_else(|| VrtError::invalid_window("Sample index out of bounds"))?;

    // Rounding to nearest before the cast matches GDAL's RasterIO: a plain
    // `as` truncates, which biases every warped integer band downward.
    match data_type {
        RasterDataType::UInt8 => slot[0] = value.round().clamp(0.0, 255.0) as u8,
        RasterDataType::Int8 => slot[0] = value.round().clamp(-128.0, 127.0) as i8 as u8,
        RasterDataType::UInt16 => {
            slot.copy_from_slice(&(value.round().clamp(0.0, 65535.0) as u16).to_ne_bytes());
        }
        RasterDataType::Int16 => {
            slot.copy_from_slice(&(value.round().clamp(-32768.0, 32767.0) as i16).to_ne_bytes());
        }
        RasterDataType::UInt32 => {
            slot.copy_from_slice(
                &(value.round().clamp(0.0, f64::from(u32::MAX)) as u32).to_ne_bytes(),
            );
        }
        RasterDataType::Int32 => {
            slot.copy_from_slice(
                &(value
                    .round()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
                    .to_ne_bytes(),
            );
        }
        RasterDataType::Float32 => slot.copy_from_slice(&(value as f32).to_ne_bytes()),
        RasterDataType::Float64 => slot.copy_from_slice(&value.to_ne_bytes()),
        _ => return Err(VrtError::invalid_source("Unsupported warp data type")),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gt(origin_x: f64, px: f64, origin_y: f64, py: f64) -> GeoTransform {
        GeoTransform {
            origin_x,
            pixel_width: px,
            row_rotation: 0.0,
            origin_y,
            col_rotation: 0.0,
            pixel_height: py,
        }
    }

    #[test]
    fn test_affine_inverse_roundtrip() {
        let transform = gt(100.0, 0.01, 20.0, -0.01);
        let inverse = AffineInverse::new(&transform).expect("invertible");

        // Pixel (30, 40) centre → geo → back.
        let (px, py) = (30.5, 40.5);
        let geo_x = transform.origin_x + px * transform.pixel_width;
        let geo_y = transform.origin_y + py * transform.pixel_height;
        let (rx, ry) = inverse.apply(geo_x, geo_y);
        assert!((rx - px).abs() < 1e-9, "{rx}");
        assert!((ry - py).abs() < 1e-9, "{ry}");
    }

    #[test]
    fn test_affine_inverse_rejects_degenerate() {
        assert!(AffineInverse::new(&gt(0.0, 0.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn test_write_sample_rounds() {
        let mut buf = vec![0u8; 2];
        write_sample(&mut buf, 0, 12.6, RasterDataType::UInt8).expect("write");
        assert_eq!(buf[0], 13, "warped integer bands must round, not truncate");
        write_sample(&mut buf, 1, 12.4, RasterDataType::UInt8).expect("write");
        assert_eq!(buf[1], 12);
    }

    #[test]
    fn test_write_sample_saturates() {
        let mut buf = vec![0u8; 1];
        write_sample(&mut buf, 0, 1e9, RasterDataType::UInt8).expect("write");
        assert_eq!(buf[0], 255);
        write_sample(&mut buf, 0, -5.0, RasterDataType::UInt8).expect("write");
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_read_sample_bounds() {
        let buf = [1u8, 2, 3];
        assert_eq!(read_sample(&buf, 2, RasterDataType::UInt8), Some(3.0));
        assert_eq!(read_sample(&buf, 3, RasterDataType::UInt8), None);
    }

    #[test]
    fn test_bounds_to_rect_clamps() {
        let mut bounds = PixelBounds::default();
        bounds.include((-5, -5, 3, 3));
        let rect = bounds.to_rect(10, 10, 0).expect("rect");
        assert_eq!((rect.x_off, rect.y_off), (0, 0));
        assert_eq!((rect.x_size, rect.y_size), (4, 4));

        // Entirely outside the raster.
        let mut outside = PixelBounds::default();
        outside.include((-20, -20, -10, -10));
        assert!(outside.to_rect(10, 10, 0).is_none());
    }

    #[test]
    fn test_union() {
        let u = union(&PixelRect::new(0, 0, 2, 2), &PixelRect::new(5, 5, 2, 2));
        assert_eq!((u.x_off, u.y_off, u.x_size, u.y_size), (0, 0, 7, 7));
    }

    #[test]
    fn test_covers() {
        let rect = PixelRect::new(0, 0, 10, 10);
        assert!(covers(&rect, (1, 1, 2, 2), 100, 100));
        assert!(!covers(&rect, (9, 9, 10, 10), 100, 100));
        // Taps entirely outside the source need no coverage.
        assert!(covers(&rect, (200, 200, 201, 201), 100, 100));
    }
}
