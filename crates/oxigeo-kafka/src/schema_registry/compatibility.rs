//! Schema compatibility checking

use serde::{Deserialize, Serialize};

/// Compatibility level for schema evolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatibilityLevel {
    /// No compatibility checking
    None,
    /// Backward compatibility - new schema can read old data
    Backward,
    /// Forward compatibility - old schema can read new data
    Forward,
    /// Full compatibility - both backward and forward
    Full,
    /// Backward transitive - backward compatibility with all previous versions
    BackwardTransitive,
    /// Forward transitive - forward compatibility with all previous versions
    ForwardTransitive,
    /// Full transitive - full compatibility with all previous versions
    FullTransitive,
}

impl CompatibilityLevel {
    /// Convert to schema registry string
    pub fn to_string(&self) -> &'static str {
        match self {
            CompatibilityLevel::None => "NONE",
            CompatibilityLevel::Backward => "BACKWARD",
            CompatibilityLevel::Forward => "FORWARD",
            CompatibilityLevel::Full => "FULL",
            CompatibilityLevel::BackwardTransitive => "BACKWARD_TRANSITIVE",
            CompatibilityLevel::ForwardTransitive => "FORWARD_TRANSITIVE",
            CompatibilityLevel::FullTransitive => "FULL_TRANSITIVE",
        }
    }
}

/// Compatibility checker for Avro schemas
pub struct CompatibilityChecker;

impl CompatibilityChecker {
    /// Check if two schemas are backward compatible
    /// New schema can read data written with old schema
    pub fn check_backward(old_schema: &str, new_schema: &str) -> bool {
        // Parse both schemas
        let old = match apache_avro::Schema::parse_str(old_schema) {
            Ok(s) => s,
            Err(_) => return false,
        };

        let new = match apache_avro::Schema::parse_str(new_schema) {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Check if schemas can be resolved
        Self::schemas_compatible(&old, &new)
    }

    /// Check if two schemas are forward compatible
    /// Old schema can read data written with new schema
    pub fn check_forward(old_schema: &str, new_schema: &str) -> bool {
        Self::check_backward(new_schema, old_schema)
    }

    /// Check if two schemas are fully compatible
    pub fn check_full(old_schema: &str, new_schema: &str) -> bool {
        Self::check_backward(old_schema, new_schema) && Self::check_forward(old_schema, new_schema)
    }

    /// Check if two Avro schemas are compatible
    fn schemas_compatible(
        writer_schema: &apache_avro::Schema,
        reader_schema: &apache_avro::Schema,
    ) -> bool {
        use apache_avro::Schema;

        match (writer_schema, reader_schema) {
            // Same type compatibility
            (Schema::Null, Schema::Null) => true,
            (Schema::Boolean, Schema::Boolean) => true,
            (Schema::Int, Schema::Int) => true,
            (Schema::Long, Schema::Long) => true,
            (Schema::Float, Schema::Float) => true,
            (Schema::Double, Schema::Double) => true,
            (Schema::Bytes, Schema::Bytes) => true,
            (Schema::String, Schema::String) => true,

            // Type promotions
            (Schema::Int, Schema::Long) => true,
            (Schema::Int, Schema::Float) => true,
            (Schema::Int, Schema::Double) => true,
            (Schema::Long, Schema::Float) => true,
            (Schema::Long, Schema::Double) => true,
            (Schema::Float, Schema::Double) => true,
            (Schema::String, Schema::Bytes) => true,
            (Schema::Bytes, Schema::String) => true,

            // Union compatibility
            (_, Schema::Union(_)) => true, // Any schema can be promoted to union

            // Arrays and maps
            (Schema::Array(w_array), Schema::Array(r_array)) => {
                Self::schemas_compatible(&w_array.items, &r_array.items)
            }
            (Schema::Map(w_map), Schema::Map(r_map)) => {
                Self::schemas_compatible(&w_map.types, &r_map.types)
            }

            // Records - field-by-field compatibility
            (Schema::Record(w_record), Schema::Record(r_record)) => {
                // Names should match
                if w_record.name.name != r_record.name.name {
                    return false;
                }

                // Check all reader fields exist in writer or have defaults
                for r_field in &r_record.fields {
                    let w_field = w_record.fields.iter().find(|f| f.name == r_field.name);

                    match w_field {
                        Some(wf) => {
                            if !Self::schemas_compatible(&wf.schema, &r_field.schema) {
                                return false;
                            }
                        }
                        None => {
                            // Reader has field that writer doesn't - must have default
                            if r_field.default.is_none() {
                                return false;
                            }
                        }
                    }
                }

                true
            }

            // Enums - symbols should be compatible
            (Schema::Enum(w_enum), Schema::Enum(r_enum)) => {
                w_enum.name.name == r_enum.name.name
                    && w_enum.symbols.iter().all(|s| r_enum.symbols.contains(s))
            }

            // Fixed - sizes must match
            (Schema::Fixed(w_fixed), Schema::Fixed(r_fixed)) => {
                w_fixed.name.name == r_fixed.name.name && w_fixed.size == r_fixed.size
            }

            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compatibility_level_to_string() {
        assert_eq!(CompatibilityLevel::None.to_string(), "NONE");
        assert_eq!(CompatibilityLevel::Backward.to_string(), "BACKWARD");
        assert_eq!(CompatibilityLevel::Forward.to_string(), "FORWARD");
        assert_eq!(CompatibilityLevel::Full.to_string(), "FULL");
    }

    #[test]
    fn test_backward_compatibility_same_schema() {
        let schema = r#"{"type": "string"}"#;
        assert!(CompatibilityChecker::check_backward(schema, schema));
    }

    #[test]
    fn test_backward_compatibility_type_promotion() {
        let old_schema = r#"{"type": "int"}"#;
        let new_schema = r#"{"type": "long"}"#;
        assert!(CompatibilityChecker::check_backward(old_schema, new_schema));
    }

    #[test]
    fn test_forward_compatibility() {
        let old_schema = r#"{"type": "long"}"#;
        let new_schema = r#"{"type": "int"}"#;
        assert!(CompatibilityChecker::check_forward(old_schema, new_schema));
    }

    #[test]
    fn test_full_compatibility() {
        let schema1 = r#"{"type": "string"}"#;
        let schema2 = r#"{"type": "string"}"#;
        assert!(CompatibilityChecker::check_full(schema1, schema2));
    }

    #[test]
    fn test_record_backward_compatibility_with_default() {
        let old_schema = r#"
        {
            "type": "record",
            "name": "Test",
            "fields": [
                {"name": "field1", "type": "string"}
            ]
        }
        "#;

        let new_schema = r#"
        {
            "type": "record",
            "name": "Test",
            "fields": [
                {"name": "field1", "type": "string"},
                {"name": "field2", "type": "int", "default": 0}
            ]
        }
        "#;

        assert!(CompatibilityChecker::check_backward(old_schema, new_schema));
    }
}
