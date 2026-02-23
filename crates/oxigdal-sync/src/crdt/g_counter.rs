//! Grow-only Counter CRDT
//!
//! A counter that can only be incremented, never decremented.

use crate::crdt::{Crdt, DeviceAware};
use crate::{DeviceId, SyncResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Grow-only Counter
///
/// A CRDT that represents a counter that can only increase.
/// Each device maintains its own increment count, and the total
/// is the sum of all device counts.
///
/// # Example
///
/// ```rust
/// use oxigdal_sync::crdt::{GCounter, Crdt};
///
/// let mut counter1 = GCounter::new("device-1".to_string());
/// counter1.increment(5);
///
/// let mut counter2 = GCounter::new("device-2".to_string());
/// counter2.increment(3);
///
/// counter1.merge(&counter2).ok();
/// assert_eq!(counter1.value(), 8);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GCounter {
    /// Per-device increment counts
    counts: HashMap<DeviceId, u64>,
    /// This device's ID
    device_id: DeviceId,
}

impl GCounter {
    /// Creates a new G-Counter
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device ID
    pub fn new(device_id: DeviceId) -> Self {
        let mut counts = HashMap::new();
        counts.insert(device_id.clone(), 0);

        Self { counts, device_id }
    }

    /// Increments the counter by the given amount
    ///
    /// # Arguments
    ///
    /// * `delta` - The amount to increment by
    ///
    /// # Returns
    ///
    /// The new total value
    pub fn increment(&mut self, delta: u64) -> u64 {
        let entry = self.counts.entry(self.device_id.clone()).or_insert(0);
        *entry = entry.saturating_add(delta);
        self.value()
    }

    /// Increments the counter by 1
    ///
    /// # Returns
    ///
    /// The new total value
    pub fn inc(&mut self) -> u64 {
        self.increment(1)
    }

    /// Gets the current total value
    ///
    /// This is the sum of all device counts.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Gets the count for a specific device
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device to query
    ///
    /// # Returns
    ///
    /// The count for that device, or 0 if not present
    pub fn get_device_count(&self, device_id: &DeviceId) -> u64 {
        self.counts.get(device_id).copied().unwrap_or(0)
    }

    /// Gets all device counts
    pub fn counts(&self) -> &HashMap<DeviceId, u64> {
        &self.counts
    }

    /// Gets the number of devices that have contributed
    pub fn device_count(&self) -> usize {
        self.counts.len()
    }
}

impl Crdt for GCounter {
    fn merge(&mut self, other: &Self) -> SyncResult<()> {
        for (device_id, &count) in &other.counts {
            let entry = self.counts.entry(device_id.clone()).or_insert(0);
            *entry = (*entry).max(count);
        }
        Ok(())
    }

    fn dominated_by(&self, other: &Self) -> bool {
        for (device_id, &count) in &self.counts {
            let other_count = other.counts.get(device_id).copied().unwrap_or(0);
            if count > other_count {
                return false;
            }
        }
        true
    }
}

impl DeviceAware for GCounter {
    fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    fn set_device_id(&mut self, device_id: DeviceId) {
        // Transfer count from old device to new device
        if let Some(count) = self.counts.remove(&self.device_id) {
            self.counts.insert(device_id.clone(), count);
        }
        self.device_id = device_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_g_counter_creation() {
        let counter = GCounter::new("device-1".to_string());
        assert_eq!(counter.value(), 0);
        assert_eq!(counter.device_id(), "device-1");
    }

    #[test]
    fn test_g_counter_increment() {
        let mut counter = GCounter::new("device-1".to_string());
        counter.increment(5);
        assert_eq!(counter.value(), 5);
        counter.increment(3);
        assert_eq!(counter.value(), 8);
    }

    #[test]
    fn test_g_counter_inc() {
        let mut counter = GCounter::new("device-1".to_string());
        counter.inc();
        counter.inc();
        counter.inc();
        assert_eq!(counter.value(), 3);
    }

    #[test]
    fn test_g_counter_merge() {
        let mut counter1 = GCounter::new("device-1".to_string());
        let mut counter2 = GCounter::new("device-2".to_string());

        counter1.increment(5);
        counter2.increment(3);

        counter1.merge(&counter2).ok();

        assert_eq!(counter1.value(), 8);
        assert_eq!(counter1.get_device_count(&"device-1".to_string()), 5);
        assert_eq!(counter1.get_device_count(&"device-2".to_string()), 3);
    }

    #[test]
    fn test_g_counter_merge_same_device() {
        let mut counter1 = GCounter::new("device-1".to_string());
        let mut counter2 = GCounter::new("device-1".to_string());

        counter1.increment(5);
        counter2.increment(3);

        counter1.merge(&counter2).ok();

        // Should take max
        assert_eq!(counter1.value(), 5);
    }

    #[test]
    fn test_g_counter_dominated_by() {
        let mut counter1 = GCounter::new("device-1".to_string());
        let mut counter2 = GCounter::new("device-1".to_string());

        counter1.increment(3);
        counter2.increment(5);

        assert!(counter1.dominated_by(&counter2));
        assert!(!counter2.dominated_by(&counter1));
    }

    #[test]
    fn test_g_counter_device_count() {
        let mut counter1 = GCounter::new("device-1".to_string());
        let mut counter2 = GCounter::new("device-2".to_string());
        let mut counter3 = GCounter::new("device-3".to_string());

        counter1.increment(1);
        counter2.increment(1);
        counter3.increment(1);

        counter1.merge(&counter2).ok();
        counter1.merge(&counter3).ok();

        assert_eq!(counter1.device_count(), 3);
    }
}
