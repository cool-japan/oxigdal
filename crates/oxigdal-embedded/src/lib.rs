//! # OxiGDAL Embedded
//!
//! Embedded systems support for OxiGDAL providing no_std compatible geospatial processing
//! for ARM, RISC-V, ESP32, and other embedded targets.
//!
//! ## Features
//!
//! - `no_std` compatibility with optional `alloc` support
//! - Static memory pools for predictable allocation behavior
//! - Target-specific optimizations (ARM, RISC-V, ESP32)
//! - Low-power operation modes
//! - Real-time constraints support
//! - Minimal binary footprint
//!
//! ## Usage
//!
//! ```rust
//! use oxigdal_embedded::memory_pool::StaticPool;
//! use oxigdal_embedded::minimal::MinimalRasterMeta;
//!
//! // Create a static memory pool for no_std environments
//! static POOL: StaticPool<4096> = StaticPool::new();
//!
//! // Use minimal raster metadata for constrained resources
//! let meta = MinimalRasterMeta::new(256, 256, 3, 1);
//! assert_eq!(meta.total_size(), 256 * 256 * 3);
//! ```
//!
//! ## Architecture
//!
//! The crate is organized into:
//! - `alloc/` - Custom allocators for no_std environments
//! - `memory_pool` - Static memory pool implementations
//! - `target/` - Target-specific optimizations
//! - `power` - Power management utilities
//! - `realtime` - Real-time scheduling support
//! - `minimal` - Minimal feature set for ultra-constrained environments

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
// Allow unsafe code - embedded systems require unsafe for memory pools, allocators,
// and target-specific operations (GlobalAlloc, assembly, atomic operations)
#![allow(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod error;

#[cfg(feature = "alloc")]
pub mod alloc_utils;

pub mod memory_pool;
pub mod target;

#[cfg(feature = "low-power")]
pub mod power;

#[cfg(feature = "realtime")]
pub mod realtime;

pub mod buffer;
pub mod config;
pub mod minimal;
pub mod sync;

/// Prelude module containing commonly used types and traits
pub mod prelude {
    pub use crate::buffer::FixedBuffer;
    pub use crate::error::{EmbeddedError, Result};
    pub use crate::memory_pool::{MemoryPool, StaticPool};
    pub use crate::minimal::{MinimalBounds, MinimalCoordinate};
    pub use crate::sync::AtomicCounter;

    #[cfg(feature = "alloc")]
    pub use crate::alloc_utils::BumpAllocator;

    #[cfg(feature = "realtime")]
    pub use crate::realtime::RealtimeScheduler;

    #[cfg(feature = "low-power")]
    pub use crate::power::PowerMode;
}

#[cfg(test)]
mod unit_tests {

    #[test]
    fn test_basic_functionality() {
        // Basic sanity check that the crate compiles
        use crate::prelude::*;
        let _pool = StaticPool::<1024>::new();
    }
}
