//! Data anonymization.

pub mod differential_privacy;
pub mod generalization;
pub mod masking;

use serde::{Deserialize, Serialize};

/// Anonymization strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnonymizationStrategy {
    /// Mask with fixed character.
    Masking,
    /// K-anonymity.
    KAnonymity,
    /// L-diversity.
    LDiversity,
    /// Differential privacy.
    DifferentialPrivacy,
}

/// Anonymization config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnonymizationConfig {
    /// Strategy to use.
    pub strategy: AnonymizationStrategy,
    /// Fields to anonymize.
    pub fields: Vec<String>,
    /// Strategy-specific parameters.
    pub parameters: std::collections::HashMap<String, String>,
}
