//! Schema types and versions

use serde::{Deserialize, Serialize};

/// Schema type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaType {
    /// Avro schema
    Avro,
    /// JSON schema
    Json,
    /// Protobuf schema
    Protobuf,
}

/// Schema version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaVersion {
    /// Latest version
    Latest,
    /// Specific version
    Version(i32),
}

/// Schema definition
#[derive(Debug, Clone)]
pub struct Schema {
    /// Schema ID
    pub id: i32,
    /// Schema string
    pub schema: String,
    /// Schema type
    pub schema_type: SchemaType,
    /// Schema version (if available)
    pub version: Option<i32>,
    /// Subject name (if available)
    pub subject: Option<String>,
}

impl Schema {
    /// Create a new schema
    pub fn new(id: i32, schema: String) -> Self {
        Self {
            id,
            schema,
            schema_type: SchemaType::Avro,
            version: None,
            subject: None,
        }
    }

    /// Set version
    pub fn with_version(mut self, version: i32) -> Self {
        self.version = Some(version);
        self
    }

    /// Set subject
    pub fn with_subject(mut self, subject: String) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Set schema type
    pub fn with_type(mut self, schema_type: SchemaType) -> Self {
        self.schema_type = schema_type;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creation() {
        let schema = Schema::new(1, r#"{"type": "record"}"#.to_string())
            .with_version(1)
            .with_subject("test-value".to_string());

        assert_eq!(schema.id, 1);
        assert_eq!(schema.version, Some(1));
        assert_eq!(schema.subject, Some("test-value".to_string()));
    }

    #[test]
    fn test_schema_version() {
        assert_eq!(SchemaVersion::Latest, SchemaVersion::Latest);
        assert_eq!(SchemaVersion::Version(1), SchemaVersion::Version(1));
        assert_ne!(SchemaVersion::Latest, SchemaVersion::Version(1));
    }
}
