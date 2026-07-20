//! Change Detection Module
//!
//! This module provides algorithms for detecting changes between multi-temporal images:
//! - Image differencing
//! - Change Vector Analysis (CVA)
//! - Principal Component Analysis (PCA)
//! - Threshold optimization

pub mod detection;

pub use detection::{
    ChangeDetector, ChangeMethod, ChangeResult, ChangeVectorAnalysis, ImageDifferencing,
    PrincipalComponentAnalysis, ThresholdOptimizer,
};
