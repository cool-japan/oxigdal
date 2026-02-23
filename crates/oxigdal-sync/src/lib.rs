//! Multi-device synchronization for OxiGDAL
//!
//! This crate provides advanced synchronization capabilities for distributed
//! geospatial data management, including:
//!
//! - **CRDTs (Conflict-free Replicated Data Types)**: LWW-Register, G-Counter,
//!   PN-Counter, OR-Set for conflict-free distributed state
//! - **Vector Clocks**: Causality tracking for distributed events
//! - **Operational Transformation**: Concurrent edit reconciliation
//! - **Multi-device Coordination**: Device discovery and state management
//! - **Merkle Trees**: Efficient change detection and synchronization
//! - **Delta Encoding**: Bandwidth-efficient state transfer
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_sync::crdt::LwwRegister;
//! use oxigdal_sync::vector_clock::VectorClock;
//!
//! // Create a Last-Write-Wins register with vector clock
//! let mut register = LwwRegister::new("device-1".to_string(), "initial".to_string());
//!
//! // Update with causality tracking
//! register.set("updated".to_string());
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod coordinator;
pub mod crdt;
pub mod delta;
pub mod error;
pub mod merkle;
pub mod ot;
pub mod vector_clock;

#[cfg(test)]
mod tests;

pub use error::{SyncError, SyncResult};

/// Device identifier type
pub type DeviceId = String;

/// Timestamp type for synchronization
pub type Timestamp = u64;

/// Version vector type for causality tracking
pub type VersionVector = std::collections::HashMap<DeviceId, Timestamp>;
