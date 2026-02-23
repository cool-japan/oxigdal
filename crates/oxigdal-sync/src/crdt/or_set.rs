//! Observed-Remove Set CRDT
//!
//! A set that supports add and remove operations with proper
//! causal consistency.

use crate::crdt::{Crdt, DeviceAware};
use crate::{DeviceId, SyncResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use uuid::Uuid;

/// Unique identifier for set elements
pub type ElementId = Uuid;

/// Element metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct ElementMetadata {
    /// Unique ID for this add operation
    id: ElementId,
    /// Device that added the element
    device_id: DeviceId,
}

/// Observed-Remove Set
///
/// A CRDT set that supports add and remove operations.
/// Each add operation gets a unique ID, and removes only affect
/// observed add operations. This ensures that concurrent add/remove
/// operations are handled correctly.
///
/// # Example
///
/// ```rust
/// use oxigdal_sync::crdt::{OrSet, Crdt};
///
/// let mut set1 = OrSet::new("device-1".to_string());
/// set1.insert("apple".to_string());
/// set1.insert("banana".to_string());
///
/// let mut set2 = OrSet::new("device-2".to_string());
/// set2.insert("cherry".to_string());
///
/// set1.merge(&set2).ok();
/// assert_eq!(set1.len(), 3);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize"))]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct OrSet<T>
where
    T: Clone + Eq + Hash,
{
    /// Map from element to its add operation IDs
    elements: HashMap<T, HashSet<ElementMetadata>>,
    /// Tombstones (removed element IDs)
    tombstones: HashSet<ElementId>,
    /// Device ID
    device_id: DeviceId,
}

impl<T> OrSet<T>
where
    T: Clone + Eq + Hash,
{
    /// Creates a new OR-Set
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device ID
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            elements: HashMap::new(),
            tombstones: HashSet::new(),
            device_id,
        }
    }

    /// Inserts an element into the set
    ///
    /// # Arguments
    ///
    /// * `element` - The element to insert
    ///
    /// # Returns
    ///
    /// True if the element was newly inserted, false if already present
    pub fn insert(&mut self, element: T) -> bool {
        let metadata = ElementMetadata {
            id: Uuid::new_v4(),
            device_id: self.device_id.clone(),
        };

        let entry = self.elements.entry(element).or_default();
        entry.insert(metadata)
    }

    /// Removes an element from the set
    ///
    /// # Arguments
    ///
    /// * `element` - The element to remove
    ///
    /// # Returns
    ///
    /// True if the element was present and removed
    pub fn remove(&mut self, element: &T) -> bool {
        if let Some(metadata_set) = self.elements.get(element) {
            // Add all observed IDs to tombstones
            for metadata in metadata_set {
                self.tombstones.insert(metadata.id);
            }

            // Remove the element
            self.elements.remove(element);
            true
        } else {
            false
        }
    }

    /// Checks if the set contains an element
    ///
    /// # Arguments
    ///
    /// * `element` - The element to check
    ///
    /// # Returns
    ///
    /// True if the element is in the set
    pub fn contains(&self, element: &T) -> bool {
        if let Some(metadata_set) = self.elements.get(element) {
            // Element is present if it has at least one non-tombstoned ID
            metadata_set
                .iter()
                .any(|m| !self.tombstones.contains(&m.id))
        } else {
            false
        }
    }

    /// Gets the number of elements in the set
    pub fn len(&self) -> usize {
        self.elements
            .iter()
            .filter(|(_, metadata_set)| {
                metadata_set
                    .iter()
                    .any(|m| !self.tombstones.contains(&m.id))
            })
            .count()
    }

    /// Checks if the set is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Gets an iterator over the elements
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements
            .iter()
            .filter(|(_, metadata_set)| {
                metadata_set
                    .iter()
                    .any(|m| !self.tombstones.contains(&m.id))
            })
            .map(|(element, _)| element)
    }

    /// Converts the set to a HashSet
    pub fn to_hashset(&self) -> HashSet<T> {
        self.iter().cloned().collect()
    }

    /// Clears all elements from the set
    pub fn clear(&mut self) {
        for metadata_set in self.elements.values() {
            for metadata in metadata_set {
                self.tombstones.insert(metadata.id);
            }
        }
        self.elements.clear();
    }
}

impl<T> Crdt for OrSet<T>
where
    T: Clone + Eq + Hash + Serialize + for<'de> serde::Deserialize<'de>,
{
    fn merge(&mut self, other: &Self) -> SyncResult<()> {
        // Merge elements
        for (element, metadata_set) in &other.elements {
            let entry = self.elements.entry(element.clone()).or_default();
            for metadata in metadata_set {
                entry.insert(metadata.clone());
            }
        }

        // Merge tombstones
        for tombstone in &other.tombstones {
            self.tombstones.insert(*tombstone);
        }

        // Clean up elements that are fully tombstoned
        self.elements.retain(|_, metadata_set| {
            metadata_set
                .iter()
                .any(|m| !self.tombstones.contains(&m.id))
        });

        Ok(())
    }

    fn dominated_by(&self, other: &Self) -> bool {
        // Check if all our elements are in the other set
        for (element, metadata_set) in &self.elements {
            if let Some(other_metadata_set) = other.elements.get(element) {
                for metadata in metadata_set {
                    if !self.tombstones.contains(&metadata.id) {
                        // This element is live in our set
                        if !other_metadata_set.contains(metadata)
                            || other.tombstones.contains(&metadata.id)
                        {
                            // Not in other set or tombstoned in other
                            return false;
                        }
                    }
                }
            } else {
                // Element not in other set at all
                if metadata_set
                    .iter()
                    .any(|m| !self.tombstones.contains(&m.id))
                {
                    return false;
                }
            }
        }

        // Check if all our tombstones are in the other set
        for tombstone in &self.tombstones {
            if !other.tombstones.contains(tombstone) {
                return false;
            }
        }

        true
    }
}

impl<T> DeviceAware for OrSet<T>
where
    T: Clone + Eq + Hash + Serialize + for<'de> serde::Deserialize<'de>,
{
    fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    fn set_device_id(&mut self, device_id: DeviceId) {
        self.device_id = device_id;
    }
}

impl<T> PartialEq for OrSet<T>
where
    T: Clone + Eq + Hash,
{
    fn eq(&self, other: &Self) -> bool {
        self.to_hashset() == other.to_hashset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_or_set_creation() {
        let set: OrSet<String> = OrSet::new("device-1".to_string());
        assert_eq!(set.len(), 0);
        assert!(set.is_empty());
    }

    #[test]
    fn test_or_set_insert() {
        let mut set = OrSet::new("device-1".to_string());
        assert!(set.insert("apple".to_string()));
        assert!(set.contains(&"apple".to_string()));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_or_set_remove() {
        let mut set = OrSet::new("device-1".to_string());
        set.insert("apple".to_string());
        assert!(set.remove(&"apple".to_string()));
        assert!(!set.contains(&"apple".to_string()));
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_or_set_multiple_elements() {
        let mut set = OrSet::new("device-1".to_string());
        set.insert("apple".to_string());
        set.insert("banana".to_string());
        set.insert("cherry".to_string());

        assert_eq!(set.len(), 3);
        assert!(set.contains(&"apple".to_string()));
        assert!(set.contains(&"banana".to_string()));
        assert!(set.contains(&"cherry".to_string()));
    }

    #[test]
    fn test_or_set_merge() {
        let mut set1 = OrSet::new("device-1".to_string());
        let mut set2 = OrSet::new("device-2".to_string());

        set1.insert("apple".to_string());
        set1.insert("banana".to_string());

        set2.insert("cherry".to_string());
        set2.insert("date".to_string());

        set1.merge(&set2).ok();

        assert_eq!(set1.len(), 4);
        assert!(set1.contains(&"apple".to_string()));
        assert!(set1.contains(&"banana".to_string()));
        assert!(set1.contains(&"cherry".to_string()));
        assert!(set1.contains(&"date".to_string()));
    }

    #[test]
    fn test_or_set_concurrent_add_remove() {
        let mut set1 = OrSet::new("device-1".to_string());
        let mut set2 = OrSet::new("device-2".to_string());

        // Both add the same element
        set1.insert("apple".to_string());
        set2.insert("apple".to_string());

        // set1 removes it
        set1.remove(&"apple".to_string());

        // Merge - concurrent add should win
        set1.merge(&set2).ok();

        // The element from device-2 should still be present
        assert!(set1.contains(&"apple".to_string()));
    }

    #[test]
    fn test_or_set_clear() {
        let mut set = OrSet::new("device-1".to_string());
        set.insert("apple".to_string());
        set.insert("banana".to_string());

        set.clear();

        assert_eq!(set.len(), 0);
        assert!(set.is_empty());
    }

    #[test]
    fn test_or_set_iter() {
        let mut set = OrSet::new("device-1".to_string());
        set.insert("apple".to_string());
        set.insert("banana".to_string());
        set.insert("cherry".to_string());

        let elements: HashSet<_> = set.iter().cloned().collect();
        assert_eq!(elements.len(), 3);
        assert!(elements.contains("apple"));
        assert!(elements.contains("banana"));
        assert!(elements.contains("cherry"));
    }
}
