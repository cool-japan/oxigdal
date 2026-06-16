//! Raster quality control modules.

pub mod accuracy;
pub mod cog;
pub mod completeness;
pub mod consistency;
pub mod crs_extent;
pub mod nodata;
pub mod radiometric;

pub use accuracy::{AccuracyChecker, AccuracyConfig, AccuracyResult};
pub use cog::{CogComplianceChecker, CogComplianceResult, StrictMode};
pub use completeness::{CompletenessChecker, CompletenessConfig, CompletenessResult};
pub use consistency::{ConsistencyChecker, ConsistencyConfig, ConsistencyResult};
pub use crs_extent::{CrsAndExtentValidator, CrsExtentValidationResult};
pub use nodata::{NoDataBandStats, NoDataValidationResult, NoDataValidator};
pub use radiometric::{
    BandRadiometricResult, BandRange, RadiometricValidationResult, RadiometricValidator,
    SensorProfile,
};
