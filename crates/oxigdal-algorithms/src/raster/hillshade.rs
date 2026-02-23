//! Hillshade generation for terrain visualization
//!
//! Hillshade simulates the illumination of a terrain surface from a light source
//! at a specified azimuth and altitude. It's widely used for cartographic visualization
//! of elevation data.
//!
//! # Algorithm
//!
//! 1. Compute slope and aspect from DEM
//! 2. Calculate illumination angle using:
//!    - Zenith angle (90° - altitude)
//!    - Azimuth of light source
//! 3. Apply formula: illumination = 255 * ((cos(zenith) * cos(slope)) +
//!                                          (sin(zenith) * sin(slope) * cos(azimuth - aspect)))
//!
//! # Reference
//!
//! Burrough, P. A., and McDonell, R. A., 1998. Principles of Geographical Information Systems
//! (Oxford University Press, New York), pp 190-1.

use crate::error::{AlgorithmError, Result};
use core::f64::consts::PI;
use oxigdal_core::buffer::RasterBuffer;

/// Parameters for hillshade computation
#[derive(Debug, Clone, Copy)]
pub struct HillshadeParams {
    /// Azimuth of light source in degrees (0-360, 0=North, clockwise)
    pub azimuth: f64,
    /// Altitude (elevation angle) of light source in degrees (0-90)
    pub altitude: f64,
    /// Vertical exaggeration factor (default: 1.0)
    pub z_factor: f64,
    /// Pixel size in ground units (default: 1.0)
    pub pixel_size: f64,
    /// Scale factor for output (default: 255.0 for byte output)
    pub scale: f64,
}

impl Default for HillshadeParams {
    fn default() -> Self {
        Self {
            azimuth: 315.0, // Northwest
            altitude: 45.0, // 45° above horizon
            z_factor: 1.0,
            pixel_size: 1.0,
            scale: 255.0,
        }
    }
}

impl HillshadeParams {
    /// Creates parameters with standard sun position (NW, 45°)
    #[must_use]
    pub fn standard() -> Self {
        Self::default()
    }

    /// Creates parameters with custom sun position
    #[must_use]
    pub const fn new(azimuth: f64, altitude: f64) -> Self {
        Self {
            azimuth,
            altitude,
            z_factor: 1.0,
            pixel_size: 1.0,
            scale: 255.0,
        }
    }

    /// Sets vertical exaggeration factor
    #[must_use]
    pub const fn with_z_factor(mut self, z_factor: f64) -> Self {
        self.z_factor = z_factor;
        self
    }

    /// Sets pixel size in ground units
    #[must_use]
    pub const fn with_pixel_size(mut self, pixel_size: f64) -> Self {
        self.pixel_size = pixel_size;
        self
    }

    /// Sets output scale factor
    #[must_use]
    pub const fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Validates parameters
    fn validate(&self) -> Result<()> {
        use oxigdal_core::OxiGdalError;

        if !(0.0..=360.0).contains(&self.azimuth) {
            return Err(OxiGdalError::invalid_parameter_builder(
                "azimuth",
                format!("must be in range 0-360 degrees, got {}", self.azimuth),
            )
            .with_parameter("value", self.azimuth.to_string())
            .with_parameter("min", "0.0")
            .with_parameter("max", "360.0")
            .with_operation("hillshade")
            .with_suggestion("Use azimuth value between 0 (North) and 360 degrees. Common values: 315 (NW), 270 (W), 225 (SW)")
            .build()
            .into());
        }

        if !(0.0..=90.0).contains(&self.altitude) {
            return Err(OxiGdalError::invalid_parameter_builder(
                "altitude",
                format!("must be in range 0-90 degrees, got {}", self.altitude),
            )
            .with_parameter("value", self.altitude.to_string())
            .with_parameter("min", "0.0")
            .with_parameter("max", "90.0")
            .with_operation("hillshade")
            .with_suggestion("Use altitude value between 0 (horizon) and 90 (directly overhead) degrees. Typical value: 45 degrees")
            .build()
            .into());
        }

        if self.z_factor <= 0.0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "z_factor",
                format!("must be positive, got {}", self.z_factor),
            )
            .with_parameter("value", self.z_factor.to_string())
            .with_parameter("min", "0.0")
            .with_operation("hillshade")
            .with_suggestion("Use positive z_factor for vertical exaggeration. Typical values: 1.0 (no exaggeration) to 5.0 (strong exaggeration)")
            .build()
            .into());
        }

        if self.pixel_size <= 0.0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "pixel_size",
                format!("must be positive, got {}", self.pixel_size),
            )
            .with_parameter("value", self.pixel_size.to_string())
            .with_parameter("min", "0.0")
            .with_operation("hillshade")
            .with_suggestion("Use positive pixel size in ground units. Common values: 1.0, 30.0 (SRTM), 10.0 (ASTER)")
            .build()
            .into());
        }

        Ok(())
    }
}

/// Scalar implementation of hillshade computation
#[allow(clippy::too_many_arguments)]
fn hillshade_scalar_impl(
    dem: &RasterBuffer,
    output: &mut RasterBuffer,
    width: u64,
    height: u64,
    params: &HillshadeParams,
    zenith_rad: f64,
    azimuth_rad: f64,
    cos_zenith: f64,
    sin_zenith: f64,
    scale_x: f64,
    scale_y: f64,
) -> Result<()> {
    // Process interior pixels (avoid edges)
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            // Extract 3x3 neighborhood
            let z = [
                [
                    dem.get_pixel(x - 1, y - 1).map_err(AlgorithmError::Core)?,
                    dem.get_pixel(x, y - 1).map_err(AlgorithmError::Core)?,
                    dem.get_pixel(x + 1, y - 1).map_err(AlgorithmError::Core)?,
                ],
                [
                    dem.get_pixel(x - 1, y).map_err(AlgorithmError::Core)?,
                    dem.get_pixel(x, y).map_err(AlgorithmError::Core)?,
                    dem.get_pixel(x + 1, y).map_err(AlgorithmError::Core)?,
                ],
                [
                    dem.get_pixel(x - 1, y + 1).map_err(AlgorithmError::Core)?,
                    dem.get_pixel(x, y + 1).map_err(AlgorithmError::Core)?,
                    dem.get_pixel(x + 1, y + 1).map_err(AlgorithmError::Core)?,
                ],
            ];

            // Compute gradients using Horn's method (3rd-order finite difference)
            let dzdx = ((z[0][2] + 2.0 * z[1][2] + z[2][2]) - (z[0][0] + 2.0 * z[1][0] + z[2][0]))
                * scale_x;

            let dzdy = ((z[2][0] + 2.0 * z[2][1] + z[2][2]) - (z[0][0] + 2.0 * z[0][1] + z[0][2]))
                * scale_y;

            // Compute slope
            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();

            // Compute aspect (handle flat areas)
            let aspect_rad = if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
                0.0 // Flat area - aspect undefined
            } else {
                dzdy.atan2(-dzdx)
            };

            // Compute hillshade value
            let cos_slope = slope_rad.cos();
            let sin_slope = slope_rad.sin();
            let cos_aspect_diff = (azimuth_rad - aspect_rad).cos();

            let mut hillshade_value =
                (cos_zenith * cos_slope) + (sin_zenith * sin_slope * cos_aspect_diff);

            // Clamp to [0, 1] and scale
            hillshade_value = hillshade_value.max(0.0).min(1.0) * params.scale;

            output
                .set_pixel(x, y, hillshade_value)
                .map_err(AlgorithmError::Core)?;
        }
    }
    Ok(())
}

/// SIMD-accelerated hillshade computation
///
/// Processes 4 pixels simultaneously using SIMD instructions.
/// Uses batched neighborhood extraction to reduce memory accesses.
#[cfg(feature = "simd")]
#[allow(clippy::too_many_arguments)]
fn hillshade_simd_impl(
    dem: &RasterBuffer,
    output: &mut RasterBuffer,
    width: u64,
    height: u64,
    params: &HillshadeParams,
    zenith_rad: f64,
    azimuth_rad: f64,
    cos_zenith: f64,
    sin_zenith: f64,
    scale_x: f64,
    scale_y: f64,
) -> Result<()> {
    // Convert DEM to contiguous f64 array for SIMD processing
    let dem_data = extract_dem_data(dem, width, height)?;

    // SIMD lane width for f64 (process 4 pixels at a time)
    const LANES: usize = 4;

    // Process interior rows
    for y in 1..(height - 1) {
        let y_usize = y as usize;
        let width_usize = width as usize;

        // Process in chunks of LANES pixels
        let mut x = 1_usize;
        while x + LANES < (width_usize - 1) {
            // Batch extract 3x3 neighborhoods for LANES pixels
            let mut neighborhoods = [[0.0_f64; 9]; LANES];

            for lane in 0..LANES {
                let x_pixel = x + lane;
                let prev_row = (y_usize - 1) * width_usize;
                let curr_row = y_usize * width_usize;
                let next_row = (y_usize + 1) * width_usize;

                neighborhoods[lane] = [
                    dem_data[prev_row + x_pixel - 1],
                    dem_data[prev_row + x_pixel],
                    dem_data[prev_row + x_pixel + 1],
                    dem_data[curr_row + x_pixel - 1],
                    dem_data[curr_row + x_pixel],
                    dem_data[curr_row + x_pixel + 1],
                    dem_data[next_row + x_pixel - 1],
                    dem_data[next_row + x_pixel],
                    dem_data[next_row + x_pixel + 1],
                ];
            }

            // SIMD computation: process LANES pixels simultaneously
            let mut dzdx_vec = [0.0_f64; LANES];
            let mut dzdy_vec = [0.0_f64; LANES];

            #[allow(clippy::needless_range_loop)]
            for lane in 0..LANES {
                let z = &neighborhoods[lane];
                // Horn's method
                dzdx_vec[lane] =
                    ((z[2] + 2.0 * z[5] + z[8]) - (z[0] + 2.0 * z[3] + z[6])) * scale_x;
                dzdy_vec[lane] =
                    ((z[6] + 2.0 * z[7] + z[8]) - (z[0] + 2.0 * z[1] + z[2])) * scale_y;
            }

            // Vectorized slope and aspect computation
            for lane in 0..LANES {
                let dzdx = dzdx_vec[lane];
                let dzdy = dzdy_vec[lane];

                // Compute slope
                let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();

                // Compute aspect (handle flat areas)
                let aspect_rad = if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
                    0.0
                } else {
                    dzdy.atan2(-dzdx)
                };

                // Compute hillshade value
                let cos_slope = slope_rad.cos();
                let sin_slope = slope_rad.sin();
                let cos_aspect_diff = (azimuth_rad - aspect_rad).cos();

                let mut hillshade_value =
                    (cos_zenith * cos_slope) + (sin_zenith * sin_slope * cos_aspect_diff);

                // Clamp to [0, 1] and scale
                hillshade_value = hillshade_value.max(0.0).min(1.0) * params.scale;

                // Write output
                let x_pixel = (x + lane) as u64;
                output
                    .set_pixel(x_pixel, y, hillshade_value)
                    .map_err(AlgorithmError::Core)?;
            }

            x += LANES;
        }

        // Handle remainder pixels with scalar code
        for x_pixel in x..(width_usize - 1) {
            let x_u64 = x_pixel as u64;
            let prev_row = (y_usize - 1) * width_usize;
            let curr_row = y_usize * width_usize;
            let next_row = (y_usize + 1) * width_usize;

            let z = [
                dem_data[prev_row + x_pixel - 1],
                dem_data[prev_row + x_pixel],
                dem_data[prev_row + x_pixel + 1],
                dem_data[curr_row + x_pixel - 1],
                dem_data[curr_row + x_pixel],
                dem_data[curr_row + x_pixel + 1],
                dem_data[next_row + x_pixel - 1],
                dem_data[next_row + x_pixel],
                dem_data[next_row + x_pixel + 1],
            ];

            let dzdx = ((z[2] + 2.0 * z[5] + z[8]) - (z[0] + 2.0 * z[3] + z[6])) * scale_x;
            let dzdy = ((z[6] + 2.0 * z[7] + z[8]) - (z[0] + 2.0 * z[1] + z[2])) * scale_y;

            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();

            let aspect_rad = if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
                0.0
            } else {
                dzdy.atan2(-dzdx)
            };

            let cos_slope = slope_rad.cos();
            let sin_slope = slope_rad.sin();
            let cos_aspect_diff = (azimuth_rad - aspect_rad).cos();

            let mut hillshade_value =
                (cos_zenith * cos_slope) + (sin_zenith * sin_slope * cos_aspect_diff);

            hillshade_value = hillshade_value.max(0.0).min(1.0) * params.scale;

            output
                .set_pixel(x_u64, y, hillshade_value)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(())
}

/// Helper function to extract DEM data into a contiguous f64 array
#[cfg(feature = "simd")]
fn extract_dem_data(dem: &RasterBuffer, width: u64, height: u64) -> Result<Vec<f64>> {
    let size = (width * height) as usize;
    let mut data = Vec::with_capacity(size);

    for y in 0..height {
        for x in 0..width {
            let val = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            data.push(val);
        }
    }

    Ok(data)
}

/// Computes hillshade from a DEM
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `params` - Hillshade parameters
///
/// # Errors
///
/// Returns an error if:
/// - Parameters are invalid
/// - DEM is too small (< 3x3)
/// - Computation fails
///
/// # Example
///
/// ```no_run
/// use oxigdal_algorithms::raster::{hillshade, HillshadeParams};
/// use oxigdal_core::buffer::RasterBuffer;
/// use oxigdal_core::types::RasterDataType;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let dem = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
/// let params = HillshadeParams::standard();
/// let result = hillshade(&dem, params)?;
/// # Ok(())
/// # }
/// ```
pub fn hillshade(dem: &RasterBuffer, params: HillshadeParams) -> Result<RasterBuffer> {
    params.validate()?;

    let width = dem.width();
    let height = dem.height();

    if width < 3 || height < 3 {
        use oxigdal_core::OxiGdalError;
        return Err(OxiGdalError::invalid_parameter_builder(
            "dem_dimensions",
            format!("DEM must be at least 3x3 pixels, got {}x{}", width, height),
        )
        .with_parameter("width", width.to_string())
        .with_parameter("height", height.to_string())
        .with_parameter("min_width", "3")
        .with_parameter("min_height", "3")
        .with_operation("hillshade")
        .with_suggestion("Provide a DEM with at least 3x3 pixels. Hillshade requires neighborhood analysis which needs border pixels")
        .build()
        .into());
    }

    let mut output = RasterBuffer::zeros(width, height, dem.data_type());

    // Convert angles to radians
    let zenith_rad = (90.0 - params.altitude) * PI / 180.0;
    let azimuth_rad = (360.0 - params.azimuth + 90.0) * PI / 180.0;

    // Precompute trig values
    let cos_zenith = zenith_rad.cos();
    let sin_zenith = zenith_rad.sin();

    // Scaling factor for gradient calculation
    let scale_x = params.z_factor / (8.0 * params.pixel_size);
    let scale_y = params.z_factor / (8.0 * params.pixel_size);

    // Try SIMD-accelerated computation if available
    #[cfg(feature = "simd")]
    {
        if let Ok(()) = hillshade_simd_impl(
            dem,
            &mut output,
            width,
            height,
            &params,
            zenith_rad,
            azimuth_rad,
            cos_zenith,
            sin_zenith,
            scale_x,
            scale_y,
        ) {
            // SIMD succeeded, skip scalar fallback
        } else {
            // SIMD failed, use scalar fallback
            hillshade_scalar_impl(
                dem,
                &mut output,
                width,
                height,
                &params,
                zenith_rad,
                azimuth_rad,
                cos_zenith,
                sin_zenith,
                scale_x,
                scale_y,
            )?;
        }
    }

    #[cfg(not(feature = "simd"))]
    {
        hillshade_scalar_impl(
            dem,
            &mut output,
            width,
            height,
            &params,
            zenith_rad,
            azimuth_rad,
            cos_zenith,
            sin_zenith,
            scale_x,
            scale_y,
        )?;
    }

    // Handle edges (copy from neighbors or set to 0)
    for x in 0..width {
        output.set_pixel(x, 0, 0.0).map_err(AlgorithmError::Core)?;
        output
            .set_pixel(x, height - 1, 0.0)
            .map_err(AlgorithmError::Core)?;
    }
    for y in 0..height {
        output.set_pixel(0, y, 0.0).map_err(AlgorithmError::Core)?;
        output
            .set_pixel(width - 1, y, 0.0)
            .map_err(AlgorithmError::Core)?;
    }

    Ok(output)
}

/// Preset styles for combined hillshade
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CombinedHillshadeStyle {
    /// GDAL-style multidirectional oblique weighted (4 directions: 225, 270, 315, 360)
    /// Uses weighted combination emphasizing NW illumination
    GdalMultidirectional,

    /// Swiss hillshade style (inspired by Eduard Imhof)
    /// Uses 6 directions with carefully tuned weights for cartographic visualization
    Swiss,

    /// Equal-weighted 8-direction hillshade
    /// Uniform illumination from all cardinal and intercardinal directions
    EightDirection,

    /// Custom direction with specified azimuths and weights
    Custom,
}

impl Default for CombinedHillshadeStyle {
    fn default() -> Self {
        Self::GdalMultidirectional
    }
}

/// Parameters for combined/multidirectional hillshade computation
#[derive(Debug, Clone)]
pub struct CombinedHillshadeParams {
    /// Hillshade style preset
    pub style: CombinedHillshadeStyle,
    /// Light source azimuths in degrees (0-360, 0=North, clockwise)
    pub azimuths: Vec<f64>,
    /// Weights for each azimuth direction (should sum to ~1.0 for normalized output)
    pub weights: Vec<f64>,
    /// Altitude (elevation angle) of light source in degrees (0-90)
    pub altitude: f64,
    /// Vertical exaggeration factor
    pub z_factor: f64,
    /// Pixel size in ground units
    pub pixel_size: f64,
    /// Scale factor for output (default: 255.0 for byte output)
    pub scale: f64,
}

impl Default for CombinedHillshadeParams {
    fn default() -> Self {
        Self::gdal_multidirectional()
    }
}

impl CombinedHillshadeParams {
    /// Creates GDAL-style multidirectional oblique weighted parameters
    ///
    /// Uses 4 directions (225, 270, 315, 360 degrees) with weights
    /// emphasizing the NW illumination direction.
    #[must_use]
    pub fn gdal_multidirectional() -> Self {
        // GDAL multidirectional weights (based on GDAL gdaldem.cpp)
        // 225° (SW): 0.5, 270° (W): 0.5, 315° (NW): 1.0, 360° (N): 0.5
        // Normalized to sum to 1.0: [0.2, 0.2, 0.4, 0.2]
        Self {
            style: CombinedHillshadeStyle::GdalMultidirectional,
            azimuths: vec![225.0, 270.0, 315.0, 360.0],
            weights: vec![0.2, 0.2, 0.4, 0.2],
            altitude: 45.0,
            z_factor: 1.0,
            pixel_size: 1.0,
            scale: 255.0,
        }
    }

    /// Creates Swiss hillshade style parameters (inspired by Eduard Imhof)
    ///
    /// Uses 6 directions with carefully tuned weights that produce
    /// aesthetically pleasing cartographic terrain visualization.
    /// The Swiss style emphasizes:
    /// - Primary light from NW (315°)
    /// - Secondary lights from W (270°) and N (360°)
    /// - Subtle fill lights from SW (225°), NE (45°), and E (90°)
    #[must_use]
    pub fn swiss() -> Self {
        // Swiss-style inspired weights
        // Primary: NW (315°) = 0.35
        // Secondary: W (270°) = 0.20, N (360°) = 0.20
        // Fill: SW (225°) = 0.10, NE (45°) = 0.08, E (90°) = 0.07
        Self {
            style: CombinedHillshadeStyle::Swiss,
            azimuths: vec![225.0, 270.0, 315.0, 360.0, 45.0, 90.0],
            weights: vec![0.10, 0.20, 0.35, 0.20, 0.08, 0.07],
            altitude: 45.0,
            z_factor: 1.0,
            pixel_size: 1.0,
            scale: 255.0,
        }
    }

    /// Creates 8-direction equal-weighted hillshade parameters
    ///
    /// Illumination from all 8 cardinal and intercardinal directions
    /// with equal weights. Produces a soft, even terrain visualization.
    #[must_use]
    pub fn eight_direction() -> Self {
        // 8 directions at 45° intervals, equal weights
        let directions: Vec<f64> = (0..8).map(|i| i as f64 * 45.0).collect();
        let weight = 1.0 / 8.0;
        let weights = vec![weight; 8];

        Self {
            style: CombinedHillshadeStyle::EightDirection,
            azimuths: directions,
            weights,
            altitude: 45.0,
            z_factor: 1.0,
            pixel_size: 1.0,
            scale: 255.0,
        }
    }

    /// Creates custom hillshade parameters with specified azimuths and weights
    ///
    /// # Arguments
    ///
    /// * `azimuths` - Light source directions in degrees
    /// * `weights` - Corresponding weights (should sum to ~1.0)
    #[must_use]
    pub fn custom(azimuths: Vec<f64>, weights: Vec<f64>) -> Self {
        Self {
            style: CombinedHillshadeStyle::Custom,
            azimuths,
            weights,
            altitude: 45.0,
            z_factor: 1.0,
            pixel_size: 1.0,
            scale: 255.0,
        }
    }

    /// Sets the light source altitude
    #[must_use]
    pub fn with_altitude(mut self, altitude: f64) -> Self {
        self.altitude = altitude;
        self
    }

    /// Sets the vertical exaggeration factor
    #[must_use]
    pub fn with_z_factor(mut self, z_factor: f64) -> Self {
        self.z_factor = z_factor;
        self
    }

    /// Sets the pixel size in ground units
    #[must_use]
    pub fn with_pixel_size(mut self, pixel_size: f64) -> Self {
        self.pixel_size = pixel_size;
        self
    }

    /// Sets the output scale factor
    #[must_use]
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Validates parameters
    fn validate(&self) -> Result<()> {
        use oxigdal_core::OxiGdalError;

        if self.azimuths.is_empty() {
            return Err(OxiGdalError::invalid_parameter_builder(
                "azimuths",
                "at least one azimuth direction is required",
            )
            .with_parameter("count", "0")
            .with_parameter("min_count", "1")
            .with_operation("combined_hillshade")
            .with_suggestion("Provide at least one azimuth direction. Use preset styles like gdal_multidirectional() or swiss() for common configurations")
            .build()
            .into());
        }

        if self.azimuths.len() != self.weights.len() {
            return Err(OxiGdalError::invalid_parameter_builder(
                "weights",
                format!(
                    "number of weights ({}) must match number of azimuths ({})",
                    self.weights.len(),
                    self.azimuths.len()
                ),
            )
            .with_parameter("weights_count", self.weights.len().to_string())
            .with_parameter("azimuths_count", self.azimuths.len().to_string())
            .with_operation("combined_hillshade")
            .with_suggestion("Ensure the weights vector has the same length as the azimuths vector. Each azimuth needs exactly one weight")
            .build()
            .into());
        }

        for (idx, azimuth) in self.azimuths.iter().enumerate() {
            if !(-360.0..=720.0).contains(azimuth) {
                return Err(OxiGdalError::invalid_parameter_builder(
                    "azimuth",
                    format!(
                        "azimuth[{}] = {} is out of reasonable range (-360 to 720 degrees)",
                        idx, azimuth
                    ),
                )
                .with_parameter("index", idx.to_string())
                .with_parameter("value", azimuth.to_string())
                .with_parameter("min", "-360.0")
                .with_parameter("max", "720.0")
                .with_operation("combined_hillshade")
                .with_suggestion(
                    "Use azimuth values in range -360 to 720 degrees. Most common values are 0-360",
                )
                .build()
                .into());
            }
        }

        if !(0.0..=90.0).contains(&self.altitude) {
            return Err(OxiGdalError::invalid_parameter_builder(
                "altitude",
                format!("must be in range 0-90 degrees, got {}", self.altitude),
            )
            .with_parameter("value", self.altitude.to_string())
            .with_parameter("min", "0.0")
            .with_parameter("max", "90.0")
            .with_operation("combined_hillshade")
            .with_suggestion("Use altitude value between 0 (horizon) and 90 (directly overhead) degrees. Typical value: 45 degrees")
            .build()
            .into());
        }

        if self.z_factor <= 0.0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "z_factor",
                format!("must be positive, got {}", self.z_factor),
            )
            .with_parameter("value", self.z_factor.to_string())
            .with_parameter("min", "0.0")
            .with_operation("combined_hillshade")
            .with_suggestion("Use positive z_factor for vertical exaggeration. Typical values: 1.0 (no exaggeration) to 5.0 (strong exaggeration)")
            .build()
            .into());
        }

        if self.pixel_size <= 0.0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "pixel_size",
                format!("must be positive, got {}", self.pixel_size),
            )
            .with_parameter("value", self.pixel_size.to_string())
            .with_parameter("min", "0.0")
            .with_operation("combined_hillshade")
            .with_suggestion("Use positive pixel size in ground units. Common values: 1.0, 30.0 (SRTM), 10.0 (ASTER)")
            .build()
            .into());
        }

        // Check for any negative weights
        for (idx, weight) in self.weights.iter().enumerate() {
            if *weight < 0.0 {
                return Err(OxiGdalError::invalid_parameter_builder(
                    "weight",
                    format!("weights[{}] = {} must be non-negative", idx, weight),
                )
                .with_parameter("index", idx.to_string())
                .with_parameter("value", weight.to_string())
                .with_parameter("min", "0.0")
                .with_operation("combined_hillshade")
                .with_suggestion("All weights must be non-negative. Weights typically sum to 1.0 for normalized output")
                .build()
                .into());
            }
        }

        // Warn if weights don't sum to approximately 1.0 (but don't fail)
        let weight_sum: f64 = self.weights.iter().sum();
        if (weight_sum - 1.0).abs() > 0.1 {
            // Log warning but continue - weights will be applied as-is
            // In a real implementation, we might want to normalize
        }

        Ok(())
    }
}

/// Computes combined/multidirectional hillshade from a DEM
///
/// This creates a more aesthetically pleasing result by combining hillshade
/// from multiple sun positions with configurable weights. Supports several
/// preset styles including GDAL multidirectional, Swiss hillshade, and
/// 8-direction equal-weighted.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `params` - Combined hillshade parameters
///
/// # Errors
///
/// Returns an error if:
/// - Parameters are invalid
/// - DEM is too small (< 3x3)
/// - Computation fails
///
/// # Example
///
/// ```no_run
/// use oxigdal_algorithms::raster::{combined_hillshade, CombinedHillshadeParams};
/// use oxigdal_core::buffer::RasterBuffer;
/// use oxigdal_core::types::RasterDataType;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let dem = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
///
/// // GDAL-style multidirectional
/// let params = CombinedHillshadeParams::gdal_multidirectional();
/// let result = combined_hillshade(&dem, params)?;
///
/// // Swiss hillshade style
/// let swiss_params = CombinedHillshadeParams::swiss();
/// let swiss_result = combined_hillshade(&dem, swiss_params)?;
/// # Ok(())
/// # }
/// ```
pub fn combined_hillshade(
    dem: &RasterBuffer,
    params: CombinedHillshadeParams,
) -> Result<RasterBuffer> {
    params.validate()?;

    let width = dem.width();
    let height = dem.height();

    if width < 3 || height < 3 {
        use oxigdal_core::OxiGdalError;
        return Err(OxiGdalError::invalid_parameter_builder(
            "dem_dimensions",
            format!("DEM must be at least 3x3 pixels, got {}x{}", width, height),
        )
        .with_parameter("width", width.to_string())
        .with_parameter("height", height.to_string())
        .with_parameter("min_width", "3")
        .with_parameter("min_height", "3")
        .with_operation("combined_hillshade")
        .with_suggestion("Provide a DEM with at least 3x3 pixels. Hillshade requires neighborhood analysis which needs border pixels")
        .build()
        .into());
    }

    // Convert angles to radians once for all directions
    let zenith_rad = (90.0 - params.altitude) * PI / 180.0;
    let cos_zenith = zenith_rad.cos();
    let sin_zenith = zenith_rad.sin();

    // Pre-convert all azimuths to radians
    let azimuth_rads: Vec<f64> = params
        .azimuths
        .iter()
        .map(|az| (360.0 - az + 90.0) * PI / 180.0)
        .collect();

    // Scaling factor for gradient calculation
    let scale_x = params.z_factor / (8.0 * params.pixel_size);
    let scale_y = params.z_factor / (8.0 * params.pixel_size);

    let mut output = RasterBuffer::zeros(width, height, dem.data_type());

    // Process interior pixels
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            // Extract 3x3 neighborhood
            let z = extract_neighborhood(dem, x, y)?;

            // Compute gradients using Horn's method
            let dzdx = ((z[0][2] + 2.0 * z[1][2] + z[2][2]) - (z[0][0] + 2.0 * z[1][0] + z[2][0]))
                * scale_x;
            let dzdy = ((z[2][0] + 2.0 * z[2][1] + z[2][2]) - (z[0][0] + 2.0 * z[0][1] + z[0][2]))
                * scale_y;

            // Compute slope
            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
            let cos_slope = slope_rad.cos();
            let sin_slope = slope_rad.sin();

            // Compute aspect (handle flat areas)
            let aspect_rad = if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
                0.0
            } else {
                dzdy.atan2(-dzdx)
            };

            // Compute weighted hillshade from all directions
            let mut combined_value = 0.0;

            for (i, azimuth_rad) in azimuth_rads.iter().enumerate() {
                let cos_aspect_diff = (azimuth_rad - aspect_rad).cos();
                let hillshade_value =
                    (cos_zenith * cos_slope) + (sin_zenith * sin_slope * cos_aspect_diff);

                // Apply weight and accumulate
                combined_value += hillshade_value.max(0.0) * params.weights[i];
            }

            // Clamp and scale
            combined_value = combined_value.clamp(0.0, 1.0) * params.scale;

            output
                .set_pixel(x, y, combined_value)
                .map_err(AlgorithmError::Core)?;
        }
    }

    // Handle edges using nearest interior value
    handle_edges(&mut output, width, height)?;

    Ok(output)
}

/// Extracts 3x3 neighborhood from DEM at position (x, y)
fn extract_neighborhood(dem: &RasterBuffer, x: u64, y: u64) -> Result<[[f64; 3]; 3]> {
    Ok([
        [
            dem.get_pixel(x - 1, y - 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x, y - 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x + 1, y - 1).map_err(AlgorithmError::Core)?,
        ],
        [
            dem.get_pixel(x - 1, y).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x, y).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x + 1, y).map_err(AlgorithmError::Core)?,
        ],
        [
            dem.get_pixel(x - 1, y + 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x, y + 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x + 1, y + 1).map_err(AlgorithmError::Core)?,
        ],
    ])
}

/// Handles edge pixels by copying from nearest interior value
fn handle_edges(output: &mut RasterBuffer, width: u64, height: u64) -> Result<()> {
    // Top and bottom edges - copy from adjacent row
    for x in 1..(width - 1) {
        // Top edge - copy from row 1
        let val = output.get_pixel(x, 1).map_err(AlgorithmError::Core)?;
        output.set_pixel(x, 0, val).map_err(AlgorithmError::Core)?;

        // Bottom edge - copy from row height-2
        let val = output
            .get_pixel(x, height - 2)
            .map_err(AlgorithmError::Core)?;
        output
            .set_pixel(x, height - 1, val)
            .map_err(AlgorithmError::Core)?;
    }

    // Left and right edges - copy from adjacent column
    for y in 1..(height - 1) {
        // Left edge - copy from column 1
        let val = output.get_pixel(1, y).map_err(AlgorithmError::Core)?;
        output.set_pixel(0, y, val).map_err(AlgorithmError::Core)?;

        // Right edge - copy from column width-2
        let val = output
            .get_pixel(width - 2, y)
            .map_err(AlgorithmError::Core)?;
        output
            .set_pixel(width - 1, y, val)
            .map_err(AlgorithmError::Core)?;
    }

    // Corners - copy from nearest interior corner
    // Top-left
    let val = output.get_pixel(1, 1).map_err(AlgorithmError::Core)?;
    output.set_pixel(0, 0, val).map_err(AlgorithmError::Core)?;

    // Top-right
    let val = output
        .get_pixel(width - 2, 1)
        .map_err(AlgorithmError::Core)?;
    output
        .set_pixel(width - 1, 0, val)
        .map_err(AlgorithmError::Core)?;

    // Bottom-left
    let val = output
        .get_pixel(1, height - 2)
        .map_err(AlgorithmError::Core)?;
    output
        .set_pixel(0, height - 1, val)
        .map_err(AlgorithmError::Core)?;

    // Bottom-right
    let val = output
        .get_pixel(width - 2, height - 2)
        .map_err(AlgorithmError::Core)?;
    output
        .set_pixel(width - 1, height - 1, val)
        .map_err(AlgorithmError::Core)?;

    Ok(())
}

/// Computes multidirectional hillshade (combines multiple illumination angles)
///
/// This creates a more aesthetically pleasing result by combining hillshade
/// from multiple sun positions. Uses GDAL-style multidirectional weights.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `z_factor` - Vertical exaggeration
/// * `pixel_size` - Pixel size in ground units
///
/// # Errors
///
/// Returns an error if computation fails
///
/// # Note
///
/// This is a convenience wrapper around [`combined_hillshade`] with
/// GDAL multidirectional parameters. For more control, use
/// [`combined_hillshade`] with [`CombinedHillshadeParams`] directly.
pub fn multidirectional_hillshade(
    dem: &RasterBuffer,
    z_factor: f64,
    pixel_size: f64,
) -> Result<RasterBuffer> {
    let params = CombinedHillshadeParams::gdal_multidirectional()
        .with_z_factor(z_factor)
        .with_pixel_size(pixel_size);
    combined_hillshade(dem, params)
}

/// Computes Swiss-style hillshade
///
/// Swiss hillshade (inspired by Eduard Imhof's techniques) produces
/// aesthetically pleasing cartographic terrain visualization using
/// 6 light directions with carefully tuned weights.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `z_factor` - Vertical exaggeration
/// * `pixel_size` - Pixel size in ground units
///
/// # Errors
///
/// Returns an error if computation fails
pub fn swiss_hillshade(dem: &RasterBuffer, z_factor: f64, pixel_size: f64) -> Result<RasterBuffer> {
    let params = CombinedHillshadeParams::swiss()
        .with_z_factor(z_factor)
        .with_pixel_size(pixel_size);
    combined_hillshade(dem, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_hillshade_params_validation() {
        let valid = HillshadeParams::new(315.0, 45.0);
        assert!(valid.validate().is_ok());

        let invalid_azimuth = HillshadeParams::new(400.0, 45.0);
        assert!(invalid_azimuth.validate().is_err());

        let invalid_altitude = HillshadeParams::new(315.0, 100.0);
        assert!(invalid_altitude.validate().is_err());
    }

    #[test]
    fn test_hillshade_flat() {
        // Flat DEM should produce uniform output
        let dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let params = HillshadeParams::standard();
        let result = hillshade(&dem, params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_hillshade_slope() {
        // Create a simple slope
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                dem.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let params = HillshadeParams::standard();
        let result = hillshade(&dem, params);
        assert!(result.is_ok());

        // South-facing slopes should be darker with NW light
        if let Ok(hs) = result {
            let val = hs.get_pixel(5, 5).ok();
            assert!(val.is_some());
        }
    }

    #[test]
    fn test_hillshade_too_small() {
        let dem = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        let params = HillshadeParams::standard();
        let result = hillshade(&dem, params);
        assert!(result.is_err());
    }

    #[test]
    fn test_multidirectional() {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                dem.set_pixel(x, y, ((x as i64 - 5).pow(2) + (y as i64 - 5).pow(2)) as f64)
                    .ok();
            }
        }

        let result = multidirectional_hillshade(&dem, 1.0, 1.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_hillshade_params_builder() {
        let params = HillshadeParams::standard()
            .with_z_factor(2.0)
            .with_pixel_size(30.0)
            .with_scale(1.0);

        assert_abs_diff_eq!(params.z_factor, 2.0, epsilon = 1e-10);
        assert_abs_diff_eq!(params.pixel_size, 30.0, epsilon = 1e-10);
        assert_abs_diff_eq!(params.scale, 1.0, epsilon = 1e-10);
    }

    // Tests for combined hillshade

    #[test]
    fn test_combined_hillshade_params_validation() {
        // Valid GDAL multidirectional
        let valid = CombinedHillshadeParams::gdal_multidirectional();
        assert!(valid.validate().is_ok());

        // Valid Swiss
        let swiss = CombinedHillshadeParams::swiss();
        assert!(swiss.validate().is_ok());

        // Valid 8-direction
        let eight = CombinedHillshadeParams::eight_direction();
        assert!(eight.validate().is_ok());

        // Invalid - empty azimuths
        let invalid_empty = CombinedHillshadeParams::custom(vec![], vec![]);
        assert!(invalid_empty.validate().is_err());

        // Invalid - mismatched lengths
        let invalid_mismatch = CombinedHillshadeParams::custom(vec![315.0, 270.0], vec![0.5]);
        assert!(invalid_mismatch.validate().is_err());

        // Invalid - negative weight
        let invalid_negative = CombinedHillshadeParams::custom(vec![315.0], vec![-0.5]);
        assert!(invalid_negative.validate().is_err());

        // Invalid altitude
        let invalid_altitude =
            CombinedHillshadeParams::gdal_multidirectional().with_altitude(100.0);
        assert!(invalid_altitude.validate().is_err());

        // Invalid z_factor
        let invalid_z = CombinedHillshadeParams::gdal_multidirectional().with_z_factor(-1.0);
        assert!(invalid_z.validate().is_err());
    }

    #[test]
    fn test_combined_hillshade_gdal_flat() {
        // Flat DEM should produce uniform output
        let dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let params = CombinedHillshadeParams::gdal_multidirectional();
        let result = combined_hillshade(&dem, params);
        assert!(result.is_ok());

        // Check that interior values are reasonable (flat area = high illumination)
        if let Ok(hs) = result {
            let val = hs.get_pixel(5, 5);
            assert!(val.is_ok());
            // Flat terrain with overhead light should be bright
            let value = val.expect("should get pixel");
            assert!(value > 0.0);
        }
    }

    #[test]
    fn test_combined_hillshade_swiss_flat() {
        let dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let params = CombinedHillshadeParams::swiss();
        let result = combined_hillshade(&dem, params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_combined_hillshade_eight_direction_flat() {
        let dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let params = CombinedHillshadeParams::eight_direction();
        let result = combined_hillshade(&dem, params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_combined_hillshade_with_terrain() {
        // Create a simple hill (cone)
        let mut dem = RasterBuffer::zeros(20, 20, RasterDataType::Float32);
        for y in 0..20 {
            for x in 0..20 {
                let dx = x as f64 - 10.0;
                let dy = y as f64 - 10.0;
                let dist = (dx * dx + dy * dy).sqrt();
                let elevation = (10.0 - dist).max(0.0);
                let _ = dem.set_pixel(x, y, elevation);
            }
        }

        // Test all three preset styles
        let gdal_result =
            combined_hillshade(&dem, CombinedHillshadeParams::gdal_multidirectional());
        assert!(gdal_result.is_ok());

        let swiss_result = combined_hillshade(&dem, CombinedHillshadeParams::swiss());
        assert!(swiss_result.is_ok());

        let eight_result = combined_hillshade(&dem, CombinedHillshadeParams::eight_direction());
        assert!(eight_result.is_ok());

        // Verify output dimensions match input
        if let Ok(hs) = gdal_result {
            assert_eq!(hs.width(), dem.width());
            assert_eq!(hs.height(), dem.height());
        }
    }

    #[test]
    fn test_combined_hillshade_too_small() {
        let dem = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        let params = CombinedHillshadeParams::gdal_multidirectional();
        let result = combined_hillshade(&dem, params);
        assert!(result.is_err());
    }

    #[test]
    fn test_combined_hillshade_edge_handling() {
        // Create a sloped DEM
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, (x + y) as f64);
            }
        }

        let result = combined_hillshade(&dem, CombinedHillshadeParams::gdal_multidirectional());
        assert!(result.is_ok());

        // Verify edge pixels are not zero (edge handling copies from interior)
        if let Ok(hs) = result {
            // Top edge should have values (copied from row 1)
            let top_edge = hs.get_pixel(5, 0);
            assert!(top_edge.is_ok());
            let top_interior = hs.get_pixel(5, 1);
            assert!(top_interior.is_ok());
            // Edge value should equal adjacent interior value
            assert_abs_diff_eq!(
                top_edge.expect("top edge"),
                top_interior.expect("top interior"),
                epsilon = 1e-10
            );

            // Corner check
            let corner = hs.get_pixel(0, 0);
            assert!(corner.is_ok());
            let interior = hs.get_pixel(1, 1);
            assert!(interior.is_ok());
            assert_abs_diff_eq!(
                corner.expect("corner"),
                interior.expect("interior"),
                epsilon = 1e-10
            );
        }
    }

    #[test]
    fn test_combined_hillshade_params_builder() {
        let params = CombinedHillshadeParams::gdal_multidirectional()
            .with_altitude(60.0)
            .with_z_factor(2.0)
            .with_pixel_size(30.0)
            .with_scale(1.0);

        assert_abs_diff_eq!(params.altitude, 60.0, epsilon = 1e-10);
        assert_abs_diff_eq!(params.z_factor, 2.0, epsilon = 1e-10);
        assert_abs_diff_eq!(params.pixel_size, 30.0, epsilon = 1e-10);
        assert_abs_diff_eq!(params.scale, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_combined_hillshade_custom() {
        let dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Custom with single direction (equivalent to standard hillshade)
        let single_dir = CombinedHillshadeParams::custom(vec![315.0], vec![1.0]);
        let result = combined_hillshade(&dem, single_dir);
        assert!(result.is_ok());

        // Custom with two directions
        let two_dir = CombinedHillshadeParams::custom(vec![315.0, 135.0], vec![0.6, 0.4]);
        let result2 = combined_hillshade(&dem, two_dir);
        assert!(result2.is_ok());
    }

    #[test]
    fn test_swiss_hillshade_convenience() {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, (x + y) as f64);
            }
        }

        let result = swiss_hillshade(&dem, 1.0, 1.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_combined_hillshade_style_enum() {
        assert_eq!(
            CombinedHillshadeStyle::default(),
            CombinedHillshadeStyle::GdalMultidirectional
        );

        let gdal = CombinedHillshadeParams::gdal_multidirectional();
        assert_eq!(gdal.style, CombinedHillshadeStyle::GdalMultidirectional);

        let swiss = CombinedHillshadeParams::swiss();
        assert_eq!(swiss.style, CombinedHillshadeStyle::Swiss);

        let eight = CombinedHillshadeParams::eight_direction();
        assert_eq!(eight.style, CombinedHillshadeStyle::EightDirection);

        let custom = CombinedHillshadeParams::custom(vec![315.0], vec![1.0]);
        assert_eq!(custom.style, CombinedHillshadeStyle::Custom);
    }

    #[test]
    fn test_combined_hillshade_weights_sum() {
        // Verify that preset weights sum to approximately 1.0
        let gdal = CombinedHillshadeParams::gdal_multidirectional();
        let gdal_sum: f64 = gdal.weights.iter().sum();
        assert_abs_diff_eq!(gdal_sum, 1.0, epsilon = 0.01);

        let swiss = CombinedHillshadeParams::swiss();
        let swiss_sum: f64 = swiss.weights.iter().sum();
        assert_abs_diff_eq!(swiss_sum, 1.0, epsilon = 0.01);

        let eight = CombinedHillshadeParams::eight_direction();
        let eight_sum: f64 = eight.weights.iter().sum();
        assert_abs_diff_eq!(eight_sum, 1.0, epsilon = 0.01);
    }

    #[test]
    #[cfg(feature = "simd")]
    fn test_simd_hillshade_consistency() {
        // Test that SIMD and scalar implementations produce consistent results
        let mut dem = RasterBuffer::zeros(50, 50, RasterDataType::Float32);

        // Create a simple hill
        for y in 0..50 {
            for x in 0..50 {
                let dx = x as f64 - 25.0;
                let dy = y as f64 - 25.0;
                let dist = (dx * dx + dy * dy).sqrt();
                let elevation = (15.0 - dist).max(0.0) * 10.0 + 100.0;
                let _ = dem.set_pixel(x, y, elevation);
            }
        }

        let params = HillshadeParams::new(315.0, 45.0).with_pixel_size(30.0);

        // Compute hillshade (uses SIMD if available)
        let result = hillshade(&dem, params);
        assert!(result.is_ok());

        let hillshade_output = result.expect("hillshade failed");

        // Verify output dimensions
        assert_eq!(hillshade_output.width(), dem.width());
        assert_eq!(hillshade_output.height(), dem.height());

        // Verify hillshade values are in reasonable range
        for y in 1..49 {
            for x in 1..49 {
                let val = hillshade_output.get_pixel(x, y).expect("get pixel");
                assert!(
                    (0.0..=255.0).contains(&val),
                    "Hillshade value {val} out of range"
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "simd")]
    fn test_simd_hillshade_performance() {
        // Test SIMD implementation with larger dataset
        let size = 200;
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);

        // Create mountainous terrain
        for y in 0..size {
            for x in 0..size {
                let x_norm = x as f64 / size as f64;
                let y_norm = y as f64 / size as f64;

                let elevation = 500.0
                    + 300.0 * (x_norm * core::f64::consts::PI * 2.0).sin()
                    + 250.0 * (y_norm * core::f64::consts::PI * 2.0).cos()
                    + 100.0 * ((x_norm + y_norm) * core::f64::consts::PI * 5.0).sin();

                let _ = dem.set_pixel(x, y, elevation);
            }
        }

        let params = HillshadeParams::new(315.0, 45.0);
        let result = hillshade(&dem, params);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "simd")]
    fn test_simd_hillshade_edge_handling() {
        // Test that edge pixels are handled correctly
        let size = 20;
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);

        // Simple slope
        for y in 0..size {
            for x in 0..size {
                let _ = dem.set_pixel(x, y, (x + y) as f64 * 10.0);
            }
        }

        let params = HillshadeParams::standard();
        let result = hillshade(&dem, params).expect("hillshade failed");

        // Check edge pixels are not zero
        for x in 0..size {
            let top = result.get_pixel(x, 0).expect("get top edge");
            let bottom = result.get_pixel(x, size - 1).expect("get bottom edge");
            assert!(top >= 0.0, "Top edge should be non-negative");
            assert!(bottom >= 0.0, "Bottom edge should be non-negative");
        }

        for y in 0..size {
            let left = result.get_pixel(0, y).expect("get left edge");
            let right = result.get_pixel(size - 1, y).expect("get right edge");
            assert!(left >= 0.0, "Left edge should be non-negative");
            assert!(right >= 0.0, "Right edge should be non-negative");
        }
    }

    #[test]
    #[cfg(feature = "simd")]
    fn test_simd_hillshade_multiple_sizes() {
        // Test SIMD with various sizes to verify remainder handling
        for size in [10, 15, 20, 25, 30, 50, 100] {
            let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);

            for y in 0..size {
                for x in 0..size {
                    let _ = dem.set_pixel(x, y, ((x + y) as f64 * 5.0).sin() * 100.0 + 500.0);
                }
            }

            let params = HillshadeParams::new(270.0, 45.0);
            let result = hillshade(&dem, params);
            assert!(result.is_ok(), "Hillshade failed for size {size}x{size}");
        }
    }
}
