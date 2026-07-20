//! Codec registry for managing available codecs
//!
//! This module provides a registry for discovering and creating codecs
//! based on their identifiers or configurations.

use super::{Codec, CodecV3, CompressorConfig, NullCodec};
use crate::error::{CodecError, Result, ZarrError};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Type for codec factory functions
type CodecFactory = Box<dyn Fn(&serde_json::Value) -> Result<Box<dyn Codec>> + Send + Sync>;

/// Global codec registry
pub struct CodecRegistry {
    /// Map of codec ID to factory function
    factories: Arc<RwLock<HashMap<String, CodecFactory>>>,
}

impl CodecRegistry {
    /// Creates a new empty codec registry
    #[must_use]
    pub fn new() -> Self {
        Self {
            factories: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a registry with default codecs
    #[must_use]
    pub fn with_defaults() -> Self {
        let registry = Self::new();

        // Register null codec
        registry
            .register("null", |_config| Ok(Box::new(NullCodec)))
            .ok();

        // Register gzip
        #[cfg(feature = "gzip")]
        registry
            .register("gzip", |config| {
                let level = config.get("level").and_then(|v| v.as_u64()).unwrap_or(6) as u32;
                super::gzip::GzipCodec::new(level).map(|c| Box::new(c) as Box<dyn Codec>)
            })
            .ok();

        // Register zstd
        #[cfg(feature = "zstd")]
        registry
            .register("zstd", |config| {
                let level = config.get("level").and_then(|v| v.as_i64()).unwrap_or(3) as i32;
                super::zstd_codec::ZstdCodec::new(level).map(|c| Box::new(c) as Box<dyn Codec>)
            })
            .ok();

        // Register lz4
        #[cfg(feature = "lz4")]
        registry
            .register("lz4", |config| {
                let acceleration = config
                    .get("acceleration")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32);
                super::lz4_codec::Lz4Codec::new(acceleration).map(|c| Box::new(c) as Box<dyn Codec>)
            })
            .ok();

        registry
    }

    /// Registers a codec factory
    ///
    /// # Errors
    /// Returns error if the registry lock is poisoned
    pub fn register<F>(&self, id: impl Into<String>, factory: F) -> Result<()>
    where
        F: Fn(&serde_json::Value) -> Result<Box<dyn Codec>> + Send + Sync + 'static,
    {
        let mut factories = self.factories.write().map_err(|e| {
            ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("Registry lock poisoned: {e}"),
            })
        })?;

        factories.insert(id.into(), Box::new(factory));
        Ok(())
    }

    /// Checks if a codec is registered
    ///
    /// # Errors
    /// Returns error if the registry lock is poisoned
    pub fn has_codec(&self, id: &str) -> Result<bool> {
        let factories = self.factories.read().map_err(|e| {
            ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("Registry lock poisoned: {e}"),
            })
        })?;

        Ok(factories.contains_key(id))
    }

    /// Creates a codec from a configuration value
    ///
    /// # Errors
    /// Returns error if the codec is not registered or creation fails
    pub fn create(&self, id: &str, config: &serde_json::Value) -> Result<Box<dyn Codec>> {
        let factories = self.factories.read().map_err(|e| {
            ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("Registry lock poisoned: {e}"),
            })
        })?;

        let factory = factories.get(id).ok_or_else(|| {
            ZarrError::Codec(CodecError::UnknownCodec {
                codec: id.to_string(),
            })
        })?;

        factory(config)
    }

    /// Creates a codec from a v2 compressor configuration
    ///
    /// # Errors
    /// Returns error if the codec cannot be created
    pub fn from_v2_config(&self, config: &CompressorConfig) -> Result<Box<dyn Codec>> {
        config.build()
    }

    /// Creates a codec from a v3 codec configuration
    ///
    /// # Errors
    /// Returns error if the codec cannot be created
    pub fn from_v3_config(&self, config: &CodecV3) -> Result<Box<dyn Codec>> {
        let json_config = serde_json::to_value(&config.configuration).map_err(|e| {
            ZarrError::Codec(CodecError::InvalidConfiguration {
                codec: config.name.clone(),
                message: format!("Invalid configuration: {e}"),
            })
        })?;

        self.create(&config.name, &json_config)
    }

    /// Returns a list of registered codec IDs
    ///
    /// # Errors
    /// Returns error if the registry lock is poisoned
    pub fn list_codecs(&self) -> Result<Vec<String>> {
        let factories = self.factories.read().map_err(|e| {
            ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("Registry lock poisoned: {e}"),
            })
        })?;

        Ok(factories.keys().cloned().collect())
    }
}

impl Default for CodecRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl Clone for CodecRegistry {
    fn clone(&self) -> Self {
        Self {
            factories: Arc::clone(&self.factories),
        }
    }
}

/// Global default codec registry
static DEFAULT_REGISTRY: once_cell::sync::Lazy<CodecRegistry> =
    once_cell::sync::Lazy::new(CodecRegistry::with_defaults);

/// Returns the global default codec registry
#[must_use]
pub fn default_registry() -> &'static CodecRegistry {
    &DEFAULT_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let registry = CodecRegistry::new();
        let codecs = registry.list_codecs().expect("list codecs");
        assert_eq!(codecs.len(), 0);
    }

    #[test]
    fn test_registry_with_defaults() {
        let registry = CodecRegistry::with_defaults();
        let codecs = registry.list_codecs().expect("list codecs");
        assert!(!codecs.is_empty());
        assert!(codecs.contains(&"null".to_string()));
    }

    #[test]
    fn test_registry_register() {
        let registry = CodecRegistry::new();

        registry
            .register("test", |_config| Ok(Box::new(NullCodec)))
            .expect("register codec");

        assert!(registry.has_codec("test").expect("check codec"));
        assert!(!registry.has_codec("unknown").expect("check codec"));
    }

    #[test]
    fn test_registry_create() {
        let registry = CodecRegistry::with_defaults();

        let config = serde_json::json!({});
        let codec = registry.create("null", &config).expect("create codec");
        assert_eq!(codec.id(), "null");
    }

    #[test]
    fn test_registry_create_unknown() {
        let registry = CodecRegistry::with_defaults();

        let config = serde_json::json!({});
        assert!(registry.create("unknown_codec", &config).is_err());
    }

    #[test]
    #[cfg(feature = "gzip")]
    fn test_registry_gzip() {
        let registry = CodecRegistry::with_defaults();

        assert!(registry.has_codec("gzip").expect("check codec"));

        let config = serde_json::json!({"level": 6});
        let codec = registry.create("gzip", &config).expect("create codec");
        assert_eq!(codec.id(), "gzip");

        let data = b"test data";
        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    #[cfg(feature = "zstd")]
    fn test_registry_zstd() {
        let registry = CodecRegistry::with_defaults();

        assert!(registry.has_codec("zstd").expect("check codec"));

        let config = serde_json::json!({"level": 3});
        let codec = registry.create("zstd", &config).expect("create codec");
        assert_eq!(codec.id(), "zstd");

        let data = b"test data";
        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    #[cfg(feature = "lz4")]
    fn test_registry_lz4() {
        let registry = CodecRegistry::with_defaults();

        assert!(registry.has_codec("lz4").expect("check codec"));

        let config = serde_json::json!({"acceleration": 1});
        let codec = registry.create("lz4", &config).expect("create codec");
        assert_eq!(codec.id(), "lz4");

        let data = b"test data";
        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_registry_from_v2_config() {
        let registry = CodecRegistry::with_defaults();

        let config = CompressorConfig::Null;
        let codec = registry.from_v2_config(&config).expect("create codec");
        assert_eq!(codec.id(), "null");
    }

    #[test]
    fn test_registry_from_v3_config() {
        let registry = CodecRegistry::with_defaults();

        let config = CodecV3::new("null");
        let codec = registry.from_v3_config(&config).expect("create codec");
        assert_eq!(codec.id(), "null");
    }

    #[test]
    fn test_default_registry() {
        let registry = default_registry();
        assert!(registry.has_codec("null").expect("check codec"));

        let data = b"test data";
        let codec = registry
            .create("null", &serde_json::json!({}))
            .expect("create codec");

        let encoded = codec.encode(data).expect("encode");
        assert_eq!(encoded, data);
    }

    #[test]
    fn test_registry_clone() {
        let registry1 = CodecRegistry::with_defaults();
        let registry2 = registry1.clone();

        let codecs1 = registry1.list_codecs().expect("list codecs");
        let codecs2 = registry2.list_codecs().expect("list codecs");
        assert_eq!(codecs1, codecs2);
    }

    #[test]
    fn test_registry_custom_codec() {
        let registry = CodecRegistry::new();

        // Register a custom codec that just reverses bytes
        registry
            .register("reverse", |_config| {
                struct ReverseCodec;
                impl Codec for ReverseCodec {
                    fn id(&self) -> &str {
                        "reverse"
                    }
                    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
                        Ok(data.iter().rev().copied().collect())
                    }
                    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
                        Ok(data.iter().rev().copied().collect())
                    }
                    fn clone_box(&self) -> Box<dyn Codec> {
                        Box::new(ReverseCodec)
                    }
                }
                Ok(Box::new(ReverseCodec))
            })
            .expect("register codec");

        let codec = registry
            .create("reverse", &serde_json::json!({}))
            .expect("create codec");

        let data = b"Hello";
        let encoded = codec.encode(data).expect("encode");
        assert_eq!(encoded, b"olleH");

        let decoded = codec.decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }
}
