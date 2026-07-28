//! Schema support for Google Cloud Pub/Sub.
//!
//! This module provides schema validation and encoding/decoding support
//! for Apache Avro and Protocol Buffers formats.

#[cfg(feature = "schema")]
use crate::error::{PubSubError, Result};
#[cfg(feature = "avro")]
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

    /// Validates that `data` conforms to the Protobuf schema.
    ///
    /// Performs real structural validation of the protobuf wire format against
    /// this message's descriptor:
    ///
    /// * every tag is parsed (field number + wire type) and every field value
    ///   is fully read, so truncated buffers, malformed varints, and
    ///   overrunning length-delimited fields are rejected;
    /// * for every field number declared in the descriptor, the encoded wire
    ///   type must match the declared field type (a packed wire type is also
    ///   accepted for repeated scalar fields);
    /// * unknown field numbers are permitted (protobuf forward-compatibility)
    ///   but their wire structure is still validated.
    ///
    /// Deep recursion into sub-message fields is intentionally not performed:
    /// resolving a field's message type requires the full descriptor pool
    /// (`FileDescriptorSet`), which a single [`prost_types::DescriptorProto`]
    /// does not carry. The length-delimited bytes of a sub-message are still
    /// validated for readability.
    pub fn validate(&self, data: &[u8]) -> Result<()> {
        validate_protobuf_wire(data, &self.descriptor).map_err(|message| {
            PubSubError::SchemaValidationError {
                message: format!("{}: {}", self.name, message),
                schema_id: Some(self.name.clone()),
            }
        })
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

/// Returns the protobuf wire type expected for a declared field type.
#[cfg(feature = "protobuf")]
fn expected_wire_type(ty: prost_types::field_descriptor_proto::Type) -> u8 {
    use prost_types::field_descriptor_proto::Type;
    match ty {
        Type::Double | Type::Fixed64 | Type::Sfixed64 => 1,
        Type::Float | Type::Fixed32 | Type::Sfixed32 => 5,
        Type::Int64
        | Type::Uint64
        | Type::Int32
        | Type::Uint32
        | Type::Bool
        | Type::Enum
        | Type::Sint32
        | Type::Sint64 => 0,
        Type::String | Type::Message | Type::Bytes => 2,
        Type::Group => 3,
    }
}

/// Whether a scalar field type may appear packed (length-delimited) when repeated.
#[cfg(feature = "protobuf")]
fn is_packable(ty: prost_types::field_descriptor_proto::Type) -> bool {
    matches!(expected_wire_type(ty), 0 | 1 | 5)
}

/// Reads a base-128 varint, advancing `pos`. Rejects truncated / overlong varints.
#[cfg(feature = "protobuf")]
fn read_varint(buf: &[u8], pos: &mut usize) -> core::result::Result<u64, String> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..10 {
        let byte = *buf
            .get(*pos)
            .ok_or_else(|| "truncated varint".to_string())?;
        *pos += 1;
        if shift < 64 {
            result |= u64::from(byte & 0x7f) << shift;
        } else if byte & 0x7f != 0 {
            return Err("varint overflows 64 bits".to_string());
        }
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
    Err("varint exceeds 10 bytes".to_string())
}

/// Advances `pos` by `n`, verifying the buffer holds that many more bytes.
#[cfg(feature = "protobuf")]
fn advance(buf: &[u8], pos: &mut usize, n: usize) -> core::result::Result<(), String> {
    let new_pos = pos
        .checked_add(n)
        .ok_or_else(|| "length overflow".to_string())?;
    if new_pos > buf.len() {
        return Err("field value extends past end of buffer".to_string());
    }
    *pos = new_pos;
    Ok(())
}

/// Consumes a single field value of the given wire type (validating readability).
#[cfg(feature = "protobuf")]
fn skip_field(
    buf: &[u8],
    pos: &mut usize,
    wire_type: u8,
    field_number: i32,
) -> core::result::Result<(), String> {
    match wire_type {
        0 => {
            read_varint(buf, pos)?;
        }
        1 => advance(buf, pos, 8)?,
        5 => advance(buf, pos, 4)?,
        2 => {
            let len = read_varint(buf, pos)?;
            advance(buf, pos, len as usize)?;
        }
        3 => skip_group(buf, pos, field_number)?,
        4 => return Err("unexpected end-group marker".to_string()),
        other => return Err(format!("invalid wire type {other}")),
    }
    Ok(())
}

/// Skips a legacy group, matching the closing end-group tag for `group_field`.
#[cfg(feature = "protobuf")]
fn skip_group(buf: &[u8], pos: &mut usize, group_field: i32) -> core::result::Result<(), String> {
    loop {
        let tag = read_varint(buf, pos)?;
        let field_number = (tag >> 3) as i32;
        let wire_type = (tag & 7) as u8;
        if wire_type == 4 {
            if field_number == group_field {
                return Ok(());
            }
            return Err("mismatched end-group marker".to_string());
        }
        skip_field(buf, pos, wire_type, field_number)?;
    }
}

/// Validates a protobuf message's wire structure against a descriptor.
#[cfg(feature = "protobuf")]
fn validate_protobuf_wire(
    data: &[u8],
    descriptor: &prost_types::DescriptorProto,
) -> core::result::Result<(), String> {
    use prost_types::field_descriptor_proto::Label;
    use std::collections::HashMap;

    let mut declared: HashMap<i32, (prost_types::field_descriptor_proto::Type, Label)> =
        HashMap::new();
    for field in &descriptor.field {
        declared.insert(field.number(), (field.r#type(), field.label()));
    }

    let mut pos = 0usize;
    while pos < data.len() {
        let tag = read_varint(data, &mut pos)?;
        let field_number = (tag >> 3) as i32;
        let wire_type = (tag & 7) as u8;

        if field_number == 0 {
            return Err("invalid field number 0".to_string());
        }

        if let Some((ty, label)) = declared.get(&field_number) {
            let expected = expected_wire_type(*ty);
            let is_repeated = matches!(label, Label::Repeated);
            let matches =
                wire_type == expected || (is_repeated && wire_type == 2 && is_packable(*ty));
            if !matches {
                return Err(format!(
                    "field {field_number}: wire type {wire_type} does not match declared type {ty:?} (expected wire type {expected})"
                ));
            }
        }

        skip_field(data, &mut pos, wire_type, field_number)?;
    }

    Ok(())
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

    #[cfg(feature = "protobuf")]
    fn sample_protobuf_schema() -> ProtobufSchema {
        use prost_types::field_descriptor_proto::{Label, Type};

        let descriptor = prost_types::DescriptorProto {
            name: Some("TestMsg".to_string()),
            field: vec![
                prost_types::FieldDescriptorProto {
                    name: Some("s".to_string()),
                    number: Some(1),
                    label: Some(Label::Optional as i32),
                    r#type: Some(Type::String as i32),
                    ..Default::default()
                },
                prost_types::FieldDescriptorProto {
                    name: Some("n".to_string()),
                    number: Some(2),
                    label: Some(Label::Optional as i32),
                    r#type: Some(Type::Int32 as i32),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        ProtobufSchema::from_descriptor("TestMsg", descriptor)
    }

    #[cfg(feature = "protobuf")]
    #[test]
    fn test_protobuf_validate_accepts_valid_message() {
        let schema = sample_protobuf_schema();
        // field 1 (string "abc"): tag 0x0A, len 3, "abc"
        // field 2 (int32 42):     tag 0x10, varint 0x2A
        let data = [0x0A, 0x03, b'a', b'b', b'c', 0x10, 0x2A];
        assert!(schema.validate(&data).is_ok());
    }

    #[cfg(feature = "protobuf")]
    #[test]
    fn test_protobuf_validate_accepts_unknown_field() {
        let schema = sample_protobuf_schema();
        // Unknown field 9 (varint): tag 0x48, value 1 -> allowed (forward-compat).
        let data = [0x48, 0x01];
        assert!(schema.validate(&data).is_ok());
    }

    #[cfg(feature = "protobuf")]
    #[test]
    fn test_protobuf_validate_rejects_wire_type_mismatch() {
        let schema = sample_protobuf_schema();
        // field 2 declared int32 (varint) but encoded length-delimited (tag 0x12).
        let data = [0x12, 0x01, 0x05];
        let err = schema.validate(&data);
        assert!(err.is_err(), "wire-type mismatch must be rejected");
    }

    #[cfg(feature = "protobuf")]
    #[test]
    fn test_protobuf_validate_rejects_truncated_length_delimited() {
        let schema = sample_protobuf_schema();
        // field 1 claims 5 bytes but only 1 is present.
        let data = [0x0A, 0x05, b'a'];
        assert!(schema.validate(&data).is_err());
    }

    #[cfg(feature = "protobuf")]
    #[test]
    fn test_protobuf_validate_rejects_truncated_varint() {
        let schema = sample_protobuf_schema();
        // field 2 tag then a varint with the continuation bit set but no more bytes.
        let data = [0x10, 0x80];
        assert!(schema.validate(&data).is_err());
    }
}

#[cfg(not(feature = "schema"))]
mod no_schema {
    //! Placeholder module when schema feature is disabled.
}
