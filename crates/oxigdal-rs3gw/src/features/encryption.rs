//! Encryption configuration for rs3gw
//!
//! This module provides encryption-at-rest configuration for geospatial data
//! stored in rs3gw backends.

/// AEAD nonce length in bytes (96-bit nonce, shared by AES-256-GCM and
/// ChaCha20-Poly1305).
#[cfg(feature = "encryption")]
const NONCE_LEN: usize = 12;

/// Errors that can occur while encrypting or decrypting object payloads.
#[cfg(feature = "encryption")]
#[derive(Debug, thiserror::Error)]
pub enum EncryptionError {
    /// Encryption was requested but the config is disabled or has no key.
    #[error("encryption is not enabled or no key is configured")]
    NotEnabled,

    /// The configured key does not have the required length (32 bytes).
    #[error("invalid key length: {0} bytes (expected 32)")]
    InvalidKeyLength(usize),

    /// The system random number generator failed while producing a nonce.
    #[error("random number generator failure while producing a nonce")]
    Rng,

    /// The AEAD cipher rejected the plaintext during encryption.
    #[error("AEAD encryption failed")]
    Encrypt,

    /// The AEAD cipher rejected the ciphertext during decryption (wrong key,
    /// tampered data, or the object was never encrypted).
    #[error("AEAD decryption failed (wrong key or corrupted/plaintext data)")]
    Decrypt,

    /// The stored blob is shorter than the nonce prefix, so it cannot be a
    /// valid ciphertext produced by [`EncryptionConfig::encrypt`].
    #[error("ciphertext too short: {0} bytes (need at least {NONCE_LEN} for the nonce)")]
    CiphertextTooShort(usize),
}

/// Encryption algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM (recommended for most use cases)
    #[default]
    Aes256Gcm,

    /// ChaCha20-Poly1305 (faster on platforms without AES hardware acceleration)
    ChaCha20Poly1305,
}

/// Encryption configuration
///
/// Provides encryption-at-rest for sensitive geospatial data.
///
/// # Security Notes
/// - Keys should be stored securely (e.g., in a key management system)
/// - Use unique keys for different datasets/projects
/// - Rotate keys periodically
/// - Never commit keys to version control
#[derive(Debug, Clone, Default)]
pub struct EncryptionConfig {
    /// Whether encryption is enabled
    pub enabled: bool,

    /// Encryption algorithm
    pub algorithm: EncryptionAlgorithm,

    /// Encryption key (32 bytes for AES-256)
    ///
    /// In production, load this from a secure key management system,
    /// not from hardcoded values or environment variables.
    key: Option<Vec<u8>>,

    /// Encrypt metadata in addition to data
    pub encrypt_metadata: bool,
}

impl EncryptionConfig {
    /// Creates a new encryption configuration (disabled by default)
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables encryption with the specified key
    ///
    /// # Arguments
    /// * `key` - 32-byte encryption key for AES-256 or ChaCha20
    ///
    /// # Security
    /// The key should come from a secure source. Never hardcode keys.
    #[must_use]
    pub fn with_key(mut self, key: Vec<u8>) -> Self {
        self.enabled = true;
        self.key = Some(key);
        self
    }

    /// Sets the encryption algorithm
    #[must_use]
    pub fn with_algorithm(mut self, algorithm: EncryptionAlgorithm) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Enables metadata encryption
    #[must_use]
    pub fn with_metadata_encryption(mut self, enabled: bool) -> Self {
        self.encrypt_metadata = enabled;
        self
    }

    /// Disables encryption
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            algorithm: EncryptionAlgorithm::default(),
            key: None,
            encrypt_metadata: false,
        }
    }

    /// Returns whether encryption is enabled
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.key.is_some()
    }

    /// Returns the encryption key (if set)
    #[must_use]
    pub fn key(&self) -> Option<&[u8]> {
        self.key.as_deref()
    }

    /// Validates the configuration
    ///
    /// # Errors
    /// Returns an error if:
    /// - Encryption is enabled but no key is provided
    /// - The key size is incorrect for the algorithm
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let key = self
            .key
            .as_ref()
            .ok_or("Encryption enabled but no key provided")?;

        let expected_size = match self.algorithm {
            EncryptionAlgorithm::Aes256Gcm => 32,
            EncryptionAlgorithm::ChaCha20Poly1305 => 32,
        };

        if key.len() != expected_size {
            return Err(format!(
                "Invalid key size: expected {expected_size} bytes, got {}",
                key.len()
            ));
        }

        Ok(())
    }

    /// Encrypts a payload with the configured AEAD algorithm.
    ///
    /// The returned blob is `nonce (12 bytes) || ciphertext || auth-tag`. A
    /// fresh random nonce is generated for every call, so encrypting the same
    /// plaintext twice yields different ciphertexts — this is required because
    /// Zarr chunks are written independently and reusing a nonce with the same
    /// key would break the security guarantees of GCM/Poly1305.
    ///
    /// # Errors
    /// Returns an error if encryption is not enabled, the key length is wrong,
    /// the RNG fails, or the underlying AEAD cipher rejects the input.
    #[cfg(feature = "encryption")]
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        let key = self.key.as_deref().ok_or(EncryptionError::NotEnabled)?;
        if key.len() != 32 {
            return Err(EncryptionError::InvalidKeyLength(key.len()));
        }

        let mut nonce_bytes = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce_bytes).map_err(|_| EncryptionError::Rng)?;

        let ciphertext = match self.algorithm {
            EncryptionAlgorithm::Aes256Gcm => {
                use aes_gcm::aead::{Aead, KeyInit};
                use aes_gcm::{Aes256Gcm, Nonce};

                let cipher = Aes256Gcm::new_from_slice(key)
                    .map_err(|_| EncryptionError::InvalidKeyLength(key.len()))?;
                let nonce = Nonce::from(nonce_bytes);
                cipher
                    .encrypt(&nonce, plaintext)
                    .map_err(|_| EncryptionError::Encrypt)?
            }
            EncryptionAlgorithm::ChaCha20Poly1305 => {
                use chacha20poly1305::aead::{Aead, KeyInit};
                use chacha20poly1305::{ChaCha20Poly1305, Nonce};

                let cipher = ChaCha20Poly1305::new_from_slice(key)
                    .map_err(|_| EncryptionError::InvalidKeyLength(key.len()))?;
                let nonce = Nonce::from(nonce_bytes);
                cipher
                    .encrypt(&nonce, plaintext)
                    .map_err(|_| EncryptionError::Encrypt)?
            }
        };

        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypts a payload previously produced by [`Self::encrypt`].
    ///
    /// The input must be `nonce (12 bytes) || ciphertext || auth-tag`.
    ///
    /// # Errors
    /// Returns an error if encryption is not enabled, the key length is wrong,
    /// the blob is shorter than the nonce prefix, or authentication fails
    /// (wrong key, tampered ciphertext, or data that was never encrypted).
    #[cfg(feature = "encryption")]
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        let key = self.key.as_deref().ok_or(EncryptionError::NotEnabled)?;
        if key.len() != 32 {
            return Err(EncryptionError::InvalidKeyLength(key.len()));
        }
        if data.len() < NONCE_LEN {
            return Err(EncryptionError::CiphertextTooShort(data.len()));
        }

        let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);

        let plaintext = match self.algorithm {
            EncryptionAlgorithm::Aes256Gcm => {
                use aes_gcm::aead::{Aead, KeyInit};
                use aes_gcm::{Aes256Gcm, Nonce};

                let cipher = Aes256Gcm::new_from_slice(key)
                    .map_err(|_| EncryptionError::InvalidKeyLength(key.len()))?;
                let nonce = Nonce::try_from(nonce_bytes).map_err(|_| EncryptionError::Decrypt)?;
                cipher
                    .decrypt(&nonce, ciphertext)
                    .map_err(|_| EncryptionError::Decrypt)?
            }
            EncryptionAlgorithm::ChaCha20Poly1305 => {
                use chacha20poly1305::aead::{Aead, KeyInit};
                use chacha20poly1305::{ChaCha20Poly1305, Nonce};

                let cipher = ChaCha20Poly1305::new_from_slice(key)
                    .map_err(|_| EncryptionError::InvalidKeyLength(key.len()))?;
                let nonce = Nonce::try_from(nonce_bytes).map_err(|_| EncryptionError::Decrypt)?;
                cipher
                    .decrypt(&nonce, ciphertext)
                    .map_err(|_| EncryptionError::Decrypt)?
            }
        };

        Ok(plaintext)
    }
}

/// Helper for generating secure random encryption keys
///
/// # Examples
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use oxigdal_rs3gw::features::encryption::generate_key;
///
/// let key = generate_key()?;
/// println!("Generated key length: {} bytes", key.len());
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// Returns an error if the system random number generator fails
#[cfg(feature = "encryption")]
pub fn generate_key() -> Result<Vec<u8>, getrandom::Error> {
    let mut key = vec![0u8; 32];
    // getrandom 0.4 API: fill() fills a buffer with random bytes
    getrandom::fill(&mut key)?;
    Ok(key)
}

/// Helper for deriving an encryption key from a password
///
/// Uses PBKDF2 with SHA-256 to derive a key from a password.
///
/// # Arguments
/// * `password` - The password to derive from
/// * `salt` - Salt for key derivation (must be unique per dataset)
/// * `iterations` - Number of PBKDF2 iterations (minimum 100,000)
///
/// # Security Notes
/// - Use a strong, unique password
/// - Use a unique salt per dataset
/// - Use at least 100,000 iterations (more is better)
/// - Store the salt securely alongside the encrypted data
///
/// # Errors
/// Returns an error if:
/// - Iterations is less than 100,000 (too weak)
/// - PBKDF2 derivation fails
#[cfg(feature = "encryption")]
pub fn derive_key_from_password(
    password: &str,
    salt: &[u8],
    iterations: u32,
) -> Result<Vec<u8>, &'static str> {
    use hmac::Hmac;
    use sha2::Sha256;

    if iterations < 100_000 {
        return Err("Iterations must be at least 100,000 for security");
    }

    let mut key = vec![0u8; 32];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), salt, iterations, &mut key)?;
    Ok(key)
}

#[cfg(feature = "encryption")]
mod pbkdf2 {
    use hmac::digest::typenum::Unsigned;
    use hmac::digest::{KeyInit, Mac};

    pub fn pbkdf2<M: Mac + KeyInit>(
        password: &[u8],
        salt: &[u8],
        iterations: u32,
        output: &mut [u8],
    ) -> Result<(), &'static str> {
        if output.is_empty() {
            return Err("Output buffer is empty");
        }

        let hlen = M::OutputSize::to_usize();
        let mut current_block = 1u32;
        let mut offset = 0;

        while offset < output.len() {
            let block_len = std::cmp::min(hlen, output.len() - offset);

            let mut u = vec![0u8; hlen];
            let mut f = vec![0u8; hlen];

            // U_1 = PRF(password, salt || block_index)
            let mut mac =
                <M as KeyInit>::new_from_slice(password).map_err(|_| "Invalid key length")?;
            mac.update(salt);
            mac.update(&current_block.to_be_bytes());
            let result = mac.finalize();
            u.copy_from_slice(&result.into_bytes());
            f.copy_from_slice(&u);

            // U_i = PRF(password, U_{i-1})
            for _ in 1..iterations {
                let mut mac =
                    <M as KeyInit>::new_from_slice(password).map_err(|_| "Invalid key length")?;
                mac.update(&u);
                let result = mac.finalize();
                u.copy_from_slice(&result.into_bytes());

                // F = U_1 XOR U_2 XOR ... XOR U_iterations
                for (f_byte, u_byte) in f.iter_mut().zip(u.iter()) {
                    *f_byte ^= u_byte;
                }
            }

            output[offset..offset + block_len].copy_from_slice(&f[..block_len]);
            offset += block_len;
            current_block = current_block
                .checked_add(1)
                .ok_or("Block counter overflow")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EncryptionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.algorithm, EncryptionAlgorithm::Aes256Gcm);
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_with_key() {
        let key = vec![0u8; 32];
        let config = EncryptionConfig::new().with_key(key.clone());

        assert!(config.is_enabled());
        assert_eq!(config.key(), Some(key.as_slice()));
    }

    #[test]
    fn test_validate_valid() {
        let key = vec![0u8; 32];
        let config = EncryptionConfig::new().with_key(key);

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_key_size() {
        let key = vec![0u8; 16]; // Too short
        let config = EncryptionConfig::new().with_key(key);

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_no_key() {
        let mut config = EncryptionConfig::new();
        config.enabled = true;
        // No key set

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_generate_key() {
        let key1 = generate_key().expect("Failed to generate key");
        let key2 = generate_key().expect("Failed to generate key");

        assert_eq!(key1.len(), 32);
        assert_eq!(key2.len(), 32);
        assert_ne!(key1, key2); // Should be different (extremely high probability)
    }

    #[test]
    fn test_derive_key_from_password() {
        let password = "my_secure_password";
        let salt = b"unique_salt_12345";

        let key = derive_key_from_password(password, salt, 100_000).expect("Failed to derive key");
        assert_eq!(key.len(), 32);

        // Same inputs should produce same key
        let key2 = derive_key_from_password(password, salt, 100_000).expect("Failed to derive key");
        assert_eq!(key, key2);

        // Different salt should produce different key
        let key3 = derive_key_from_password(password, b"different_salt", 100_000)
            .expect("Failed to derive key");
        assert_ne!(key, key3);
    }

    #[test]
    #[allow(clippy::panic)]
    fn test_derive_key_weak_iterations() {
        let result = derive_key_from_password("password", b"salt", 1000); // Too few iterations
        match result {
            Err(e) => assert_eq!(e, "Iterations must be at least 100,000 for security"),
            Ok(_) => panic!("Expected error for weak iterations"),
        }
    }

    #[test]
    fn test_algorithm_variants() {
        let config = EncryptionConfig::new()
            .with_key(vec![0u8; 32])
            .with_algorithm(EncryptionAlgorithm::ChaCha20Poly1305);

        assert_eq!(config.algorithm, EncryptionAlgorithm::ChaCha20Poly1305);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_aes() {
        let key = generate_key().expect("Failed to generate key");
        let config = EncryptionConfig::new().with_key(key);

        let plaintext = b"sensitive geospatial chunk data";
        let blob = config.encrypt(plaintext).expect("encrypt failed");

        // Ciphertext must not contain the plaintext in the clear.
        assert!(!blob.windows(plaintext.len()).any(|w| w == plaintext));
        // nonce (12) + tag (16) overhead is present.
        assert!(blob.len() >= plaintext.len() + NONCE_LEN + 16);

        let recovered = config.decrypt(&blob).expect("decrypt failed");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_chacha() {
        let key = generate_key().expect("Failed to generate key");
        let config = EncryptionConfig::new()
            .with_key(key)
            .with_algorithm(EncryptionAlgorithm::ChaCha20Poly1305);

        let plaintext = b"another chunk that must stay private";
        let blob = config.encrypt(plaintext).expect("encrypt failed");
        assert!(!blob.windows(plaintext.len()).any(|w| w == plaintext));

        let recovered = config.decrypt(&blob).expect("decrypt failed");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_encrypt_unique_nonce_per_call() {
        let key = generate_key().expect("Failed to generate key");
        let config = EncryptionConfig::new().with_key(key);

        let plaintext = b"repeatable content";
        let a = config.encrypt(plaintext).expect("encrypt failed");
        let b = config.encrypt(plaintext).expect("encrypt failed");

        // Same plaintext + same key must yield different ciphertext (fresh nonce).
        assert_ne!(a, b);
    }

    #[test]
    fn test_decrypt_rejects_tampered_ciphertext() {
        let key = generate_key().expect("Failed to generate key");
        let config = EncryptionConfig::new().with_key(key);

        let mut blob = config.encrypt(b"trusted bytes").expect("encrypt failed");
        // Flip a bit in the ciphertext body (past the nonce).
        let last = blob.len() - 1;
        blob[last] ^= 0x01;

        assert!(matches!(
            config.decrypt(&blob),
            Err(EncryptionError::Decrypt)
        ));
    }

    #[test]
    fn test_decrypt_rejects_wrong_key() {
        let config_a = EncryptionConfig::new().with_key(generate_key().expect("key"));
        let config_b = EncryptionConfig::new().with_key(generate_key().expect("key"));

        let blob = config_a.encrypt(b"secret").expect("encrypt failed");
        assert!(matches!(
            config_b.decrypt(&blob),
            Err(EncryptionError::Decrypt)
        ));
    }

    #[test]
    fn test_decrypt_rejects_short_blob() {
        let config = EncryptionConfig::new().with_key(generate_key().expect("key"));
        let short = [0u8; 5];
        assert!(matches!(
            config.decrypt(&short),
            Err(EncryptionError::CiphertextTooShort(5))
        ));
    }

    #[test]
    fn test_encrypt_requires_enabled() {
        let config = EncryptionConfig::disabled();
        assert!(matches!(
            config.encrypt(b"data"),
            Err(EncryptionError::NotEnabled)
        ));
    }
}
