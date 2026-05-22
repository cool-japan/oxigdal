//! Solar geometry and topographic irradiance implementation.
//!
//! # Solar geometry
//!
//! The astronomical relations follow standard solar engineering references
//! (Iqbal, *An Introduction to Solar Radiation*, 1983; Duffie & Beckman,
//! *Solar Engineering of Thermal Processes*). Each formula cites its source
//! inline.
//!
//! # Topographic irradiance
//!
//! The per-cell illumination, cast-shadow ray marching, Beer-Lambert direct
//! beam and isotropic diffuse sky follow the model of GRASS GIS `r.sun`
//! (Hofierka & Suri, 2002) and the ArcGIS Area Solar Radiation toolset.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Solar position in the local horizontal (topocentric) coordinate frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolarPosition {
    /// Solar altitude (elevation) above the horizon, in degrees. Negative when
    /// the Sun is below the horizon.
    pub altitude_deg: f64,
    /// Solar azimuth, in degrees measured clockwise from geographic north
    /// (N = 0, E = 90, S = 180, W = 270). This matches the crate's aspect frame.
    pub azimuth_deg: f64,
    /// Solar zenith angle (angle from the local vertical), in degrees.
    /// `zenith_deg == 90 - altitude_deg`.
    pub zenith_deg: f64,
}

/// Configuration for time-integrated solar radiation modeling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolarOptions {
    /// Site latitude in degrees (positive north, negative south).
    pub latitude_deg: f64,
    /// Day of year `n` in `[1, 366]` (1 = 1 January).
    pub day_of_year: u32,
    /// Start of the integration window, in solar hours `[0, 24]`.
    pub start_hour: f64,
    /// End of the integration window, in solar hours `[0, 24]`.
    pub end_hour: f64,
    /// Integration time step, in minutes.
    pub time_step_minutes: f64,
    /// Clear-sky atmospheric transmittance per unit air mass, in `(0, 1]`.
    /// Typical clear-sky value ~0.7.
    pub transmittance: f64,
    /// If `true`, terrain cast shadows are evaluated by ray marching toward the
    /// Sun; shadowed cells receive no direct beam.
    pub cast_shadows: bool,
    /// If `true`, an isotropic diffuse sky component is added to the global
    /// irradiance.
    pub compute_diffuse: bool,
    /// Extraterrestrial solar constant `I0`, in W/m² (≈1367).
    pub solar_constant: f64,
}

impl Default for SolarOptions {
    fn default() -> Self {
        // Sensible mid-latitude clear-sky defaults: integrate a full day at a
        // 30-minute step with cast shadows and diffuse sky enabled.
        Self {
            latitude_deg: 45.0,
            day_of_year: 172, // ~summer solstice (21 June)
            start_hour: 0.0,
            end_hour: 24.0,
            time_step_minutes: 30.0,
            transmittance: 0.7,
            cast_shadows: true,
            compute_diffuse: true,
            solar_constant: 1367.0,
        }
    }
}

/// Time-integrated solar radiation result over a DEM.
///
/// Energy arrays are in watt-hours per square metre (Wh/m²); `duration_hours`
/// records, per cell, the total time for which that cell was sunlit
/// (Sun above the horizon and, if `cast_shadows` is enabled, not shadowed).
/// NoData cells are `NaN` throughout.
#[derive(Debug, Clone)]
pub struct SolarRadiationResult {
    /// Global (direct + diffuse) insolation, Wh/m².
    pub global: Array2<f64>,
    /// Direct-beam insolation, Wh/m².
    pub direct: Array2<f64>,
    /// Diffuse-sky insolation, Wh/m².
    pub diffuse: Array2<f64>,
    /// Sunlit duration per cell, hours.
    pub duration_hours: Array2<f64>,
}

/// Compute the topocentric solar position for a site and instant.
///
/// # Arguments
/// * `latitude_deg` - site latitude in degrees (positive north).
/// * `day_of_year` - day number `n` (1 = 1 January).
/// * `solar_time_hours` - local apparent (solar) time in hours, where 12.0 is
///   solar noon.
///
/// # Returns
/// A [`SolarPosition`] with altitude, azimuth (clockwise from north) and zenith.
///
/// # Formulas
/// * Declination `δ = 23.45°·sin(360°·(284 + n)/365)` — Cooper (1969).
/// * Hour angle `ω = 15°·(t_solar − 12)`.
/// * Zenith `cos θz = sin φ·sin δ + cos φ·cos δ·cos ω` — Iqbal (1983, eq. 1.5.5).
/// * Azimuth `cos γ_s = (sin δ·cos φ − cos δ·sin φ·cos ω) / sin θz`, with the
///   sign of `γ_s` taken from the hour angle so morning Sun is east and
///   afternoon Sun is west — Iqbal (1983, eq. 1.5.6).
pub fn solar_position(latitude_deg: f64, day_of_year: u32, solar_time_hours: f64) -> SolarPosition {
    let n = day_of_year as f64;
    let phi = latitude_deg.to_radians();

    // Cooper (1969) declination.
    let decl = (23.45_f64).to_radians() * (360.0 * (284.0 + n) / 365.0).to_radians().sin();

    // Hour angle: negative before solar noon (morning), positive after.
    let omega = (15.0 * (solar_time_hours - 12.0)).to_radians();

    // Solar zenith (Iqbal 1983). Clamp for numerical safety before acos.
    let cos_zenith =
        (phi.sin() * decl.sin() + phi.cos() * decl.cos() * omega.cos()).clamp(-1.0, 1.0);
    let zenith = cos_zenith.acos();
    let altitude = core::f64::consts::FRAC_PI_2 - zenith;

    // Solar azimuth measured clockwise from north.
    let azimuth = solar_azimuth_rad(phi, decl, omega);

    SolarPosition {
        altitude_deg: altitude.to_degrees(),
        azimuth_deg: azimuth.to_degrees(),
        zenith_deg: zenith.to_degrees(),
    }
}

/// Solar azimuth in radians, clockwise from north, in `[0, 2π)`.
///
/// Uses the numerically robust `atan2` formulation (NOAA / PSA solar position
/// algorithm). The azimuth is first obtained relative to the south meridian and
/// positive toward the west,
/// `γ_south = atan2(sin ω, cos ω·sin φ − tan δ·cos φ)`,
/// then shifted by π to a bearing clockwise from north. This resolves the
/// east/west sign automatically via the hour angle and stays correct in both
/// hemispheres (e.g. the noon Sun is due south for sites north of the subsolar
/// point and due north for sites south of it).
fn solar_azimuth_rad(phi: f64, decl: f64, omega: f64) -> f64 {
    let gamma_south = omega
        .sin()
        .atan2(omega.cos() * phi.sin() - decl.tan() * phi.cos());
    (gamma_south + core::f64::consts::PI).rem_euclid(core::f64::consts::TAU)
}

/// Eccentricity correction factor `E0` for the Earth-Sun distance.
///
/// `E0 = 1 + 0.033·cos(360°·n/365)` — Iqbal (1983, eq. 1.2.1, simplified Spencer
/// form). Scales the extraterrestrial irradiance over the year.
fn eccentricity_correction(day_of_year: u32) -> f64 {
    let n = day_of_year as f64;
    1.0 + 0.033 * (360.0 * n / 365.0).to_radians().cos()
}

/// Compute instantaneous shaded relief for an explicit Sun position.
///
/// This is the "hillshade with sun position" deliverable. The returned value is
/// the cosine of the angle of incidence between the Sun's rays and the local
/// terrain surface, clamped to `[0, 1]` (0 where the Sun is at or below the
/// horizon, or where the surface faces away from the Sun). Cast shadows are
/// **not** evaluated here; use [`solar_radiation`] for shadowing.
///
/// # Arguments
/// * `dem` - input DEM.
/// * `cell_size` - cell size in the same horizontal units as the elevations.
/// * `sun_altitude_deg` - Sun altitude above the horizon, degrees.
/// * `sun_azimuth_deg` - Sun azimuth, degrees clockwise from north.
///
/// # Formula
/// `cos i = cos θz·cos β + sin θz·sin β·cos(γ_s − A)` where `β` is slope, `A`
/// aspect, `θz = 90° − altitude` the zenith angle, `γ_s` the solar azimuth.
pub fn hillshade_at<T>(
    dem: &Array2<T>,
    cell_size: f64,
    sun_altitude_deg: f64,
    sun_azimuth_deg: f64,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut out = Array2::zeros((height, width));

    let zenith_rad = (90.0 - sun_altitude_deg).to_radians();
    let sun_az_rad = sun_azimuth_deg.to_radians();
    let cos_zenith = zenith_rad.cos();
    let sin_zenith = zenith_rad.sin();
    let sun_below = sun_altitude_deg <= 0.0;

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]].into();
            if center.is_nan() {
                out[[y, x]] = f64::NAN;
                continue;
            }

            if sun_below {
                out[[y, x]] = 0.0;
                continue;
            }

            let (slope_rad, aspect_rad) = slope_aspect_horn(dem, y, x, cell_size);
            let cos_i = cos_incidence(cos_zenith, sin_zenith, slope_rad, aspect_rad, sun_az_rad);
            out[[y, x]] = cos_i.max(0.0);
        }
    }

    Ok(out)
}

/// Compute time-integrated solar radiation over a DEM.
///
/// Integrates the direct beam (and optional diffuse sky) from `start_hour` to
/// `end_hour` at `time_step_minutes` resolution, accumulating energy in Wh/m².
///
/// # Arguments
/// * `dem` - input DEM.
/// * `cell_size` - cell size in the same horizontal units as the elevations.
/// * `options` - integration window and atmospheric/model parameters.
///
/// # Model
/// At each time step the Sun position is computed by [`solar_position`]. For each
/// illuminated cell:
/// * air mass `m = 1/cos θz` (Kasten-style, with θz clamped near the horizon),
/// * direct normal irradiance `I_n = I0·E0·τ^m`,
/// * direct on the inclined surface `I_direct = I_n·max(cos i, 0)`,
/// * cast shadows zero out the direct beam when enabled,
/// * isotropic diffuse `I_diffuse = I_dif0·(1 + cos β)/2`, with the diffuse
///   horizontal irradiance a small fraction of `I0·E0`,
/// * global `= direct + diffuse`.
///
/// Each instantaneous irradiance is multiplied by the step length in hours and
/// accumulated. Sunlit duration accrues only while the cell receives direct beam.
pub fn solar_radiation<T>(
    dem: &Array2<T>,
    cell_size: f64,
    options: &SolarOptions,
) -> Result<SolarRadiationResult>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;
    validate_options(options)?;

    let (height, width) = dem.dim();
    let mut global = Array2::<f64>::zeros((height, width));
    let mut direct = Array2::<f64>::zeros((height, width));
    let mut diffuse = Array2::<f64>::zeros((height, width));
    let mut duration = Array2::<f64>::zeros((height, width));

    // Precompute the NoData mask and per-cell slope/aspect once; geometry is
    // time-invariant. NoData cells stay NaN in every output.
    let dem_f64 = dem.mapv(|v| v.into());
    let mut nodata_mask = Array2::<bool>::from_elem((height, width), false);
    let mut slope_rad = Array2::<f64>::zeros((height, width));
    let mut aspect_rad = Array2::<f64>::zeros((height, width));
    for y in 0..height {
        for x in 0..width {
            if dem_f64[[y, x]].is_nan() {
                nodata_mask[[y, x]] = true;
                global[[y, x]] = f64::NAN;
                direct[[y, x]] = f64::NAN;
                diffuse[[y, x]] = f64::NAN;
                duration[[y, x]] = f64::NAN;
            } else {
                let (s, a) = slope_aspect_horn(dem, y, x, cell_size);
                slope_rad[[y, x]] = s;
                aspect_rad[[y, x]] = a;
            }
        }
    }

    let step_hours = options.time_step_minutes / 60.0;
    if step_hours <= 0.0 {
        return Err(TerrainError::InvalidThreshold {
            threshold: options.time_step_minutes,
            message: "time_step_minutes must be positive".to_string(),
        });
    }

    let i0_e0 = options.solar_constant * eccentricity_correction(options.day_of_year);
    // Fraction of the extraterrestrial horizontal irradiance treated as diffuse
    // sky under clear conditions (a small isotropic background term).
    let diffuse_fraction = 0.1;

    // March across the integration window. Use a half-open accumulation so the
    // total energy equals sum(I·Δt) over the sampled instants.
    let mut t = options.start_hour;
    // Guard against pathological windows producing an unbounded loop.
    let max_steps = (((options.end_hour - options.start_hour).abs() / step_hours).ceil() as usize)
        .saturating_add(2);
    let mut steps = 0usize;
    while t <= options.end_hour + 1.0e-9 && steps < max_steps {
        steps += 1;
        let pos = solar_position(options.latitude_deg, options.day_of_year, t);
        t += step_hours;

        if pos.altitude_deg <= 0.0 {
            // Sun below the horizon: no contribution this step.
            continue;
        }

        let zenith_rad = pos.zenith_deg.to_radians();
        // Air mass m = 1/cos θz, clamped near the horizon (Iqbal 1983).
        let cos_zenith = zenith_rad.cos().max(1.0e-3);
        let sin_zenith = zenith_rad.sin();
        let air_mass = 1.0 / cos_zenith;
        let direct_normal = i0_e0 * options.transmittance.powf(air_mass);
        // Diffuse horizontal irradiance for this instant (isotropic clear sky).
        let diffuse_horizontal = if options.compute_diffuse {
            diffuse_fraction * i0_e0 * cos_zenith
        } else {
            0.0
        };
        let sun_az_rad = pos.azimuth_deg.to_radians();

        for y in 0..height {
            for x in 0..width {
                if nodata_mask[[y, x]] {
                    continue;
                }

                let beta = slope_rad[[y, x]];
                let aspect = aspect_rad[[y, x]];

                // Direct beam on the inclined surface.
                let cos_i =
                    cos_incidence(cos_zenith, sin_zenith, beta, aspect, sun_az_rad).max(0.0);
                let mut direct_inst = direct_normal * cos_i;

                if direct_inst > 0.0
                    && options.cast_shadows
                    && is_shadowed(&dem_f64, y, x, cell_size, pos.altitude_deg, sun_az_rad)
                {
                    direct_inst = 0.0;
                }

                // Isotropic diffuse sky (sky-view from slope only).
                let diffuse_inst = if options.compute_diffuse {
                    diffuse_horizontal * (1.0 + beta.cos()) / 2.0
                } else {
                    0.0
                };

                direct[[y, x]] += direct_inst * step_hours;
                diffuse[[y, x]] += diffuse_inst * step_hours;
                global[[y, x]] += (direct_inst + diffuse_inst) * step_hours;
                if direct_inst > 0.0 {
                    duration[[y, x]] += step_hours;
                }
            }
        }
    }

    Ok(SolarRadiationResult {
        global,
        direct,
        diffuse,
        duration_hours: duration,
    })
}

/// Cosine of the angle of incidence on an inclined surface.
///
/// `cos i = cos θz·cos β + sin θz·sin β·cos(γ_s − A)` (Iqbal 1983, eq. 11.3.2),
/// with `β` slope, `A` aspect, `γ_s` solar azimuth (all consistent radians).
#[inline]
fn cos_incidence(
    cos_zenith: f64,
    sin_zenith: f64,
    slope_rad: f64,
    aspect_rad: f64,
    sun_az_rad: f64,
) -> f64 {
    cos_zenith * slope_rad.cos() + sin_zenith * slope_rad.sin() * (sun_az_rad - aspect_rad).cos()
}

/// Horn (1981) slope (radians) and aspect (radians, clockwise from north) at a
/// single cell, reproducing the crate's existing convention exactly.
///
/// Aspect is returned as a bearing where north = 0, east = π/2 (clockwise),
/// matching [`crate::derivatives::aspect`] so the `cos(γ_s − A)` term aligns with
/// the solar azimuth frame. Flat cells return aspect 0.
fn slope_aspect_horn<T>(dem: &Array2<T>, y: usize, x: usize, cell_size: f64) -> (f64, f64)
where
    T: Float + Into<f64> + Copy,
{
    let a = edge_value(dem, y.wrapping_sub(1), x.wrapping_sub(1)).into();
    let b = edge_value(dem, y.wrapping_sub(1), x).into();
    let c = edge_value(dem, y.wrapping_sub(1), x + 1).into();
    let d = edge_value(dem, y, x.wrapping_sub(1)).into();
    let f = edge_value(dem, y, x + 1).into();
    let g = edge_value(dem, y + 1, x.wrapping_sub(1)).into();
    let h = edge_value(dem, y + 1, x).into();
    let i = edge_value(dem, y + 1, x + 1).into();

    // Horn (1981) third-order finite-difference gradients, as used by the
    // crate's slope/aspect modules.
    let dzdx = ((c + 2.0 * f + i) - (a + 2.0 * d + g)) / (8.0 * cell_size);
    let dzdy = ((g + 2.0 * h + i) - (a + 2.0 * b + c)) / (8.0 * cell_size);

    let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();

    // Aspect: geographic bearing clockwise from north, matching the crate's
    // `calculate_aspect_from_gradients`.
    let aspect_rad = if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
        0.0
    } else {
        let aspect_math = dzdy.atan2(dzdx);
        let mut deg = 90.0 - aspect_math.to_degrees();
        if deg < 0.0 {
            deg += 360.0;
        }
        if deg >= 360.0 {
            deg -= 360.0;
        }
        deg.to_radians()
    };

    (slope_rad, aspect_rad)
}

/// March from a cell toward the Sun, testing whether terrain occludes the beam.
///
/// Grid orientation: row `y` increases southward, column `x` increases eastward.
/// A unit step toward an azimuth `γ_s` (clockwise from north) is therefore
/// `(Δx, Δy) = (sin γ_s, −cos γ_s)`. The cell is shadowed if, along the ray, the
/// terrain elevation ever rises above the straight line of sight defined by the
/// solar altitude angle (Hofierka & Suri 2002 horizon test).
fn is_shadowed(
    dem_f64: &Array2<f64>,
    y: usize,
    x: usize,
    cell_size: f64,
    sun_altitude_deg: f64,
    sun_az_rad: f64,
) -> bool {
    let (height, width) = dem_f64.dim();
    let z0 = dem_f64[[y, x]];
    if z0.is_nan() {
        return false;
    }

    // Direction toward the Sun in grid space.
    let dx = sun_az_rad.sin();
    let dy = -sun_az_rad.cos();
    let tan_alt = sun_altitude_deg.to_radians().tan();

    let x0 = x as f64;
    let y0 = y as f64;
    // Diagonal of the grid bounds the useful ray length.
    let max_dist = ((height * height + width * width) as f64).sqrt();

    let mut dist = 1.0;
    while dist <= max_dist {
        let xi = x0 + dx * dist;
        let yi = y0 + dy * dist;
        if xi < 0.0 || yi < 0.0 || xi > (width as f64 - 1.0) || yi > (height as f64 - 1.0) {
            break;
        }

        let ix = xi.round() as usize;
        let iy = yi.round() as usize;
        if ix >= width || iy >= height {
            break;
        }

        let z = dem_f64[[iy, ix]];
        if !z.is_nan() {
            // Horizontal ground distance and required line-of-sight height.
            let ground = dist * cell_size;
            let los = z0 + ground * tan_alt;
            if z > los {
                return true;
            }
        }
        dist += 1.0;
    }

    false
}

/// Edge-extended cell access matching the crate's `EdgeStrategy::Extend`.
fn edge_value<T: Copy>(dem: &Array2<T>, y: usize, x: usize) -> T {
    let (height, width) = dem.dim();
    if y < height && x < width {
        dem[[y, x]]
    } else {
        let y_clamped = y.min(height - 1);
        let x_clamped = x.min(width - 1);
        dem[[y_clamped, x_clamped]]
    }
}

fn validate_inputs<T>(dem: &Array2<T>, cell_size: f64) -> Result<()> {
    let (height, width) = dem.dim();
    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }
    Ok(())
}

fn validate_options(options: &SolarOptions) -> Result<()> {
    if options.day_of_year < 1 || options.day_of_year > 366 {
        return Err(TerrainError::InvalidThreshold {
            threshold: options.day_of_year as f64,
            message: "day_of_year must be in [1, 366]".to_string(),
        });
    }
    if !(options.transmittance > 0.0 && options.transmittance <= 1.0) {
        return Err(TerrainError::InvalidThreshold {
            threshold: options.transmittance,
            message: "transmittance must be in (0, 1]".to_string(),
        });
    }
    if options.solar_constant <= 0.0 {
        return Err(TerrainError::InvalidThreshold {
            threshold: options.solar_constant,
            message: "solar_constant must be positive".to_string(),
        });
    }
    if options.start_hour > options.end_hour {
        return Err(TerrainError::InvalidThreshold {
            threshold: options.start_hour,
            message: "start_hour must not exceed end_hour".to_string(),
        });
    }
    Ok(())
}
