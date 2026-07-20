//! Template parameterization system.

use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Parameter type enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParameterType {
    /// String parameter.
    String,
    /// Integer parameter.
    Integer,
    /// Float parameter.
    Float,
    /// Boolean parameter.
    Boolean,
    /// Array parameter.
    Array,
    /// Object parameter.
    Object,
    /// File path parameter.
    FilePath,
    /// URL parameter.
    Url,
    /// Enum parameter (with allowed values).
    Enum {
        /// List of allowed values for this enum.
        allowed_values: Vec<String>,
    },
}

/// Parameter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter name.
    pub name: String,
    /// Parameter type.
    pub param_type: ParameterType,
    /// Parameter description.
    pub description: String,
    /// Whether the parameter is required.
    pub required: bool,
    /// Default value (if not required).
    pub default_value: Option<ParameterValue>,
    /// Parameter constraints.
    pub constraints: Option<ParameterConstraints>,
}

/// Parameter value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterValue {
    /// String value.
    String(String),
    /// Integer value.
    Integer(i64),
    /// Float value.
    Float(f64),
    /// Boolean value.
    Boolean(bool),
    /// Array value.
    Array(Vec<ParameterValue>),
    /// Object value.
    Object(HashMap<String, ParameterValue>),
}

impl ParameterValue {
    /// Convert to JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::String(s) => serde_json::Value::String(s.clone()),
            Self::Integer(i) => serde_json::Value::Number((*i).into()),
            Self::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Self::Boolean(b) => serde_json::Value::Bool(*b),
            Self::Array(arr) => serde_json::Value::Array(arr.iter().map(|v| v.to_json()).collect()),
            Self::Object(obj) => {
                let mut map = serde_json::Map::new();
                for (k, v) in obj {
                    map.insert(k.clone(), v.to_json());
                }
                serde_json::Value::Object(map)
            }
        }
    }

    /// Get as string (if applicable).
    pub fn as_string(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    /// Get as integer (if applicable).
    pub fn as_integer(&self) -> Option<i64> {
        if let Self::Integer(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    /// Get as float (if applicable).
    pub fn as_float(&self) -> Option<f64> {
        if let Self::Float(f) = self {
            Some(*f)
        } else {
            None
        }
    }

    /// Get as boolean (if applicable).
    pub fn as_boolean(&self) -> Option<bool> {
        if let Self::Boolean(b) = self {
            Some(*b)
        } else {
            None
        }
    }
}

/// Parameter constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterConstraints {
    /// Minimum value (for numeric types).
    pub min: Option<f64>,
    /// Maximum value (for numeric types).
    pub max: Option<f64>,
    /// Minimum length (for strings/arrays).
    pub min_length: Option<usize>,
    /// Maximum length (for strings/arrays).
    pub max_length: Option<usize>,
    /// Regex pattern (for strings).
    pub pattern: Option<String>,
}

/// Template parameterizer for applying parameters to templates.
pub struct TemplateParameterizer {
    placeholder_prefix: String,
    placeholder_suffix: String,
}

impl TemplateParameterizer {
    /// Create a new template parameterizer.
    pub fn new() -> Self {
        Self {
            placeholder_prefix: "{{".to_string(),
            placeholder_suffix: "}}".to_string(),
        }
    }

    /// Create a parameterizer with custom placeholder markers.
    pub fn with_markers<S: Into<String>>(prefix: S, suffix: S) -> Self {
        Self {
            placeholder_prefix: prefix.into(),
            placeholder_suffix: suffix.into(),
        }
    }

    /// Apply parameters to a template string.
    pub fn apply_parameters(
        &self,
        template: &str,
        params: &HashMap<String, ParameterValue>,
    ) -> Result<String> {
        let mut result = template.to_string();

        for (name, value) in params {
            let placeholder = format!(
                "{}{}{}",
                self.placeholder_prefix, name, self.placeholder_suffix
            );

            let replacement = match value {
                ParameterValue::String(s) => s.clone(),
                ParameterValue::Integer(i) => i.to_string(),
                ParameterValue::Float(f) => f.to_string(),
                ParameterValue::Boolean(b) => b.to_string(),
                ParameterValue::Array(_) | ParameterValue::Object(_) => {
                    serde_json::to_string(&value.to_json()).map_err(|e| {
                        WorkflowError::template(format!("Failed to serialize value: {}", e))
                    })?
                }
            };

            result = result.replace(&placeholder, &replacement);
        }

        // Check for unreplaced placeholders.
        //
        // We inspect the *source* template rather than the substituted result: a
        // blind substring scan of the output false-positives whenever a parameter
        // value legitimately contains the placeholder markers (e.g. a JSON value
        // with literal `{{`/`}}`), and scanning the result would also mistake such
        // literal braces for a dangling placeholder. Extracting the paired
        // placeholders from the original template and checking them against the
        // supplied parameters detects genuinely missing values without those
        // false positives.
        let missing: Vec<String> = self
            .extract_placeholders(template)
            .into_iter()
            .filter(|name| !params.contains_key(name))
            .collect();
        if !missing.is_empty() {
            return Err(WorkflowError::template(format!(
                "Template contains unreplaced placeholders: {}",
                missing.join(", ")
            )));
        }

        Ok(result)
    }

    /// Extract placeholders from a template.
    pub fn extract_placeholders(&self, template: &str) -> Vec<String> {
        let mut placeholders = Vec::new();
        let mut start_pos = 0;

        while let Some(start) = template[start_pos..].find(&self.placeholder_prefix) {
            let absolute_start = start_pos + start + self.placeholder_prefix.len();

            if let Some(end) = template[absolute_start..].find(&self.placeholder_suffix) {
                let placeholder = template[absolute_start..absolute_start + end].to_string();
                if !placeholders.contains(&placeholder) {
                    placeholders.push(placeholder);
                }
                start_pos = absolute_start + end + self.placeholder_suffix.len();
            } else {
                break;
            }
        }

        placeholders
    }

    /// Validate that all placeholders can be filled.
    pub fn validate_coverage(
        &self,
        template: &str,
        params: &HashMap<String, ParameterValue>,
    ) -> Result<()> {
        let placeholders = self.extract_placeholders(template);

        for placeholder in placeholders {
            if !params.contains_key(&placeholder) {
                return Err(WorkflowError::template(format!(
                    "Missing parameter value for placeholder '{}'",
                    placeholder
                )));
            }
        }

        Ok(())
    }
}

impl Default for TemplateParameterizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_value_conversions() {
        let string_val = ParameterValue::String("test".to_string());
        assert_eq!(string_val.as_string(), Some("test"));

        let int_val = ParameterValue::Integer(42);
        assert_eq!(int_val.as_integer(), Some(42));

        let bool_val = ParameterValue::Boolean(true);
        assert_eq!(bool_val.as_boolean(), Some(true));
    }

    #[test]
    fn test_parameterizer_apply() {
        let parameterizer = TemplateParameterizer::new();
        let template = r#"{"name": "{{workflow_name}}", "version": "{{version}}"}"#;

        let mut params = HashMap::new();
        params.insert(
            "workflow_name".to_string(),
            ParameterValue::String("test-workflow".to_string()),
        );
        params.insert(
            "version".to_string(),
            ParameterValue::String("1.0.0".to_string()),
        );

        let result = parameterizer
            .apply_parameters(template, &params)
            .expect("Failed to apply parameters");

        assert!(result.contains("test-workflow"));
        assert!(result.contains("1.0.0"));
    }

    #[test]
    fn test_extract_placeholders() {
        let parameterizer = TemplateParameterizer::new();
        let template = "Hello {{name}}, your age is {{age}}";

        let placeholders = parameterizer.extract_placeholders(template);

        assert_eq!(placeholders.len(), 2);
        assert!(placeholders.contains(&"name".to_string()));
        assert!(placeholders.contains(&"age".to_string()));
    }

    #[test]
    fn test_validate_coverage() {
        let parameterizer = TemplateParameterizer::new();
        let template = "{{param1}} and {{param2}}";

        let mut params = HashMap::new();
        params.insert(
            "param1".to_string(),
            ParameterValue::String("value1".to_string()),
        );

        // Missing param2
        assert!(parameterizer.validate_coverage(template, &params).is_err());

        params.insert(
            "param2".to_string(),
            ParameterValue::String("value2".to_string()),
        );

        // Now should be ok
        assert!(parameterizer.validate_coverage(template, &params).is_ok());
    }

    #[test]
    fn test_custom_markers() {
        let parameterizer = TemplateParameterizer::with_markers("${", "}");
        let template = "Hello ${name}";

        let mut params = HashMap::new();
        params.insert(
            "name".to_string(),
            ParameterValue::String("World".to_string()),
        );

        let result = parameterizer
            .apply_parameters(template, &params)
            .expect("Failed to apply");

        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_apply_allows_value_with_literal_braces() {
        // A substituted value that itself contains the placeholder markers must
        // not be mistaken for an unreplaced placeholder.
        let parameterizer = TemplateParameterizer::new();
        let template = "value = {{payload}}";

        let mut params = HashMap::new();
        params.insert(
            "payload".to_string(),
            ParameterValue::String("literal {{brace}} text".to_string()),
        );

        let result = parameterizer
            .apply_parameters(template, &params)
            .expect("value containing braces should not be rejected");
        assert_eq!(result, "value = literal {{brace}} text");
    }

    #[test]
    fn test_apply_detects_missing_placeholder() {
        // A genuinely unprovided placeholder must still be reported (by name).
        let parameterizer = TemplateParameterizer::new();
        let template = "{{provided}} and {{missing}}";

        let mut params = HashMap::new();
        params.insert(
            "provided".to_string(),
            ParameterValue::String("ok".to_string()),
        );

        let err = parameterizer
            .apply_parameters(template, &params)
            .expect_err("missing placeholder should error");
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn test_apply_allows_array_value_with_braces() {
        // JSON-encoded array/object values contain literal braces; they must pass.
        let parameterizer = TemplateParameterizer::new();
        let template = r#"{"items": {{items}}}"#;

        let mut params = HashMap::new();
        params.insert(
            "items".to_string(),
            ParameterValue::Array(vec![ParameterValue::Integer(1), ParameterValue::Integer(2)]),
        );

        let result = parameterizer
            .apply_parameters(template, &params)
            .expect("array value should not be rejected");
        assert!(result.contains("[1,2]"));
    }

    #[test]
    fn test_parameter_value_to_json() {
        let value = ParameterValue::Integer(42);
        let json = value.to_json();
        assert_eq!(json, serde_json::json!(42));

        let value = ParameterValue::Boolean(true);
        let json = value.to_json();
        assert_eq!(json, serde_json::json!(true));
    }
}
