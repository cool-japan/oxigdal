//! Group metadata structures
//!
//! This module provides metadata types for Zarr groups.

use super::ZarrFormat;
use serde::{Deserialize, Serialize};

/// Group metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMetadata {
    /// Zarr format version
    pub zarr_format: ZarrFormat,
}

impl GroupMetadata {
    /// Creates new group metadata
    #[must_use]
    pub fn new(zarr_format: ZarrFormat) -> Self {
        Self { zarr_format }
    }

    /// Creates v2 group metadata
    #[must_use]
    pub fn v2() -> Self {
        Self::new(ZarrFormat::V2)
    }

    /// Creates v3 group metadata
    #[must_use]
    pub fn v3() -> Self {
        Self::new(ZarrFormat::V3)
    }
}
