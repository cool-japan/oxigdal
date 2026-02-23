//! Key management and rotation.

use crate::encryption::{AtRestEncryptor, EncryptionAlgorithm};
use crate::error::{Result, SecurityError};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Key metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMetadata {
    /// Key ID.
    pub key_id: String,
    /// Algorithm.
    pub algorithm: EncryptionAlgorithm,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Expires at.
    pub expires_at: Option<DateTime<Utc>>,
    /// Rotation period in days.
    pub rotation_period_days: Option<u32>,
    /// Whether the key is active.
    pub active: bool,
    /// Key version.
    pub version: u32,
}

impl KeyMetadata {
    /// Create new key metadata.
    pub fn new(
        key_id: String,
        algorithm: EncryptionAlgorithm,
        rotation_period_days: Option<u32>,
    ) -> Self {
        let expires_at =
            rotation_period_days.map(|days| Utc::now() + chrono::Duration::days(days as i64));

        Self {
            key_id,
            algorithm,
            created_at: Utc::now(),
            expires_at,
            rotation_period_days,
            active: true,
            version: 1,
        }
    }

    /// Check if the key is expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| Utc::now() > exp)
    }

    /// Check if the key needs rotation (within 7 days of expiration).
    pub fn needs_rotation(&self) -> bool {
        self.expires_at
            .is_some_and(|exp| Utc::now() + chrono::Duration::days(7) > exp)
    }
}

/// Key manager for storing and rotating encryption keys.
pub struct KeyManager {
    keys: Arc<DashMap<String, (Vec<u8>, KeyMetadata)>>,
    current_key_id: Arc<parking_lot::RwLock<Option<String>>>,
}

impl KeyManager {
    /// Create a new key manager.
    pub fn new() -> Self {
        Self {
            keys: Arc::new(DashMap::new()),
            current_key_id: Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    /// Generate a new key.
    pub fn generate_key(
        &self,
        algorithm: EncryptionAlgorithm,
        rotation_period_days: Option<u32>,
    ) -> Result<String> {
        let key_id = Uuid::new_v4().to_string();
        let key = AtRestEncryptor::generate_key(algorithm);
        let metadata = KeyMetadata::new(key_id.clone(), algorithm, rotation_period_days);

        self.keys.insert(key_id.clone(), (key, metadata));

        // Set as current key if no current key exists
        {
            let mut current = self.current_key_id.write();
            if current.is_none() {
                *current = Some(key_id.clone());
            }
        }

        Ok(key_id)
    }

    /// Add an existing key.
    pub fn add_key(
        &self,
        key_id: String,
        key: Vec<u8>,
        algorithm: EncryptionAlgorithm,
        rotation_period_days: Option<u32>,
    ) -> Result<()> {
        let metadata = KeyMetadata::new(key_id.clone(), algorithm, rotation_period_days);
        self.keys.insert(key_id, (key, metadata));
        Ok(())
    }

    /// Get a key by ID.
    pub fn get_key(&self, key_id: &str) -> Result<(Vec<u8>, KeyMetadata)> {
        self.keys
            .get(key_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| SecurityError::key_management(format!("Key not found: {}", key_id)))
    }

    /// Get the current active key.
    pub fn get_current_key(&self) -> Result<(String, Vec<u8>, KeyMetadata)> {
        let current_id = self
            .current_key_id
            .read()
            .clone()
            .ok_or_else(|| SecurityError::key_management("No current key set"))?;

        let (key, metadata) = self.get_key(&current_id)?;
        Ok((current_id, key, metadata))
    }

    /// Set the current active key.
    pub fn set_current_key(&self, key_id: String) -> Result<()> {
        // Verify key exists
        if !self.keys.contains_key(&key_id) {
            return Err(SecurityError::key_management(format!(
                "Key not found: {}",
                key_id
            )));
        }

        let mut current = self.current_key_id.write();
        *current = Some(key_id);
        Ok(())
    }

    /// Rotate the current key.
    pub fn rotate_key(&self) -> Result<String> {
        let (current_id, _, metadata) = self.get_current_key()?;

        // Generate new key with same parameters
        let new_key_id = self.generate_key(metadata.algorithm, metadata.rotation_period_days)?;

        // Deactivate old key
        if let Some(mut entry) = self.keys.get_mut(&current_id) {
            entry.value_mut().1.active = false;
        }

        // Set new key as current
        self.set_current_key(new_key_id.clone())?;

        Ok(new_key_id)
    }

    /// List all keys.
    pub fn list_keys(&self) -> Vec<(String, KeyMetadata)> {
        self.keys
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().1.clone()))
            .collect()
    }

    /// List expired keys.
    pub fn list_expired_keys(&self) -> Vec<(String, KeyMetadata)> {
        self.keys
            .iter()
            .filter(|entry| entry.value().1.is_expired())
            .map(|entry| (entry.key().clone(), entry.value().1.clone()))
            .collect()
    }

    /// List keys that need rotation.
    pub fn list_keys_needing_rotation(&self) -> Vec<(String, KeyMetadata)> {
        self.keys
            .iter()
            .filter(|entry| entry.value().1.needs_rotation())
            .map(|entry| (entry.key().clone(), entry.value().1.clone()))
            .collect()
    }

    /// Delete a key.
    pub fn delete_key(&self, key_id: &str) -> Result<()> {
        // Prevent deleting current key
        {
            let current = self.current_key_id.read();
            if current.as_ref().is_some_and(|id| id == key_id) {
                return Err(SecurityError::key_management("Cannot delete current key"));
            }
        }

        self.keys
            .remove(key_id)
            .ok_or_else(|| SecurityError::key_management(format!("Key not found: {}", key_id)))?;

        Ok(())
    }

    /// Create an encryptor for a specific key.
    pub fn create_encryptor(&self, key_id: &str) -> Result<AtRestEncryptor> {
        let (key, metadata) = self.get_key(key_id)?;

        if metadata.is_expired() {
            return Err(SecurityError::key_management(format!(
                "Key expired: {}",
                key_id
            )));
        }

        AtRestEncryptor::new(metadata.algorithm, key, key_id.to_string())
    }

    /// Create an encryptor for the current key.
    pub fn create_current_encryptor(&self) -> Result<AtRestEncryptor> {
        let (key_id, key, metadata) = self.get_current_key()?;

        if metadata.is_expired() {
            return Err(SecurityError::key_management("Current key expired"));
        }

        AtRestEncryptor::new(metadata.algorithm, key, key_id)
    }

    /// Get the number of keys.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Clear all keys.
    pub fn clear(&self) {
        self.keys.clear();
        *self.current_key_id.write() = None;
    }
}

impl Default for KeyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let manager = KeyManager::new();
        let key_id = manager
            .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
            .expect("Failed to generate key");

        assert!(!key_id.is_empty());

        let (key, metadata) = manager.get_key(&key_id).expect("Failed to get key");
        assert_eq!(key.len(), 32);
        assert_eq!(metadata.algorithm, EncryptionAlgorithm::Aes256Gcm);
        assert!(metadata.active);
        assert!(!metadata.is_expired());
    }

    #[test]
    fn test_current_key() {
        let manager = KeyManager::new();
        let key_id = manager
            .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
            .expect("Failed to generate key");

        let (current_id, _, _) = manager
            .get_current_key()
            .expect("Failed to get current key");
        assert_eq!(current_id, key_id);
    }

    #[test]
    fn test_key_rotation() {
        let manager = KeyManager::new();
        let old_key_id = manager
            .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
            .expect("Failed to generate key");

        let new_key_id = manager.rotate_key().expect("Failed to rotate key");
        assert_ne!(old_key_id, new_key_id);

        let (current_id, _, _) = manager
            .get_current_key()
            .expect("Failed to get current key");
        assert_eq!(current_id, new_key_id);

        let (_, old_metadata) = manager.get_key(&old_key_id).expect("Failed to get old key");
        assert!(!old_metadata.active);
    }

    #[test]
    fn test_list_keys() {
        let manager = KeyManager::new();
        manager
            .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
            .expect("Failed to generate key");
        manager
            .generate_key(EncryptionAlgorithm::ChaCha20Poly1305, Some(365))
            .expect("Failed to generate key");

        let keys = manager.list_keys();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_delete_key() {
        let manager = KeyManager::new();
        let key_id1 = manager
            .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
            .expect("Failed to generate key");
        let key_id2 = manager
            .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
            .expect("Failed to generate key");

        // Cannot delete current key
        assert!(manager.delete_key(&key_id1).is_err());

        // Can delete non-current key
        assert!(manager.delete_key(&key_id2).is_ok());
        assert_eq!(manager.key_count(), 1);
    }

    #[test]
    fn test_create_encryptor() {
        let manager = KeyManager::new();
        let key_id = manager
            .generate_key(EncryptionAlgorithm::Aes256Gcm, Some(365))
            .expect("Failed to generate key");

        let encryptor = manager
            .create_encryptor(&key_id)
            .expect("Failed to create encryptor");

        assert_eq!(encryptor.algorithm(), EncryptionAlgorithm::Aes256Gcm);
        assert_eq!(encryptor.key_id(), key_id);
    }
}
