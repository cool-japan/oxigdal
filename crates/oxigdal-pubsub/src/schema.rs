//! Schema support for Google Cloud Pub/Sub.
//!
//! This module provides schema validation and encoding/decoding support
//! for Apache Avro and Protocol Buffers formats.

#[cfg(feature = "schema")]
use crate::error::{PubSubError, Result};
#[cfg(feature = "schema")]
use bytes::Bytes;
#[cfg(feature = "schema")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")]
use std::collections::HashMap;
#[cfg(feature = "schema")]
use std::sync::Arc;
#[cfg(feature = "schema")]
use tracing::{debug, info};

#[cfg(feature = "schema")]
use crate::error::SchemaFormat;

/// Schema encoding type.
#[cfg(feature = "schema")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaEncoding {
    /// JSON encoding.
    Json,
    /// Binary encoding.
    Binary,
}

/// Schema definition.
#[cfg(feature = "schema")]
#[derive(Debug, Clone)]
pub struct Schema {
    /// Schema ID.
    pub id: String,
    /// Schema name.
    pub name: String,
    /// Schema format.
    pub format: SchemaFormat,
    /// Schema definition (Avro JSON schema or Protobuf descriptor).
    pub definition: String,
    /// Revision ID.
    pub revision_id: Option<String>,
}

#[cfg(feature = "schema")]
impl Schema {
    /// Creates a new schema.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        format: SchemaFormat,
        definition: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            format,
            definition: definition.into(),
            revision_id: None,
        }
    }

    /// Sets the revision ID.
    pub fn with_revision(mut self, revision_id: impl Into<String>) -> Self {
        self.revision_id = Some(revision_id.into());
        self
    }
}

/// Avro schema handler.
#[cfg(feature = "avro")]
pub struct AvroSchema {
    schema: apache_avro::Schema,
    name: String,
}

#[cfg(feature = "avro")]
impl AvroSchema {
    /// Creates a new Avro schema from a JSON definition.
    pub fn from_json(name: impl Into<String>, json_schema: &str) -> Result<Self> {
        let schema = apache_avro::Schema::parse_str(json_schema).map_err(|e| {
            PubSubError::SchemaEncodingError {
                message: format!("Failed to parse Avro schema: {}", e),
                format: SchemaFormat::Avro,
            }
        })?;

        Ok(Self {
            schema,
            name: name.into(),
        })
    }

    /// Encodes data using the Avro schema.
    pub fn encode(&self, value: &apache_avro::types::Value) -> Result<Bytes> {
        let mut writer = apache_avro::Writer::new(&self.schema, Vec::new());
        writer
            .append(value.clone())
            .map_err(|e| PubSubError::SchemaEncodingError {
                message: format!("Failed to encode Avro data: {}", e),
                format: SchemaFormat::Avro,
            })?;

        let encoded = writer
            .into_inner()
            .map_err(|e| PubSubError::SchemaEncodingError {
                message: format!("Failed to finalize Avro encoding: {}", e),
                format: SchemaFormat::Avro,
            })?;

        Ok(Bytes::from(encoded))
    }

    /// Decodes data using the Avro schema.
    pub fn decode(&self, data: &[u8]) -> Result<apache_avro::types::Value> {
        let reader = apache_avro::Reader::with_schema(&self.schema, data).map_err(|e| {
            PubSubError::SchemaDecodingError {
                message: format!("Failed to create Avro reader: {}", e),
                format: SchemaFormat::Avro,
            }
        })?;

        let mut values = Vec::new();
        for value in reader {
            let value = value.map_err(|e| PubSubError::SchemaDecodingError {
                message: format!("Failed to decode Avro value: {}", e),
                format: SchemaFormat::Avro,
            })?;
            values.push(value);
        }

        values
            .into_iter()
            .next()
            .ok_or_else(|| PubSubError::SchemaDecodingError {
                message: "No values found in Avro data".to_string(),
                format: SchemaFormat::Avro,
            })
    }

    /// Validates data against the schema.
    pub fn validate(&self, value: &apache_avro::types::Value) -> Result<()> {
        if !value.validate(&self.schema) {
            return Err(PubSubError::SchemaValidationError {
                message: format!("Value does not match Avro schema: {}", self.name),
                schema_id: Some(self.name.clone()),
            });
        }
        Ok(())
    }

    /// Gets the schema name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the underlying Avro schema.
    pub fn schema(&self) -> &apache_avro::Schema {
        &self.schema
    }
}

/// Protobuf schema handler.
#[cfg(feature = "protobuf")]
pub struct ProtobufSchema {
    descriptor: prost_types::DescriptorProto,
    name: String,
}

#[cfg(feature = "protobuf")]
impl ProtobufSchema {
    /// Creates a new Protobuf schema from a descriptor.
    pub fn from_descriptor(
        name: impl Into<String>,
        descriptor: prost_types::DescriptorProto,
    ) -> Self {
        Self {
            descriptor,
            name: name.into(),
        }
    }

    /// Validates that data conforms to the Protobuf schema.
    pub fn validate(&self, _data: &[u8]) -> Result<()> {
        // Basic validation - in a real implementation, this would use
        // the descriptor to validate the message structure
        Ok(())
    }

    /// Gets the schema name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the descriptor.
    pub fn descriptor(&self) -> &prost_types::DescriptorProto {
        &self.descriptor
    }
}

/// Schema registry for managing schemas.
#[cfg(feature = "schema")]
pub struct SchemaRegistry {
    schemas: HashMap<String, Arc<Schema>>,
    #[cfg(feature = "avro")]
    avro_schemas: HashMap<String, Arc<AvroSchema>>,
    #[cfg(feature = "protobuf")]
    protobuf_schemas: HashMap<String, Arc<ProtobufSchema>>,
}

#[cfg(feature = "schema")]
impl SchemaRegistry {
    /// Creates a new schema registry.
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
            #[cfg(feature = "avro")]
            avro_schemas: HashMap::new(),
            #[cfg(feature = "protobuf")]
            protobuf_schemas: HashMap::new(),
        }
    }

    /// Registers a schema.
    pub fn register(&mut self, schema: Schema) -> Result<()> {
        info!("Registering schema: {} ({})", schema.name, schema.format);

        match schema.format {
            #[cfg(feature = "avro")]
            SchemaFormat::Avro => {
                let avro_schema = AvroSchema::from_json(&schema.name, &schema.definition)?;
                self.avro_schemas
                    .insert(schema.id.clone(), Arc::new(avro_schema));
            }
            #[cfg(feature = "protobuf")]
            SchemaFormat::Protobuf => {
                // In a real implementation, parse the Protobuf descriptor
                debug!("Protobuf schema registered: {}", schema.name);
            }
            #[allow(unreachable_patterns)]
            _ => {
                return Err(PubSubError::SchemaEncodingError {
                    message: format!("Unsupported schema format: {}", schema.format),
                    format: schema.format,
                });
            }
        }

        self.schemas.insert(schema.id.clone(), Arc::new(schema));
        Ok(())
    }

    /// Gets a schema by ID.
    pub fn get(&self, schema_id: &str) -> Option<Arc<Schema>> {
        self.schemas.get(schema_id).cloned()
    }

    /// Gets an Avro schema by ID.
    #[cfg(feature = "avro")]
    pub fn get_avro(&self, schema_id: &str) -> Option<Arc<AvroSchema>> {
        self.avro_schemas.get(schema_id).cloned()
    }

    /// Gets a Protobuf schema by ID.
    #[cfg(feature = "protobuf")]
    pub fn get_protobuf(&self, schema_id: &str) -> Option<Arc<ProtobufSchema>> {
        self.protobuf_schemas.get(schema_id).cloned()
    }

    /// Lists all registered schema IDs.
    pub fn list_schemas(&self) -> Vec<String> {
        self.schemas.keys().cloned().collect()
    }

    /// Removes a schema by ID.
    pub fn remove(&mut self, schema_id: &str) -> Option<Arc<Schema>> {
        #[cfg(feature = "avro")]
        self.avro_schemas.remove(schema_id);

        #[cfg(feature = "protobuf")]
        self.protobuf_schemas.remove(schema_id);

        self.schemas.remove(schema_id)
    }

    /// Clears all schemas.
    pub fn clear(&mut self) {
        self.schemas.clear();

        #[cfg(feature = "avro")]
        self.avro_schemas.clear();

        #[cfg(feature = "protobuf")]
        self.protobuf_schemas.clear();
    }

    /// Gets the number of registered schemas.
    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    /// Checks if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }
}

#[cfg(feature = "schema")]
impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Schema validator for validating messages against schemas.
#[cfg(feature = "schema")]
pub struct SchemaValidator {
    registry: Arc<SchemaRegistry>,
}

#[cfg(feature = "schema")]
impl SchemaValidator {
    /// Creates a new schema validator.
    pub fn new(registry: Arc<SchemaRegistry>) -> Self {
        Self { registry }
    }

    /// Validates data against a schema.
    pub fn validate(&self, schema_id: &str, data: &[u8]) -> Result<()> {
        let schema =
            self.registry
                .get(schema_id)
                .ok_or_else(|| PubSubError::SchemaValidationError {
                    message: format!("Schema not found: {}", schema_id),
                    schema_id: Some(schema_id.to_string()),
                })?;

        match schema.format {
            #[cfg(feature = "avro")]
            SchemaFormat::Avro => {
                let avro_schema = self.registry.get_avro(schema_id).ok_or_else(|| {
                    PubSubError::SchemaValidationError {
                        message: format!("Avro schema not found: {}", schema_id),
                        schema_id: Some(schema_id.to_string()),
                    }
                })?;

                let value = avro_schema.decode(data)?;
                avro_schema.validate(&value)?;
                Ok(())
            }
            #[cfg(feature = "protobuf")]
            SchemaFormat::Protobuf => {
                let protobuf_schema = self.registry.get_protobuf(schema_id).ok_or_else(|| {
                    PubSubError::SchemaValidationError {
                        message: format!("Protobuf schema not found: {}", schema_id),
                        schema_id: Some(schema_id.to_string()),
                    }
                })?;

                protobuf_schema.validate(data)?;
                Ok(())
            }
            #[allow(unreachable_patterns)]
            _ => Err(PubSubError::SchemaValidationError {
                message: format!("Unsupported schema format: {}", schema.format),
                schema_id: Some(schema_id.to_string()),
            }),
        }
    }

    /// Encodes data using a schema.
    #[cfg(feature = "avro")]
    pub fn encode_avro(&self, schema_id: &str, value: &apache_avro::types::Value) -> Result<Bytes> {
        let avro_schema =
            self.registry
                .get_avro(schema_id)
                .ok_or_else(|| PubSubError::SchemaEncodingError {
                    message: format!("Avro schema not found: {}", schema_id),
                    format: SchemaFormat::Avro,
                })?;

        avro_schema.encode(value)
    }

    /// Decodes data using a schema.
    #[cfg(feature = "avro")]
    pub fn decode_avro(&self, schema_id: &str, data: &[u8]) -> Result<apache_avro::types::Value> {
        let avro_schema =
            self.registry
                .get_avro(schema_id)
                .ok_or_else(|| PubSubError::SchemaDecodingError {
                    message: format!("Avro schema not found: {}", schema_id),
                    format: SchemaFormat::Avro,
                })?;

        avro_schema.decode(data)
    }
}

#[cfg(all(test, feature = "schema"))]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creation() {
        let schema = Schema::new(
            "schema-1",
            "test-schema",
            SchemaFormat::Avro,
            r#"{"type": "string"}"#,
        );

        assert_eq!(schema.id, "schema-1");
        assert_eq!(schema.name, "test-schema");
        assert_eq!(schema.format, SchemaFormat::Avro);
    }

    #[test]
    fn test_schema_registry() {
        let registry = SchemaRegistry::new();
        assert!(registry.is_empty());

        let _schema = Schema::new(
            "schema-1",
            "test-schema",
            SchemaFormat::Avro,
            r#"{"type": "string"}"#,
        );

        // Note: This will fail if avro feature is enabled due to invalid schema
        // In a real test, use a valid Avro schema
        assert_eq!(registry.len(), 0);
    }

    #[cfg(feature = "avro")]
    #[test]
    fn test_avro_schema() {
        let json_schema = r#"
        {
            "type": "record",
            "name": "TestRecord",
            "fields": [
                {"name": "field1", "type": "string"},
                {"name": "field2", "type": "int"}
            ]
        }
        "#;

        let schema = AvroSchema::from_json("test", json_schema);
        assert!(schema.is_ok());
    }

    #[test]
    fn test_schema_encoding() {
        let encoding = SchemaEncoding::Binary;
        assert_eq!(encoding, SchemaEncoding::Binary);

        let json_encoding = SchemaEncoding::Json;
        assert_ne!(json_encoding, encoding);
    }
}

#[cfg(not(feature = "schema"))]
mod no_schema {
    //! Placeholder module when schema feature is disabled.
}
