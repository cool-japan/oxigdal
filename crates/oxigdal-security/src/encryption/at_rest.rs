//! Data encryption at rest.

use crate::encryption::{EncryptedData, EncryptionAlgorithm, EncryptionMetadata};
use crate::error::{Result, SecurityError};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chacha20poly1305::ChaCha20Poly1305;
use rand::RngCore;

/// Encryptor for data at rest.
pub struct AtRestEncryptor {
    algorithm: EncryptionAlgorithm,
    key: Vec<u8>,
    key_id: String,
}

impl AtRestEncryptor {
    /// Create a new encryptor with the given algorithm and key.
    pub fn new(algorithm: EncryptionAlgorithm, key: Vec<u8>, key_id: String) -> Result<Self> {
        // Validate key length
        let required_length = match algorithm {
            EncryptionAlgorithm::Aes256Gcm => 32,
            EncryptionAlgorithm::ChaCha20Poly1305 => 32,
        };

        if key.len() != required_length {
            return Err(SecurityError::encryption(format!(
                "Invalid key length: expected {}, got {}",
                required_length,
                key.len()
            )));
        }

        Ok(Self {
            algorithm,
            key,
            key_id,
        })
    }

    /// Generate a random key for the given algorithm.
    pub fn generate_key(algorithm: EncryptionAlgorithm) -> Vec<u8> {
        let mut key = vec![
            0u8;
            match algorithm {
                EncryptionAlgorithm::Aes256Gcm => 32,
                EncryptionAlgorithm::ChaCha20Poly1305 => 32,
            }
        ];
        OsRng.fill_bytes(&mut key);
        key
    }

    /// Encrypt data.
    pub fn encrypt(&self, plaintext: &[u8], aad: Option<&[u8]>) -> Result<EncryptedData> {
        match self.algorithm {
            EncryptionAlgorithm::Aes256Gcm => self.encrypt_aes_gcm(plaintext, aad),
            EncryptionAlgorithm::ChaCha20Poly1305 => self.encrypt_chacha(plaintext, aad),
        }
    }

    /// Decrypt data.
    pub fn decrypt(&self, encrypted: &EncryptedData) -> Result<Vec<u8>> {
        // Verify algorithm matches
        if encrypted.metadata.algorithm != self.algorithm {
            return Err(SecurityError::decryption(format!(
                "Algorithm mismatch: expected {:?}, got {:?}",
                self.algorithm, encrypted.metadata.algorithm
            )));
        }

        // Verify key ID matches
        if encrypted.metadata.key_id != self.key_id {
            return Err(SecurityError::decryption(format!(
                "Key ID mismatch: expected {}, got {}",
                self.key_id, encrypted.metadata.key_id
            )));
        }

        match self.algorithm {
            EncryptionAlgorithm::Aes256Gcm => self.decrypt_aes_gcm(encrypted),
            EncryptionAlgorithm::ChaCha20Poly1305 => self.decrypt_chacha(encrypted),
        }
    }

    fn encrypt_aes_gcm(&self, plaintext: &[u8], aad: Option<&[u8]>) -> Result<EncryptedData> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| SecurityError::encryption(format!("Failed to create cipher: {}", e)))?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt with optional AAD
        let ciphertext = if let Some(aad_data) = aad {
            cipher
                .encrypt(
                    nonce,
                    aes_gcm::aead::Payload {
                        msg: plaintext,
                        aad: aad_data,
                    },
                )
                .map_err(|e| SecurityError::encryption(format!("Encryption failed: {}", e)))?
        } else {
            cipher
                .encrypt(nonce, plaintext)
                .map_err(|e| SecurityError::encryption(format!("Encryption failed: {}", e)))?
        };

        let metadata = EncryptionMetadata::new(
            self.algorithm,
            self.key_id.clone(),
            nonce_bytes.to_vec(),
            aad.map(|a| a.to_vec()),
        );

        Ok(EncryptedData::new(ciphertext, metadata))
    }

    fn decrypt_aes_gcm(&self, encrypted: &EncryptedData) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| SecurityError::decryption(format!("Failed to create cipher: {}", e)))?;

        if encrypted.metadata.iv.len() != 12 {
            return Err(SecurityError::decryption(format!(
                "Invalid nonce length: expected 12, got {}",
                encrypted.metadata.iv.len()
            )));
        }

        let nonce = Nonce::from_slice(&encrypted.metadata.iv);

        let plaintext = if let Some(ref aad) = encrypted.metadata.aad {
            cipher
                .decrypt(
                    nonce,
                    aes_gcm::aead::Payload {
                        msg: &encrypted.ciphertext,
                        aad,
                    },
                )
                .map_err(|e| SecurityError::decryption(format!("Decryption failed: {}", e)))?
        } else {
            cipher
                .decrypt(nonce, encrypted.ciphertext.as_ref())
                .map_err(|e| SecurityError::decryption(format!("Decryption failed: {}", e)))?
        };

        Ok(plaintext)
    }

    fn encrypt_chacha(&self, plaintext: &[u8], aad: Option<&[u8]>) -> Result<EncryptedData> {
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key)
            .map_err(|e| SecurityError::encryption(format!("Failed to create cipher: {}", e)))?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

        // Encrypt with optional AAD
        let ciphertext = if let Some(aad_data) = aad {
            cipher
                .encrypt(
                    nonce,
                    chacha20poly1305::aead::Payload {
                        msg: plaintext,
                        aad: aad_data,
                    },
                )
                .map_err(|e| SecurityError::encryption(format!("Encryption failed: {}", e)))?
        } else {
            cipher
                .encrypt(nonce, plaintext)
                .map_err(|e| SecurityError::encryption(format!("Encryption failed: {}", e)))?
        };

        let metadata = EncryptionMetadata::new(
            self.algorithm,
            self.key_id.clone(),
            nonce_bytes.to_vec(),
            aad.map(|a| a.to_vec()),
        );

        Ok(EncryptedData::new(ciphertext, metadata))
    }

    fn decrypt_chacha(&self, encrypted: &EncryptedData) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key)
            .map_err(|e| SecurityError::decryption(format!("Failed to create cipher: {}", e)))?;

        if encrypted.metadata.iv.len() != 12 {
            return Err(SecurityError::decryption(format!(
                "Invalid nonce length: expected 12, got {}",
                encrypted.metadata.iv.len()
            )));
        }

        let nonce = chacha20poly1305::Nonce::from_slice(&encrypted.metadata.iv);

        let plaintext = if let Some(ref aad) = encrypted.metadata.aad {
            cipher
                .decrypt(
                    nonce,
                    chacha20poly1305::aead::Payload {
                        msg: &encrypted.ciphertext,
                        aad,
                    },
                )
                .map_err(|e| SecurityError::decryption(format!("Decryption failed: {}", e)))?
        } else {
            cipher
                .decrypt(nonce, encrypted.ciphertext.as_ref())
                .map_err(|e| SecurityError::decryption(format!("Decryption failed: {}", e)))?
        };

        Ok(plaintext)
    }

    /// Encrypt data in place (overwrites input buffer).
    pub fn encrypt_in_place(
        &self,
        buffer: &mut Vec<u8>,
        aad: Option<&[u8]>,
    ) -> Result<EncryptionMetadata> {
        let encrypted = self.encrypt(buffer, aad)?;
        buffer.clear();
        buffer.extend_from_slice(&encrypted.ciphertext);
        Ok(encrypted.metadata)
    }

    /// Get the algorithm used by this encryptor.
    pub fn algorithm(&self) -> EncryptionAlgorithm {
        self.algorithm
    }

    /// Get the key ID.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

/// Field-level encryptor for encrypting specific fields in structured data.
pub struct FieldEncryptor {
    encryptor: AtRestEncryptor,
}

impl FieldEncryptor {
    /// Create a new field encryptor.
    pub fn new(encryptor: AtRestEncryptor) -> Self {
        Self { encryptor }
    }

    /// Encrypt a string field.
    pub fn encrypt_string(&self, value: &str) -> Result<String> {
        let encrypted = self.encryptor.encrypt(value.as_bytes(), None)?;
        let json = serde_json::to_string(&encrypted)?;
        Ok(BASE64.encode(json))
    }

    /// Decrypt a string field.
    pub fn decrypt_string(&self, encrypted: &str) -> Result<String> {
        let json = BASE64
            .decode(encrypted)
            .map_err(|e| SecurityError::decryption(format!("Base64 decode failed: {}", e)))?;
        let encrypted_data: EncryptedData = serde_json::from_slice(&json)?;
        let plaintext = self.encryptor.decrypt(&encrypted_data)?;
        String::from_utf8(plaintext)
            .map_err(|e| SecurityError::decryption(format!("UTF-8 decode failed: {}", e)))
    }

    /// Encrypt a JSON-serializable value.
    pub fn encrypt_json<T: serde::Serialize>(&self, value: &T) -> Result<String> {
        let json = serde_json::to_vec(value)?;
        let encrypted = self.encryptor.encrypt(&json, None)?;
        let encrypted_json = serde_json::to_string(&encrypted)?;
        Ok(BASE64.encode(encrypted_json))
    }

    /// Decrypt a JSON-serializable value.
    pub fn decrypt_json<T: serde::de::DeserializeOwned>(&self, encrypted: &str) -> Result<T> {
        let json = BASE64
            .decode(encrypted)
            .map_err(|e| SecurityError::decryption(format!("Base64 decode failed: {}", e)))?;
        let encrypted_data: EncryptedData = serde_json::from_slice(&json)?;
        let plaintext = self.encryptor.decrypt(&encrypted_data)?;
        serde_json::from_slice(&plaintext).map_err(SecurityError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_gcm_encryption() {
        let key = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encryptor =
            AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, key, "test-key".to_string())
                .expect("Failed to create encryptor");

        let plaintext = b"Hello, World!";
        let encrypted = encryptor
            .encrypt(plaintext, None)
            .expect("Encryption failed");

        assert_ne!(encrypted.ciphertext, plaintext);
        assert_eq!(encrypted.metadata.algorithm, EncryptionAlgorithm::Aes256Gcm);

        let decrypted = encryptor.decrypt(&encrypted).expect("Decryption failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_aes_gcm_with_aad() {
        let key = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encryptor =
            AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, key, "test-key".to_string())
                .expect("Failed to create encryptor");

        let plaintext = b"Hello, World!";
        let aad = b"additional data";
        let encrypted = encryptor
            .encrypt(plaintext, Some(aad))
            .expect("Encryption failed");

        let decrypted = encryptor.decrypt(&encrypted).expect("Decryption failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_chacha_encryption() {
        let key = AtRestEncryptor::generate_key(EncryptionAlgorithm::ChaCha20Poly1305);
        let encryptor = AtRestEncryptor::new(
            EncryptionAlgorithm::ChaCha20Poly1305,
            key,
            "test-key".to_string(),
        )
        .expect("Failed to create encryptor");

        let plaintext = b"Hello, World!";
        let encrypted = encryptor
            .encrypt(plaintext, None)
            .expect("Encryption failed");

        assert_ne!(encrypted.ciphertext, plaintext);
        assert_eq!(
            encrypted.metadata.algorithm,
            EncryptionAlgorithm::ChaCha20Poly1305
        );

        let decrypted = encryptor.decrypt(&encrypted).expect("Decryption failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_field_encryptor_string() {
        let key = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encryptor =
            AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, key, "test-key".to_string())
                .expect("Failed to create encryptor");
        let field_encryptor = FieldEncryptor::new(encryptor);

        let original = "sensitive data";
        let encrypted = field_encryptor
            .encrypt_string(original)
            .expect("Encryption failed");

        assert_ne!(encrypted, original);

        let decrypted = field_encryptor
            .decrypt_string(&encrypted)
            .expect("Decryption failed");
        assert_eq!(decrypted, original);
    }

    #[test]
    fn test_encrypt_in_place() {
        let key = AtRestEncryptor::generate_key(EncryptionAlgorithm::Aes256Gcm);
        let encryptor =
            AtRestEncryptor::new(EncryptionAlgorithm::Aes256Gcm, key, "test-key".to_string())
                .expect("Failed to create encryptor");

        let mut buffer = b"Hello, World!".to_vec();
        let original = buffer.clone();

        let metadata = encryptor
            .encrypt_in_place(&mut buffer, None)
            .expect("Encryption failed");

        assert_ne!(buffer, original);

        let encrypted = EncryptedData::new(buffer, metadata);
        let decrypted = encryptor.decrypt(&encrypted).expect("Decryption failed");
        assert_eq!(decrypted, original);
    }
}
