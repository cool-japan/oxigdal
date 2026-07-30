//! Per-band raster statistics: [`BandStatistics`] and
//! [`Dataset::statistics`](crate::Dataset::statistics).
//!
//! The computation reuses the same clip-window plumbing as the pixel readers in
//! [`crate::raster_read`], so statistics of a clipped dataset describe the
//! clipped region rather than the whole file.

use crate::{Dataset, OxiGeoError, Result};

#[cfg(feature = "geotiff")]
use crate::DatasetFormat;

/// Statistics for a single raster band.
///
/// Returned by [`Dataset::statistics`].
#[derive(Debug, Clone, PartialEq)]
pub struct BandStatistics {
    /// 0-based band index.
    pub band: u32,
    /// Minimum valid pixel value (non-nodata, finite).
    pub min: f64,
    /// Maximum valid pixel value (non-nodata, finite).
    pub max: f64,
    /// Arithmetic mean of valid pixels.
    pub mean: f64,
    /// Population standard deviation of valid pixels.
    pub std_dev: f64,
    /// Count of valid (non-nodata, finite) pixels.
    pub valid_count: u64,
}

impl Dataset {
    /// Compute per-band raster statistics (min / max / mean / std_dev / valid_count).
    ///
    /// Currently supported for GeoTIFF datasets (requires the `geotiff` feature).
    /// For all other formats or when the feature flag is absent the method returns
    /// [`OxiGeoError::NotSupported`].
    ///
    /// `band` is **0-based**: band 0 is the first raster band.
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — format is not a supported raster type or
    ///   the required feature flag is disabled.
    /// - [`OxiGeoError::InvalidParameter`] — `band` index is out of range.
    /// - [`OxiGeoError::Io`] / [`OxiGeoError::Format`] — underlying read failure.
    pub fn statistics(&self, band: u32) -> Result<BandStatistics> {
        self.compute_band_statistics(band)
    }

    /// Inner implementation for [`Self::statistics`].
    fn compute_band_statistics(&self, band: u32) -> Result<BandStatistics> {
        // Validate band range against known band count (only when we have metadata)
        if self.info.band_count > 0 && band >= self.info.band_count {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "band",
                message: format!(
                    "band index {} is out of range (dataset has {} bands)",
                    band, self.info.band_count
                ),
            });
        }

        // Dispatch to the GeoTIFF reader path when the feature is compiled in.
        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            return self.statistics_geotiff(band);
        }

        Err(OxiGeoError::NotSupported {
            operation: format!(
                "statistics() is not supported for format '{}' (enable the 'geotiff' feature for GeoTIFF support)",
                self.info.format.driver_name()
            ),
        })
    }

    /// GeoTIFF-specific statistics reader.
    ///
    /// Delegates the pixel fetch to [`Dataset::read_band`], which already reads
    /// only the blocks the dataset's current (possibly clipped) extent covers —
    /// so statistics of a clipped dataset cost a windowed read, not a full-band
    /// read followed by a crop.
    #[cfg(feature = "geotiff")]
    fn statistics_geotiff(&self, band: u32) -> Result<BandStatistics> {
        let buf = self.read_band(band)?;
        let buf_stats = buf.compute_statistics()?;

        Ok(BandStatistics {
            band,
            min: buf_stats.min,
            max: buf_stats.max,
            mean: buf_stats.mean,
            std_dev: buf_stats.std_dev,
            valid_count: buf_stats.valid_count,
        })
    }
}
