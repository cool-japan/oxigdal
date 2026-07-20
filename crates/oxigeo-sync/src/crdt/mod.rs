//! Conflict-free Replicated Data Types (CRDTs)
//!
//! This module provides various CRDT implementations for distributed
//! state synchronization:
//!
//! - **LWW-Register**: Last-Write-Wins Register for single values
//! - **G-Counter**: Grow-only Counter
//! - **PN-Counter**: Positive-Negative Counter (increment/decrement)
//! - **OR-Set**: Observed-Remove Set for add/remove operations
//!
//! CRDTs guarantee eventual consistency without requiring coordination
//! between replicas.

pub mod g_counter;
pub mod lww_register;
pub mod or_set;
pub mod pn_counter;

pub use g_counter::GCounter;
pub use lww_register::LwwRegister;
pub use or_set::OrSet;
pub use pn_counter::PnCounter;

use crate::{DeviceId, SyncResult};
use serde::{Deserialize, Serialize};

/// Trait for mergeable CRDTs
///
/// All CRDTs must implement this trait to allow merging of
/// concurrent states.
pub trait Crdt: Clone + Serialize + for<'de> Deserialize<'de> {
    /// Merges another CRDT state into this one
    ///
    /// After merging, this CRDT should reflect the combined state
    /// of both CRDTs in a way that guarantees convergence.
    ///
    /// # Arguments
    ///
    /// * `other` - The CRDT to merge with
    fn merge(&mut self, other: &Self) -> SyncResult<()>;

    /// Checks if this CRDT is causally dominated by another
    ///
    /// Returns true if all changes in this CRDT are also present
    /// in the other CRDT.
    fn dominated_by(&self, other: &Self) -> bool;
}

/// Trait for CRDTs that track device state
pub trait DeviceAware {
    /// Gets the device ID associated with this CRDT
    fn device_id(&self) -> &DeviceId;

    /// Sets the device ID
    fn set_device_id(&mut self, device_id: DeviceId);
}
