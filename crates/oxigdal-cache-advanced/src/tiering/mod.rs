//! Cache tiering management
//!
//! Intelligent promotion and demotion policies for multi-tier caches.

pub mod policy;

pub use policy::{
    AccessStats, AdaptiveTierSizer, CostAwarePolicy, FrequencyBasedPolicy, TierInfo, TieringAction,
};
