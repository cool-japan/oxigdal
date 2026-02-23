//! OxiGDAL Offline - Offline-First Data Management
//!
//! This crate provides comprehensive offline-first data management capabilities for OxiGDAL,
//! including local storage, sync queue management, conflict resolution, and optimistic updates.
//!
//! # Features
//!
//! - **Offline-first architecture**: Local-first data layer with background sync
//! - **Multi-platform storage**: IndexedDB for WASM, SQLite for native platforms
//! - **Sync queue management**: Persistent queue with automatic retry
//! - **Conflict detection**: Automatic detection of concurrent modifications
//! - **Merge strategies**: Configurable strategies (Last-Write-Wins, Three-Way-Merge, Custom)
//! - **Background sync**: Automatic sync when connectivity is restored
//! - **Optimistic updates**: Immediate UI updates with eventual consistency
//! - **Retry mechanisms**: Exponential backoff with jitter
//!
//! # Architecture
//!
//! The offline system consists of several key components:
//!
//! 1. **Storage Layer**: Abstraction over IndexedDB (WASM) and SQLite (native)
//! 2. **Sync Queue**: Persistent queue of pending operations
//! 3. **Conflict Detector**: Detects concurrent modifications
//! 4. **Merge Engine**: Resolves conflicts using configurable strategies
//! 5. **Retry Manager**: Handles failed sync attempts with exponential backoff
//! 6. **Optimistic Update Tracker**: Tracks optimistic changes for rollback
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_offline::{OfflineManager, Config, MergeStrategy};
//! use oxigdal_offline::storage::StorageBackend;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure offline manager
//! let config = Config::builder()
//!     .max_queue_size(1000)
//!     .merge_strategy(MergeStrategy::LastWriteWins)
//!     .retry_max_attempts(5)
//!     .build()?;
//!
//! // Create offline manager
//! let manager = OfflineManager::new(config).await?;
//!
//! // Write data (automatically queued for sync)
//! manager.write("key1", b"value1").await?;
//!
//! // Read data (from local cache)
//! let value = manager.read("key1").await?;
//!
//! // Sync when online
//! manager.sync().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # WASM Support
//!
//! The crate is fully WASM-compatible, using IndexedDB for storage in browsers:
//!
//! ```rust,ignore
//! // Enable WASM feature in Cargo.toml
//! // oxigdal-offline = { version = "0.1", features = ["wasm"] }
//!
//! use oxigdal_offline::OfflineManager;
//! use oxigdal_offline::error::Result;
//!
//! async fn wasm_example() -> Result<()> {
//!     let manager = OfflineManager::new_wasm("my-database").await?;
//!     manager.write("key", b"value").await?;
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![allow(clippy::module_name_repetitions)]

pub mod config;
pub mod conflict;
pub mod error;
pub mod manager;
pub mod merge;
pub mod optimistic;
pub mod queue;
pub mod retry;
pub mod storage;
pub mod sync;
pub mod types;

// Re-export commonly used items
pub use config::{Config, ConfigBuilder};
pub use error::{Error, Result};
pub use manager::OfflineManager;
pub use merge::MergeStrategy;
pub use types::{Operation, OperationId, Record, RecordId, Version};

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
        assert_eq!(NAME, "oxigdal-offline");
    }
}
