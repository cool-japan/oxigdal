//! Vector clock implementation for causality tracking
//!
//! Vector clocks provide a mechanism to capture causal relationships between
//! events in distributed systems. They allow us to determine if events are
//! causally related or concurrent.

use crate::{DeviceId, Timestamp, VersionVector};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

/// Comparison result for vector clocks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockOrdering {
    /// This clock is less than (happened before) the other
    Before,
    /// This clock is greater than (happened after) the other
    After,
    /// Clocks are equal (same causal history)
    Equal,
    /// Clocks are concurrent (independent causal histories)
    Concurrent,
}

/// Vector clock for tracking causality in distributed systems
///
/// A vector clock is a data structure used for determining the partial ordering
/// of events in a distributed system and detecting causality violations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClock {
    /// Map of device ID to logical timestamp
    clock: VersionVector,
    /// The device ID that owns this clock
    device_id: DeviceId,
}

impl VectorClock {
    /// Creates a new vector clock for the given device
    ///
    /// # Arguments
    ///
    /// * `device_id` - The identifier of the device owning this clock
    ///
    /// # Example
    ///
    /// ```rust
    /// use oxigdal_sync::vector_clock::VectorClock;
    ///
    /// let clock = VectorClock::new("device-1".to_string());
    /// ```
    pub fn new(device_id: DeviceId) -> Self {
        let mut clock = HashMap::new();
        clock.insert(device_id.clone(), 0);

        Self { clock, device_id }
    }

    /// Creates a vector clock from an existing version vector
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device ID
    /// * `clock` - Existing version vector
    pub fn from_version_vector(device_id: DeviceId, clock: VersionVector) -> Self {
        Self { clock, device_id }
    }

    /// Increments the clock for this device
    ///
    /// This should be called when a new event occurs on this device.
    ///
    /// # Example
    ///
    /// ```rust
    /// use oxigdal_sync::vector_clock::VectorClock;
    ///
    /// let mut clock = VectorClock::new("device-1".to_string());
    /// clock.tick();
    /// assert_eq!(clock.get_time(&"device-1".to_string()), Some(1));
    /// ```
    pub fn tick(&mut self) -> Timestamp {
        let entry = self.clock.entry(self.device_id.clone()).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Gets the timestamp for a specific device
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device to query
    ///
    /// # Returns
    ///
    /// The timestamp for the device, or None if the device is not in the clock
    pub fn get_time(&self, device_id: &DeviceId) -> Option<Timestamp> {
        self.clock.get(device_id).copied()
    }

    /// Gets the current timestamp for this device
    pub fn current_time(&self) -> Timestamp {
        self.clock.get(&self.device_id).copied().unwrap_or(0)
    }

    /// Merges another vector clock into this one
    ///
    /// Takes the maximum timestamp for each device. This is used when
    /// receiving an event from another device.
    ///
    /// # Arguments
    ///
    /// * `other` - The clock to merge
    ///
    /// # Example
    ///
    /// ```rust
    /// use oxigdal_sync::vector_clock::VectorClock;
    ///
    /// let mut clock1 = VectorClock::new("device-1".to_string());
    /// let mut clock2 = VectorClock::new("device-2".to_string());
    ///
    /// clock1.tick();
    /// clock2.tick();
    ///
    /// clock1.merge(&clock2);
    /// assert_eq!(clock1.get_time(&"device-2".to_string()), Some(1));
    /// ```
    pub fn merge(&mut self, other: &VectorClock) {
        for (device_id, &timestamp) in &other.clock {
            let entry = self.clock.entry(device_id.clone()).or_insert(0);
            *entry = (*entry).max(timestamp);
        }
    }

    /// Compares this clock with another to determine causal ordering
    ///
    /// # Arguments
    ///
    /// * `other` - The clock to compare with
    ///
    /// # Returns
    ///
    /// The ordering relationship between the clocks
    ///
    /// # Example
    ///
    /// ```rust
    /// use oxigdal_sync::vector_clock::{VectorClock, ClockOrdering};
    ///
    /// let mut clock1 = VectorClock::new("device-1".to_string());
    /// let mut clock2 = VectorClock::new("device-1".to_string());
    ///
    /// clock2.tick();
    ///
    /// assert_eq!(clock1.compare(&clock2), ClockOrdering::Before);
    /// assert_eq!(clock2.compare(&clock1), ClockOrdering::After);
    /// ```
    pub fn compare(&self, other: &VectorClock) -> ClockOrdering {
        let mut less_than = false;
        let mut greater_than = false;

        // Collect all device IDs from both clocks
        let mut all_devices = self.clock.keys().collect::<Vec<_>>();
        for device in other.clock.keys() {
            if !all_devices.contains(&device) {
                all_devices.push(device);
            }
        }

        // Compare timestamps for each device
        for device_id in all_devices {
            let self_time = self.clock.get(device_id).copied().unwrap_or(0);
            let other_time = other.clock.get(device_id).copied().unwrap_or(0);

            match self_time.cmp(&other_time) {
                Ordering::Less => less_than = true,
                Ordering::Greater => greater_than = true,
                Ordering::Equal => {}
            }
        }

        match (less_than, greater_than) {
            (false, false) => ClockOrdering::Equal,
            (true, false) => ClockOrdering::Before,
            (false, true) => ClockOrdering::After,
            (true, true) => ClockOrdering::Concurrent,
        }
    }

    /// Checks if this clock happened before the other
    pub fn happened_before(&self, other: &VectorClock) -> bool {
        matches!(self.compare(other), ClockOrdering::Before)
    }

    /// Checks if this clock happened after the other
    pub fn happened_after(&self, other: &VectorClock) -> bool {
        matches!(self.compare(other), ClockOrdering::After)
    }

    /// Checks if clocks are concurrent (causally independent)
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        matches!(self.compare(other), ClockOrdering::Concurrent)
    }

    /// Gets the device ID associated with this clock
    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    /// Gets a reference to the underlying version vector
    pub fn as_version_vector(&self) -> &VersionVector {
        &self.clock
    }

    /// Converts the clock into a version vector
    pub fn into_version_vector(self) -> VersionVector {
        self.clock
    }

    /// Creates a clone with a new device ID
    pub fn with_device_id(&self, device_id: DeviceId) -> Self {
        Self {
            clock: self.clock.clone(),
            device_id,
        }
    }

    /// Updates the timestamp for a specific device
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device to update
    /// * `timestamp` - The new timestamp
    pub fn update(&mut self, device_id: DeviceId, timestamp: Timestamp) {
        let entry = self.clock.entry(device_id).or_insert(0);
        *entry = (*entry).max(timestamp);
    }

    /// Gets the sum of all timestamps (useful for debugging)
    pub fn sum(&self) -> Timestamp {
        self.clock.values().sum()
    }

    /// Checks if the clock is empty (no devices)
    pub fn is_empty(&self) -> bool {
        self.clock.is_empty()
    }

    /// Gets the number of devices in the clock
    pub fn len(&self) -> usize {
        self.clock.len()
    }

    /// Gets all device IDs in the clock
    pub fn devices(&self) -> Vec<DeviceId> {
        self.clock.keys().cloned().collect()
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new("default".to_string())
    }
}

/// Extension trait for working with causality
pub trait CausalOrdering {
    /// Checks if this event causally precedes another
    fn precedes(&self, other: &Self) -> bool;

    /// Checks if this event is concurrent with another
    fn concurrent_with(&self, other: &Self) -> bool;
}

impl CausalOrdering for VectorClock {
    fn precedes(&self, other: &Self) -> bool {
        self.happened_before(other)
    }

    fn concurrent_with(&self, other: &Self) -> bool {
        self.is_concurrent(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_creation() {
        let clock = VectorClock::new("device-1".to_string());
        assert_eq!(clock.get_time(&"device-1".to_string()), Some(0));
        assert_eq!(clock.device_id(), "device-1");
    }

    #[test]
    fn test_tick() {
        let mut clock = VectorClock::new("device-1".to_string());
        assert_eq!(clock.tick(), 1);
        assert_eq!(clock.tick(), 2);
        assert_eq!(clock.get_time(&"device-1".to_string()), Some(2));
    }

    #[test]
    fn test_merge() {
        let mut clock1 = VectorClock::new("device-1".to_string());
        let mut clock2 = VectorClock::new("device-2".to_string());

        clock1.tick();
        clock1.tick();
        clock2.tick();

        clock1.merge(&clock2);

        assert_eq!(clock1.get_time(&"device-1".to_string()), Some(2));
        assert_eq!(clock1.get_time(&"device-2".to_string()), Some(1));
    }

    #[test]
    fn test_compare_equal() {
        let clock1 = VectorClock::new("device-1".to_string());
        let clock2 = VectorClock::new("device-1".to_string());

        assert_eq!(clock1.compare(&clock2), ClockOrdering::Equal);
    }

    #[test]
    fn test_compare_before() {
        let clock1 = VectorClock::new("device-1".to_string());
        let mut clock2 = VectorClock::new("device-1".to_string());

        clock2.tick();

        assert_eq!(clock1.compare(&clock2), ClockOrdering::Before);
        assert!(clock1.happened_before(&clock2));
    }

    #[test]
    fn test_compare_after() {
        let mut clock1 = VectorClock::new("device-1".to_string());
        let clock2 = VectorClock::new("device-1".to_string());

        clock1.tick();

        assert_eq!(clock1.compare(&clock2), ClockOrdering::After);
        assert!(clock1.happened_after(&clock2));
    }

    #[test]
    fn test_compare_concurrent() {
        let mut clock1 = VectorClock::new("device-1".to_string());
        let mut clock2 = VectorClock::new("device-2".to_string());

        clock1.tick();
        clock2.tick();

        assert_eq!(clock1.compare(&clock2), ClockOrdering::Concurrent);
        assert!(clock1.is_concurrent(&clock2));
    }

    #[test]
    fn test_complex_merge() {
        let mut clock1 = VectorClock::new("device-1".to_string());
        let mut clock2 = VectorClock::new("device-2".to_string());
        let mut clock3 = VectorClock::new("device-3".to_string());

        clock1.tick();
        clock1.tick();

        clock2.tick();
        clock2.merge(&clock1);
        clock2.tick();

        clock3.tick();

        clock1.merge(&clock2);
        clock1.merge(&clock3);

        assert_eq!(clock1.get_time(&"device-1".to_string()), Some(2));
        assert_eq!(clock1.get_time(&"device-2".to_string()), Some(2));
        assert_eq!(clock1.get_time(&"device-3".to_string()), Some(1));
    }
}
