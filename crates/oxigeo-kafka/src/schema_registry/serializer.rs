//! Avro serialization and deserialization

use crate::error::{Error, Result};
use crate::schema_registry::client::SchemaRegistryClient;
use apache_avro::to_avro_datum;
use bytes::{BufMut, Bytes, BytesMut};
use std::sync::Arc;

/// Magic byte for Confluent wire format
const MAGIC_BYTE: u8 = 0;

/// Avro serializer with schema registry integration
pub struct AvroSerializer {
    client: Arc<SchemaRegistryClient>,
}

impl AvroSerializer {
    /// Create a new Avro serializer
    pub fn new(client: Arc<SchemaRegistryClient>) -> Self {
        Self { client }
    }

    /// Serialize a value with schema
    pub async fn serialize<T: serde::Serialize>(&self, subject: &str, value: &T) -> Result<Bytes> {
        // Convert value to Avro Value
        let avro_value = serde_json::to_value(value)
            .map_err(|e| Error::Serialization(format!("Failed to convert to JSON: {}", e)))?;

        // Generate schema from value
        let schema_str = Self::generate_schema_from_value(&avro_value)?;

        // Register or get schema ID
        let schema_id = self.client.register_schema(subject, &schema_str).await?;

        // Parse schema
        let schema = apache_avro::Schema::parse_str(&schema_str)
            .map_err(|e| Error::Serialization(format!("Failed to parse schema: {}", e)))?;

        // Convert JSON value to Avro value
        let avro_value = apache_avro::to_value(&avro_value)
            .map_err(|e| Error::Serialization(format!("Failed to convert to Avro: {}", e)))?;

        let avro_value = avro_value
            .resolve(&schema)
            .map_err(|e| Error::Serialization(format!("Failed to resolve Avro schema: {}", e)))?;

        // Serialize with Confluent wire format
        self.encode_with_schema_id(schema_id, &schema, &avro_value)
    }

    /// Encode value with schema ID in Confluent wire format
    fn encode_with_schema_id(
        &self,
        schema_id: i32,
        schema: &apache_avro::Schema,
        value: &apache_avro::types::Value,
    ) -> Result<Bytes> {
        // Create Avro writer
        let mut buf = BytesMut::new();

        // Write magic byte
        buf.put_u8(MAGIC_BYTE);

        // Write schema ID (big-endian)
        buf.put_i32(schema_id);

        // Serialize value
        let encoded = to_avro_datum(schema, value.clone())
            .map_err(|e| Error::Serialization(format!("Failed to encode Avro: {}", e)))?;

        buf.put_slice(&encoded);

        Ok(buf.freeze())
    }

    /// Generate Avro schema from JSON value
    fn generate_schema_from_value(value: &serde_json::Value) -> Result<String> {
        let schema = match value {
            serde_json::Value::Null => r#"{"type": "null"}"#.to_string(),
            serde_json::Value::Bool(_) => r#"{"type": "boolean"}"#.to_string(),
            serde_json::Value::Number(n) => {
                if n.is_i64() {
                    r#"{"type": "long"}"#.to_string()
                } else if n.is_f64() {
                    r#"{"type": "double"}"#.to_string()
                } else {
                    r#"{"type": "int"}"#.to_string()
                }
            }
            serde_json::Value::String(_) => r#"{"type": "string"}"#.to_string(),
            serde_json::Value::Array(_) => r#"{"type": "array", "items": "string"}"#.to_string(),
            serde_json::Value::Object(_) => {
                // For objects, we need to generate a record schema
                // This is a simplified version - in production, use proper schema inference
                r#"{"type": "string"}"#.to_string()
            }
        };

        Ok(schema)
    }
}

/// Avro deserializer with schema registry integration
pub struct AvroDeserializer {
    client: Arc<SchemaRegistryClient>,
}

impl AvroDeserializer {
    /// Create a new Avro deserializer
    pub fn new(client: Arc<SchemaRegistryClient>) -> Self {
        Self { client }
    }

    /// Deserialize a value
    pub async fn deserialize<T: serde::de::DeserializeOwned>(&self, data: &[u8]) -> Result<T> {
        if data.is_empty() {
            return Err(Error::Deserialization("Empty data".to_string()));
        }

        // Check magic byte
        if data[0] != MAGIC_BYTE {
            return Err(Error::Deserialization(format!(
                "Invalid magic byte: expected {}, got {}",
                MAGIC_BYTE, data[0]
            )));
        }

        if data.len() < 5 {
            return Err(Error::Deserialization(
                "Data too short for schema ID".to_string(),
            ));
        }

        // Read schema ID (big-endian)
        let schema_id = i32::from_be_bytes([data[1], data[2], data[3], data[4]]);

        // Get schema from registry
        let schema_def = self.client.get_schema_by_id(schema_id).await?;

        // Parse schema
        let schema = apache_avro::Schema::parse_str(&schema_def.schema)
            .map_err(|e| Error::Deserialization(format!("Failed to parse schema: {}", e)))?;

        // Deserialize Avro data
        let avro_data = &data[5..];
        let avro_value = apache_avro::from_avro_datum(&schema, &mut &avro_data[..], None)
            .map_err(|e| Error::Deserialization(format!("Failed to decode Avro: {}", e)))?;

        // Convert Avro value to target type directly
        apache_avro::from_value(&avro_value)
            .map_err(|e| Error::Deserialization(format!("Failed to deserialize Avro value: {}", e)))
    }

    /// Extract schema ID from data without deserializing
    pub fn extract_schema_id(data: &[u8]) -> Result<i32> {
        if data.is_empty() {
            return Err(Error::Deserialization("Empty data".to_string()));
        }

        if data[0] != MAGIC_BYTE {
            return Err(Error::Deserialization(format!(
                "Invalid magic byte: expected {}, got {}",
                MAGIC_BYTE, data[0]
            )));
        }

        if data.len() < 5 {
            return Err(Error::Deserialization(
                "Data too short for schema ID".to_string(),
            ));
        }

        let schema_id = i32::from_be_bytes([data[1], data[2], data[3], data[4]]);
        Ok(schema_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_schema_id() {
        let data = vec![MAGIC_BYTE, 0, 0, 0, 42, 1, 2, 3]; // Schema ID = 42
        let schema_id = AvroDeserializer::extract_schema_id(&data);
        assert_eq!(schema_id.ok(), Some(42));
    }

    #[test]
    fn test_extract_schema_id_invalid_magic() {
        let data = vec![1, 0, 0, 0, 42]; // Invalid magic byte
        let schema_id = AvroDeserializer::extract_schema_id(&data);
        assert!(schema_id.is_err());
    }

    #[test]
    fn test_extract_schema_id_too_short() {
        let data = vec![MAGIC_BYTE, 0, 0]; // Too short
        let schema_id = AvroDeserializer::extract_schema_id(&data);
        assert!(schema_id.is_err());
    }

    #[test]
    fn test_generate_schema_from_value() {
        let value = serde_json::json!("test");
        let schema = AvroSerializer::generate_schema_from_value(&value);
        assert!(schema.is_ok());
        if let Ok(schema_str) = schema {
            assert!(schema_str.contains("string"));
        }
    }
}
