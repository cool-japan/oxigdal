//! Metadata quality control modules.

pub mod completeness;

pub use completeness::{MetadataChecker, MetadataConfig, MetadataResult, MetadataStandard};
