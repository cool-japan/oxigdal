//! GRIB2 Product Definition Template (PDT) support
//!
//! Templates defined in WMO Manual on Codes, Vol I.2.
//! Reference: <https://www.nco.ncep.noaa.gov/pmb/docs/grib2/grib2_doc/>

// ---------------------------------------------------------------------------
// PdtType — Product Definition Template number
// ---------------------------------------------------------------------------

/// GRIB2 Product Definition Template (PDT) type discriminator.
///
/// The PDT number identifies how Section 4 is laid out.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PdtType {
    /// Template 0.0 — Analysis or forecast at a horizontal level/layer
    T0,
    /// Template 0.1 — Individual ensemble forecast
    T1,
    /// Template 0.2 — Derived forecasts (ensemble mean, spread)
    T2,
    /// Template 0.5 — Probability forecasts
    T5,
    /// Template 0.6 — Percentile forecasts
    T6,
    /// Template 0.8 — Average/accumulation/extreme over a time interval
    T8,
    /// Template 0.9 — Probability over a time interval
    T9,
    /// Template 0.10 — Percentile over a time interval
    T10,
    /// Template 0.11 — Individual ensemble forecast over a time interval
    T11,
    /// Template 0.12 — Derived ensemble forecast over a time interval
    T12,
    /// Template 0.15 — Average/accumulation/extreme over a spatial area
    T15,
    /// Template 0.30 — Satellite product
    T30,
    /// Template 0.31 — Satellite simulation product
    T31,
    /// Template 0.32 — Analysis/forecast for cross-sections
    T32,
    /// Template 0.33 — Average/accumulation over cross-sections
    T33,
    /// Template 0.40 — Optical properties of aerosol
    T40,
    /// Template 0.44 — Aerosol analysis/forecast
    T44,
    /// Template 0.48 — Aerosol analysis/forecast within atmospheric layer
    T48,
    /// Unknown or reserved template number
    Unknown(u16),
}

impl PdtType {
    /// Construct from a GRIB2 PDT number.
    #[must_use]
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Self::T0,
            1 => Self::T1,
            2 => Self::T2,
            5 => Self::T5,
            6 => Self::T6,
            8 => Self::T8,
            9 => Self::T9,
            10 => Self::T10,
            11 => Self::T11,
            12 => Self::T12,
            15 => Self::T15,
            30 => Self::T30,
            31 => Self::T31,
            32 => Self::T32,
            33 => Self::T33,
            40 => Self::T40,
            44 => Self::T44,
            48 => Self::T48,
            other => Self::Unknown(other),
        }
    }

    /// Returns the numeric PDT value, or `None` for `Unknown`.
    #[must_use]
    pub const fn to_u16(&self) -> Option<u16> {
        match self {
            Self::T0 => Some(0),
            Self::T1 => Some(1),
            Self::T2 => Some(2),
            Self::T5 => Some(5),
            Self::T6 => Some(6),
            Self::T8 => Some(8),
            Self::T9 => Some(9),
            Self::T10 => Some(10),
            Self::T11 => Some(11),
            Self::T12 => Some(12),
            Self::T15 => Some(15),
            Self::T30 => Some(30),
            Self::T31 => Some(31),
            Self::T32 => Some(32),
            Self::T33 => Some(33),
            Self::T40 => Some(40),
            Self::T44 => Some(44),
            Self::T48 => Some(48),
            Self::Unknown(_) => None,
        }
    }

    /// Human-readable name from WMO Manual on Codes.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::T0 => "Analysis or Forecast at Horizontal Level",
            Self::T1 => "Individual Ensemble Forecast",
            Self::T2 => "Derived Ensemble Forecasts",
            Self::T5 => "Probability Forecasts",
            Self::T6 => "Percentile Forecasts",
            Self::T8 => "Average/Accumulation Over Time Interval",
            Self::T9 => "Probability Over Time Interval",
            Self::T10 => "Percentile Over Time Interval",
            Self::T11 => "Individual Ensemble Forecast Over Time Interval",
            Self::T12 => "Derived Ensemble Forecast Over Time Interval",
            Self::T15 => "Average/Accumulation Over Spatial Area",
            Self::T30 => "Satellite Product",
            Self::T31 => "Satellite Simulation Product",
            Self::T32 => "Analysis/Forecast for Cross-Sections",
            Self::T33 => "Average/Accumulation Over Cross-Sections",
            Self::T40 => "Optical Properties of Aerosol",
            Self::T44 => "Aerosol Analysis/Forecast",
            Self::T48 => "Aerosol Within Atmospheric Layer",
            Self::Unknown(_) => "Unknown/Reserved Template",
        }
    }

    /// Returns `true` if this template includes ensemble member information.
    #[must_use]
    pub fn has_ensemble_info(&self) -> bool {
        matches!(self, Self::T1 | Self::T2 | Self::T11 | Self::T12)
    }

    /// Returns `true` if this template covers a time interval.
    #[must_use]
    pub fn has_time_interval(&self) -> bool {
        matches!(
            self,
            Self::T8 | Self::T9 | Self::T10 | Self::T11 | Self::T12 | Self::T15
        )
    }

    /// Returns `true` if this template includes probability information.
    #[must_use]
    pub fn has_probability_info(&self) -> bool {
        matches!(self, Self::T5 | Self::T9)
    }

    /// Returns `true` if this template involves satellite data.
    #[must_use]
    pub fn is_satellite(&self) -> bool {
        matches!(self, Self::T30 | Self::T31)
    }

    /// Returns `true` if this template involves aerosol data.
    #[must_use]
    pub fn is_aerosol(&self) -> bool {
        matches!(self, Self::T40 | Self::T44 | Self::T48)
    }

    /// Returns `true` for a known (non-`Unknown`) variant.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

// ---------------------------------------------------------------------------
// SurfaceType — WMO Code Table 4.5
// ---------------------------------------------------------------------------

/// GRIB2 fixed surface type (WMO Code Table 4.5).
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceType {
    /// Code 1 — Ground or water surface
    GroundOrWaterSurface,
    /// Code 2 — Cloud base level
    CloudBase,
    /// Code 3 — Cloud top level
    CloudTop,
    /// Code 4 — 0°C isotherm level
    ZeroDegreeCIsotherm,
    /// Code 6 — Level of maximum wind speed
    LevelMaxWindSpeed,
    /// Code 7 — Tropopause level
    Tropopause,
    /// Code 8 — Top of atmosphere
    TopOfAtmosphere,
    /// Code 101 — Mean sea level
    MeanSeaLevel,
    /// Code 100 — Isobaric surface (Pa)
    Isobaric {
        /// Pressure in Pascals.
        pressure_pa: u32,
    },
    /// Code 103 — Specified height above ground (m)
    HeightAboveGround {
        /// Height in metres.
        height_m: u32,
    },
    /// Code 104 — Sigma level
    SigmaLevel {
        /// Sigma value (0–1).
        sigma: f64,
    },
    /// Code 105 — Hybrid level
    HybridLevel {
        /// Hybrid level number.
        level: u32,
    },
    /// Code 106 — Depth below land surface (cm)
    DepthBelowLandSurface {
        /// Depth in centimetres.
        depth_cm: u32,
    },
    /// Code 107 — Isentropic (theta) level (K)
    IsentropicLevel {
        /// Potential temperature in Kelvin.
        theta_k: u32,
    },
    /// Code 108 — Level at specified pressure difference from ground (Pa)
    PressureDifferenceLevel {
        /// Pressure difference in Pascals.
        delta_pa: u32,
    },
    /// Unknown surface type
    Unknown(u8),
}

impl SurfaceType {
    /// Construct from a surface type code + scale factor + scaled value.
    ///
    /// `scaled_value / 10^scale_factor` gives the physical value.
    #[must_use]
    pub fn from_code(code: u8, scale_factor: i8, scaled_value: i32) -> Self {
        let value = (scaled_value as f64) * 10_f64.powi(-(scale_factor as i32));
        match code {
            1 => Self::GroundOrWaterSurface,
            2 => Self::CloudBase,
            3 => Self::CloudTop,
            4 => Self::ZeroDegreeCIsotherm,
            6 => Self::LevelMaxWindSpeed,
            7 => Self::Tropopause,
            8 => Self::TopOfAtmosphere,
            100 => Self::Isobaric {
                pressure_pa: value as u32,
            },
            101 => Self::MeanSeaLevel,
            103 => Self::HeightAboveGround {
                height_m: value as u32,
            },
            104 => Self::SigmaLevel { sigma: value },
            105 => Self::HybridLevel {
                level: value as u32,
            },
            106 => Self::DepthBelowLandSurface {
                depth_cm: value as u32,
            },
            107 => Self::IsentropicLevel {
                theta_k: value as u32,
            },
            108 => Self::PressureDifferenceLevel {
                delta_pa: value as u32,
            },
            other => Self::Unknown(other),
        }
    }

    /// Returns the WMO code table code for this surface type.
    #[must_use]
    pub const fn code(&self) -> u8 {
        match self {
            Self::GroundOrWaterSurface => 1,
            Self::CloudBase => 2,
            Self::CloudTop => 3,
            Self::ZeroDegreeCIsotherm => 4,
            Self::LevelMaxWindSpeed => 6,
            Self::Tropopause => 7,
            Self::TopOfAtmosphere => 8,
            Self::Isobaric { .. } => 100,
            Self::MeanSeaLevel => 101,
            Self::HeightAboveGround { .. } => 103,
            Self::SigmaLevel { .. } => 104,
            Self::HybridLevel { .. } => 105,
            Self::DepthBelowLandSurface { .. } => 106,
            Self::IsentropicLevel { .. } => 107,
            Self::PressureDifferenceLevel { .. } => 108,
            Self::Unknown(c) => *c,
        }
    }

    /// Returns a human-readable description.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::GroundOrWaterSurface => "Ground or Water Surface",
            Self::CloudBase => "Cloud Base Level",
            Self::CloudTop => "Cloud Top Level",
            Self::ZeroDegreeCIsotherm => "0°C Isotherm Level",
            Self::LevelMaxWindSpeed => "Level of Maximum Wind Speed",
            Self::Tropopause => "Tropopause",
            Self::TopOfAtmosphere => "Top of Atmosphere",
            Self::MeanSeaLevel => "Mean Sea Level",
            Self::Isobaric { .. } => "Isobaric Surface (Pa)",
            Self::HeightAboveGround { .. } => "Specified Height Above Ground (m)",
            Self::SigmaLevel { .. } => "Sigma Level",
            Self::HybridLevel { .. } => "Hybrid Level",
            Self::DepthBelowLandSurface { .. } => "Depth Below Land Surface (cm)",
            Self::IsentropicLevel { .. } => "Isentropic (Theta) Level (K)",
            Self::PressureDifferenceLevel { .. } => "Pressure Difference Level (Pa)",
            Self::Unknown(_) => "Unknown Surface Type",
        }
    }

    /// Returns `true` if the surface has a numeric level value.
    #[must_use]
    pub fn has_level_value(&self) -> bool {
        matches!(
            self,
            Self::Isobaric { .. }
                | Self::HeightAboveGround { .. }
                | Self::SigmaLevel { .. }
                | Self::HybridLevel { .. }
                | Self::DepthBelowLandSurface { .. }
                | Self::IsentropicLevel { .. }
                | Self::PressureDifferenceLevel { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// StatisticalMethod — WMO Code Table 4.10
// ---------------------------------------------------------------------------

/// GRIB2 statistical processing method (WMO Code Table 4.10).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StatisticalMethod {
    /// Code 0 — Average
    Average,
    /// Code 1 — Accumulation
    Accumulation,
    /// Code 2 — Maximum
    Maximum,
    /// Code 3 — Minimum
    Minimum,
    /// Code 4 — Difference (end - start)
    Difference,
    /// Code 5 — Root mean square
    RootMeanSquare,
    /// Code 6 — Standard deviation
    StandardDeviation,
    /// Code 7 — Covariance
    Covariance,
    /// Code 8 — Difference (start - end)
    DifferenceFromStart,
    /// Code 9 — Ratio
    Ratio,
    /// Unknown method code
    Unknown(u8),
}

impl StatisticalMethod {
    /// Construct from a WMO code table value.
    #[must_use]
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Average,
            1 => Self::Accumulation,
            2 => Self::Maximum,
            3 => Self::Minimum,
            4 => Self::Difference,
            5 => Self::RootMeanSquare,
            6 => Self::StandardDeviation,
            7 => Self::Covariance,
            8 => Self::DifferenceFromStart,
            9 => Self::Ratio,
            other => Self::Unknown(other),
        }
    }

    /// Returns the numeric code, or `None` for `Unknown`.
    #[must_use]
    pub const fn to_u8(&self) -> Option<u8> {
        match self {
            Self::Average => Some(0),
            Self::Accumulation => Some(1),
            Self::Maximum => Some(2),
            Self::Minimum => Some(3),
            Self::Difference => Some(4),
            Self::RootMeanSquare => Some(5),
            Self::StandardDeviation => Some(6),
            Self::Covariance => Some(7),
            Self::DifferenceFromStart => Some(8),
            Self::Ratio => Some(9),
            Self::Unknown(_) => None,
        }
    }

    /// Returns a human-readable name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Average => "Average",
            Self::Accumulation => "Accumulation",
            Self::Maximum => "Maximum",
            Self::Minimum => "Minimum",
            Self::Difference => "Difference (end - start)",
            Self::RootMeanSquare => "Root Mean Square",
            Self::StandardDeviation => "Standard Deviation",
            Self::Covariance => "Covariance",
            Self::DifferenceFromStart => "Difference (start - end)",
            Self::Ratio => "Ratio",
            Self::Unknown(_) => "Unknown Method",
        }
    }

    /// Returns `true` for a known (non-`Unknown`) variant.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

// ---------------------------------------------------------------------------
// ProbabilityType — WMO Code Table 4.9
// ---------------------------------------------------------------------------

/// GRIB2 probability type (WMO Code Table 4.9).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProbabilityType {
    /// Code 0 — Probability of event below lower limit
    BelowLowerLimit,
    /// Code 1 — Probability of event above upper limit
    AboveUpperLimit,
    /// Code 2 — Probability of event between lower and upper limits
    BetweenLimits,
    /// Code 3 — Probability of event above lower limit
    AboveLowerLimit,
    /// Code 4 — Probability of event below upper limit
    BelowUpperLimit,
    /// Unknown probability type
    Unknown(u8),
}

impl ProbabilityType {
    /// Construct from a WMO code table value.
    #[must_use]
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::BelowLowerLimit,
            1 => Self::AboveUpperLimit,
            2 => Self::BetweenLimits,
            3 => Self::AboveLowerLimit,
            4 => Self::BelowUpperLimit,
            other => Self::Unknown(other),
        }
    }

    /// Returns the numeric code, or `None` for `Unknown`.
    #[must_use]
    pub const fn to_u8(&self) -> Option<u8> {
        match self {
            Self::BelowLowerLimit => Some(0),
            Self::AboveUpperLimit => Some(1),
            Self::BetweenLimits => Some(2),
            Self::AboveLowerLimit => Some(3),
            Self::BelowUpperLimit => Some(4),
            Self::Unknown(_) => None,
        }
    }

    /// Returns a human-readable description.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::BelowLowerLimit => "Probability of event below lower limit",
            Self::AboveUpperLimit => "Probability of event above upper limit",
            Self::BetweenLimits => "Probability of event between lower and upper limits",
            Self::AboveLowerLimit => "Probability of event above lower limit",
            Self::BelowUpperLimit => "Probability of event below upper limit",
            Self::Unknown(_) => "Unknown probability type",
        }
    }

    /// Returns `true` for a known (non-`Unknown`) variant.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }

    /// Returns `true` if the type involves a lower limit.
    #[must_use]
    pub fn has_lower_limit(&self) -> bool {
        matches!(
            self,
            Self::BelowLowerLimit | Self::BetweenLimits | Self::AboveLowerLimit
        )
    }

    /// Returns `true` if the type involves an upper limit.
    #[must_use]
    pub fn has_upper_limit(&self) -> bool {
        matches!(
            self,
            Self::AboveUpperLimit | Self::BetweenLimits | Self::BelowUpperLimit
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- PdtType --

    #[test]
    fn test_pdt_type_from_u16_known() {
        assert_eq!(PdtType::from_u16(0), PdtType::T0);
        assert_eq!(PdtType::from_u16(1), PdtType::T1);
        assert_eq!(PdtType::from_u16(8), PdtType::T8);
        assert_eq!(PdtType::from_u16(48), PdtType::T48);
    }

    #[test]
    fn test_pdt_type_from_u16_unknown() {
        let u = PdtType::from_u16(999);
        assert_eq!(u, PdtType::Unknown(999));
        assert!(!u.is_known());
    }

    #[test]
    fn test_pdt_type_roundtrip() {
        let known = [
            0u16, 1, 2, 5, 6, 8, 9, 10, 11, 12, 15, 30, 31, 32, 33, 40, 44, 48,
        ];
        for &v in &known {
            let t = PdtType::from_u16(v);
            assert!(t.is_known(), "expected known for {v}");
            assert_eq!(t.to_u16(), Some(v));
        }
    }

    #[test]
    fn test_pdt_type_names_non_empty() {
        let types = [
            PdtType::T0,
            PdtType::T1,
            PdtType::T8,
            PdtType::T30,
            PdtType::Unknown(255),
        ];
        for t in &types {
            assert!(!t.name().is_empty());
        }
    }

    #[test]
    fn test_pdt_type_has_ensemble_info() {
        assert!(PdtType::T1.has_ensemble_info());
        assert!(PdtType::T2.has_ensemble_info());
        assert!(PdtType::T11.has_ensemble_info());
        assert!(PdtType::T12.has_ensemble_info());
        assert!(!PdtType::T0.has_ensemble_info());
        assert!(!PdtType::T8.has_ensemble_info());
    }

    #[test]
    fn test_pdt_type_has_time_interval() {
        assert!(PdtType::T8.has_time_interval());
        assert!(PdtType::T9.has_time_interval());
        assert!(PdtType::T10.has_time_interval());
        assert!(PdtType::T11.has_time_interval());
        assert!(PdtType::T12.has_time_interval());
        assert!(PdtType::T15.has_time_interval());
        assert!(!PdtType::T0.has_time_interval());
        assert!(!PdtType::T1.has_time_interval());
    }

    #[test]
    fn test_pdt_type_has_probability_info() {
        assert!(PdtType::T5.has_probability_info());
        assert!(PdtType::T9.has_probability_info());
        assert!(!PdtType::T0.has_probability_info());
        assert!(!PdtType::T8.has_probability_info());
    }

    #[test]
    fn test_pdt_type_is_satellite() {
        assert!(PdtType::T30.is_satellite());
        assert!(PdtType::T31.is_satellite());
        assert!(!PdtType::T0.is_satellite());
    }

    #[test]
    fn test_pdt_type_is_aerosol() {
        assert!(PdtType::T40.is_aerosol());
        assert!(PdtType::T44.is_aerosol());
        assert!(PdtType::T48.is_aerosol());
        assert!(!PdtType::T0.is_aerosol());
    }

    // -- SurfaceType --

    #[test]
    fn test_surface_type_ground() {
        let s = SurfaceType::from_code(1, 0, 0);
        assert_eq!(s, SurfaceType::GroundOrWaterSurface);
        assert_eq!(s.code(), 1);
        assert!(!s.has_level_value());
    }

    #[test]
    fn test_surface_type_isobaric() {
        let s = SurfaceType::from_code(100, 0, 85000);
        assert!(matches!(s, SurfaceType::Isobaric { pressure_pa: 85000 }));
        assert_eq!(s.code(), 100);
        assert!(s.has_level_value());
    }

    #[test]
    fn test_surface_type_height_above_ground() {
        let s = SurfaceType::from_code(103, 0, 2);
        assert!(matches!(s, SurfaceType::HeightAboveGround { height_m: 2 }));
    }

    #[test]
    fn test_surface_type_mean_sea_level() {
        let s = SurfaceType::from_code(101, 0, 0);
        assert_eq!(s, SurfaceType::MeanSeaLevel);
    }

    #[test]
    fn test_surface_type_tropopause() {
        let s = SurfaceType::from_code(7, 0, 0);
        assert_eq!(s, SurfaceType::Tropopause);
        assert!(!s.has_level_value());
    }

    #[test]
    fn test_surface_type_unknown() {
        let s = SurfaceType::from_code(200, 0, 0);
        assert!(matches!(s, SurfaceType::Unknown(200)));
        assert!(!s.has_level_value());
    }

    #[test]
    fn test_surface_type_descriptions_non_empty() {
        let types = [
            SurfaceType::from_code(1, 0, 0),
            SurfaceType::from_code(100, 0, 1000),
            SurfaceType::from_code(103, 0, 10),
        ];
        for t in &types {
            assert!(!t.description().is_empty());
        }
    }

    // -- StatisticalMethod --

    #[test]
    fn test_statistical_method_roundtrip() {
        for code in 0u8..=9 {
            let m = StatisticalMethod::from_u8(code);
            assert!(m.is_known(), "code {code} should be known");
            assert_eq!(m.to_u8(), Some(code));
        }
    }

    #[test]
    fn test_statistical_method_unknown() {
        let m = StatisticalMethod::from_u8(99);
        assert!(!m.is_known());
        assert!(m.to_u8().is_none());
    }

    #[test]
    fn test_statistical_method_names() {
        assert_eq!(StatisticalMethod::Average.name(), "Average");
        assert_eq!(StatisticalMethod::Accumulation.name(), "Accumulation");
        assert_eq!(StatisticalMethod::RootMeanSquare.name(), "Root Mean Square");
    }

    // -- ProbabilityType --

    #[test]
    fn test_probability_type_roundtrip() {
        for code in 0u8..=4 {
            let p = ProbabilityType::from_u8(code);
            assert!(p.is_known());
            assert_eq!(p.to_u8(), Some(code));
        }
    }

    #[test]
    fn test_probability_type_unknown() {
        let p = ProbabilityType::from_u8(10);
        assert!(!p.is_known());
    }

    #[test]
    fn test_probability_type_has_limits() {
        assert!(ProbabilityType::BelowLowerLimit.has_lower_limit());
        assert!(!ProbabilityType::BelowLowerLimit.has_upper_limit());

        assert!(ProbabilityType::AboveUpperLimit.has_upper_limit());
        assert!(!ProbabilityType::AboveUpperLimit.has_lower_limit());

        assert!(ProbabilityType::BetweenLimits.has_lower_limit());
        assert!(ProbabilityType::BetweenLimits.has_upper_limit());
    }

    #[test]
    fn test_probability_type_descriptions_non_empty() {
        let types = [
            ProbabilityType::BelowLowerLimit,
            ProbabilityType::BetweenLimits,
            ProbabilityType::Unknown(255),
        ];
        for t in &types {
            assert!(!t.description().is_empty());
        }
    }
}
