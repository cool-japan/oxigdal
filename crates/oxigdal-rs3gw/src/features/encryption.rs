//! Encryption configuration for rs3gw
//!
//! This module provides encryption-at-rest configuration for geospatial data
//! stored in rs3gw backends.

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
    // getrandom 0.3 API: getrandom fills a buffer with random bytes
    getrandom::getrandom(&mut key)?;
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
}
