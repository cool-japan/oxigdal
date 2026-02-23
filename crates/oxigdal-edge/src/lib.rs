//! # OxiGDAL Edge Computing Platform
//!
//! `oxigdal-edge` provides edge computing capabilities for geospatial data processing
//! with a focus on:
//! - Minimal binary footprint for embedded deployment
//! - Offline-first architecture with local caching
//! - Edge-to-cloud synchronization with conflict resolution
//! - Resource-constrained device support
//!
//! ## Features
//!
//! - **Edge Runtime**: Lightweight runtime optimized for resource-constrained devices
//! - **Synchronization**: Edge-to-cloud sync protocols with conflict resolution
//! - **CRDT-based Conflict Resolution**: Automatic conflict resolution for distributed nodes
//! - **Edge Compression**: Optimized compression for bandwidth-limited environments
//! - **Local Caching**: Offline-first caching strategy
//! - **Resource Management**: Memory and CPU management for constrained devices
//!
//! ## Example
//!
//! ```rust,no_run
//! use oxigdal_edge::{EdgeRuntime, EdgeConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create edge runtime with minimal footprint
//! let config = EdgeConfig::minimal();
//! let runtime = EdgeRuntime::new(config).await?;
//!
//! // Process data locally with caching
//! // Sync with cloud when connection available
//! # Ok(())
//! # }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod cache;
pub mod compression;
pub mod conflict;
pub mod error;
pub mod resource;
pub mod runtime;
pub mod sync;

pub use cache::{Cache, CacheConfig, CacheEntry, CachePolicy};
pub use compression::{AdaptiveCompressor, CompressionLevel, CompressionStrategy, EdgeCompressor};
pub use conflict::{ConflictResolver, CrdtMap, CrdtSet, VectorClock};
pub use error::{EdgeError, Result};
pub use resource::{ResourceConstraints, ResourceManager, ResourceMetrics};
pub use runtime::{EdgeConfig, EdgeRuntime, RuntimeMode};
pub use sync::{SyncManager, SyncProtocol, SyncStatus, SyncStrategy};

/// Edge computing version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default cache size for edge devices (10 MB)
pub const DEFAULT_CACHE_SIZE: usize = 10 * 1024 * 1024;

/// Default sync interval (5 minutes)
pub const DEFAULT_SYNC_INTERVAL_SECS: u64 = 300;

/// Maximum memory usage for edge runtime (50 MB)
pub const MAX_MEMORY_USAGE: usize = 50 * 1024 * 1024;
