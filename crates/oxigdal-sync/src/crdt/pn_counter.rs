//! Positive-Negative Counter CRDT
//!
//! A counter that supports both increment and decrement operations.

use crate::crdt::{Crdt, DeviceAware, GCounter};
use crate::{DeviceId, SyncResult};
use serde::{Deserialize, Serialize};

/// Positive-Negative Counter
///
/// A CRDT counter that supports both increment and decrement operations.
/// Implemented using two G-Counters: one for increments (positive) and
/// one for decrements (negative). The value is the difference.
///
/// # Example
///
/// ```rust
/// use oxigdal_sync::crdt::{PnCounter, Crdt};
///
/// let mut counter1 = PnCounter::new("device-1".to_string());
/// counter1.increment(10);
/// counter1.decrement(3);
///
/// let mut counter2 = PnCounter::new("device-2".to_string());
/// counter2.increment(5);
///
/// counter1.merge(&counter2).ok();
/// assert_eq!(counter1.value(), 12); // 10 - 3 + 5
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PnCounter {
    /// Positive increments
    positive: GCounter,
    /// Negative decrements
    negative: GCounter,
    /// Device ID
    device_id: DeviceId,
}

impl PnCounter {
    /// Creates a new PN-Counter
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device ID
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            positive: GCounter::new(device_id.clone()),
            negative: GCounter::new(device_id.clone()),
            device_id,
        }
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
    pub fn increment(&mut self, delta: u64) -> i64 {
        self.positive.increment(delta);
        self.value()
    }

    /// Decrements the counter by the given amount
    ///
    /// # Arguments
    ///
    /// * `delta` - The amount to decrement by
    ///
    /// # Returns
    ///
    /// The new total value
    pub fn decrement(&mut self, delta: u64) -> i64 {
        self.negative.increment(delta);
        self.value()
    }

    /// Increments the counter by 1
    ///
    /// # Returns
    ///
    /// The new total value
    pub fn inc(&mut self) -> i64 {
        self.increment(1)
    }

    /// Decrements the counter by 1
    ///
    /// # Returns
    ///
    /// The new total value
    pub fn dec(&mut self) -> i64 {
        self.decrement(1)
    }

    /// Gets the current value
    ///
    /// This is the difference between positive and negative counts.
    pub fn value(&self) -> i64 {
        let pos = self.positive.value() as i64;
        let neg = self.negative.value() as i64;
        pos - neg
    }

    /// Gets the positive counter
    pub fn positive_counter(&self) -> &GCounter {
        &self.positive
    }

    /// Gets the negative counter
    pub fn negative_counter(&self) -> &GCounter {
        &self.negative
    }

    /// Gets the total number of increments
    pub fn total_increments(&self) -> u64 {
        self.positive.value()
    }

    /// Gets the total number of decrements
    pub fn total_decrements(&self) -> u64 {
        self.negative.value()
    }
}

impl Crdt for PnCounter {
    fn merge(&mut self, other: &Self) -> SyncResult<()> {
        self.positive.merge(&other.positive)?;
        self.negative.merge(&other.negative)?;
        Ok(())
    }

    fn dominated_by(&self, other: &Self) -> bool {
        self.positive.dominated_by(&other.positive) && self.negative.dominated_by(&other.negative)
    }
}

impl DeviceAware for PnCounter {
    fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    fn set_device_id(&mut self, device_id: DeviceId) {
        self.device_id = device_id.clone();
        self.positive.set_device_id(device_id.clone());
        self.negative.set_device_id(device_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pn_counter_creation() {
        let counter = PnCounter::new("device-1".to_string());
        assert_eq!(counter.value(), 0);
        assert_eq!(counter.device_id(), "device-1");
    }

    #[test]
    fn test_pn_counter_increment() {
        let mut counter = PnCounter::new("device-1".to_string());
        counter.increment(5);
        assert_eq!(counter.value(), 5);
        counter.increment(3);
        assert_eq!(counter.value(), 8);
    }

    #[test]
    fn test_pn_counter_decrement() {
        let mut counter = PnCounter::new("device-1".to_string());
        counter.decrement(5);
        assert_eq!(counter.value(), -5);
        counter.decrement(3);
        assert_eq!(counter.value(), -8);
    }

    #[test]
    fn test_pn_counter_inc_dec() {
        let mut counter = PnCounter::new("device-1".to_string());
        counter.inc();
        counter.inc();
        counter.inc();
        assert_eq!(counter.value(), 3);
        counter.dec();
        assert_eq!(counter.value(), 2);
    }

    #[test]
    fn test_pn_counter_mixed_operations() {
        let mut counter = PnCounter::new("device-1".to_string());
        counter.increment(10);
        counter.decrement(3);
        counter.increment(5);
        counter.decrement(2);
        assert_eq!(counter.value(), 10); // 10 - 3 + 5 - 2
    }

    #[test]
    fn test_pn_counter_merge() {
        let mut counter1 = PnCounter::new("device-1".to_string());
        let mut counter2 = PnCounter::new("device-2".to_string());

        counter1.increment(10);
        counter1.decrement(3);

        counter2.increment(5);
        counter2.decrement(2);

        counter1.merge(&counter2).ok();

        assert_eq!(counter1.value(), 10); // (10 + 5) - (3 + 2)
    }

    #[test]
    fn test_pn_counter_dominated_by() {
        let mut counter1 = PnCounter::new("device-1".to_string());
        let mut counter2 = PnCounter::new("device-1".to_string());

        counter1.increment(3);
        counter1.decrement(1);

        counter2.increment(5);
        counter2.decrement(2);

        assert!(counter1.dominated_by(&counter2));
        assert!(!counter2.dominated_by(&counter1));
    }

    #[test]
    fn test_pn_counter_total_ops() {
        let mut counter = PnCounter::new("device-1".to_string());
        counter.increment(10);
        counter.decrement(3);
        counter.increment(5);

        assert_eq!(counter.total_increments(), 15);
        assert_eq!(counter.total_decrements(), 3);
        assert_eq!(counter.value(), 12);
    }
}
