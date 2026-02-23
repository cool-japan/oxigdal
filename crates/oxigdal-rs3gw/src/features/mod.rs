//! Advanced features for rs3gw integration
//!
//! This module provides optional advanced features for optimizing
//! geospatial data access with rs3gw.

#[cfg(feature = "ml-cache")]
pub mod caching;

#[cfg(feature = "dedup")]
pub mod dedup;

#[cfg(feature = "encryption")]
pub mod encryption;

// Re-exports
#[cfg(feature = "ml-cache")]
pub use caching::{CogAccessPattern, CogCacheConfig};

#[cfg(feature = "dedup")]
pub use dedup::{ZarrDedupConfig, ZarrDedupPresets};

#[cfg(feature = "encryption")]
pub use encryption::{EncryptionConfig, generate_key};
