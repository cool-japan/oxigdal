//! Envelope encryption for KMS integration.

use crate::encryption::{AtRestEncryptor, EncryptedData, EncryptionAlgorithm};
use crate::error::{Result, SecurityError};
use serde::{Deserialize, Serialize};

/// Key encryption key (KEK) provider trait.
pub trait KekProvider: Send + Sync {
    /// Encrypt a data encryption key (DEK).
    fn encrypt_dek(&self, dek: &[u8]) -> Result<Vec<u8>>;

    /// Decrypt a data encryption key (DEK).
    fn decrypt_dek(&self, encrypted_dek: &[u8]) -> Result<Vec<u8>>;

    /// Get the KEK ID.
    fn kek_id(&self) -> &str;
}

/// Envelope encrypted data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeEncryptedData {
    /// Encrypted data encryption key (DEK).
    pub encrypted_dek: Vec<u8>,
    /// KEK ID used to encrypt the DEK.
    pub kek_id: String,
    /// Encrypted payload.
    pub encrypted_payload: EncryptedData,
    /// DEK algorithm.
    pub dek_algorithm: EncryptionAlgorithm,
}

impl EnvelopeEncryptedData {
    /// Serialize to JSON bytes.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(SecurityError::from)
    }

    /// Deserialize from JSON bytes.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(SecurityError::from)
    }
}

/// Envelope encryptor using envelope encryption pattern.
pub struct EnvelopeEncryptor {
    kek_provider: Box<dyn KekProvider>,
    dek_algorithm: EncryptionAlgorithm,
}

impl EnvelopeEncryptor {
    /// Create a new envelope encryptor.
    pub fn new(kek_provider: Box<dyn KekProvider>, dek_algorithm: EncryptionAlgorithm) -> Self {
        Self {
            kek_provider,
            dek_algorithm,
        }
    }

    /// Encrypt data using envelope encryption.
    pub fn encrypt(&self, plaintext: &[u8], aad: Option<&[u8]>) -> Result<EnvelopeEncryptedData> {
        // Generate random DEK
        let dek = AtRestEncryptor::generate_key(self.dek_algorithm);

        // Encrypt plaintext with DEK
        let dek_id = uuid::Uuid::new_v4().to_string();
        let encryptor = AtRestEncryptor::new(self.dek_algorithm, dek.clone(), dek_id)?;
        let encrypted_payload = encryptor.encrypt(plaintext, aad)?;

        // Encrypt DEK with KEK
        let encrypted_dek = self.kek_provider.encrypt_dek(&dek)?;

        Ok(EnvelopeEncryptedData {
            encrypted_dek,
            kek_id: self.kek_provider.kek_id().to_string(),
            encrypted_payload,
            dek_algorithm: self.dek_algorithm,
        })
    }

    /// Decrypt data using envelope encryption.
    pub fn decrypt(&self, envelope: &EnvelopeEncryptedData) -> Result<Vec<u8>> {
        // Verify KEK ID matches
        if envelope.kek_id != self.kek_provider.kek_id() {
            return Err(SecurityError::decryption(format!(
                "KEK ID mismatch: expected {}, got {}",
                self.kek_provider.kek_id(),
                envelope.kek_id
            )));
        }

        // Decrypt DEK with KEK
        let dek = self.kek_provider.decrypt_dek(&envelope.encrypted_dek)?;

        // Decrypt payload with DEK
        let dek_id = envelope.encrypted_payload.metadata.key_id.clone();
        let encryptor = AtRestEncryptor::new(envelope.dek_algorithm, dek, dek_id)?;
        encryptor.decrypt(&envelope.encrypted_payload)
    }

    /// Get the KEK ID.
    pub fn kek_id(&self) -> &str {
        self.kek_provider.kek_id()
    }

    /// Get the DEK algorithm.
    pub fn dek_algorithm(&self) -> EncryptionAlgorithm {
        self.dek_algorithm
    }
}

/// In-memory KEK provider for testing and development.
pub struct InMemoryKekProvider {
    kek_id: String,
    encryptor: AtRestEncryptor,
}

impl InMemoryKekProvider {
    /// Create a new in-memory KEK provider.
    pub fn new(kek_id: String) -> Result<Self> {
        let kek = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encryptor = AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, kek, kek_id.clone())?;

        Ok(Self { kek_id, encryptor })
    }

    /// Create with a specific KEK.
    pub fn with_kek(kek_id: String, kek: Vec<u8>) -> Result<Self> {
        let encryptor = AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, kek, kek_id.clone())?;

        Ok(Self { kek_id, encryptor })
    }
}

impl KekProvider for InMemoryKekProvider {
    fn encrypt_dek(&self, dek: &[u8]) -> Result<Vec<u8>> {
        let encrypted = self.encryptor.encrypt(dek, None)?;
        encrypted.to_json_bytes()
    }

    fn decrypt_dek(&self, encrypted_dek: &[u8]) -> Result<Vec<u8>> {
        let encrypted = EncryptedData::from_json_bytes(encrypted_dek)?;
        self.encryptor.decrypt(&encrypted)
    }

    fn kek_id(&self) -> &str {
        &self.kek_id
    }
}

/// Multi-region KEK provider for disaster recovery.
pub struct MultiRegionKekProvider {
    kek_id: String,
    primary: Box<dyn KekProvider>,
    secondary: Option<Box<dyn KekProvider>>,
}

impl MultiRegionKekProvider {
    /// Create a new multi-region KEK provider.
    pub fn new(
        kek_id: String,
        primary: Box<dyn KekProvider>,
        secondary: Option<Box<dyn KekProvider>>,
    ) -> Self {
        Self {
            kek_id,
            primary,
            secondary,
        }
    }

    /// Encrypt with both primary and secondary (for migration).
    pub fn encrypt_with_both(&self, dek: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        let primary_encrypted = self.primary.encrypt_dek(dek)?;
        let secondary_encrypted = if let Some(ref secondary) = self.secondary {
            Some(secondary.encrypt_dek(dek)?)
        } else {
            None
        };

        Ok((primary_encrypted, secondary_encrypted))
    }
}

impl KekProvider for MultiRegionKekProvider {
    fn encrypt_dek(&self, dek: &[u8]) -> Result<Vec<u8>> {
        self.primary.encrypt_dek(dek)
    }

    fn decrypt_dek(&self, encrypted_dek: &[u8]) -> Result<Vec<u8>> {
        // Try primary first
        match self.primary.decrypt_dek(encrypted_dek) {
            Ok(dek) => Ok(dek),
            Err(e) => {
                // Fallback to secondary if available
                if let Some(ref secondary) = self.secondary {
                    secondary.decrypt_dek(encrypted_dek)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn kek_id(&self) -> &str {
        &self.kek_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_kek_provider() {
        let provider =
            InMemoryKekProvider::new("test-kek".to_string()).expect("Failed to create provider");

        let dek = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encrypted_dek = provider.encrypt_dek(&dek).expect("Encryption failed");

        assert_ne!(encrypted_dek, dek);

        let decrypted_dek = provider
            .decrypt_dek(&encrypted_dek)
            .expect("Decryption failed");
        assert_eq!(decrypted_dek, dek);
    }

    #[test]
    fn test_envelope_encryption() {
        let kek_provider =
            InMemoryKekProvider::new("test-kek".to_string()).expect("Failed to create provider");
        let encryptor =
            EnvelopeEncryptor::new(Box::new(kek_provider), EncryptionAlgorithm::Aes256Gcm);

        let plaintext = b"sensitive data";
        let envelope = encryptor
            .encrypt(plaintext, None)
            .expect("Encryption failed");

        assert_ne!(envelope.encrypted_payload.ciphertext, plaintext);
        assert!(!envelope.encrypted_dek.is_empty());

        let decrypted = encryptor.decrypt(&envelope).expect("Decryption failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_envelope_with_aad() {
        let kek_provider =
            InMemoryKekProvider::new("test-kek".to_string()).expect("Failed to create provider");
        let encryptor =
            EnvelopeEncryptor::new(Box::new(kek_provider), EncryptionAlgorithm::Aes256Gcm);

        let plaintext = b"sensitive data";
        let aad = b"additional data";
        let envelope = encryptor
            .encrypt(plaintext, Some(aad))
            .expect("Encryption failed");

        let decrypted = encryptor.decrypt(&envelope).expect("Decryption failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_multi_region_kek_provider() {
        let primary =
            InMemoryKekProvider::new("primary-kek".to_string()).expect("Failed to create primary");
        let secondary = InMemoryKekProvider::new("secondary-kek".to_string())
            .expect("Failed to create secondary");

        let multi = MultiRegionKekProvider::new(
            "multi-kek".to_string(),
            Box::new(primary),
            Some(Box::new(secondary)),
        );

        let dek = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encrypted_dek = multi.encrypt_dek(&dek).expect("Encryption failed");

        let decrypted_dek = multi
            .decrypt_dek(&encrypted_dek)
            .expect("Decryption failed");
        assert_eq!(decrypted_dek, dek);
    }

    #[test]
    fn test_envelope_serialization() {
        let kek_provider =
            InMemoryKekProvider::new("test-kek".to_string()).expect("Failed to create provider");
        let encryptor =
            EnvelopeEncryptor::new(Box::new(kek_provider), EncryptionAlgorithm::Aes256Gcm);

        let plaintext = b"sensitive data";
        let envelope = encryptor
            .encrypt(plaintext, None)
            .expect("Encryption failed");

        let json = envelope.to_json_bytes().expect("Serialization failed");
        let deserialized =
            EnvelopeEncryptedData::from_json_bytes(&json).expect("Deserialization failed");

        let decrypted = encryptor.decrypt(&deserialized).expect("Decryption failed");
        assert_eq!(decrypted, plaintext);
    }
}
