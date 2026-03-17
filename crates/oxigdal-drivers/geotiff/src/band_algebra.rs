//! Raster Band Algebra and Spectral Indices
//!
//! This module provides:
//! - [`Band`]: A single raster band with nodata support
//! - [`BandMath`]: Element-wise band arithmetic operations
//! - [`SpectralIndex`]: Remote sensing spectral index computations
//! - [`NodataMask`]: Nodata mask creation and manipulation
//! - [`BandStack`]: Multi-band raster container with per-pixel statistics
//! - [`ThresholdClassifier`]: Threshold-based band classification
//! - [`AlgebraError`]: Error types for band algebra operations

use thiserror::Error;

// ─── Error type ────────────────────────────────────────────────────────────

/// Errors that can occur during band algebra operations.
#[derive(Debug, Error, PartialEq)]
pub enum AlgebraError {
    /// The two bands have incompatible dimensions.
    #[error("dimension mismatch: band A is {a:?}, band B is {b:?}")]
    DimensionMismatch { a: (u32, u32), b: (u32, u32) },

    /// The band contains no pixels.
    #[error("band is empty (zero pixels)")]
    EmptyBand,

    /// All pixels in the band are nodata.
    #[error("all pixels are nodata — cannot compute statistics")]
    AllNodata,

    /// An invalid parameter was supplied.
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
}

// ─── Band ──────────────────────────────────────────────────────────────────

/// A single raster band storing `f64` pixel values.
///
/// Pixels that equal `nodata` (or NaN when `nodata` is `None`) are treated as
/// invalid in all arithmetic and statistical operations.
#[derive(Debug, Clone)]
pub struct Band {
    /// Pixel values in row-major order (row 0 first, left to right).
    pub data: Vec<f64>,
    /// Width of the band in pixels.
    pub width: u32,
    /// Height of the band in pixels.
    pub height: u32,
    /// Optional nodata sentinel value.
    pub nodata: Option<f64>,
    /// Optional human-readable band name (e.g. `"NIR"`, `"Red"`).
    pub name: Option<String>,
}

impl Band {
    /// Creates a new band with the given data and dimensions.
    ///
    /// # Panics
    /// Does not panic, but `data.len()` should equal `width * height` for
    /// correct behaviour.
    #[must_use]
    pub fn new(data: Vec<f64>, width: u32, height: u32) -> Self {
        Self {
            data,
            width,
            height,
            nodata: None,
            name: None,
        }
    }

    /// Builder — sets the nodata sentinel value.
    #[must_use]
    pub fn with_nodata(mut self, nodata: f64) -> Self {
        self.nodata = Some(nodata);
        self
    }

    /// Builder — sets the band name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Returns the total number of pixels (`width × height`).
    #[must_use]
    pub fn pixel_count(&self) -> u32 {
        self.width.saturating_mul(self.height)
    }

    /// Returns the number of pixels that are not nodata.
    #[must_use]
    pub fn valid_count(&self) -> u64 {
        self.data.iter().filter(|&&v| !self.is_nodata(v)).count() as u64
    }

    /// Returns the pixel value at `(x, y)`, or `None` if out of bounds.
    #[must_use]
    pub fn pixel_at(&self, x: u32, y: u32) -> Option<f64> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.data.get((y * self.width + x) as usize).copied()
    }

    /// Returns `true` if `v` is considered a nodata value.
    ///
    /// When `self.nodata` is `Some(nd)`, an exact bitwise comparison is made
    /// (both `v` and `nd` are NaN → nodata; `v == nd` and both finite → nodata).
    /// When `self.nodata` is `None`, only NaN values are treated as nodata.
    #[must_use]
    pub fn is_nodata(&self, v: f64) -> bool {
        match self.nodata {
            Some(nd) => {
                // NaN-safe: two NaNs are considered equal (both nodata)
                (v.is_nan() && nd.is_nan()) || (!v.is_nan() && !nd.is_nan() && v == nd)
            }
            None => v.is_nan(),
        }
    }

    /// Sets the pixel at `(x, y)` to `value`.  Returns `true` on success,
    /// `false` if the coordinates are out of bounds.
    pub fn set_pixel(&mut self, x: u32, y: u32, value: f64) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = (y * self.width + x) as usize;
        if let Some(cell) = self.data.get_mut(idx) {
            *cell = value;
            true
        } else {
            false
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    /// Returns an iterator over valid pixel values.
    fn valid_pixels(&self) -> impl Iterator<Item = f64> + '_ {
        self.data.iter().copied().filter(|&v| !self.is_nodata(v))
    }

    /// Checks that `self` and `other` have identical dimensions.
    fn check_dims(&self, other: &Band) -> Result<(), AlgebraError> {
        if self.width != other.width || self.height != other.height {
            Err(AlgebraError::DimensionMismatch {
                a: (self.width, self.height),
                b: (other.width, other.height),
            })
        } else {
            Ok(())
        }
    }
}

// ─── BandMath ──────────────────────────────────────────────────────────────

/// Element-wise band arithmetic operations.
///
/// For all binary operations the result pixel is nodata when *either* input
/// pixel is nodata.  The nodata value of the *first* band is propagated to the
/// result.
pub struct BandMath;

impl BandMath {
    /// Element-wise addition: `result[i] = a[i] + b[i]`.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn add(a: &Band, b: &Band) -> Result<Band, AlgebraError> {
        a.check_dims(b)?;
        let nodata = a.nodata;
        let data = binary_op(a, b, |x, y| x + y);
        Ok(Band::new(data, a.width, a.height).maybe_nodata(nodata))
    }

    /// Element-wise subtraction: `result[i] = a[i] - b[i]`.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn subtract(a: &Band, b: &Band) -> Result<Band, AlgebraError> {
        a.check_dims(b)?;
        let nodata = a.nodata;
        let data = binary_op(a, b, |x, y| x - y);
        Ok(Band::new(data, a.width, a.height).maybe_nodata(nodata))
    }

    /// Element-wise multiplication: `result[i] = a[i] * b[i]`.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn multiply(a: &Band, b: &Band) -> Result<Band, AlgebraError> {
        a.check_dims(b)?;
        let nodata = a.nodata;
        let data = binary_op(a, b, |x, y| x * y);
        Ok(Band::new(data, a.width, a.height).maybe_nodata(nodata))
    }

    /// Element-wise division: `result[i] = a[i] / b[i]`.
    ///
    /// Pixels where `b[i] == 0` **or** either input is nodata are set to
    /// nodata in the result.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn divide(a: &Band, b: &Band) -> Result<Band, AlgebraError> {
        a.check_dims(b)?;
        let nodata_val = a.nodata.unwrap_or(f64::NAN);
        let data: Vec<f64> = a
            .data
            .iter()
            .zip(b.data.iter())
            .map(|(&av, &bv)| {
                if a.is_nodata(av) || b.is_nodata(bv) || bv == 0.0 {
                    nodata_val
                } else {
                    av / bv
                }
            })
            .collect();
        Ok(Band::new(data, a.width, a.height).maybe_nodata(a.nodata))
    }

    /// Adds a scalar to every pixel: `result[i] = a[i] + scalar`.
    ///
    /// Nodata pixels remain nodata.
    #[must_use]
    pub fn add_scalar(a: &Band, scalar: f64) -> Band {
        let nodata = a.nodata;
        let data: Vec<f64> = a
            .data
            .iter()
            .map(|&v| if a.is_nodata(v) { v } else { v + scalar })
            .collect();
        Band::new(data, a.width, a.height).maybe_nodata(nodata)
    }

    /// Multiplies every pixel by a scalar: `result[i] = a[i] * scalar`.
    ///
    /// Nodata pixels remain nodata.
    #[must_use]
    pub fn multiply_scalar(a: &Band, scalar: f64) -> Band {
        let nodata = a.nodata;
        let data: Vec<f64> = a
            .data
            .iter()
            .map(|&v| if a.is_nodata(v) { v } else { v * scalar })
            .collect();
        Band::new(data, a.width, a.height).maybe_nodata(nodata)
    }

    /// Clamps all valid pixels to `[min, max]`.
    ///
    /// Nodata pixels are left unchanged.
    #[must_use]
    pub fn clamp(a: &Band, min: f64, max: f64) -> Band {
        let nodata = a.nodata;
        let data: Vec<f64> = a
            .data
            .iter()
            .map(|&v| {
                if a.is_nodata(v) {
                    v
                } else {
                    v.max(min).min(max)
                }
            })
            .collect();
        Band::new(data, a.width, a.height).maybe_nodata(nodata)
    }

    /// Applies an arbitrary function `f` to every valid pixel.
    ///
    /// Nodata pixels are passed through unchanged.
    #[must_use]
    pub fn apply<F: Fn(f64) -> f64>(a: &Band, f: F) -> Band {
        let nodata = a.nodata;
        let data: Vec<f64> = a
            .data
            .iter()
            .map(|&v| if a.is_nodata(v) { v } else { f(v) })
            .collect();
        Band::new(data, a.width, a.height).maybe_nodata(nodata)
    }

    /// Normalises all valid pixels to the `[0, 1]` range using min/max
    /// scaling.
    ///
    /// When all valid pixels have the same value (range == 0), returns
    /// [`AlgebraError::AllNodata`] (they would all map to 0 anyway and the
    /// normalisation is degenerate).
    ///
    /// # Errors
    /// - [`AlgebraError::EmptyBand`] — no pixels at all.
    /// - [`AlgebraError::AllNodata`] — no valid (non-nodata) pixels, or all
    ///   valid pixels are equal (range = 0).
    pub fn normalize(a: &Band) -> Result<Band, AlgebraError> {
        if a.data.is_empty() {
            return Err(AlgebraError::EmptyBand);
        }

        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for v in a.valid_pixels() {
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }

        if min.is_infinite() {
            // No valid pixels found
            return Err(AlgebraError::AllNodata);
        }

        let range = max - min;
        if range == 0.0 {
            return Err(AlgebraError::AllNodata);
        }

        let nodata = a.nodata;
        let data: Vec<f64> = a
            .data
            .iter()
            .map(|&v| if a.is_nodata(v) { v } else { (v - min) / range })
            .collect();
        Ok(Band::new(data, a.width, a.height).maybe_nodata(nodata))
    }
}

// ─── SpectralIndex ─────────────────────────────────────────────────────────

/// Common remote sensing spectral indices.
///
/// All indices propagate nodata from both input bands (if either input pixel is
/// nodata the output pixel is nodata).  The nodata sentinel is taken from the
/// first band argument.
pub struct SpectralIndex;

impl SpectralIndex {
    /// NDVI = (NIR − Red) / (NIR + Red)
    ///
    /// Result is clamped to `[−1, 1]`.  Pixels where the denominator equals 0
    /// are set to nodata.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn ndvi(nir: &Band, red: &Band) -> Result<Band, AlgebraError> {
        nir.check_dims(red)?;
        let nodata_val = nir.nodata.unwrap_or(f64::NAN);
        let data: Vec<f64> = nir
            .data
            .iter()
            .zip(red.data.iter())
            .map(|(&n, &r)| {
                if nir.is_nodata(n) || red.is_nodata(r) {
                    return nodata_val;
                }
                let denom = n + r;
                if denom == 0.0 {
                    nodata_val
                } else {
                    ((n - r) / denom).max(-1.0).min(1.0)
                }
            })
            .collect();
        Ok(Band::new(data, nir.width, nir.height)
            .maybe_nodata(nir.nodata)
            .with_name("NDVI"))
    }

    /// EVI = 2.5 × (NIR − Red) / (NIR + 6·Red − 7.5·Blue + 1)
    ///
    /// MODIS-standard coefficients.  Pixels where the denominator equals 0 are
    /// set to nodata.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn evi(nir: &Band, red: &Band, blue: &Band) -> Result<Band, AlgebraError> {
        nir.check_dims(red)?;
        nir.check_dims(blue)?;
        let nodata_val = nir.nodata.unwrap_or(f64::NAN);
        let data: Vec<f64> = nir
            .data
            .iter()
            .zip(red.data.iter())
            .zip(blue.data.iter())
            .map(|((&n, &r), &b)| {
                if nir.is_nodata(n) || red.is_nodata(r) || blue.is_nodata(b) {
                    return nodata_val;
                }
                let denom = n + 6.0 * r - 7.5 * b + 1.0;
                if denom == 0.0 {
                    nodata_val
                } else {
                    2.5 * (n - r) / denom
                }
            })
            .collect();
        Ok(Band::new(data, nir.width, nir.height)
            .maybe_nodata(nir.nodata)
            .with_name("EVI"))
    }

    /// NDWI = (Green − NIR) / (Green + NIR)
    ///
    /// Positive values indicate water; negative values indicate non-water.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn ndwi(green: &Band, nir: &Band) -> Result<Band, AlgebraError> {
        normalized_difference(green, nir, "NDWI")
    }

    /// NDSI = (Green − SWIR) / (Green + SWIR)
    ///
    /// Snow index.  Positive values indicate snow/ice.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn ndsi(green: &Band, swir: &Band) -> Result<Band, AlgebraError> {
        normalized_difference(green, swir, "NDSI")
    }

    /// SAVI = ((NIR − Red) / (NIR + Red + L)) × (1 + L)
    ///
    /// Soil-adjusted vegetation index.  Typical value for `L` is `0.5`.
    ///
    /// # Errors
    /// - [`AlgebraError::DimensionMismatch`] when dimensions differ.
    /// - [`AlgebraError::InvalidParameter`] when `L < 0`.
    pub fn savi(nir: &Band, red: &Band, l: f64) -> Result<Band, AlgebraError> {
        if l < 0.0 {
            return Err(AlgebraError::InvalidParameter(format!(
                "L must be ≥ 0, got {l}"
            )));
        }
        nir.check_dims(red)?;
        let nodata_val = nir.nodata.unwrap_or(f64::NAN);
        let data: Vec<f64> = nir
            .data
            .iter()
            .zip(red.data.iter())
            .map(|(&n, &r)| {
                if nir.is_nodata(n) || red.is_nodata(r) {
                    return nodata_val;
                }
                let denom = n + r + l;
                if denom == 0.0 {
                    nodata_val
                } else {
                    ((n - r) / denom) * (1.0 + l)
                }
            })
            .collect();
        Ok(Band::new(data, nir.width, nir.height)
            .maybe_nodata(nir.nodata)
            .with_name("SAVI"))
    }

    /// NBR = (NIR − SWIR) / (NIR + SWIR)
    ///
    /// Normalised Burn Ratio.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn nbr(nir: &Band, swir: &Band) -> Result<Band, AlgebraError> {
        normalized_difference(nir, swir, "NBR")
    }

    /// BSI = ((Red + SWIR) − (NIR + Blue)) / ((Red + SWIR) + (NIR + Blue))
    ///
    /// Bare Soil Index.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn bsi(red: &Band, swir: &Band, nir: &Band, blue: &Band) -> Result<Band, AlgebraError> {
        red.check_dims(swir)?;
        red.check_dims(nir)?;
        red.check_dims(blue)?;
        let nodata_val = red.nodata.unwrap_or(f64::NAN);
        let data: Vec<f64> = red
            .data
            .iter()
            .zip(swir.data.iter())
            .zip(nir.data.iter())
            .zip(blue.data.iter())
            .map(|(((&r, &sw), &n), &b)| {
                if red.is_nodata(r) || swir.is_nodata(sw) || nir.is_nodata(n) || blue.is_nodata(b) {
                    return nodata_val;
                }
                let pos = r + sw;
                let neg = n + b;
                let denom = pos + neg;
                if denom == 0.0 {
                    nodata_val
                } else {
                    (pos - neg) / denom
                }
            })
            .collect();
        Ok(Band::new(data, red.width, red.height)
            .maybe_nodata(red.nodata)
            .with_name("BSI"))
    }

    /// MNDWI = (Green − SWIR) / (Green + SWIR)
    ///
    /// Modified NDWI (uses SWIR instead of NIR).
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] when dimensions differ.
    pub fn mndwi(green: &Band, swir: &Band) -> Result<Band, AlgebraError> {
        let mut result = normalized_difference(green, swir, "MNDWI")?;
        result.name = Some("MNDWI".to_string());
        Ok(result)
    }
}

// ─── NodataMask ────────────────────────────────────────────────────────────

/// A boolean validity mask for a raster band.
///
/// `true` = valid pixel, `false` = nodata pixel.
#[derive(Debug, Clone)]
pub struct NodataMask {
    /// Validity flags in row-major order.
    pub mask: Vec<bool>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl NodataMask {
    /// Creates a mask from a [`Band`], marking nodata pixels as invalid.
    #[must_use]
    pub fn from_band(band: &Band) -> Self {
        let mask = band.data.iter().map(|&v| !band.is_nodata(v)).collect();
        Self {
            mask,
            width: band.width,
            height: band.height,
        }
    }

    /// Creates a mask from a raw slice of values and an explicit nodata
    /// sentinel.
    #[must_use]
    pub fn from_value(data: &[f64], nodata: f64) -> Self {
        let len = data.len();
        let mask = data.iter().map(|&v| v != nodata).collect();
        // We don't know the spatial dimensions here, so use len×1
        Self {
            mask,
            width: len as u32,
            height: 1,
        }
    }

    /// Creates a mask where every pixel is valid.
    #[must_use]
    pub fn all_valid(width: u32, height: u32) -> Self {
        let n = (width as usize).saturating_mul(height as usize);
        Self {
            mask: vec![true; n],
            width,
            height,
        }
    }

    /// Creates a mask where every pixel is invalid (nodata).
    #[must_use]
    pub fn all_invalid(width: u32, height: u32) -> Self {
        let n = (width as usize).saturating_mul(height as usize);
        Self {
            mask: vec![false; n],
            width,
            height,
        }
    }

    /// Returns the number of valid pixels.
    #[must_use]
    pub fn valid_count(&self) -> u64 {
        self.mask.iter().filter(|&&v| v).count() as u64
    }

    /// Returns the intersection of two masks (pixel is valid only when valid
    /// in *both* masks).
    ///
    /// Lengths are zip-matched; if they differ, the result has the shorter
    /// length.
    #[must_use]
    pub fn and(&self, other: &NodataMask) -> NodataMask {
        let mask = self
            .mask
            .iter()
            .zip(other.mask.iter())
            .map(|(&a, &b)| a && b)
            .collect();
        NodataMask {
            mask,
            width: self.width,
            height: self.height,
        }
    }

    /// Returns the union of two masks (pixel is valid when valid in *either*
    /// mask).
    #[must_use]
    pub fn or(&self, other: &NodataMask) -> NodataMask {
        let mask = self
            .mask
            .iter()
            .zip(other.mask.iter())
            .map(|(&a, &b)| a || b)
            .collect();
        NodataMask {
            mask,
            width: self.width,
            height: self.height,
        }
    }

    /// Returns the inverse mask (valid ↔ invalid).
    #[must_use]
    pub fn invert(&self) -> NodataMask {
        let mask = self.mask.iter().map(|&v| !v).collect();
        NodataMask {
            mask,
            width: self.width,
            height: self.height,
        }
    }

    /// Applies the mask to `band`, replacing invalid pixels with
    /// `nodata_value`.
    pub fn apply_to_band(&self, band: &mut Band, nodata_value: f64) {
        for (v, &valid) in band.data.iter_mut().zip(self.mask.iter()) {
            if !valid {
                *v = nodata_value;
            }
        }
    }
}

// ─── BandStack ─────────────────────────────────────────────────────────────

/// A multi-band raster: a collection of [`Band`]s with identical dimensions.
#[derive(Debug, Clone)]
pub struct BandStack {
    /// The individual bands.
    pub bands: Vec<Band>,
    /// Common width (all bands must share this).
    pub width: u32,
    /// Common height (all bands must share this).
    pub height: u32,
}

impl BandStack {
    /// Creates an empty `BandStack` with the given dimensions.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            bands: Vec::new(),
            width,
            height,
        }
    }

    /// Appends a band to the stack.
    ///
    /// # Errors
    /// Returns [`AlgebraError::DimensionMismatch`] if the band's dimensions do
    /// not match the stack dimensions.
    pub fn push_band(&mut self, band: Band) -> Result<(), AlgebraError> {
        if band.width != self.width || band.height != self.height {
            return Err(AlgebraError::DimensionMismatch {
                a: (self.width, self.height),
                b: (band.width, band.height),
            });
        }
        self.bands.push(band);
        Ok(())
    }

    /// Returns the number of bands.
    #[must_use]
    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    /// Returns a reference to the band at `index`, or `None` if out of
    /// bounds.
    #[must_use]
    pub fn get_band(&self, index: usize) -> Option<&Band> {
        self.bands.get(index)
    }

    /// Returns the first band whose `name` matches `name`, or `None`.
    #[must_use]
    pub fn get_band_by_name(&self, name: &str) -> Option<&Band> {
        self.bands.iter().find(|b| b.name.as_deref() == Some(name))
    }

    /// Returns the minimum valid pixel value at `(x, y)` across all bands, or
    /// `None` if no valid values exist.
    #[must_use]
    pub fn pixel_min(&self, x: u32, y: u32) -> Option<f64> {
        self.valid_pixel_values(x, y).reduce(f64::min)
    }

    /// Returns the maximum valid pixel value at `(x, y)` across all bands, or
    /// `None` if no valid values exist.
    #[must_use]
    pub fn pixel_max(&self, x: u32, y: u32) -> Option<f64> {
        self.valid_pixel_values(x, y).reduce(f64::max)
    }

    /// Returns the mean of valid pixel values at `(x, y)` across all bands,
    /// or `None` if no valid values exist.
    #[must_use]
    pub fn pixel_mean(&self, x: u32, y: u32) -> Option<f64> {
        let mut sum = 0.0_f64;
        let mut count = 0u64;
        for v in self.valid_pixel_values(x, y) {
            sum += v;
            count += 1;
        }
        if count == 0 {
            None
        } else {
            Some(sum / count as f64)
        }
    }

    /// Reduces the stack to a single band by applying `f` to the slice of
    /// *valid* values at each pixel.
    ///
    /// If all values at a pixel are nodata, `f` is called with an empty slice;
    /// the result is stored as-is (callers can return a sentinel value from
    /// `f`).
    #[must_use]
    pub fn reduce<F>(&self, f: F) -> Band
    where
        F: Fn(&[f64]) -> f64,
    {
        let n = (self.width as usize).saturating_mul(self.height as usize);
        let mut data = Vec::with_capacity(n);
        for idx in 0..n {
            let valid: Vec<f64> = self
                .bands
                .iter()
                .filter_map(|b| {
                    let v = b.data.get(idx).copied()?;
                    if b.is_nodata(v) { None } else { Some(v) }
                })
                .collect();
            data.push(f(&valid));
        }
        Band::new(data, self.width, self.height)
    }

    // ── Private ───────────────────────────────────────────────────────────

    fn valid_pixel_values(&self, x: u32, y: u32) -> impl Iterator<Item = f64> + '_ {
        self.bands.iter().filter_map(move |b| {
            let v = b.pixel_at(x, y)?;
            if b.is_nodata(v) { None } else { Some(v) }
        })
    }
}

// ─── ThresholdClassifier ───────────────────────────────────────────────────

/// Classifies a single band into discrete integer classes based on ascending
/// threshold values.
///
/// Thresholds are compared against each valid pixel from lowest to highest;
/// the first threshold that the pixel falls *below* determines the class.
/// Pixels ≥ all thresholds receive `default_class`.
///
/// Nodata pixels are written as `default_class as f64` (they remain
/// semantically "unclassified").
#[derive(Debug, Clone)]
pub struct ThresholdClassifier {
    /// `(threshold, class_value)` pairs, kept in ascending order of threshold.
    pub thresholds: Vec<(f64, u32)>,
    /// The class assigned when no threshold matches (i.e. value ≥ all
    /// thresholds, or the pixel is nodata).
    pub default_class: u32,
}

impl ThresholdClassifier {
    /// Creates a new classifier with the given default class.
    #[must_use]
    pub fn new(default_class: u32) -> Self {
        Self {
            thresholds: Vec::new(),
            default_class,
        }
    }

    /// Adds a threshold entry and keeps the list sorted.
    #[must_use]
    pub fn add_class(mut self, threshold: f64, class_value: u32) -> Self {
        self.thresholds.push((threshold, class_value));
        self.thresholds
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        self
    }

    /// Classifies all pixels in `band`, returning a new [`Band`] whose values
    /// are the integer class codes stored as `f64`.
    #[must_use]
    pub fn classify(&self, band: &Band) -> Band {
        let data: Vec<f64> = band
            .data
            .iter()
            .map(|&v| {
                if band.is_nodata(v) {
                    return self.default_class as f64;
                }
                for &(threshold, class) in &self.thresholds {
                    if v < threshold {
                        return class as f64;
                    }
                }
                self.default_class as f64
            })
            .collect();
        Band::new(data, band.width, band.height)
    }

    /// Returns a pre-configured NDVI classifier:
    ///
    /// | Range          | Class | Meaning      |
    /// |----------------|-------|--------------|
    /// | < −0.1         |   0   | Water/barren |
    /// | −0.1 … 0.2     |   1   | Sparse veg.  |
    /// | 0.2 … 0.5      |   2   | Moderate     |
    /// | ≥ 0.5          |   3   | Dense veg.   |
    #[must_use]
    pub fn ndvi_classes() -> Self {
        Self::new(3)
            .add_class(-0.1, 0)
            .add_class(0.2, 1)
            .add_class(0.5, 2)
    }
}

// ─── Private helpers ───────────────────────────────────────────────────────

/// Generic element-wise binary operation with nodata propagation.
fn binary_op<F: Fn(f64, f64) -> f64>(a: &Band, b: &Band, op: F) -> Vec<f64> {
    let nodata_val = a.nodata.unwrap_or(f64::NAN);
    a.data
        .iter()
        .zip(b.data.iter())
        .map(|(&av, &bv)| {
            if a.is_nodata(av) || b.is_nodata(bv) {
                nodata_val
            } else {
                op(av, bv)
            }
        })
        .collect()
}

/// Generic normalised-difference index: `(a - b) / (a + b)`.
///
/// Nodata is propagated from `a`.  Pixels where `a + b == 0` are set to
/// nodata.
fn normalized_difference(a: &Band, b: &Band, name: &str) -> Result<Band, AlgebraError> {
    a.check_dims(b)?;
    let nodata_val = a.nodata.unwrap_or(f64::NAN);
    let data: Vec<f64> = a
        .data
        .iter()
        .zip(b.data.iter())
        .map(|(&av, &bv)| {
            if a.is_nodata(av) || b.is_nodata(bv) {
                return nodata_val;
            }
            let denom = av + bv;
            if denom == 0.0 {
                nodata_val
            } else {
                (av - bv) / denom
            }
        })
        .collect();
    Ok(Band::new(data, a.width, a.height)
        .maybe_nodata(a.nodata)
        .with_name(name))
}

/// Extension trait to conditionally set nodata on a `Band`.
trait BandExt {
    fn maybe_nodata(self, nodata: Option<f64>) -> Self;
}

impl BandExt for Band {
    fn maybe_nodata(self, nodata: Option<f64>) -> Self {
        match nodata {
            Some(nd) => self.with_nodata(nd),
            None => self,
        }
    }
}
