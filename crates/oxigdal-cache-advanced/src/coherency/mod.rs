//! Cache coherency protocols
//!
//! Provides coherency protocols for distributed caching scenarios.

pub mod protocol;

pub use protocol::{
    CoherencyMessage, DirectoryCoherency, InvalidationBatcher, MESIProtocol, MESIState,
    MSIProtocol, MSIState,
};
