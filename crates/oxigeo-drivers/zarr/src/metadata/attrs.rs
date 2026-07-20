//! Attribute handling for Zarr arrays and groups
//!
//! This module provides types for working with user-defined attributes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Attributes - user-defined metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Attributes(pub HashMap<String, serde_json::Value>);

impl Attributes {
    /// Creates new empty attributes
    #[must_use]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Creates from a hashmap
    #[must_use]
    pub fn from_map(map: HashMap<String, serde_json::Value>) -> Self {
        Self(map)
    }

    /// Gets an attribute value
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.get(key)
    }

    /// Sets an attribute value
    pub fn set(&mut self, key: String, value: serde_json::Value) {
        self.0.insert(key, value);
    }

    /// Checks if attributes is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for Attributes {
    fn default() -> Self {
        Self::new()
    }
}
