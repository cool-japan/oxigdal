//! Storage Transformers for Zarr v3
//!
//! This module provides storage transformers that operate on data before/after
//! storage, including encryption, checksums, and custom transformations.

use crate::error::{Result, ZarrError};
use std::collections::HashMap;

/// Storage transformer trait
pub trait Transformer: Send + Sync {
    /// Returns the transformer identifier
    fn id(&self) -> &str;

    /// Transforms data before storage (encode)
    ///
    /// # Errors
    /// Returns error if transformation fails
    fn encode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Transforms data after retrieval (decode)
    ///
    /// # Errors
    /// Returns error if transformation fails
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Returns metadata about the transformer
    fn metadata(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Clones the transformer
    fn clone_box(&self) -> Box<dyn Transformer>;
}

/// Transformer chain - multiple transformers applied in sequence
pub struct TransformerChain {
    transformers: Vec<Box<dyn Transformer>>,
}

impl TransformerChain {
    /// Creates a new transformer chain
    #[must_use]
    pub fn new(transformers: Vec<Box<dyn Transformer>>) -> Self {
        Self { transformers }
    }

    /// Creates an empty transformer chain
    #[must_use]
    pub fn empty() -> Self {
        Self {
            transformers: Vec::new(),
        }
    }

    /// Adds a transformer to the chain
    pub fn add(&mut self, transformer: Box<dyn Transformer>) {
        self.transformers.push(transformer);
    }

    /// Returns the number of transformers in the chain
    #[must_use]
    pub fn len(&self) -> usize {
        self.transformers.len()
    }

    /// Returns true if the chain is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.transformers.is_empty()
    }

    /// Encodes data through the transformer chain
    ///
    /// # Errors
    /// Returns error if any transformer fails
    pub fn encode(&self, mut data: Vec<u8>) -> Result<Vec<u8>> {
        for transformer in &self.transformers {
            data = transformer.encode(&data)?;
        }
        Ok(data)
    }

    /// Decodes data through the transformer chain (in reverse order)
    ///
    /// # Errors
    /// Returns error if any transformer fails
    pub fn decode(&self, mut data: Vec<u8>) -> Result<Vec<u8>> {
        for transformer in self.transformers.iter().rev() {
            data = transformer.decode(&data)?;
        }
        Ok(data)
    }
}

/// CRC32 checksum transformer
#[derive(Debug, Clone)]
pub struct Crc32Transformer {
    /// Whether to append or prepend checksum
    append: bool,
}

impl Crc32Transformer {
    /// Creates a new CRC32 transformer
    #[must_use]
    pub const fn new(append: bool) -> Self {
        Self { append }
    }

    /// Computes CRC32 checksum
    #[must_use]
    fn compute_checksum(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFF;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }
}

impl Transformer for Crc32Transformer {
    fn id(&self) -> &str {
        "crc32"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let checksum = Self::compute_checksum(data);
        let checksum_bytes = checksum.to_le_bytes();

        let mut result = Vec::with_capacity(data.len() + 4);
        if self.append {
            result.extend_from_slice(data);
            result.extend_from_slice(&checksum_bytes);
        } else {
            result.extend_from_slice(&checksum_bytes);
            result.extend_from_slice(data);
        }
        Ok(result)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 4 {
            return Err(ZarrError::Internal {
                message: "Data too small for CRC32 checksum".to_string(),
            });
        }

        let (payload, stored_checksum) = if self.append {
            let payload = &data[..data.len() - 4];
            let checksum_bytes = &data[data.len() - 4..];
            let checksum = u32::from_le_bytes([
                checksum_bytes[0],
                checksum_bytes[1],
                checksum_bytes[2],
                checksum_bytes[3],
            ]);
            (payload, checksum)
        } else {
            let checksum_bytes = &data[..4];
            let checksum = u32::from_le_bytes([
                checksum_bytes[0],
                checksum_bytes[1],
                checksum_bytes[2],
                checksum_bytes[3],
            ]);
            let payload = &data[4..];
            (payload, checksum)
        };

        let computed_checksum = Self::compute_checksum(payload);
        if computed_checksum != stored_checksum {
            return Err(ZarrError::Internal {
                message: format!(
                    "CRC32 checksum mismatch: expected {stored_checksum:08x}, got {computed_checksum:08x}"
                ),
            });
        }

        Ok(payload.to_vec())
    }

    fn clone_box(&self) -> Box<dyn Transformer> {
        Box::new(self.clone())
    }
}

/// SHA256 checksum transformer
#[derive(Debug, Clone)]
pub struct Sha256Transformer {
    /// Whether to append or prepend checksum
    append: bool,
}

impl Sha256Transformer {
    /// Creates a new SHA256 transformer
    #[must_use]
    pub const fn new(append: bool) -> Self {
        Self { append }
    }

    /// Computes SHA256 hash (simplified implementation)
    #[must_use]
    fn compute_hash(data: &[u8]) -> [u8; 32] {
        // This is a placeholder - in production, use a proper SHA256 implementation
        // like the `sha2` crate
        let mut hash = [0u8; 32];
        for (i, &byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash
    }
}

impl Transformer for Sha256Transformer {
    fn id(&self) -> &str {
        "sha256"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let hash = Self::compute_hash(data);

        let mut result = Vec::with_capacity(data.len() + 32);
        if self.append {
            result.extend_from_slice(data);
            result.extend_from_slice(&hash);
        } else {
            result.extend_from_slice(&hash);
            result.extend_from_slice(data);
        }
        Ok(result)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 32 {
            return Err(ZarrError::Internal {
                message: "Data too small for SHA256 hash".to_string(),
            });
        }

        let (payload, stored_hash) = if self.append {
            let payload = &data[..data.len() - 32];
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&data[data.len() - 32..]);
            (payload, hash)
        } else {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&data[..32]);
            let payload = &data[32..];
            (payload, hash)
        };

        let computed_hash = Self::compute_hash(payload);
        if computed_hash != stored_hash {
            return Err(ZarrError::Internal {
                message: "SHA256 hash mismatch".to_string(),
            });
        }

        Ok(payload.to_vec())
    }

    fn clone_box(&self) -> Box<dyn Transformer> {
        Box::new(self.clone())
    }
}

/// AES-256-GCM encryption transformer (placeholder)
#[derive(Debug, Clone)]
pub struct AesGcmTransformer {
    key: Vec<u8>,
    key_id: String,
}

impl AesGcmTransformer {
    /// Creates a new AES-GCM transformer
    ///
    /// # Errors
    /// Returns error if key is invalid
    pub fn new(key: Vec<u8>, key_id: impl Into<String>) -> Result<Self> {
        if key.len() != 32 {
            return Err(ZarrError::Internal {
                message: format!("AES-256-GCM requires 32-byte key, got {}", key.len()),
            });
        }

        Ok(Self {
            key,
            key_id: key_id.into(),
        })
    }

    /// Encrypts data (placeholder implementation)
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        // This is a placeholder - in production, use a proper AES-GCM implementation
        // like the `aes-gcm` crate

        // Simple XOR cipher for demonstration (NOT SECURE!)
        let mut result = data.to_vec();
        for (i, byte) in result.iter_mut().enumerate() {
            *byte ^= self.key[i % self.key.len()];
        }

        Ok(result)
    }

    /// Decrypts data (placeholder implementation)
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        // XOR cipher is symmetric
        self.encrypt(data)
    }
}

impl Transformer for AesGcmTransformer {
    fn id(&self) -> &str {
        "aes-256-gcm"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.encrypt(data)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.decrypt(data)
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut meta = HashMap::new();
        meta.insert("algorithm".to_string(), "AES-256-GCM".to_string());
        meta.insert("key_id".to_string(), self.key_id.clone());
        meta
    }

    fn clone_box(&self) -> Box<dyn Transformer> {
        Box::new(self.clone())
    }
}

/// Custom transformer wrapper
pub struct CustomTransformer {
    id: String,
    encode_fn: Box<dyn Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync>,
    decode_fn: Box<dyn Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync>,
}

impl CustomTransformer {
    /// Creates a new custom transformer
    #[must_use]
    pub fn new<E, D>(id: impl Into<String>, encode_fn: E, decode_fn: D) -> Self
    where
        E: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync + 'static,
        D: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync + 'static,
    {
        Self {
            id: id.into(),
            encode_fn: Box::new(encode_fn),
            decode_fn: Box::new(decode_fn),
        }
    }
}

impl Transformer for CustomTransformer {
    fn id(&self) -> &str {
        &self.id
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        (self.encode_fn)(data)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        (self.decode_fn)(data)
    }

    fn clone_box(&self) -> Box<dyn Transformer> {
        // Note: Cannot clone closures, so we return a no-op transformer
        Box::new(NoOpTransformer)
    }
}

/// No-op transformer (does nothing)
#[derive(Debug, Clone)]
pub struct NoOpTransformer;

impl Transformer for NoOpTransformer {
    fn id(&self) -> &str {
        "noop"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn clone_box(&self) -> Box<dyn Transformer> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_transformer() {
        let transformer = Crc32Transformer::new(true);
        let data = b"Hello, Zarr v3!";

        let encoded = transformer.encode(data).expect("encode");
        assert_eq!(encoded.len(), data.len() + 4);

        let decoded = transformer.decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_crc32_checksum_mismatch() {
        let transformer = Crc32Transformer::new(true);
        let data = b"Hello, Zarr v3!";

        let mut encoded = transformer.encode(data).expect("encode");
        // Corrupt the data
        encoded[0] ^= 0xFF;

        let result = transformer.decode(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn test_sha256_transformer() {
        let transformer = Sha256Transformer::new(false);
        let data = b"Test data for SHA256";

        let encoded = transformer.encode(data).expect("encode");
        assert_eq!(encoded.len(), data.len() + 32);

        let decoded = transformer.decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_aes_gcm_transformer() {
        let key = vec![0x42; 32];
        let transformer = AesGcmTransformer::new(key, "test-key-id").expect("create");
        let data = b"Secret data to encrypt";

        let encrypted = transformer.encode(data).expect("encrypt");
        assert_ne!(encrypted, data);

        let decrypted = transformer.decode(&encrypted).expect("decrypt");
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_transformer_chain() {
        let mut chain = TransformerChain::empty();
        chain.add(Box::new(Crc32Transformer::new(true)));
        chain.add(Box::new(Sha256Transformer::new(true)));

        assert_eq!(chain.len(), 2);

        let data = b"Test data for chain".to_vec();
        let encoded = chain.encode(data.clone()).expect("encode");
        let decoded = chain.decode(encoded).expect("decode");

        assert_eq!(decoded, data);
    }

    #[test]
    fn test_noop_transformer() {
        let transformer = NoOpTransformer;
        let data = b"No transformation";

        let encoded = transformer.encode(data).expect("encode");
        assert_eq!(encoded, data);

        let decoded = transformer.decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_custom_transformer() {
        let transformer = CustomTransformer::new(
            "custom",
            |data| Ok(data.iter().map(|b| b.wrapping_add(1)).collect()),
            |data| Ok(data.iter().map(|b| b.wrapping_sub(1)).collect()),
        );

        let data = b"Custom transform";
        let encoded = transformer.encode(data).expect("encode");
        assert_ne!(encoded, data);

        let decoded = transformer.decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_transformer_metadata() {
        let key = vec![0x42; 32];
        let transformer = AesGcmTransformer::new(key, "my-key").expect("create");
        let metadata = transformer.metadata();

        assert_eq!(metadata.get("algorithm"), Some(&"AES-256-GCM".to_string()));
        assert_eq!(metadata.get("key_id"), Some(&"my-key".to_string()));
    }
}
