//! Raster pixel-reading surface of [`Dataset`]: full-band reads, windowed
//! reads, read-into-caller-buffer reads, band iteration, and the clip-window
//! plumbing they share.
//!
//! # Which reader should I use?
//!
//! Every reader below opens the file, decodes only the blocks it needs, and
//! honours any [`PixelWindow`] recorded by [`Dataset::clip`](crate::Dataset::clip).
//! They differ in how many bands they return, who owns the destination memory,
//! and whether the element type is converted on the way out.
//!
//! | Method | Bands | Allocates | Reads | Converts |
//! |---|---|---|---|---|
//! | [`Dataset::read_band`] | one | one `RasterBuffer` (whole band) | every block of the band (or of the clip window) | no — file's element type |
//! | [`Dataset::read_window`] | one | one `RasterBuffer` (window) | only the blocks the window overlaps | no — file's element type |
//! | [`Dataset::read_band_into`] | one | **nothing** — you own `dst` | every block of the band (or of the clip window) | yes — to `T`, fused into the decode |
//! | [`Dataset::read_window_into`] | one | **nothing** — you own `dst` | only the blocks the window overlaps | yes — to `T`, fused into the decode |
//! | [`Dataset::read_interleaved`] | many, interleaved | one `Vec<T>` + fixed scratch | every block once, whatever the band count | yes — to `T`, fused into the decode |
//! | [`Dataset::read_interleaved_into`] | many, interleaved | fixed scratch — you own `dst` | every block once, whatever the band count | yes — to `T`, fused into the decode |
//! | [`Dataset::read_window_interleaved`] | many, interleaved | one `Vec<T>` + fixed scratch | only the blocks the window overlaps, once each | yes — to `T`, fused into the decode |
//! | [`Dataset::read_window_interleaved_into`] | many, interleaved | fixed scratch — you own `dst` | only the blocks the window overlaps, once each | yes — to `T`, fused into the decode |
//!
//! "Allocates nothing" is literal: the single-band `*_into` readers reuse one
//! block-sized scratch buffer inside the driver and scatter decoded samples
//! straight into `dst`, so the peak extra memory is one tile/strip regardless of
//! raster size.  That is the direct equivalent of GDAL's
//! `RasterBand::read_into_slice`.  The interleaved readers keep the same
//! guarantee: their scratch is a fixed handful of block-sized buffers inside the
//! driver plus a list of the band indices, so peak memory is set by the file's
//! block geometry and the band count, never by the raster's dimensions.
//!
//! "Every block once" is also literal, and is the reason the interleaved readers
//! exist as a single call rather than a loop over [`Dataset::read_band_into`].
//! In a chunky GeoTIFF (`PlanarConfiguration = 1` — nearly every RGB, RGBA or
//! multispectral file) one block physically holds every band of the pixels it
//! covers, so `n` single-band reads decompress each block `n` times and throw
//! `n − 1` bands away each pass.  Decompression dominates the read; asking for
//! the bands together pays it once.
//!
//! # One band or many?
//!
//! [`Dataset::read_band`] returns **one** band.  Before 0.2.2 it returned the
//! whole pixel-interleaved image whatever band index it was given; if you are
//! porting code that relied on that, [`Dataset::read_interleaved`] is the
//! replacement and says what it does.
//!
//! # Clip semantics
//!
//! Every reader here works in the dataset's **current** pixel grid.  On a
//! dataset returned by [`Dataset::clip`] that grid is the clipped region, so
//! "the whole band" means the clip window and window coordinates are relative to
//! the clip's upper-left corner.  The dimensions are exactly
//! [`Dataset::width`] × [`Dataset::height`], which is what
//! [`Dataset::read_band_into`] requires `dst.len()` to match.  Clipping is
//! applied by *reading only the clipped blocks*, not by reading the file and
//! discarding pixels.

use crate::{Dataset, OxiGeoError, Result};
use oxigeo_core::buffer::{RasterBuffer, RasterElement};

// Needed by both the GeoTIFF and the VRT read paths, which are independently
// selectable features.
#[cfg(any(feature = "geotiff", feature = "vrt"))]
use crate::DatasetFormat;
#[cfg(any(feature = "geotiff", feature = "vrt"))]
use oxigeo_core::types::RasterDataType;

#[cfg(feature = "geotiff")]
use oxigeo_core::io::FileDataSource;
#[cfg(feature = "geotiff")]
use oxigeo_core::types::NoDataValue;
#[cfg(feature = "geotiff")]
use oxigeo_geotiff::GeoTiffReader;

/// The GeoTIFF reader type every raster read of a local file goes through.
#[cfg(feature = "geotiff")]
type LocalTiffReader = GeoTiffReader<FileDataSource>;

/// A pixel-space sub-rectangle of a raster dataset.
///
/// Coordinates are in the on-disk file's pixel grid: `(col, row)` is the
/// upper-left corner (0-based) and `(width, height)` the size of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelWindow {
    /// Left column (0-based) of the window in the source file.
    pub col: u32,
    /// Top row (0-based) of the window in the source file.
    pub row: u32,
    /// Window width in pixels.
    pub width: u32,
    /// Window height in pixels.
    pub height: u32,
}

/// A read request already resolved into the **source file's** pixel grid.
///
/// Produced by [`Dataset::resolve_source_window`], which folds the dataset's
/// clip window and the caller's window request into a single rectangle so the
/// driver can be asked for exactly those pixels once.
#[cfg(feature = "geotiff")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceWindow {
    /// Left column in the source file.
    x: u64,
    /// Top row in the source file.
    y: u64,
    /// Window width in pixels.
    width: u64,
    /// Window height in pixels.
    height: u64,
    /// `true` when the window covers the entire band of the source file, which
    /// lets the driver take its cheaper full-band path.
    full_band: bool,
}

#[cfg(feature = "geotiff")]
impl SourceWindow {
    /// Number of pixels the window contains.
    fn pixel_count(&self) -> Result<usize> {
        usize::try_from(self.width)
            .ok()
            .and_then(|w| {
                usize::try_from(self.height)
                    .ok()
                    .and_then(|h| w.checked_mul(h))
            })
            .ok_or_else(|| OxiGeoError::Internal {
                message: format!(
                    "window {}×{} overflows the address space",
                    self.width, self.height
                ),
            })
    }
}

/// Crop an interleaved raster byte buffer to a pixel window.
///
/// `data` is `full_width × full_height` pixels, each pixel occupying a fixed
/// stride of bytes (the stride covers every band / sample of that pixel, so
/// this works for both single-band and pixel-interleaved multi-band buffers).
/// The window `(col, row, width, height)` must fit entirely within the source
/// dimensions.
///
/// Returns `None` when the buffer length is not an exact multiple of the pixel
/// count or the window falls outside the source extent — callers treat that as
/// "cannot honour the clip" and surface an error rather than returning wrong
/// pixels.
///
/// The single-band readers no longer need this: they ask the driver for the
/// clipped rectangle directly.  It survives for
/// [`Dataset::convert`](crate::Dataset::convert), which builds a chunky
/// multi-band plane in memory before writing and therefore still crops a
/// materialised buffer.
#[cfg(feature = "geotiff")]
pub(crate) fn crop_interleaved(
    data: &[u8],
    full_width: u32,
    full_height: u32,
    window: PixelWindow,
) -> Option<Vec<u8>> {
    let full_w = full_width as usize;
    let full_h = full_height as usize;
    if full_w == 0 || full_h == 0 {
        return None;
    }
    let total_px = full_w.checked_mul(full_h)?;
    if total_px == 0 || !data.len().is_multiple_of(total_px) {
        return None;
    }
    let stride = data.len() / total_px;
    let col = window.col as usize;
    let row = window.row as usize;
    let w = window.width as usize;
    let h = window.height as usize;
    if col.checked_add(w)? > full_w || row.checked_add(h)? > full_h {
        return None;
    }
    let mut out = Vec::with_capacity(w.saturating_mul(h).saturating_mul(stride));
    for r in 0..h {
        let src_row_start = (row + r) * full_w;
        for c in 0..w {
            let px = (src_row_start + col + c) * stride;
            out.extend_from_slice(&data[px..px + stride]);
        }
    }
    Some(out)
}

impl Dataset {
    /// Read a single raster band by 0-based index and return its pixel data as
    /// a `RasterBuffer`.
    ///
    /// Requires the `geotiff` feature for GeoTIFF datasets.  Other formats
    /// return [`OxiGeoError::NotSupported`].
    ///
    /// `band` is **0-based**: band 0 is the first raster band.
    ///
    /// # Behaviour change in 0.2.2
    ///
    /// This returns **that one band's samples** — `width × height` of them.
    ///
    /// Up to and including 0.2.1 it ignored `band` for multi-band files and
    /// returned the *whole pixel-interleaved image* (`width × height × bands`
    /// samples, `b0 b1 b2 b0 b1 b2 …`), which silently mis-fed every caller that
    /// asked for one band and quietly worked for callers who wanted the lot.  The
    /// single-band case is unchanged; multi-band callers who wanted the
    /// interleaved image should move to [`Self::read_interleaved`], which returns
    /// exactly what the old `read_band` did (and lets you choose and order the
    /// bands).  `read_band(0)` on a 3-band file used to yield three times as many
    /// samples as it does now, so a length check is the quickest way to find
    /// affected code.
    ///
    /// # Cost
    ///
    /// One allocation — the returned buffer — plus one block-sized scratch
    /// inside the driver.  On a clipped dataset only the blocks overlapping the
    /// clip window are read.  If you already own the destination memory, or want
    /// the samples converted to another element type, prefer
    /// [`Self::read_band_into`], which allocates nothing at all.
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not supported.
    /// - [`OxiGeoError::InvalidParameter`] — `band` index is out of range.
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_band(&self, band: u32) -> Result<RasterBuffer> {
        self.validate_band(band)?;

        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, None)?;
            return self.read_buffer_with(&reader, band, window);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, None)?;
            return self.read_vrt_buffer(&reader, band, window);
        }

        Err(self.unsupported("read_band()"))
    }

    /// Read a whole band directly into a caller-supplied buffer, converting to
    /// `T` in one pass.
    ///
    /// This is the fast path this crate recommends, and the direct equivalent of
    /// GDAL's `RasterBand::read_into_slice`: the samples are converted from the
    /// file's element type to `T` *while* the blocks are decoded, so a
    /// `Float32` DEM lands in a `&mut [f64]` with **no** full-size intermediate
    /// buffer, no second pass, and no allocation beyond the `dst` you already
    /// own.
    ///
    /// `dst.len()` must be exactly `self.width() as usize * self.height() as
    /// usize` — one element per pixel of the dataset's **current** extent.  On a
    /// dataset returned by [`Dataset::clip`] that is the clipped extent, and
    /// only the blocks overlapping the clip window are read (see the
    /// module docs above, under `Clip semantics`).
    ///
    /// Conversion is saturating, with floats rounded to nearest (halves away
    /// from zero), matching GDAL's `RasterIO`.
    ///
    /// # Example
    ///
    /// The whole answer to "how do I read a DEM as fast as possible", end to
    /// end:
    ///
    /// ```rust,no_run
    /// use oxigeo::Dataset;
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// let ds = Dataset::open("dem.tif")?;
    ///
    /// // The element type is known from the header, before any pixel is read,
    /// // so the destination can be sized (and typed) up front.
    /// println!("on-disk type: {:?}", ds.data_type());
    ///
    /// let (width, height) = (ds.width() as usize, ds.height() as usize);
    /// let mut dem = vec![0.0f64; width * height];   // the only large allocation
    /// ds.read_band_into(0, &mut dem)?;              // decode + Float32→f64 in one pass
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Bridging to `ndarray`
    ///
    /// `oxigeo` itself has no array dependency, but the buffer above is already
    /// in row-major order, so an `Array2<f64>` is a move away — or, if you want
    /// to decode straight into an array you allocated, hand `read_band_into` the
    /// array's own backing slice:
    ///
    /// ```rust,ignore
    /// use ndarray::Array2;
    ///
    /// let mut grid = Array2::<f64>::zeros((height, width));
    /// let dst = grid.as_slice_mut().expect("standard layout");
    /// ds.read_band_into(0, dst)?;
    /// ```
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type.
    /// - [`OxiGeoError::InvalidParameter`] — `band` is out of range, or
    ///   `dst.len()` is not exactly the band's pixel count (the error names the
    ///   expected length; a wrong length is never silently truncated).
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_band_into<T: RasterElement>(&self, band: u32, dst: &mut [T]) -> Result<()> {
        self.validate_band(band)?;

        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, None)?;
            return self.read_typed_with(&reader, band, window, dst);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, None)?;
            return self.read_vrt_interleaved(&reader, &[band], window, dst);
        }

        let _ = dst;
        Err(self.unsupported("read_band_into()"))
    }

    /// Return a lazy iterator over all raster bands.
    ///
    /// Each call to `Iterator::next()` reads the next band from the underlying
    /// file.  For multi-band GeoTIFF datasets this avoids loading all bands
    /// into memory simultaneously.
    ///
    /// The file is opened — and its IFD chain parsed — **once** for the whole
    /// iteration, not once per band: the reader is created on the first
    /// `next()` and reused for every subsequent band.
    ///
    /// The iterator yields `Result<RasterBuffer>` so that per-band read errors
    /// are propagated without aborting the iteration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oxigeo::Dataset;
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// let ds = Dataset::open("elevation.tif")?;
    /// for band_result in ds.bands() {
    ///     let buf = band_result?;
    ///     println!("band pixels: {}", buf.pixel_count());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn bands(&self) -> BandIter<'_> {
        BandIter {
            dataset: self,
            next_band: 0,
            band_count: self.info.band_count,
            #[cfg(feature = "geotiff")]
            reader: None,
            #[cfg(feature = "geotiff")]
            reader_unavailable: false,
        }
    }

    /// Read a rectangular pixel window from a single raster band.
    ///
    /// Unlike [`Self::read_band`] (which reads the full band), this reads only
    /// the `width × height` sub-rectangle whose upper-left corner is
    /// `(col, row)` in this dataset's pixel grid.  It is the real
    /// pixel-populating primitive behind windowed / tiled access: callers can
    /// walk a [`crate::streaming::TileStream`]'s coordinates and call
    /// `read_window` per tile to obtain actual pixels.
    ///
    /// Coordinates are relative to the dataset's *current* extent, so on a
    /// dataset produced by [`Dataset::clip`] the window is taken within the
    /// clipped region.
    ///
    /// `band` is **0-based**.
    ///
    /// # Cost
    ///
    /// One allocation (the returned buffer).  Only the tiles or strips the
    /// window overlaps are fetched and decoded — a tile-aligned window on a
    /// tiled file touches exactly one tile's worth of bytes, not the band.
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type.
    /// - [`OxiGeoError::InvalidParameter`] — `band` is out of range, the window
    ///   has zero size, or it extends past the dataset extent.
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_window(
        &self,
        band: u32,
        col: u32,
        row: u32,
        width: u32,
        height: u32,
    ) -> Result<RasterBuffer> {
        Self::validate_window_size(width, height)?;
        self.validate_band(band)?;

        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, Some((col, row, width, height)))?;
            return self.read_buffer_with(&reader, band, window);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, Some((col, row, width, height)))?;
            return self.read_vrt_buffer(&reader, band, window);
        }

        let _ = (col, row);
        Err(self.unsupported("read_window()"))
    }

    /// Read a pixel window directly into a caller-supplied buffer, converting to
    /// `T` in one pass.
    ///
    /// The windowed counterpart of [`Self::read_band_into`]: `(col, row)` is the
    /// window's upper-left corner in this dataset's **current** pixel grid
    /// (the clipped grid on a dataset returned by [`Dataset::clip`]), and
    /// `dst.len()` must be exactly `width as usize * height as usize`.
    ///
    /// # Cost
    ///
    /// No allocation whatsoever, and only the tiles or strips the window
    /// overlaps are fetched and decoded.  This is the primitive to use when
    /// walking a large raster tile by tile with a reusable destination buffer.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oxigeo::Dataset;
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// let ds = Dataset::open("dem.tif")?;
    /// let mut tile = vec![0.0f32; 256 * 256];
    /// // Reuse `tile` for every window — no per-tile allocation.
    /// ds.read_window_into(0, 0, 0, 256, 256, &mut tile)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type.
    /// - [`OxiGeoError::InvalidParameter`] — `band` is out of range, the window
    ///   has zero size or extends past the dataset extent, or `dst.len()` is not
    ///   exactly `width × height` (the error names the expected length).
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_window_into<T: RasterElement>(
        &self,
        band: u32,
        col: u32,
        row: u32,
        width: u32,
        height: u32,
        dst: &mut [T],
    ) -> Result<()> {
        Self::validate_window_size(width, height)?;
        self.validate_band(band)?;

        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, Some((col, row, width, height)))?;
            return self.read_typed_with(&reader, band, window, dst);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, Some((col, row, width, height)))?;
            return self.read_vrt_interleaved(&reader, &[band], window, dst);
        }

        let _ = (col, row, dst);
        Err(self.unsupported("read_window_into()"))
    }

    // -- interleaved (multi-band) readers ------------------------------------

    /// Read several bands at once, pixel-interleaved, into a fresh `Vec<T>`.
    ///
    /// This is the multi-band counterpart of [`Self::read_band`] and the
    /// supported replacement for the pre-0.2.2 behaviour of `read_band`, which
    /// returned the whole interleaved image regardless of the band index it was
    /// given (see [`Self::read_band`] for the full note on that change).
    ///
    /// # Band selection
    ///
    /// `bands` names the bands to read, in output order:
    ///
    /// - `None` — every band of the dataset, in file order.  This is the common
    ///   case and costs no allocation to express.
    /// - `Some(&[..])` — exactly those 0-based band indices, in that order.  The
    ///   list may reorder bands (`&[2, 1, 0]` reads an RGB file as BGR), read a
    ///   subset (`&[0, 1, 2]` of a 5-band scene reads three bands and decodes
    ///   only those three), and may repeat an index (`&[0, 0, 0]` expands a
    ///   grey band to three channels).  This mirrors GDAL's `panBandMap`, where
    ///   `nullptr` likewise means "all bands in order".
    ///
    /// # Layout
    ///
    /// The result is `width × height × bands` elements: for every pixel, its
    /// selected bands consecutively (`b0 b1 b2 b0 b1 b2 …`), pixels in row-major
    /// order.  The sample of pixel `(col, row)` in slot `i` is at
    /// `(row * width + col) * bands + i`.
    ///
    /// Unlike [`Self::read_band`] this does **not** return a `RasterBuffer`: that
    /// type describes exactly one sample per pixel and rejects a buffer holding
    /// several.  A typed `Vec<T>` also lets the element conversion stay fused
    /// into the decode, as in [`Self::read_band_into`].
    ///
    /// # Example
    ///
    /// ```rust
    /// use oxigeo::Dataset;
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// # let path = std::env::temp_dir()
    /// #     .join(format!("oxigeo_doc_read_interleaved_{}.tif", std::process::id()));
    /// # struct Cleanup(std::path::PathBuf);
    /// # impl Drop for Cleanup {
    /// #     fn drop(&mut self) {
    /// #         let _ = std::fs::remove_file(&self.0);
    /// #     }
    /// # }
    /// # let _cleanup = Cleanup(path.clone());
    /// # {
    /// #     use oxigeo::builder::{DatasetCreateBuilder, OutputFormat};
    /// #     use oxigeo::{GeoTransform, RasterDataType};
    /// #     let mut writer =
    /// #         DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff).create()?;
    /// #     writer.set_dimensions(2, 2, 3)?;
    /// #     writer.set_data_type(RasterDataType::UInt8);
    /// #     writer.set_geo_transform(GeoTransform::north_up(0.0, 2.0, 1.0, 1.0));
    /// #     // Four RGB pixels: (10,20,30), (11,21,31), (12,22,32), (13,23,33),
    /// #     // handed to the writer band-sequentially, as it expects.
    /// #     let planes: Vec<u8> =
    /// #         [10u8, 20, 30].iter().flat_map(|b| (0..4u8).map(move |p| b + p)).collect();
    /// #     writer.write_all_bands(&planes)?;
    /// #     writer.finalize()?;
    /// # }
    /// # let path = path.to_string_lossy().into_owned();
    /// let ds = Dataset::open(&path)?;
    ///
    /// // Every band, interleaved — the old `read_band` result, now explicit.
    /// let rgb: Vec<u8> = ds.read_interleaved(None)?;
    /// assert_eq!(rgb.len(), 2 * 2 * 3);
    /// assert_eq!(&rgb[..3], &[10, 20, 30]);
    ///
    /// // Just the bands you want, in the order you want them.
    /// let bgr: Vec<u8> = ds.read_interleaved(Some(&[2, 1, 0]))?;
    /// assert_eq!(&bgr[..3], &[30, 20, 10]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Cost
    ///
    /// One allocation for the result, plus the fixed scratch
    /// [`Self::read_interleaved_into`] uses.  Every block the selection touches
    /// is decoded exactly once however many bands are named, so on a chunky file
    /// an `n`-band read costs one pass over the data rather than `n`; and naming
    /// a subset really does skip work — a block no selected band lives in is
    /// never fetched at all.
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type.
    /// - [`OxiGeoError::InvalidParameter`] — `bands` is empty or names an index
    ///   out of range.
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_interleaved<T: RasterElement>(&self, bands: Option<&[u32]>) -> Result<Vec<T>> {
        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, None)?;
            return self.alloc_interleaved(&reader, bands, window);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, None)?;
            let selection = self.resolve_vrt_bands(bands)?;
            let pixels =
                usize::try_from(window.x_size.saturating_mul(window.y_size)).map_err(|_| {
                    OxiGeoError::InvalidParameter {
                        parameter: "window",
                        message: "window is too large for this platform".to_string(),
                    }
                })?;
            let mut out = vec![T::default(); pixels.saturating_mul(selection.len())];
            self.read_vrt_interleaved(&reader, &selection, window, &mut out)?;
            return Ok(out);
        }

        let _ = bands;
        Err(self.unsupported("read_interleaved()"))
    }

    /// Read several bands at once, pixel-interleaved, directly into a
    /// caller-supplied buffer, converting to `T` in one pass.
    ///
    /// The multi-band counterpart of [`Self::read_band_into`], and the fast path
    /// for anyone who used to rely on `read_band` returning the whole
    /// interleaved image.  `dst.len()` must be exactly
    /// `width × height × bands`, where `width`/`height` are the dataset's
    /// **current** (clipped) extent — see `Clip semantics` in the module docs.
    ///
    /// See [`Self::read_interleaved`] for how `bands` selects and orders the
    /// bands, and for the destination layout.
    ///
    /// # Cost
    ///
    /// A fixed handful of block-sized scratch buffers inside the driver, plus a
    /// `bands.len()`-element index list.  Nothing is sized by the raster, and no
    /// full-size intermediate is ever materialised, so peak memory over a
    /// 10000×10000 `f64` read is the same as over a 64×64 one.  When `bands`
    /// names a single band this delegates straight to [`Self::read_band_into`]'s
    /// path, which allocates nothing at all.
    ///
    /// The bands are read *together*, not one after another: each block is
    /// decoded once and every selected band is lifted out of it before it is
    /// discarded.  On a chunky file — `PlanarConfiguration = 1`, which is what
    /// almost every RGB, RGBA or multispectral GeoTIFF is — one block holds all
    /// of a pixel's bands, so this is the difference between decompressing the
    /// file once and decompressing it once per band.  On a planar file a block
    /// holds a single band and there is nothing to share, so the read is one
    /// pass per *distinct* band; a band repeated in `bands` is copied from the
    /// slot that already holds it rather than decoded again.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oxigeo::Dataset;
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// let ds = Dataset::open("scene.tif")?;
    /// let (width, height) = (ds.width() as usize, ds.height() as usize);
    ///
    /// // Bands 3, 2, 1 of a 5-band scene as an RGB f32 image; the two unnamed
    /// // bands are never decoded.
    /// let mut rgb = vec![0.0f32; width * height * 3];
    /// ds.read_interleaved_into(Some(&[3, 2, 1]), &mut rgb)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type.
    /// - [`OxiGeoError::InvalidParameter`] — `bands` is empty or names an index
    ///   out of range, or `dst.len()` is not exactly
    ///   `width × height × bands.len()` (the error names the expected length; a
    ///   wrong length is never silently truncated).
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_interleaved_into<T: RasterElement>(
        &self,
        bands: Option<&[u32]>,
        dst: &mut [T],
    ) -> Result<()> {
        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, None)?;
            let selection = self.resolve_bands(&reader, bands)?;
            return self.read_interleaved_with(&reader, &selection, window, dst);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, None)?;
            let selection = self.resolve_vrt_bands(bands)?;
            return self.read_vrt_interleaved(&reader, &selection, window, dst);
        }

        let _ = (bands, dst);
        Err(self.unsupported("read_interleaved_into()"))
    }

    /// Read a rectangular pixel window of several bands, pixel-interleaved, into
    /// a fresh `Vec<T>`.
    ///
    /// [`Self::read_interleaved`] restricted to the `width × height`
    /// sub-rectangle at `(col, row)` in this dataset's current pixel grid, in the
    /// same way [`Self::read_window`] restricts [`Self::read_band`].  Only the
    /// tiles or strips the window overlaps are fetched and decoded, for each
    /// selected band.
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type.
    /// - [`OxiGeoError::InvalidParameter`] — `bands` is empty or out of range, or
    ///   the window has zero size or extends past the dataset extent.
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_window_interleaved<T: RasterElement>(
        &self,
        bands: Option<&[u32]>,
        col: u32,
        row: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<T>> {
        Self::validate_window_size(width, height)?;

        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, Some((col, row, width, height)))?;
            return self.alloc_interleaved(&reader, bands, window);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, Some((col, row, width, height)))?;
            let selection = self.resolve_vrt_bands(bands)?;
            let pixels =
                usize::try_from(window.x_size.saturating_mul(window.y_size)).map_err(|_| {
                    OxiGeoError::InvalidParameter {
                        parameter: "window",
                        message: "window is too large for this platform".to_string(),
                    }
                })?;
            let mut out = vec![T::default(); pixels.saturating_mul(selection.len())];
            self.read_vrt_interleaved(&reader, &selection, window, &mut out)?;
            return Ok(out);
        }

        let _ = (bands, col, row);
        Err(self.unsupported("read_window_interleaved()"))
    }

    /// Read a rectangular pixel window of several bands, pixel-interleaved,
    /// directly into a caller-supplied buffer, converting to `T` in one pass.
    ///
    /// The windowed counterpart of [`Self::read_interleaved_into`]: `(col, row)`
    /// is the window's upper-left corner in this dataset's **current** pixel grid
    /// and `dst.len()` must be exactly `width × height × bands`.
    ///
    /// This is the primitive for walking a multi-band raster tile by tile with a
    /// reusable interleaved buffer — an RGB tile server's inner loop.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oxigeo::Dataset;
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// let ds = Dataset::open("scene.tif")?;
    /// let mut tile = vec![0u8; 256 * 256 * 3];
    /// // Reuse `tile` for every window — no per-tile allocation.
    /// ds.read_window_interleaved_into(Some(&[0, 1, 2]), 0, 0, 256, 256, &mut tile)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type.
    /// - [`OxiGeoError::InvalidParameter`] — `bands` is empty or out of range, the
    ///   window has zero size or extends past the dataset extent, or `dst.len()`
    ///   is not exactly `width × height × bands.len()`.
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn read_window_interleaved_into<T: RasterElement>(
        &self,
        bands: Option<&[u32]>,
        col: u32,
        row: u32,
        width: u32,
        height: u32,
        dst: &mut [T],
    ) -> Result<()> {
        Self::validate_window_size(width, height)?;

        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            let reader = self.open_geotiff_reader()?;
            let window = self.resolve_source_window(&reader, Some((col, row, width, height)))?;
            let selection = self.resolve_bands(&reader, bands)?;
            return self.read_interleaved_with(&reader, &selection, window, dst);
        }

        #[cfg(feature = "vrt")]
        if matches!(self.info.format, DatasetFormat::Vrt) {
            let reader = self.open_vrt_reader()?;
            let window = self.resolve_vrt_window(&reader, Some((col, row, width, height)))?;
            let selection = self.resolve_vrt_bands(bands)?;
            return self.read_vrt_interleaved(&reader, &selection, window, dst);
        }

        let _ = (bands, col, row, dst);
        Err(self.unsupported("read_window_interleaved_into()"))
    }

    // -- shared validation ---------------------------------------------------

    /// Reject a band index the dataset's metadata says cannot exist.
    fn validate_band(&self, band: u32) -> Result<()> {
        if self.info.band_count > 0 && band >= self.info.band_count {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "band",
                message: format!(
                    "band index {} is out of range (dataset has {} bands)",
                    band, self.info.band_count
                ),
            });
        }
        Ok(())
    }

    /// Reject a degenerate window before any I/O happens.
    fn validate_window_size(width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "window",
                message: format!("window size must be non-zero, got {width}×{height}"),
            });
        }
        Ok(())
    }

    // -- VRT plumbing --------------------------------------------------------

    /// Open the dataset's file as a VRT reader.
    ///
    /// Every pixel-read entry point used to be gated on
    /// `DatasetFormat::GeoTiff`, so a `.vrt` — including a warped VRT, the
    /// subject of cool-japan/oxigeo#15 — answered every read with
    /// `NotSupported` even though the driver behind [`oxigeo_vrt`] could serve
    /// it.
    #[cfg(feature = "vrt")]
    fn open_vrt_reader(&self) -> Result<oxigeo_vrt::VrtReader> {
        oxigeo_vrt::VrtReader::open(&self.path).map_err(|e| {
            OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
                message: format!("failed to open VRT '{}': {e}", self.path),
            })
        })
    }

    /// Fold the clip window and the caller's request into one VRT pixel
    /// rectangle.
    #[cfg(feature = "vrt")]
    fn resolve_vrt_window(
        &self,
        reader: &oxigeo_vrt::VrtReader,
        request: Option<(u32, u32, u32, u32)>,
    ) -> Result<oxigeo_vrt::PixelRect> {
        let (file_w, file_h) = (reader.width(), reader.height());

        let (view_x, view_y, view_w, view_h) = match self.clip_window {
            Some(window) => (
                u64::from(window.col),
                u64::from(window.row),
                u64::from(window.width),
                u64::from(window.height),
            ),
            None => (0, 0, file_w, file_h),
        };

        if view_x.saturating_add(view_w) > file_w || view_y.saturating_add(view_h) > file_h {
            return Err(OxiGeoError::Internal {
                message: format!(
                    "clip window [{view_x},{view_y} {view_w}×{view_h}] does not fit VRT raster {file_w}×{file_h}"
                ),
            });
        }

        let (x, y, width, height) = match request {
            Some((col, row, width, height)) => {
                let (col, row) = (u64::from(col), u64::from(row));
                let (width, height) = (u64::from(width), u64::from(height));
                if col.saturating_add(width) > view_w || row.saturating_add(height) > view_h {
                    return Err(OxiGeoError::InvalidParameter {
                        parameter: "window",
                        message: format!(
                            "window [{col},{row} {width}×{height}] extends past the dataset extent {view_w}×{view_h}"
                        ),
                    });
                }
                (view_x + col, view_y + row, width, height)
            }
            None => (view_x, view_y, view_w, view_h),
        };

        Ok(oxigeo_vrt::PixelRect::new(x, y, width, height))
    }

    /// Read one VRT band's window as a buffer in the band's own element type.
    #[cfg(feature = "vrt")]
    fn read_vrt_buffer(
        &self,
        reader: &oxigeo_vrt::VrtReader,
        band: u32,
        window: oxigeo_vrt::PixelRect,
    ) -> Result<RasterBuffer> {
        // The VRT driver numbers bands from 1; this API numbers them from 0.
        let band_1based = usize::try_from(band)
            .map_err(|_| OxiGeoError::InvalidParameter {
                parameter: "band",
                message: format!("band index {band} does not fit this platform's usize"),
            })?
            .saturating_add(1);

        reader.read_window(band_1based, window).map_err(|e| {
            OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
                message: format!("failed to read VRT '{}' band {band}: {e}", self.path),
            })
        })
    }

    /// Read several VRT bands pixel-interleaved into `dst`, converting to `T`.
    #[cfg(feature = "vrt")]
    fn read_vrt_interleaved<T: RasterElement>(
        &self,
        reader: &oxigeo_vrt::VrtReader,
        bands: &[u32],
        window: oxigeo_vrt::PixelRect,
        dst: &mut [T],
    ) -> Result<()> {
        let pixels =
            usize::try_from(window.x_size.saturating_mul(window.y_size)).map_err(|_| {
                OxiGeoError::InvalidParameter {
                    parameter: "window",
                    message: "window is too large for this platform".to_string(),
                }
            })?;
        let expected =
            pixels
                .checked_mul(bands.len())
                .ok_or_else(|| OxiGeoError::InvalidParameter {
                    parameter: "window",
                    message: "window × band count overflows".to_string(),
                })?;

        if dst.len() != expected {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "dst",
                message: format!(
                    "destination length {} does not match the {expected} samples this read produces",
                    dst.len()
                ),
            });
        }

        let stride = bands.len();
        for (slot, band) in bands.iter().enumerate() {
            let buffer = self.read_vrt_buffer(reader, *band, window)?;
            let samples = buffer.as_bytes();
            let source_type = buffer.data_type();
            let sample_size = source_type.size_bytes();

            for index in 0..pixels {
                let offset = index * sample_size;
                let value = samples
                    .get(offset..offset + sample_size)
                    .and_then(|bytes| read_native_sample(bytes, source_type))
                    .unwrap_or(0.0);
                dst[index * stride + slot] = T::from_raster_f64(value);
            }
        }

        Ok(())
    }

    /// Expand a band selection to a concrete list, defaulting to every band.
    #[cfg(feature = "vrt")]
    fn resolve_vrt_bands(&self, bands: Option<&[u32]>) -> Result<Vec<u32>> {
        match bands {
            Some(list) => {
                if list.is_empty() {
                    return Err(OxiGeoError::InvalidParameter {
                        parameter: "bands",
                        message: "band selection must name at least one band".to_string(),
                    });
                }
                for band in list {
                    self.validate_band(*band)?;
                }
                Ok(list.to_vec())
            }
            None => Ok((0..self.info.band_count).collect()),
        }
    }

    /// The "this format cannot do pixel reads" error, worded per operation.
    fn unsupported(&self, operation: &str) -> OxiGeoError {
        OxiGeoError::NotSupported {
            operation: format!(
                "{operation} is not supported for format '{}' (enable the 'geotiff' feature for GeoTIFF support)",
                self.info.format.driver_name()
            ),
        }
    }

    // -- GeoTIFF plumbing ----------------------------------------------------

    /// Open the dataset's file as a GeoTIFF reader.
    #[cfg(feature = "geotiff")]
    fn open_geotiff_reader(&self) -> Result<LocalTiffReader> {
        let source = FileDataSource::open(&self.path).map_err(|e| {
            OxiGeoError::Io(oxigeo_core::error::IoError::Read {
                message: format!("failed to open '{}': {e}", self.path),
            })
        })?;
        GeoTiffReader::open(source)
    }

    /// Fold the dataset's clip window and an optional caller window request into
    /// a single rectangle in the source file's pixel grid.
    ///
    /// `request` is `(col, row, width, height)` in the dataset's *current*
    /// (possibly clipped) grid; `None` means "the whole current extent".
    #[cfg(feature = "geotiff")]
    fn resolve_source_window(
        &self,
        reader: &LocalTiffReader,
        request: Option<(u32, u32, u32, u32)>,
    ) -> Result<SourceWindow> {
        let file_w = reader.width();
        let file_h = reader.height();

        // The dataset's current view within the file.
        let (view_x, view_y, view_w, view_h) = match self.clip_window {
            Some(window) => (
                u64::from(window.col),
                u64::from(window.row),
                u64::from(window.width),
                u64::from(window.height),
            ),
            None => (0, 0, file_w, file_h),
        };

        // A recorded clip window that does not fit the file it was recorded
        // against is a bug, not a read the caller can be given wrong pixels for.
        if view_x.saturating_add(view_w) > file_w || view_y.saturating_add(view_h) > file_h {
            return Err(OxiGeoError::Internal {
                message: format!(
                    "clip window [{view_x},{view_y} {view_w}×{view_h}] does not fit source raster {file_w}×{file_h}"
                ),
            });
        }

        let (x, y, width, height) = match request {
            Some((col, row, width, height)) => {
                let (col, row) = (u64::from(col), u64::from(row));
                let (width, height) = (u64::from(width), u64::from(height));
                if col.saturating_add(width) > view_w || row.saturating_add(height) > view_h {
                    return Err(OxiGeoError::InvalidParameter {
                        parameter: "window",
                        message: format!(
                            "window [{col},{row} {width}×{height}] extends past dataset extent {view_w}×{view_h}"
                        ),
                    });
                }
                (view_x + col, view_y + row, width, height)
            }
            None => (view_x, view_y, view_w, view_h),
        };

        Ok(SourceWindow {
            x,
            y,
            width,
            height,
            full_band: x == 0 && y == 0 && width == file_w && height == file_h,
        })
    }

    /// Bytes occupied by one sample of `reader`'s bands.
    ///
    /// Derived from the driver's own band geometry rather than from
    /// [`RasterDataType::size_bytes`], so a file whose sample format is not a
    /// recognised [`RasterDataType`] still gets a correctly-sized destination.
    #[cfg(feature = "geotiff")]
    fn sample_size(reader: &LocalTiffReader, data_type: RasterDataType) -> Result<usize> {
        let pixels = reader.band_pixel_count(0)?;
        if pixels == 0 {
            return Ok(data_type.size_bytes());
        }
        Ok(reader.band_byte_len(0)? / pixels)
    }

    /// Read `window` of `band` into a freshly-allocated [`RasterBuffer`].
    ///
    /// Exactly one allocation happens here: the buffer that is returned.
    #[cfg(feature = "geotiff")]
    fn read_buffer_with(
        &self,
        reader: &LocalTiffReader,
        band: u32,
        window: SourceWindow,
    ) -> Result<RasterBuffer> {
        let data_type = reader.data_type().unwrap_or(RasterDataType::UInt8);
        let sample_size = Self::sample_size(reader, data_type)?;
        let len = window
            .pixel_count()?
            .checked_mul(sample_size)
            .ok_or_else(|| OxiGeoError::Internal {
                message: format!(
                    "window {}×{} of {sample_size}-byte samples overflows the address space",
                    window.width, window.height
                ),
            })?;

        let mut bytes = vec![0u8; len];
        if window.full_band {
            reader.read_band_into(0, band as usize, &mut bytes)?;
        } else {
            reader.read_window_into(
                0,
                band as usize,
                window.x,
                window.y,
                window.width,
                window.height,
                &mut bytes,
            )?;
        }

        RasterBuffer::new(
            bytes,
            window.width,
            window.height,
            data_type,
            NoDataValue::None,
        )
        .map_err(|e| OxiGeoError::Internal {
            message: format!("failed to create RasterBuffer: {e}"),
        })
    }

    /// Read `window` of `band` straight into `dst`, converting to `T` on the way.
    ///
    /// Allocates nothing.
    #[cfg(feature = "geotiff")]
    fn read_typed_with<T: RasterElement>(
        &self,
        reader: &LocalTiffReader,
        band: u32,
        window: SourceWindow,
        dst: &mut [T],
    ) -> Result<()> {
        let expected = window.pixel_count()?;
        if dst.len() != expected {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "dst",
                message: format!(
                    "destination buffer must hold exactly {expected} elements ({}×{} pixels), got {}",
                    window.width,
                    window.height,
                    dst.len()
                ),
            });
        }

        if window.full_band {
            reader.read_band_into_typed(0, band as usize, dst)
        } else {
            reader.read_window_into_typed(
                0,
                band as usize,
                window.x,
                window.y,
                window.width,
                window.height,
                dst,
            )
        }
    }

    // -- interleaved plumbing ------------------------------------------------

    /// Turn a caller's band selection into the concrete list of bands to read.
    ///
    /// `None` expands to every band in file order; a caller-supplied list is
    /// borrowed as-is after every index has been range-checked.  The dataset's
    /// recorded band count wins when it is known, so a `Dataset` built from
    /// metadata stays authoritative; otherwise the reader is asked.
    #[cfg(feature = "geotiff")]
    fn resolve_bands<'a>(
        &self,
        reader: &LocalTiffReader,
        bands: Option<&'a [u32]>,
    ) -> Result<std::borrow::Cow<'a, [u32]>> {
        match bands {
            Some(list) => {
                if list.is_empty() {
                    return Err(OxiGeoError::InvalidParameter {
                        parameter: "bands",
                        message: "band selection must name at least one band".to_string(),
                    });
                }
                for &band in list {
                    self.validate_band(band)?;
                }
                Ok(std::borrow::Cow::Borrowed(list))
            }
            None => {
                let count = if self.info.band_count > 0 {
                    self.info.band_count
                } else {
                    reader.band_count()
                };
                if count == 0 {
                    return Err(OxiGeoError::InvalidParameter {
                        parameter: "bands",
                        message: "dataset reports no raster bands to interleave".to_string(),
                    });
                }
                Ok(std::borrow::Cow::Owned((0..count).collect()))
            }
        }
    }

    /// Allocate the interleaved destination and fill it.
    ///
    /// Exactly one allocation beyond what [`Self::read_interleaved_with`] itself
    /// costs: the buffer that is returned.
    #[cfg(feature = "geotiff")]
    fn alloc_interleaved<T: RasterElement>(
        &self,
        reader: &LocalTiffReader,
        bands: Option<&[u32]>,
        window: SourceWindow,
    ) -> Result<Vec<T>> {
        let selection = self.resolve_bands(reader, bands)?;
        let len = window
            .pixel_count()?
            .checked_mul(selection.len())
            .ok_or_else(|| OxiGeoError::Internal {
                message: format!(
                    "{}×{} pixels × {} bands overflows the address space",
                    window.width,
                    window.height,
                    selection.len()
                ),
            })?;

        let mut out = vec![T::default(); len];
        self.read_interleaved_with(reader, &selection, window, &mut out)?;
        Ok(out)
    }

    /// Weave `bands` of `window` into `dst`, pixel-interleaved, converting to `T`.
    ///
    /// Validation lives here — an empty selection and a mis-sized `dst` are
    /// rejected before a single byte is read, so a rejected call never leaves
    /// half a picture behind — and the read itself is handed to the driver's
    /// multi-band entry points, `read_bands_into_typed` and
    /// `read_window_bands_into_typed`.
    ///
    /// That delegation is the whole point.  A chunky GeoTIFF
    /// (`PlanarConfiguration = 1`, which is what almost every RGB, RGBA or
    /// multispectral file is) stores all of a pixel's bands in the *same* block,
    /// so an interleave built out of one-band-at-a-time calls decompresses every
    /// block once per selected band and discards the rest of each decode.
    /// Decompression dominates the read, so that was an `n`× waste on the most
    /// common layout there is.  The driver decodes each block once and pulls
    /// every requested slot out of it before the block is discarded; on a planar
    /// file, where a block holds exactly one band and there is no shared decode
    /// to exploit, it runs one pass per *distinct* band and copies a repeated
    /// band from the slot that already holds it.
    ///
    /// Peak memory stays bounded by the block geometry rather than by the
    /// raster: the driver allocates a fixed handful of scratch buffers per read
    /// (per rayon worker under its `parallel` feature) whatever the raster size,
    /// and this side of the call allocates only the `bands.len()`-element index
    /// list the driver's signature wants.  That guarantee used to be provided
    /// here, by walking the destination in horizontal strips; it now lives where
    /// the decode does.
    #[cfg(feature = "geotiff")]
    fn read_interleaved_with<T: RasterElement>(
        &self,
        reader: &LocalTiffReader,
        bands: &[u32],
        window: SourceWindow,
        dst: &mut [T],
    ) -> Result<()> {
        // `resolve_bands` already rejects an empty selection; re-checking here
        // keeps this entry point self-contained, whoever calls it in future, and
        // makes the facade — not the driver — the one that words the error.
        if bands.is_empty() {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "bands",
                message: "band selection must name at least one band".to_string(),
            });
        }

        let pixels = window.pixel_count()?;
        let slots = bands.len();
        let expected = pixels
            .checked_mul(slots)
            .ok_or_else(|| OxiGeoError::Internal {
                message: format!(
                    "{}×{} pixels × {slots} bands overflows the address space",
                    window.width, window.height
                ),
            })?;
        if dst.len() != expected {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "dst",
                message: format!(
                    "destination buffer must hold exactly {expected} elements ({}×{} pixels × {slots} bands), got {}",
                    window.width,
                    window.height,
                    dst.len()
                ),
            });
        }

        // One band is not an interleave: hand it to the single-band path, which
        // decodes straight into `dst` with no index list and no weave at all.
        if let [band] = bands {
            return self.read_typed_with(reader, *band, window, dst);
        }

        // The driver indexes bands by `usize`; the facade's public surface uses
        // `u32`.  This is the only allocation on this side of the call, and it is
        // sized by the band count, not by the raster.
        let selection = bands
            .iter()
            .map(|&band| {
                usize::try_from(band).map_err(|_| OxiGeoError::Internal {
                    message: format!("band index {band} overflows the address space"),
                })
            })
            .collect::<Result<Vec<usize>>>()?;

        if window.full_band {
            reader.read_bands_into_typed(0, &selection, dst)
        } else {
            reader.read_window_bands_into_typed(
                0,
                &selection,
                window.x,
                window.y,
                window.width,
                window.height,
                dst,
            )
        }
    }
}

/// Lazy iterator over raster bands of a [`Dataset`].
///
/// Created by [`Dataset::bands`].  Each call to [`Iterator::next`] reads the
/// next band from the underlying file and returns `Ok(RasterBuffer)` on
/// success or an `Err` on I/O or format failure.
///
/// The underlying file is opened once, on the first `next()`, and the reader is
/// reused for every band — an N-band file costs one open and one header parse,
/// not N.
pub struct BandIter<'a> {
    /// Reference to the dataset being iterated.
    dataset: &'a Dataset,
    /// Index of the next band to yield.
    next_band: u32,
    /// Total number of bands (cached to avoid repeated accessors).
    band_count: u32,
    /// GeoTIFF reader, opened lazily on the first `next()` and reused after.
    #[cfg(feature = "geotiff")]
    reader: Option<LocalTiffReader>,
    /// Set once opening the reader has failed, so the iterator stops retrying
    /// the shared open and falls back to the per-band path (which reports the
    /// real error for every remaining band, keeping the item count exact).
    #[cfg(feature = "geotiff")]
    reader_unavailable: bool,
}

impl BandIter<'_> {
    /// Read `band` through the cached reader when one can be had.
    ///
    /// Returns `None` when this dataset has no shared-reader fast path (wrong
    /// format, or the open failed), leaving the caller to fall back to
    /// [`Dataset::read_band`].
    #[cfg(feature = "geotiff")]
    fn read_shared(&mut self, band: u32) -> Option<Result<RasterBuffer>> {
        if !matches!(self.dataset.info.format, DatasetFormat::GeoTiff) {
            return None;
        }
        if self.reader.is_none() && !self.reader_unavailable {
            match self.dataset.open_geotiff_reader() {
                Ok(reader) => self.reader = Some(reader),
                Err(_) => self.reader_unavailable = true,
            }
        }
        let reader = self.reader.as_ref()?;
        Some(
            self.dataset
                .resolve_source_window(reader, None)
                .and_then(|window| self.dataset.read_buffer_with(reader, band, window)),
        )
    }
}

impl Iterator for BandIter<'_> {
    type Item = Result<RasterBuffer>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_band >= self.band_count {
            return None;
        }
        let band = self.next_band;
        self.next_band += 1;

        #[cfg(feature = "geotiff")]
        if let Some(result) = self.read_shared(band) {
            return Some(result);
        }

        Some(self.dataset.read_band(band))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.band_count.saturating_sub(self.next_band)) as usize;
        (remaining, Some(remaining))
    }
}

impl core::iter::ExactSizeIterator for BandIter<'_> {}

/// Decode one host-native sample of `data_type` from `bytes`.
///
/// VRT buffers carry samples in the host's byte order (the driver normalises
/// them on decode), so this is a plain reinterpret, not a byte-order swap.
#[cfg(feature = "vrt")]
fn read_native_sample(bytes: &[u8], data_type: RasterDataType) -> Option<f64> {
    let value = match data_type {
        RasterDataType::UInt8 => f64::from(*bytes.first()?),
        RasterDataType::Int8 => f64::from(*bytes.first()? as i8),
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

// Every test below drives a real GeoTIFF through `Dataset`, so the whole module
// — imports included — belongs to the `geotiff` feature; there is nothing left to
// compile without it.
#[cfg(all(test, feature = "geotiff"))]
mod tests {
    // House convention: test helper fns are not covered by clippy.toml's
    // `allow-expect-in-tests`, which only exempts `#[test]` fns themselves.
    #![allow(clippy::expect_used)]

    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_raster_read_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for TempPath {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    use crate::{BoundingBox, DatasetInfo, GeoTransform};
    use oxigeo_core::types::RasterDataType;

    #[test]
    fn test_crop_interleaved_single_band() {
        // 4×4 single-band (1 byte/pixel) buffer 0..16, crop a 2×2 window at (1,1).
        let data: Vec<u8> = (0u8..16).collect();
        let window = PixelWindow {
            col: 1,
            row: 1,
            width: 2,
            height: 2,
        };
        let out = crop_interleaved(&data, 4, 4, window).expect("crop");
        // Rows 1 and 2, cols 1 and 2: {5,6, 9,10}
        assert_eq!(out, vec![5, 6, 9, 10]);
    }

    #[test]
    fn test_crop_interleaved_multiband_stride() {
        // 2×2 image, 3 bytes/pixel (RGB-like). Pixel (x,y) = [x, y, 0].
        let data: Vec<u8> = vec![
            0, 0, 0, /* (0,0) */ 1, 0, 0, /* (1,0) */
            0, 1, 0, /* (0,1) */ 1, 1, 0, /* (1,1) */
        ];
        let window = PixelWindow {
            col: 1,
            row: 0,
            width: 1,
            height: 2,
        };
        let out = crop_interleaved(&data, 2, 2, window).expect("crop");
        // Column x=1, both rows: [1,0,0] then [1,1,0]
        assert_eq!(out, vec![1, 0, 0, 1, 1, 0]);
    }

    #[test]
    fn test_crop_interleaved_out_of_bounds_none() {
        let data: Vec<u8> = (0u8..16).collect();
        let window = PixelWindow {
            col: 3,
            row: 3,
            width: 2,
            height: 2,
        };
        assert!(crop_interleaved(&data, 4, 4, window).is_none());
    }

    pub(crate) fn write_test_geotiff_4x4(path: &std::path::Path) {
        use crate::builder::{DatasetCreateBuilder, OutputFormat};
        let mut writer = DatasetCreateBuilder::new(path, OutputFormat::GeoTiff)
            .create()
            .expect("create writer");
        writer.set_dimensions(4, 4, 1).expect("dims");
        writer.set_data_type(RasterDataType::UInt8);
        writer.set_geo_transform(GeoTransform::north_up(0.0, 4.0, 1.0, 1.0));
        let data: Vec<u8> = (0u8..16).collect();
        writer.write_all_bands(&data).expect("write bands");
        writer.finalize().expect("finalize");
    }

    // Build a `Dataset` that points at a real on-disk 4×4 GeoTIFF and carries an
    // explicit geo-transform, so the clip window can be exercised against real
    // pixel reads independently of what the writer recorded on disk.
    pub(crate) fn dataset_over_4x4(path: &std::path::Path) -> Dataset {
        let gt = GeoTransform::north_up(0.0, 4.0, 1.0, 1.0);
        let info = DatasetInfo {
            format: crate::DatasetFormat::GeoTiff,
            path: Some(path.to_string_lossy().into_owned()),
            width: Some(4),
            height: Some(4),
            band_count: 1,
            geotransform: Some(gt),
            data_type: Some(RasterDataType::UInt8),
            ..DatasetInfo::default()
        };
        Dataset::from_info(path.to_string_lossy().into_owned(), info)
    }

    #[test]
    fn test_read_window_reads_real_pixels() {
        let path = TempPath::new("read_window_test.tif");
        write_test_geotiff_4x4(&path);

        let ds = Dataset::open(path.to_str().expect("path")).expect("open");
        let buf = ds.read_window(0, 1, 1, 2, 2).expect("read window");
        assert_eq!(buf.width(), 2);
        assert_eq!(buf.height(), 2);
        assert_eq!(buf.as_bytes(), &[5u8, 6, 9, 10]);

        // Out-of-bounds window is rejected.
        assert!(ds.read_window(0, 3, 3, 2, 2).is_err());
    }

    #[test]
    fn test_read_band_into_matches_read_band() {
        let path = TempPath::new("read_band_into_unit.tif");
        write_test_geotiff_4x4(&path);

        let ds = Dataset::open(path.to_str().expect("path")).expect("open");
        let buf = ds.read_band(0).expect("read band");

        let mut dst = vec![0u8; 16];
        ds.read_band_into(0, &mut dst).expect("read band into");
        assert_eq!(dst.as_slice(), buf.as_bytes());

        // Widening conversion is fused into the read.
        let mut wide = vec![0.0f64; 16];
        ds.read_band_into(0, &mut wide).expect("read band into f64");
        let expected: Vec<f64> = (0..16).map(|v| v as f64).collect();
        assert_eq!(wide, expected);

        // A wrong-length destination is an error naming the expected length.
        let mut short = vec![0.0f64; 15];
        let err = ds
            .read_band_into(0, &mut short)
            .expect_err("wrong length must error");
        assert!(
            err.to_string().contains("16"),
            "error should name the expected length: {err}"
        );
    }

    #[test]
    fn test_read_into_honours_clip_window() {
        let path = TempPath::new("read_into_clip_unit.tif");
        write_test_geotiff_4x4(&path);

        let ds = dataset_over_4x4(&path);
        let gt = ds.geotransform().copied().expect("gt");
        let (x0, y0) = gt.pixel_to_world(1.0, 1.0);
        let (x1, y1) = gt.pixel_to_world(3.0, 3.0);
        let bbox = BoundingBox::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)).expect("bbox");
        let clipped = ds.clip(bbox).expect("clip");

        // `read_band_into` on a clipped dataset wants exactly the clipped extent
        // and yields exactly the clipped pixels.
        let mut dst = vec![0u8; clipped.width() as usize * clipped.height() as usize];
        clipped.read_band_into(0, &mut dst).expect("clipped read");
        assert_eq!(dst, vec![5u8, 6, 9, 10]);

        // The full-file length is now the wrong length.
        let mut full = vec![0u8; 16];
        assert!(clipped.read_band_into(0, &mut full).is_err());

        // Windows are relative to the clipped grid.
        let mut one = vec![0u8; 1];
        clipped
            .read_window_into(0, 1, 1, 1, 1, &mut one)
            .expect("clipped window");
        assert_eq!(one, vec![10u8]);
    }

    #[test]
    fn test_clip_is_honored_by_statistics() {
        let path = TempPath::new("clip_stats_test.tif");
        write_test_geotiff_4x4(&path);

        let ds = dataset_over_4x4(&path);
        // Full-raster statistics see all 16 pixels.
        let full = ds.statistics(0).expect("full stats");
        assert_eq!(full.valid_count, 16);

        // Build a bbox covering pixel window cols 1..3, rows 1..3 using the
        // dataset's own geo-transform, then clip.
        let gt = ds.geotransform().copied().expect("gt");
        let (x0, y0) = gt.pixel_to_world(1.0, 1.0);
        let (x1, y1) = gt.pixel_to_world(3.0, 3.0);
        let bbox = BoundingBox::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)).expect("bbox");
        let clipped = ds.clip(bbox).expect("clip");
        assert_eq!(clipped.width(), 2, "clipped width");
        assert_eq!(clipped.height(), 2, "clipped height");

        // Statistics on the clipped dataset must reflect ONLY the 4 clipped
        // pixels {5,6,9,10}, not the full raster.
        let clip_stats = clipped.statistics(0).expect("clip stats");
        assert_eq!(clip_stats.valid_count, 4, "clip honored by statistics");
        assert_eq!(clip_stats.min, 5.0);
        assert_eq!(clip_stats.max, 10.0);
    }

    #[test]
    fn test_clip_is_honored_by_convert() {
        let path = TempPath::new("clip_convert_src.tif");
        write_test_geotiff_4x4(&path);
        let out = TempPath::new("clip_convert_out.tif");

        let ds = dataset_over_4x4(&path);
        let gt = ds.geotransform().copied().expect("gt");
        let (x0, y0) = gt.pixel_to_world(1.0, 1.0);
        let (x1, y1) = gt.pixel_to_world(3.0, 3.0);
        let bbox = BoundingBox::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)).expect("bbox");
        let clipped = ds.clip(bbox).expect("clip");

        let converted = clipped
            .convert(
                &out,
                crate::DatasetFormat::GeoTiff,
                crate::ConversionOptions::default(),
            )
            .expect("convert");
        // The written output must carry the clipped 2×2 dimensions, not 4×4.
        assert_eq!(converted.width(), 2);
        assert_eq!(converted.height(), 2);
        let stats = converted.statistics(0).expect("stats");
        assert_eq!(stats.valid_count, 4);
    }
}
