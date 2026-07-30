//! Band-aware, layout-agnostic raster scanning shared by the raster QC checks.
//!
//! # Why this exists
//!
//! The NoData and radiometric scanners used to walk a raster with
//! `CogReader::read_tile` and de-interleave the samples themselves, assuming
//! every block held `SamplesPerPixel` interleaved samples per pixel. That is
//! only true for `PlanarConfiguration = 1` (chunky). In a planar file
//! (`PlanarConfiguration = 2`) each block holds **one** band's plane and the
//! blocks are stored `SamplesPerPixel × TilesPerImage` in plane-major order, so
//! the old loops
//!
//! * iterated only the first plane's block grid, and
//! * ran off the end of every block after `1/SamplesPerPixel` of its pixels,
//!   because they expected `spp × bytes_per_sample` per pixel and found one.
//!
//! The result was a scanner that inspected roughly `1/SamplesPerPixel` of the
//! data, attributed those samples to the wrong bands, and then reported the file
//! clean — the worst possible failure mode for a QC tool
//! (cool-japan/oxigeo#14).
//!
//! # What it does instead
//!
//! Reads go through [`GeoTiffReader::read_window_into`], the driver's band-aware
//! read engine, which de-interleaves a chunky band and plane-selects a planar
//! one. The scan walks the raster in full-width horizontal stripes so the
//! visitor always receives whole rows of exactly one band, whatever the file's
//! block layout is, and never more than [`SCAN_CHUNK_MAX_BYTES`] at a time.
//!
//! The cost of correctness on a chunky file is that its blocks are decoded once
//! per band rather than once in total; `SamplesPerPixel` is small (1–4 in
//! practice) and these validators are offline tools.
//!
//! # Byte order
//!
//! The samples a visitor receives are in the **host's** byte order, whatever the
//! file's `II`/`MM` header says: the driver normalises decoded samples exactly
//! once, on the way out of block decode (see the *Byte order of decoded samples*
//! section of `oxigeo_geotiff`'s crate docs). Nothing in this crate may swap them
//! again — this module used to re-parse the TIFF header and hand the file's byte
//! order to both sample decoders, which was the right compensation while the
//! driver returned file-order bytes and is a data-corrupting double swap now
//! (cool-japan/oxigeo#14).

use oxigeo_core::io::DataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::tiff::SampleFormat;

use crate::error::{QcError, QcResult};

/// Upper bound, in bytes, on the stripe a scan holds in memory at once.
///
/// Bounds peak memory independently of raster size; a stripe is never smaller
/// than one pixel row, so an extremely wide raster still makes progress.
const SCAN_CHUNK_MAX_BYTES: u64 = 8 << 20;

/// The sample layout a scanner needs, probed once per file.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RasterScan {
    /// Raster width in pixels (full resolution).
    pub(crate) width: u64,
    /// Raster height in pixels (full resolution).
    pub(crate) height: u64,
    /// Number of bands.
    pub(crate) band_count: usize,
    /// Element type of every band.
    pub(crate) data_type: RasterDataType,
    /// TIFF sample format, derived from `data_type` (which the driver itself
    /// derives from `SampleFormat` + `BitsPerSample`, so the two always agree).
    pub(crate) sample_format: SampleFormat,
    /// Bytes in one sample of one band.
    pub(crate) bytes_per_sample: usize,
    /// Rows of one band read per stripe.
    chunk_rows: u64,
}

impl RasterScan {
    /// Probes `reader` for the layout the scanners need.
    ///
    /// There is deliberately no byte-order field: [`scan_band`] yields
    /// host-native samples (see the module docs). If a *report* ever needs to
    /// name the file's on-disk order, call `GeoTiffReader::byte_order()` at the
    /// point of use rather than threading it through the scan.
    ///
    /// # Errors
    /// Returns an error if the file declares a sample type this crate cannot
    /// interpret.
    pub(crate) fn probe<S: DataSource>(reader: &GeoTiffReader<S>) -> QcResult<Self> {
        let data_type = reader
            .data_type()
            .ok_or_else(|| QcError::RasterError("data type unknown".to_string()))?;
        let bytes_per_sample = data_type.size_bytes();
        if bytes_per_sample == 0 {
            return Err(QcError::RasterError(format!(
                "unsupported sample type {data_type:?} (zero bytes per sample)"
            )));
        }

        let width = reader.width();
        let row_bytes = width.max(1).saturating_mul(bytes_per_sample as u64);
        let budget_rows = (SCAN_CHUNK_MAX_BYTES / row_bytes.max(1)).max(1);
        // Prefer whole tile rows so a tiled file's blocks are decoded once per
        // stripe, but never exceed the memory budget.
        let chunk_rows = match reader.tile_size() {
            Some((_, tile_height)) if tile_height > 0 => {
                let tile_height = u64::from(tile_height);
                if tile_height <= budget_rows {
                    tile_height * (budget_rows / tile_height)
                } else {
                    budget_rows
                }
            }
            _ => budget_rows,
        };

        Ok(Self {
            width,
            height: reader.height(),
            band_count: reader.band_count() as usize,
            data_type,
            sample_format: sample_format_of(data_type),
            bytes_per_sample,
            chunk_rows: chunk_rows.max(1),
        })
    }

    /// Total pixel count of one band.
    pub(crate) const fn total_pixels(&self) -> u64 {
        self.width * self.height
    }
}

/// Host-native sample readers shared by the two scanners.
///
/// [`scan_band`] yields samples already normalised to the host's byte order, so
/// every reader here is a plain `from_ne_bytes`. They exist as one named place
/// rather than as scattered `from_ne_bytes` calls precisely so that the next
/// person to wonder "which endianness do QC samples arrive in?" finds the answer
/// attached to the code that answers it (cool-japan/oxigeo#14).
///
/// Each returns `None` when the slice is shorter than the sample it names, so a
/// truncated block degrades to "no match" instead of panicking.
pub(crate) mod native {
    macro_rules! native_reader {
        ($name:ident, $ty:ty, $width:literal) => {
            #[doc = concat!("Reads one host-native `", stringify!($ty), "` from the front of `bytes`.")]
            pub(crate) fn $name(bytes: &[u8]) -> Option<$ty> {
                let head: [u8; $width] = bytes.get(..$width)?.try_into().ok()?;
                Some(<$ty>::from_ne_bytes(head))
            }
        };
    }

    native_reader!(read_u16, u16, 2);
    native_reader!(read_i16, i16, 2);
    native_reader!(read_u32, u32, 4);
    native_reader!(read_i32, i32, 4);
    native_reader!(read_u64, u64, 8);
    native_reader!(read_i64, i64, 8);
    native_reader!(read_f32, f32, 4);
    native_reader!(read_f64, f64, 8);
}

/// Maps an element type back onto the TIFF `SampleFormat` that produced it.
///
/// `ImageInfo::data_type()` is `RasterDataType::from_tiff_sample_format(format,
/// bits)`, so the format is fully recoverable from the element type and the
/// scanners need not carry the raw tag around.
const fn sample_format_of(data_type: RasterDataType) -> SampleFormat {
    match data_type {
        RasterDataType::UInt8
        | RasterDataType::UInt16
        | RasterDataType::UInt32
        | RasterDataType::UInt64 => SampleFormat::UnsignedInteger,
        RasterDataType::Int8
        | RasterDataType::Int16
        | RasterDataType::Int32
        | RasterDataType::Int64 => SampleFormat::SignedInteger,
        RasterDataType::Float32 | RasterDataType::Float64 => SampleFormat::IeeeFloatingPoint,
        RasterDataType::CFloat32 | RasterDataType::CFloat64 => SampleFormat::ComplexFloatingPoint,
    }
}

/// Streams one band of the full-resolution image in full-width stripes.
///
/// `visit(first_row, rows, samples)` receives `rows × width` samples of `band`
/// alone, row-major, starting at pixel row `first_row`, in the **host's** byte
/// order. Chunky files are de-interleaved and planar files are plane-selected by
/// the driver, so the visitor never sees another band's samples.
///
/// # Errors
/// Propagates read/decode errors from the driver and whatever `visit` returns.
pub(crate) fn scan_band<S, F>(
    reader: &GeoTiffReader<S>,
    scan: &RasterScan,
    band: usize,
    mut visit: F,
) -> QcResult<()>
where
    S: DataSource,
    F: FnMut(u64, &[u8]) -> QcResult<()>,
{
    if scan.width == 0 || scan.height == 0 {
        return Ok(());
    }
    if band >= scan.band_count {
        return Err(QcError::RasterError(format!(
            "band {band} is out of range for a {}-band raster",
            scan.band_count
        )));
    }

    let row_bytes = usize::try_from(scan.width)
        .ok()
        .and_then(|w| w.checked_mul(scan.bytes_per_sample))
        .ok_or_else(|| {
            QcError::RasterError(format!(
                "raster row of {} samples does not fit in memory on this target",
                scan.width
            ))
        })?;

    let mut buffer: Vec<u8> = Vec::new();
    let mut first_row = 0u64;
    while first_row < scan.height {
        let rows = scan.chunk_rows.min(scan.height - first_row);
        let len = usize::try_from(rows)
            .ok()
            .and_then(|r| r.checked_mul(row_bytes))
            .ok_or_else(|| {
                QcError::RasterError("scan stripe does not fit in memory".to_string())
            })?;
        buffer.resize(len, 0);
        reader
            .read_window_into(0, band, 0, first_row, scan.width, rows, &mut buffer[..len])
            .map_err(|e| QcError::RasterError(format!("read_window_into failed: {e}")))?;
        visit(first_row, &buffer[..len])?;
        first_row += rows;
    }

    Ok(())
}
