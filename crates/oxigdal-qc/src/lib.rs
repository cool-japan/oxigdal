//! OxiGDAL Quality Control and Validation Suite
//!
//! This crate provides comprehensive quality control and validation for geospatial data,
//! including raster and vector data integrity checks, metadata validation, and automatic fixes.
//!
//! # Features
//!
//! - **Raster Quality Control**: Completeness, consistency, and accuracy checks
//! - **Vector Quality Control**: Topology validation and attribution checks
//! - **Metadata Validation**: ISO 19115 and STAC completeness
//! - **Rules Engine**: Configurable quality rules via TOML
//! - **Automatic Fixes**: Safe automatic repairs for common issues
//! - **Reporting**: HTML and JSON report generation
//!
//! # Example Usage
//!
//! ```rust
//! use oxigdal_qc::prelude::*;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! // Check raster completeness
//! let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//! let checker = CompletenessChecker::new();
//! let result = checker.check_buffer(&buffer)?;
//!
//! println!("Valid pixels: {}/{}", result.valid_pixels, result.total_pixels);
//! # Ok::<(), oxigdal_qc::error::QcError>(())
//! ```
//!
//! # Module Organization
//!
//! - [`error`] - Error types and severity levels
//! - [`raster`] - Raster quality control checks
//! - [`vector`] - Vector quality control checks
//! - [`metadata`] - Metadata validation
//! - [`report`] - Report generation
//! - [`rules`] - Quality rules engine
//! - [`fix`] - Automatic fixes

#![warn(missing_docs)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod error;
pub mod fix;
pub mod metadata;
pub mod raster;
pub mod report;
pub mod rules;
pub mod vector;

/// Prelude module for convenient imports.
pub mod prelude {
    /// Convenient re-exports of commonly used types and traits.
    pub use crate::error::{QcError, QcIssue, QcResult, Severity};
    pub use crate::fix::{FixResult, FixStrategy, TopologyFixer};
    pub use crate::metadata::{MetadataChecker, MetadataConfig, MetadataResult, MetadataStandard};
    pub use crate::raster::{
        AccuracyChecker, AccuracyConfig, AccuracyResult, CompletenessChecker, CompletenessConfig,
        CompletenessResult, ConsistencyChecker, ConsistencyConfig, ConsistencyResult,
    };
    pub use crate::report::{QualityAssessment, QualityReport, ReportSection};
    pub use crate::rules::{
        ComparisonOperator, QualityRule, RuleBuilder, RuleCategory, RuleSet, RulesEngine,
    };
    pub use crate::vector::{
        AttributionChecker, AttributionConfig, AttributionResult, TopologyChecker, TopologyConfig,
        TopologyResult,
    };
}

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-qc");
    }

    #[test]
    fn test_prelude_imports() {
        use prelude::*;

        // Test that common types are accessible
        let _severity = Severity::Major;
        let _assessment = QualityAssessment::Good;
        let _strategy = FixStrategy::Conservative;
    }
}
