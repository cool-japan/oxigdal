//! Template validation utilities.

use crate::error::{Result, WorkflowError};
use crate::templates::{Parameter, ParameterType, ParameterValue, WorkflowTemplate};
use regex::Regex;
use std::collections::HashMap;

/// Template validator for validating templates and parameters.
pub struct TemplateValidator;

impl TemplateValidator {
    /// Create a new template validator.
    pub fn new() -> Self {
        Self
    }

    /// Validate a workflow template.
    pub fn validate_template(&self, template: &WorkflowTemplate) -> Result<()> {
        // Validate basic fields
        if template.id.is_empty() {
            return Err(WorkflowError::validation("Template ID cannot be empty"));
        }

        if template.name.is_empty() {
            return Err(WorkflowError::validation("Template name cannot be empty"));
        }

        if template.version.is_empty() {
            return Err(WorkflowError::validation(
                "Template version cannot be empty",
            ));
        }

        // Validate version format (semantic versioning)
        self.validate_version(&template.version)?;

        // Validate parameters
        for param in &template.parameters {
            self.validate_parameter_definition(param)?;
        }

        // Validate template string is not empty
        if template.workflow_template.is_empty() {
            return Err(WorkflowError::validation(
                "Template workflow string cannot be empty",
            ));
        }

        // Check for duplicate parameter names
        let mut param_names: Vec<&String> = template.parameters.iter().map(|p| &p.name).collect();
        param_names.sort();
        for i in 1..param_names.len() {
            if param_names[i] == param_names[i - 1] {
                return Err(WorkflowError::validation(format!(
                    "Duplicate parameter name: {}",
                    param_names[i]
                )));
            }
        }

        Ok(())
    }

    /// Validate parameter values against parameter definitions.
    pub fn validate_parameters(
        &self,
        definitions: &[Parameter],
        values: &HashMap<String, ParameterValue>,
    ) -> Result<()> {
        // Check required parameters
        for param in definitions {
            if param.required && !values.contains_key(&param.name) {
                return Err(WorkflowError::validation(format!(
                    "Required parameter '{}' is missing",
                    param.name
                )));
            }
        }

        // Validate each provided value
        for (name, value) in values {
            if let Some(param) = definitions.iter().find(|p| p.name == *name) {
                self.validate_parameter_value(param, value)?;
            } else {
                return Err(WorkflowError::validation(format!(
                    "Unknown parameter: {}",
                    name
                )));
            }
        }

        Ok(())
    }

    /// Validate a parameter definition.
    fn validate_parameter_definition(&self, param: &Parameter) -> Result<()> {
        if param.name.is_empty() {
            return Err(WorkflowError::validation("Parameter name cannot be empty"));
        }

        if param.description.is_empty() {
            return Err(WorkflowError::validation(format!(
                "Parameter '{}' must have a description",
                param.name
            )));
        }

        // If not required, must have default value
        if !param.required && param.default_value.is_none() {
            return Err(WorkflowError::validation(format!(
                "Optional parameter '{}' must have a default value",
                param.name
            )));
        }

        // Validate default value matches type
        if let Some(default) = &param.default_value {
            self.validate_parameter_value(param, default)?;
        }

        Ok(())
    }

    /// Validate a parameter value against its definition.
    fn validate_parameter_value(&self, param: &Parameter, value: &ParameterValue) -> Result<()> {
        // Check type compatibility
        match (&param.param_type, value) {
            (ParameterType::String, ParameterValue::String(s)) => {
                self.validate_string_constraints(param, s)?;
            }
            (ParameterType::Integer, ParameterValue::Integer(i)) => {
                self.validate_numeric_constraints(param, *i as f64)?;
            }
            (ParameterType::Float, ParameterValue::Float(f)) => {
                self.validate_numeric_constraints(param, *f)?;
            }
            (ParameterType::Boolean, ParameterValue::Boolean(_)) => {
                // Boolean has no constraints to validate
            }
            (ParameterType::Array, ParameterValue::Array(arr)) => {
                self.validate_array_constraints(param, arr)?;
            }
            (ParameterType::Object, ParameterValue::Object(_)) => {
                // Object validation could be extended
            }
            (ParameterType::FilePath, ParameterValue::String(s)) => {
                self.validate_file_path(s)?;
            }
            (ParameterType::Url, ParameterValue::String(s)) => {
                self.validate_url(s)?;
            }
            (ParameterType::Enum { allowed_values }, ParameterValue::String(s)) => {
                if !allowed_values.contains(s) {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' value '{}' is not in allowed values: {:?}",
                        param.name, s, allowed_values
                    )));
                }
            }
            _ => {
                return Err(WorkflowError::validation(format!(
                    "Parameter '{}' type mismatch: expected {:?}, got incompatible value",
                    param.name, param.param_type
                )));
            }
        }

        Ok(())
    }

    /// Validate string constraints.
    fn validate_string_constraints(&self, param: &Parameter, value: &str) -> Result<()> {
        if let Some(constraints) = &param.constraints {
            if let Some(min_len) = constraints.min_length {
                if value.len() < min_len {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' length {} is less than minimum {}",
                        param.name,
                        value.len(),
                        min_len
                    )));
                }
            }

            if let Some(max_len) = constraints.max_length {
                if value.len() > max_len {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' length {} exceeds maximum {}",
                        param.name,
                        value.len(),
                        max_len
                    )));
                }
            }

            if let Some(pattern) = &constraints.pattern {
                let regex = Regex::new(pattern).map_err(|e| {
                    WorkflowError::validation(format!("Invalid regex pattern: {}", e))
                })?;

                if !regex.is_match(value) {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' value '{}' does not match pattern '{}'",
                        param.name, value, pattern
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate numeric constraints.
    fn validate_numeric_constraints(&self, param: &Parameter, value: f64) -> Result<()> {
        if let Some(constraints) = &param.constraints {
            if let Some(min) = constraints.min {
                if value < min {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' value {} is less than minimum {}",
                        param.name, value, min
                    )));
                }
            }

            if let Some(max) = constraints.max {
                if value > max {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' value {} exceeds maximum {}",
                        param.name, value, max
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate array constraints.
    fn validate_array_constraints(
        &self,
        param: &Parameter,
        value: &[ParameterValue],
    ) -> Result<()> {
        if let Some(constraints) = &param.constraints {
            if let Some(min_len) = constraints.min_length {
                if value.len() < min_len {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' array length {} is less than minimum {}",
                        param.name,
                        value.len(),
                        min_len
                    )));
                }
            }

            if let Some(max_len) = constraints.max_length {
                if value.len() > max_len {
                    return Err(WorkflowError::validation(format!(
                        "Parameter '{}' array length {} exceeds maximum {}",
                        param.name,
                        value.len(),
                        max_len
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate file path.
    fn validate_file_path(&self, _path: &str) -> Result<()> {
        // Basic validation - could be extended
        Ok(())
    }

    /// Validate URL.
    fn validate_url(&self, url: &str) -> Result<()> {
        let url_regex = Regex::new(r"^https?://[^\s/$.?#].[^\s]*$")
            .map_err(|e| WorkflowError::validation(format!("Invalid URL regex: {}", e)))?;

        if !url_regex.is_match(url) {
            return Err(WorkflowError::validation(format!(
                "Invalid URL format: {}",
                url
            )));
        }

        Ok(())
    }

    /// Validate semantic version.
    fn validate_version(&self, version: &str) -> Result<()> {
        let version_regex = Regex::new(r"^\d+\.\d+\.\d+(-[a-zA-Z0-9.]+)?(\+[a-zA-Z0-9.]+)?$")
            .map_err(|e| WorkflowError::validation(format!("Invalid version regex: {}", e)))?;

        if !version_regex.is_match(version) {
            return Err(WorkflowError::validation(format!(
                "Invalid semantic version format: {}",
                version
            )));
        }

        Ok(())
    }
}

impl Default for TemplateValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{ParameterConstraints, TemplateCategory, TemplateMetadata};
    use chrono::Utc;

    #[test]
    fn test_validate_template_basic() {
        let mut template = WorkflowTemplate::new("test", "Test", "Description");
        template.workflow_template = "{}".to_string();
        template.version = "1.0.0".to_string();

        let validator = TemplateValidator::new();
        assert!(validator.validate_template(&template).is_ok());
    }

    #[test]
    fn test_validate_empty_id() {
        let template = WorkflowTemplate {
            id: "".to_string(),
            name: "Test".to_string(),
            description: "Description".to_string(),
            version: "1.0.0".to_string(),
            author: "Test".to_string(),
            tags: vec![],
            parameters: vec![],
            workflow_template: "{}".to_string(),
            metadata: TemplateMetadata {
                created_at: Utc::now(),
                updated_at: Utc::now(),
                category: TemplateCategory::Custom,
                complexity: 1,
                estimated_duration: None,
                required_resources: vec![],
                compatible_versions: vec![],
            },
            examples: vec![],
        };

        let validator = TemplateValidator::new();
        assert!(validator.validate_template(&template).is_err());
    }

    #[test]
    fn test_validate_version() {
        let validator = TemplateValidator::new();

        assert!(validator.validate_version("1.0.0").is_ok());
        assert!(validator.validate_version("1.2.3").is_ok());
        assert!(validator.validate_version("1.0.0-alpha").is_ok());
        assert!(validator.validate_version("1.0.0+build").is_ok());
        assert!(validator.validate_version("invalid").is_err());
        assert!(validator.validate_version("1.0").is_err());
    }

    #[test]
    fn test_validate_parameters() {
        let validator = TemplateValidator::new();

        let param = Parameter {
            name: "test_param".to_string(),
            param_type: ParameterType::Integer,
            description: "Test parameter".to_string(),
            required: true,
            default_value: None,
            constraints: Some(ParameterConstraints {
                min: Some(0.0),
                max: Some(100.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        };

        let mut values = HashMap::new();
        values.insert("test_param".to_string(), ParameterValue::Integer(50));

        assert!(validator.validate_parameters(&[param], &values).is_ok());
    }

    #[test]
    fn test_validate_missing_required() {
        let validator = TemplateValidator::new();

        let param = Parameter {
            name: "required_param".to_string(),
            param_type: ParameterType::String,
            description: "Required parameter".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        };

        let values = HashMap::new();

        assert!(validator.validate_parameters(&[param], &values).is_err());
    }

    #[test]
    fn test_validate_numeric_constraints() {
        let validator = TemplateValidator::new();

        let param = Parameter {
            name: "num_param".to_string(),
            param_type: ParameterType::Integer,
            description: "Numeric parameter".to_string(),
            required: false,
            default_value: Some(ParameterValue::Integer(50)),
            constraints: Some(ParameterConstraints {
                min: Some(0.0),
                max: Some(100.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        };

        let value = ParameterValue::Integer(150);
        assert!(validator.validate_parameter_value(&param, &value).is_err());

        let value = ParameterValue::Integer(50);
        assert!(validator.validate_parameter_value(&param, &value).is_ok());
    }

    #[test]
    fn test_validate_string_pattern() {
        let validator = TemplateValidator::new();

        let param = Parameter {
            name: "email".to_string(),
            param_type: ParameterType::String,
            description: "Email parameter".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("test@example.com".to_string())),
            constraints: Some(ParameterConstraints {
                min: None,
                max: None,
                min_length: None,
                max_length: None,
                pattern: Some(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string()),
            }),
        };

        let value = ParameterValue::String("invalid-email".to_string());
        assert!(validator.validate_parameter_value(&param, &value).is_err());

        let value = ParameterValue::String("valid@example.com".to_string());
        assert!(validator.validate_parameter_value(&param, &value).is_ok());
    }
}
