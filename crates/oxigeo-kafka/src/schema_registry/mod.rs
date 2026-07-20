//! Schema Registry integration for Apache Avro

#[cfg(feature = "schema-registry")]
mod client;
#[cfg(feature = "schema-registry")]
mod compatibility;
#[cfg(feature = "schema-registry")]
mod schema;
#[cfg(feature = "schema-registry")]
mod serializer;

#[cfg(feature = "schema-registry")]
pub use client::{SchemaRegistryClient, SchemaRegistryConfig};
#[cfg(feature = "schema-registry")]
pub use compatibility::{CompatibilityChecker, CompatibilityLevel};
#[cfg(feature = "schema-registry")]
pub use schema::{Schema, SchemaType, SchemaVersion};
#[cfg(feature = "schema-registry")]
pub use serializer::{AvroDeserializer, AvroSerializer};

#[cfg(feature = "schema-registry")]
use crate::error::Result;
#[cfg(feature = "schema-registry")]
use bytes::Bytes;
#[cfg(feature = "schema-registry")]
use std::sync::Arc;

/// Schema Registry for managing Avro schemas
#[cfg(feature = "schema-registry")]
pub struct SchemaRegistry {
    client: Arc<SchemaRegistryClient>,
    serializer: AvroSerializer,
    deserializer: AvroDeserializer,
}

#[cfg(feature = "schema-registry")]
impl SchemaRegistry {
    /// Create a new Schema Registry
    pub async fn new(config: SchemaRegistryConfig) -> Result<Self> {
        let client = Arc::new(SchemaRegistryClient::new(config).await?);
        let serializer = AvroSerializer::new(Arc::clone(&client));
        let deserializer = AvroDeserializer::new(Arc::clone(&client));

        Ok(Self {
            client,
            serializer,
            deserializer,
        })
    }

    /// Register a schema
    pub async fn register_schema(&self, subject: &str, schema: &str) -> Result<i32> {
        self.client.register_schema(subject, schema).await
    }

    /// Get schema by ID
    pub async fn get_schema_by_id(&self, id: i32) -> Result<Schema> {
        self.client.get_schema_by_id(id).await
    }

    /// Get schema by subject and version
    pub async fn get_schema_by_subject(
        &self,
        subject: &str,
        version: SchemaVersion,
    ) -> Result<Schema> {
        self.client.get_schema_by_subject(subject, version).await
    }

    /// Check schema compatibility
    pub async fn test_compatibility(&self, subject: &str, schema: &str) -> Result<bool> {
        self.client.test_compatibility(subject, schema).await
    }

    /// Serialize value with schema
    pub async fn serialize<T: serde::Serialize>(&self, subject: &str, value: &T) -> Result<Bytes> {
        self.serializer.serialize(subject, value).await
    }

    /// Deserialize value
    pub async fn deserialize<T: serde::de::DeserializeOwned>(&self, data: &[u8]) -> Result<T> {
        self.deserializer.deserialize(data).await
    }

    /// Get client
    pub fn client(&self) -> &SchemaRegistryClient {
        &self.client
    }
}

#[cfg(not(feature = "schema-registry"))]
/// Dummy SchemaRegistry when feature is not enabled
pub struct SchemaRegistry;

#[cfg(not(feature = "schema-registry"))]
impl SchemaRegistry {
    /// This is a placeholder when schema-registry feature is disabled
    pub fn new() -> Self {
        SchemaRegistry
    }
}
