//! Last-Write-Wins Register CRDT
//!
//! A register that resolves conflicts by keeping the value with the
//! highest timestamp (last write wins).

use crate::crdt::{Crdt, DeviceAware};
use crate::vector_clock::{ClockOrdering, VectorClock};
use crate::{DeviceId, SyncResult};
use serde::{Deserialize, Serialize};

/// Last-Write-Wins Register
///
/// A simple CRDT that stores a single value and resolves conflicts
/// by keeping the value with the most recent timestamp.
///
/// # Example
///
/// ```rust
/// use oxigdal_sync::crdt::{LwwRegister, Crdt};
///
/// let mut reg1 = LwwRegister::new("device-1".to_string(), "hello".to_string());
/// let mut reg2 = LwwRegister::new("device-2".to_string(), "world".to_string());
///
/// reg1.set("updated".to_string());
/// reg1.merge(&reg2).ok();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize"))]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct LwwRegister<T>
where
    T: Clone,
{
    /// The current value
    value: T,
    /// Vector clock for causality tracking
    clock: VectorClock,
    /// Device ID
    device_id: DeviceId,
}

impl<T> LwwRegister<T>
where
    T: Clone,
{
    /// Creates a new LWW register with an initial value
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device ID
    /// * `initial_value` - The initial value
    pub fn new(device_id: DeviceId, initial_value: T) -> Self {
        Self {
            value: initial_value,
            clock: VectorClock::new(device_id.clone()),
            device_id,
        }
    }

    /// Gets the current value
    pub fn get(&self) -> &T {
        &self.value
    }

    /// Sets a new value
    ///
    /// This increments the local clock to track the update.
    ///
    /// # Arguments
    ///
    /// * `value` - The new value to set
    pub fn set(&mut self, value: T) {
        self.value = value;
        self.clock.tick();
    }

    /// Gets the vector clock
    pub fn clock(&self) -> &VectorClock {
        &self.clock
    }

    /// Updates the value with a specific clock
    ///
    /// This is useful for applying remote updates.
    ///
    /// # Arguments
    ///
    /// * `value` - The new value
    /// * `clock` - The vector clock for the update
    pub fn set_with_clock(&mut self, value: T, clock: VectorClock) {
        match self.clock.compare(&clock) {
            ClockOrdering::Before => {
                // Remote update is newer
                self.value = value;
                self.clock = clock;
            }
            ClockOrdering::Concurrent => {
                // Concurrent updates - use deterministic tie-breaking
                if self.tie_break(&clock) {
                    self.value = value;
                }
                self.clock.merge(&clock);
            }
            _ => {
                // Our update is newer or equal, keep it
                self.clock.merge(&clock);
            }
        }
    }

    /// Deterministic tie-breaking for concurrent updates
    ///
    /// Uses device ID comparison as a tie-breaker.
    fn tie_break(&self, other_clock: &VectorClock) -> bool {
        other_clock.device_id() > &self.device_id
    }
}

impl<T> Crdt for LwwRegister<T>
where
    T: Clone + Serialize + for<'de> serde::Deserialize<'de>,
{
    fn merge(&mut self, other: &Self) -> SyncResult<()> {
        match self.clock.compare(&other.clock) {
            ClockOrdering::Before => {
                // Other is newer, adopt its value
                self.value = other.value.clone();
                self.clock = other.clock.clone();
            }
            ClockOrdering::Concurrent => {
                // Concurrent updates - use tie-breaking
                if self.tie_break(&other.clock) {
                    self.value = other.value.clone();
                }
                self.clock.merge(&other.clock);
            }
            _ => {
                // We are newer or equal, just merge clocks
                self.clock.merge(&other.clock);
            }
        }
        Ok(())
    }

    fn dominated_by(&self, other: &Self) -> bool {
        matches!(
            self.clock.compare(&other.clock),
            ClockOrdering::Before | ClockOrdering::Equal
        )
    }
}

impl<T> DeviceAware for LwwRegister<T>
where
    T: Clone + Serialize + for<'de> serde::Deserialize<'de>,
{
    fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    fn set_device_id(&mut self, device_id: DeviceId) {
        self.device_id = device_id.clone();
        self.clock = self.clock.with_device_id(device_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lww_register_creation() {
        let reg = LwwRegister::new("device-1".to_string(), 42);
        assert_eq!(*reg.get(), 42);
        assert_eq!(reg.device_id(), "device-1");
    }

    #[test]
    fn test_lww_register_set() {
        let mut reg = LwwRegister::new("device-1".to_string(), 42);
        reg.set(100);
        assert_eq!(*reg.get(), 100);
    }

    #[test]
    fn test_lww_register_merge_before() {
        let mut reg1 = LwwRegister::new("device-1".to_string(), 42);
        let mut reg2 = LwwRegister::new("device-1".to_string(), 42);

        reg2.set(100);

        reg1.merge(&reg2).ok();
        assert_eq!(*reg1.get(), 100);
    }

    #[test]
    fn test_lww_register_merge_after() {
        let mut reg1 = LwwRegister::new("device-1".to_string(), 42);
        let reg2 = LwwRegister::new("device-1".to_string(), 42);

        reg1.set(100);

        reg1.merge(&reg2).ok();
        assert_eq!(*reg1.get(), 100);
    }

    #[test]
    fn test_lww_register_concurrent() {
        let mut reg1 = LwwRegister::new("device-1".to_string(), 42);
        let mut reg2 = LwwRegister::new("device-2".to_string(), 42);

        reg1.set(100);
        reg2.set(200);

        let _initial = *reg1.get();
        reg1.merge(&reg2).ok();

        // Should use tie-breaking (device-2 > device-1)
        assert_eq!(*reg1.get(), 200);
    }

    #[test]
    fn test_dominated_by() {
        let reg1 = LwwRegister::new("device-1".to_string(), 42);
        let mut reg2 = LwwRegister::new("device-1".to_string(), 42);

        reg2.set(100);

        assert!(reg1.dominated_by(&reg2));
        assert!(!reg2.dominated_by(&reg1));
    }
}
