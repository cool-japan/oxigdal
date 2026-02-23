//! Encryption infrastructure for data at rest and in transit.

pub mod at_rest;
pub mod envelope;
pub mod in_transit;
pub mod key_management;

// Re-export commonly used types
pub use at_rest::{AtRestEncryptor, FieldEncryptor};
pub use envelope::EnvelopeEncryptor;
pub use in_transit::TlsConfigBuilder;
pub use key_management::KeyManager;

use crate::error::{Result, SecurityError};
use serde::{Deserialize, Serialize};

/// Encryption algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM (recommended for most use cases).
    #[default]
    Aes256Gcm,
    /// ChaCha20-Poly1305 (faster on systems without AES hardware).
    ChaCha20Poly1305,
}

/// Encryption metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionMetadata {
    /// Algorithm used for encryption.
    pub algorithm: EncryptionAlgorithm,
    /// Key ID (for key rotation).
    pub key_id: String,
    /// Initialization vector/nonce.
    pub iv: Vec<u8>,
    /// Additional authenticated data.
    pub aad: Option<Vec<u8>>,
    /// Timestamp when encrypted.
    pub encrypted_at: chrono::DateTime<chrono::Utc>,
}

impl EncryptionMetadata {
    /// Create new encryption metadata.
    pub fn new(
        algorithm: EncryptionAlgorithm,
        key_id: String,
        iv: Vec<u8>,
        aad: Option<Vec<u8>>,
    ) -> Self {
        Self {
            algorithm,
            key_id,
            iv,
            aad,
            encrypted_at: chrono::Utc::now(),
        }
    }
}

/// Encrypted data with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    /// Encrypted ciphertext.
    pub ciphertext: Vec<u8>,
    /// Encryption metadata.
    pub metadata: EncryptionMetadata,
}

impl EncryptedData {
    /// Create new encrypted data.
    pub fn new(ciphertext: Vec<u8>, metadata: EncryptionMetadata) -> Self {
        Self {
            ciphertext,
            metadata,
        }
    }

    /// Serialize to JSON bytes.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(SecurityError::from)
    }

    /// Deserialize from JSON bytes.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(SecurityError::from)
    }
}

/// Key derivation function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum KeyDerivationFunction {
    /// PBKDF2 with SHA-256.
    Pbkdf2Sha256,
    /// Argon2id (recommended).
    #[default]
    Argon2id,
}

/// Key derivation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationParams {
    /// Key derivation function.
    pub kdf: KeyDerivationFunction,
    /// Salt.
    pub salt: Vec<u8>,
    /// Iterations (for PBKDF2).
    pub iterations: Option<u32>,
    /// Memory cost (for Argon2).
    pub memory_cost: Option<u32>,
    /// Time cost (for Argon2).
    pub time_cost: Option<u32>,
    /// Parallelism (for Argon2).
    pub parallelism: Option<u32>,
}

impl KeyDerivationParams {
    /// Create PBKDF2 parameters with recommended settings.
    pub fn pbkdf2_recommended(salt: Vec<u8>) -> Self {
        Self {
            kdf: KeyDerivationFunction::Pbkdf2Sha256,
            salt,
            iterations: Some(600000), // OWASP recommendation
            memory_cost: None,
            time_cost: None,
            parallelism: None,
        }
    }

    /// Create Argon2id parameters with recommended settings.
    pub fn argon2_recommended(salt: Vec<u8>) -> Self {
        Self {
            kdf: KeyDerivationFunction::Argon2id,
            salt,
            iterations: None,
            memory_cost: Some(19456), // 19 MiB
            time_cost: Some(2),
            parallelism: Some(1),
        }
    }
}

/// Derive a key from a password.
pub fn derive_key(
    password: &[u8],
    params: &KeyDerivationParams,
    key_length: usize,
) -> Result<Vec<u8>> {
    match params.kdf {
        KeyDerivationFunction::Pbkdf2Sha256 => {
            let iterations = params
                .iterations
                .ok_or_else(|| SecurityError::key_derivation("iterations required for PBKDF2"))?;

            use ring::pbkdf2;
            let mut key = vec![0u8; key_length];
            pbkdf2::derive(
                pbkdf2::PBKDF2_HMAC_SHA256,
                std::num::NonZeroU32::new(iterations)
                    .ok_or_else(|| SecurityError::key_derivation("invalid iterations"))?,
                &params.salt,
                password,
                &mut key,
            );
            Ok(key)
        }
        KeyDerivationFunction::Argon2id => {
            use argon2::{Algorithm, Argon2, Params, Version};

            let memory_cost = params
                .memory_cost
                .ok_or_else(|| SecurityError::key_derivation("memory_cost required for Argon2"))?;
            let time_cost = params
                .time_cost
                .ok_or_else(|| SecurityError::key_derivation("time_cost required for Argon2"))?;
            let parallelism = params
                .parallelism
                .ok_or_else(|| SecurityError::key_derivation("parallelism required for Argon2"))?;

            let argon2_params = Params::new(memory_cost, time_cost, parallelism, Some(key_length))
                .map_err(|e| {
                    SecurityError::key_derivation(format!("invalid Argon2 params: {}", e))
                })?;

            let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);

            let mut key = vec![0u8; key_length];
            argon2
                .hash_password_into(password, &params.salt, &mut key)
                .map_err(|e| SecurityError::key_derivation(format!("Argon2 error: {}", e)))?;

            Ok(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivation_pbkdf2() {
        let password = b"test_password";
        let salt = b"test_salt_12345678";
        let params = KeyDerivationParams::pbkdf2_recommended(salt.to_vec());

        let key = derive_key(password, &params, 32).expect("key derivation failed");
        assert_eq!(key.len(), 32);

        // Same password and salt should produce same key
        let key2 = derive_key(password, &params, 32).expect("key derivation failed");
        assert_eq!(key, key2);

        // Different password should produce different key
        let key3 = derive_key(b"different", &params, 32).expect("key derivation failed");
        assert_ne!(key, key3);
    }

    #[test]
    fn test_key_derivation_argon2() {
        let password = b"test_password";
        let salt = b"test_salt_12345678";
        let params = KeyDerivationParams::argon2_recommended(salt.to_vec());

        let key = derive_key(password, &params, 32).expect("key derivation failed");
        assert_eq!(key.len(), 32);

        // Same password and salt should produce same key
        let key2 = derive_key(password, &params, 32).expect("key derivation failed");
        assert_eq!(key, key2);
    }

    #[test]
    fn test_encryption_metadata_serialization() {
        let metadata = EncryptionMetadata::new(
            EncryptionAlgorithm::Aes256Gcm,
            "key-001".to_string(),
            vec![1, 2, 3, 4, 5],
            Some(vec![6, 7, 8]),
        );

        let json = serde_json::to_string(&metadata).expect("serialization failed");
        let deserialized: EncryptionMetadata =
            serde_json::from_str(&json).expect("deserialization failed");

        assert_eq!(deserialized.algorithm, metadata.algorithm);
        assert_eq!(deserialized.key_id, metadata.key_id);
        assert_eq!(deserialized.iv, metadata.iv);
        assert_eq!(deserialized.aad, metadata.aad);
    }
}
