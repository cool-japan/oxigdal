//! Property schema inference for GeoJSON feature collections.
//!
//! Scans [`GeoJsonFeature`] property maps and detects field types and
//! nullability, accumulating per-field statistics across all features.

use crate::parser::FeatureCollection;
use crate::types::GeoJsonFeature;
use serde_json::Value;
use std::collections::HashMap;

// ─── InferredType ─────────────────────────────────────────────────────────────

/// The inferred type of a property field across all features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferredType {
    /// All observed values were JSON strings.
    String,
    /// All observed values were JSON numbers (integer or float).
    Number,
    /// All observed values were JSON integers (no fractional part, fits i64 or u64).
    Integer,
    /// All observed values were JSON booleans.
    Boolean,
    /// Field was always absent or null (never had a non-null value).
    Null,
    /// Field had JSON array values.
    Array,
    /// Field had JSON object values.
    Object,
    /// Field had values of more than one type.
    Mixed,
}

impl InferredType {
    /// Merge `self` with a new observation. Returns the combined type.
    ///
    /// Merge rules:
    /// - Same type → same type.
    /// - `Integer` + `Number` → `Number` (Integer is a subtype of Number).
    /// - `Null` + anything → the other type (nullable but typed).
    /// - Anything else → `Mixed`.
    fn merge(&self, other: &InferredType) -> InferredType {
        if self == other {
            return self.clone();
        }
        // Integer + Number → Number (Integer is a subtype)
        if (*self == InferredType::Integer && *other == InferredType::Number)
            || (*self == InferredType::Number && *other == InferredType::Integer)
        {
            return InferredType::Number;
        }
        // Null + anything → the anything (nullable but typed)
        if *self == InferredType::Null {
            return other.clone();
        }
        if *other == InferredType::Null {
            return self.clone();
        }
        // Otherwise: Mixed
        InferredType::Mixed
    }
}

/// Infer the type from a single [`serde_json::Value`].
fn infer_value_type(v: &Value) -> InferredType {
    match v {
        Value::Null => InferredType::Null,
        Value::Bool(_) => InferredType::Boolean,
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                InferredType::Integer
            } else {
                InferredType::Number
            }
        }
        Value::String(_) => InferredType::String,
        Value::Array(_) => InferredType::Array,
        Value::Object(_) => InferredType::Object,
    }
}

// ─── FieldSchema ─────────────────────────────────────────────────────────────

/// Per-field statistics gathered during schema inference.
#[derive(Debug, Clone)]
pub struct FieldSchema {
    /// Inferred type across all features that had this field.
    pub inferred_type: InferredType,
    /// True if any feature had this field absent or null.
    pub nullable: bool,
    /// Number of features where this field was present and non-null.
    pub non_null_count: usize,
    /// Total number of features scanned.
    pub total_count: usize,
    /// Min string length observed (if type is String or Mixed-with-string).
    pub min_string_len: Option<usize>,
    /// Max string length observed.
    pub max_string_len: Option<usize>,
    /// Min numeric value observed (if type is Number or Integer).
    pub min_numeric: Option<f64>,
    /// Max numeric value observed.
    pub max_numeric: Option<f64>,
}

impl FieldSchema {
    /// Create a fresh field schema with the given total feature count.
    fn new(total_count: usize) -> Self {
        Self {
            inferred_type: InferredType::Null,
            nullable: false,
            non_null_count: 0,
            total_count,
            min_string_len: None,
            max_string_len: None,
            min_numeric: None,
            max_numeric: None,
        }
    }

    /// Update this schema with a new value observation.
    fn observe(&mut self, value: &Value) {
        match value {
            Value::Null => {
                self.nullable = true;
            }
            v => {
                let ty = infer_value_type(v);
                self.inferred_type = self.inferred_type.merge(&ty);
                self.non_null_count += 1;

                // Update string stats
                if let Value::String(s) = v {
                    let len = s.len();
                    self.min_string_len = Some(self.min_string_len.map_or(len, |m| m.min(len)));
                    self.max_string_len = Some(self.max_string_len.map_or(len, |m| m.max(len)));
                }

                // Update numeric stats
                if let Value::Number(n) = v
                    && let Some(f) = n.as_f64()
                {
                    self.min_numeric = Some(self.min_numeric.map_or(f, |m: f64| m.min(f)));
                    self.max_numeric = Some(self.max_numeric.map_or(f, |m: f64| m.max(f)));
                }
            }
        }
    }

    /// Fraction of features where this field is non-null (`0.0` – `1.0`).
    ///
    /// Returns `0.0` when `total_count` is zero.
    #[must_use]
    pub fn fill_rate(&self) -> f64 {
        if self.total_count == 0 {
            0.0
        } else {
            self.non_null_count as f64 / self.total_count as f64
        }
    }
}

// ─── FeatureSchema ────────────────────────────────────────────────────────────

/// Schema inferred from a collection of GeoJSON features.
#[derive(Debug, Clone)]
pub struct FeatureSchema {
    /// Per-field type information. Key = property name.
    pub fields: HashMap<String, FieldSchema>,
    /// Total number of features scanned.
    pub feature_count: usize,
    /// Number of features with a null or missing `properties` object.
    pub null_properties_count: usize,
}

impl FeatureSchema {
    /// Returns field names in sorted order.
    #[must_use]
    pub fn field_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.fields.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Returns `true` if every field is non-null in every feature.
    ///
    /// Specifically, returns `true` when no field is nullable and every
    /// field's `fill_rate` equals `1.0`.
    #[must_use]
    pub fn is_fully_populated(&self) -> bool {
        self.fields
            .values()
            .all(|f| !f.nullable && (f.fill_rate() - 1.0).abs() < f64::EPSILON)
    }

    /// Returns fields that are present in fewer than `threshold` fraction of
    /// features (i.e. `fill_rate < threshold`).
    ///
    /// # Arguments
    ///
    /// * `threshold` – A value in `[0.0, 1.0]`. Fields whose fill rate is
    ///   strictly below this threshold are returned.
    #[must_use]
    pub fn sparse_fields(&self, threshold: f64) -> Vec<&str> {
        let mut result: Vec<&str> = self
            .fields
            .iter()
            .filter(|(_, f)| f.fill_rate() < threshold)
            .map(|(k, _)| k.as_str())
            .collect();
        result.sort_unstable();
        result
    }
}

// ─── Schema inference functions ───────────────────────────────────────────────

/// Extract a property map from a feature's `properties` field.
///
/// GeoJSON properties are stored as `Option<serde_json::Value>` where the
/// inner `Value` is expected to be a `Value::Object`. This helper downcasts
/// the value to `&serde_json::Map<String, Value>`, returning `None` for null
/// or non-object properties.
fn properties_as_map(props: &Value) -> Option<&serde_json::Map<String, Value>> {
    props.as_object()
}

/// Infer schema from an iterator of [`GeoJsonFeature`] references.
///
/// The iterator is collected into a `Vec` first so that the total feature
/// count is known before any per-field statistics are recorded.  For slices
/// where the count is already known, prefer [`infer_schema_slice`].
pub fn infer_schema<'a>(features: impl IntoIterator<Item = &'a GeoJsonFeature>) -> FeatureSchema {
    let features: Vec<&GeoJsonFeature> = features.into_iter().collect();
    infer_schema_slice(&features)
}

/// Infer schema from a slice of [`GeoJsonFeature`] references (single pass).
///
/// The total feature count is known upfront, so only one pass over the
/// slice is required.
pub fn infer_schema_slice(features: &[&GeoJsonFeature]) -> FeatureSchema {
    let total = features.len();
    let mut fields: HashMap<String, FieldSchema> = HashMap::new();
    let mut null_properties_count = 0usize;

    for feature in features {
        match &feature.properties {
            None => {
                null_properties_count += 1;
            }
            Some(props_val) => match properties_as_map(props_val) {
                None => {
                    // properties is not an object (e.g. it's a JSON null value)
                    null_properties_count += 1;
                }
                Some(props) => {
                    // Observe values for keys that are present in this feature.
                    for (key, value) in props {
                        let entry = fields
                            .entry(key.clone())
                            .or_insert_with(|| FieldSchema::new(total));
                        entry.observe(value);
                    }
                    // Mark absent fields as nullable (feature did not have them).
                    for (key, schema) in &mut fields {
                        if !props.contains_key(key) {
                            schema.nullable = true;
                        }
                    }
                }
            },
        }
    }

    // Ensure total_count is set correctly for all fields (it is set at
    // creation time via `FieldSchema::new(total)` but we reassign for safety).
    for schema in fields.values_mut() {
        schema.total_count = total;
    }

    FeatureSchema {
        fields,
        feature_count: total,
        null_properties_count,
    }
}

/// Infer schema from a [`FeatureCollection`].
///
/// Convenience wrapper around [`infer_schema_slice`].
pub fn infer_schema_from_collection(fc: &FeatureCollection) -> FeatureSchema {
    let refs: Vec<&GeoJsonFeature> = fc.features.iter().collect();
    infer_schema_slice(&refs)
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inferred_type_merge_same() {
        assert_eq!(
            InferredType::String.merge(&InferredType::String),
            InferredType::String
        );
        assert_eq!(
            InferredType::Integer.merge(&InferredType::Integer),
            InferredType::Integer
        );
    }

    #[test]
    fn test_inferred_type_integer_number_coercion() {
        assert_eq!(
            InferredType::Integer.merge(&InferredType::Number),
            InferredType::Number
        );
        assert_eq!(
            InferredType::Number.merge(&InferredType::Integer),
            InferredType::Number
        );
    }

    #[test]
    fn test_inferred_type_null_promotion() {
        assert_eq!(
            InferredType::Null.merge(&InferredType::String),
            InferredType::String
        );
        assert_eq!(
            InferredType::Boolean.merge(&InferredType::Null),
            InferredType::Boolean
        );
    }

    #[test]
    fn test_inferred_type_mixed() {
        assert_eq!(
            InferredType::String.merge(&InferredType::Number),
            InferredType::Mixed
        );
        assert_eq!(
            InferredType::Boolean.merge(&InferredType::Integer),
            InferredType::Mixed
        );
    }

    #[test]
    fn test_field_schema_fill_rate_zero_total() {
        let s = FieldSchema::new(0);
        assert!((s.fill_rate() - 0.0).abs() < f64::EPSILON);
    }
}
