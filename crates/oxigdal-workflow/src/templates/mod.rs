//! Workflow template system.
//!
//! Provides reusable workflow templates with parameterization,
//! validation, and a built-in library of common geospatial workflows.

pub mod library;
pub mod parameterization;
pub mod validation;

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use library::WorkflowTemplateLibrary;
pub use parameterization::{
    Parameter, ParameterConstraints, ParameterType, ParameterValue, TemplateParameterizer,
};
pub use validation::TemplateValidator;

/// Workflow template definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    /// Template ID.
    pub id: String,
    /// Template name.
    pub name: String,
    /// Template description.
    pub description: String,
    /// Template version.
    pub version: String,
    /// Template author.
    pub author: String,
    /// Template tags.
    pub tags: Vec<String>,
    /// Template parameters.
    pub parameters: Vec<Parameter>,
    /// Base workflow definition (with parameter placeholders).
    pub workflow_template: String,
    /// Template metadata.
    pub metadata: TemplateMetadata,
    /// Example parameter values.
    pub examples: Vec<HashMap<String, ParameterValue>>,
}

/// Template metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMetadata {
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Template category.
    pub category: TemplateCategory,
    /// Complexity rating (1-5).
    pub complexity: u8,
    /// Estimated execution time description.
    pub estimated_duration: Option<String>,
    /// Required resources.
    pub required_resources: Vec<String>,
    /// Compatible versions.
    pub compatible_versions: Vec<String>,
}

/// Template category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateCategory {
    /// Satellite image processing.
    SatelliteProcessing,
    /// Change detection.
    ChangeDetection,
    /// Terrain analysis.
    TerrainAnalysis,
    /// Vector processing.
    VectorProcessing,
    /// Batch processing.
    BatchProcessing,
    /// ETL (Extract, Transform, Load).
    Etl,
    /// Quality control.
    QualityControl,
    /// Machine learning.
    MachineLearning,
    /// Custom template.
    Custom,
}

impl WorkflowTemplate {
    /// Create a new workflow template.
    pub fn new<S: Into<String>>(id: S, name: S, description: S) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            version: "1.0.0".to_string(),
            author: "unknown".to_string(),
            tags: Vec::new(),
            parameters: Vec::new(),
            workflow_template: String::new(),
            metadata: TemplateMetadata {
                created_at: Utc::now(),
                updated_at: Utc::now(),
                category: TemplateCategory::Custom,
                complexity: 1,
                estimated_duration: None,
                required_resources: Vec::new(),
                compatible_versions: Vec::new(),
            },
            examples: Vec::new(),
        }
    }

    /// Add a parameter to the template.
    pub fn add_parameter(&mut self, parameter: Parameter) {
        self.parameters.push(parameter);
    }

    /// Set the workflow template string.
    pub fn set_template<S: Into<String>>(&mut self, template: S) {
        self.workflow_template = template.into();
    }

    /// Set the template category.
    pub fn set_category(&mut self, category: TemplateCategory) {
        self.metadata.category = category;
    }

    /// Add a tag.
    pub fn add_tag<S: Into<String>>(&mut self, tag: S) {
        self.tags.push(tag.into());
    }

    /// Add an example parameter set.
    pub fn add_example(&mut self, example: HashMap<String, ParameterValue>) {
        self.examples.push(example);
    }

    /// Instantiate the template with parameter values.
    pub fn instantiate(
        &self,
        params: HashMap<String, ParameterValue>,
    ) -> Result<WorkflowDefinition> {
        // Validate parameters
        let validator = TemplateValidator::new();
        validator.validate_parameters(&self.parameters, &params)?;

        // Instantiate template
        let parameterizer = TemplateParameterizer::new();
        let workflow_json = parameterizer.apply_parameters(&self.workflow_template, &params)?;

        // Parse workflow definition
        let workflow: WorkflowDefinition = serde_json::from_str(&workflow_json)
            .map_err(|e| WorkflowError::template(format!("Failed to parse workflow: {}", e)))?;

        Ok(workflow)
    }

    /// Get required parameters.
    pub fn get_required_parameters(&self) -> Vec<&Parameter> {
        self.parameters.iter().filter(|p| p.required).collect()
    }

    /// Get optional parameters.
    pub fn get_optional_parameters(&self) -> Vec<&Parameter> {
        self.parameters.iter().filter(|p| !p.required).collect()
    }

    /// Validate the template itself.
    pub fn validate(&self) -> Result<()> {
        let validator = TemplateValidator::new();
        validator.validate_template(self)
    }

    /// Clone the template with a new ID and version.
    pub fn clone_as(&self, new_id: String, new_version: String) -> Self {
        let mut cloned = self.clone();
        cloned.id = new_id;
        cloned.version = new_version;
        cloned.metadata.created_at = Utc::now();
        cloned.metadata.updated_at = Utc::now();
        cloned
    }
}

/// Template builder for fluent API.
pub struct TemplateBuilder {
    template: WorkflowTemplate,
}

impl TemplateBuilder {
    /// Create a new template builder.
    pub fn new<S1: Into<String>, S2: Into<String>>(id: S1, name: S2) -> Self {
        Self {
            template: WorkflowTemplate::new(id.into(), name.into(), "".to_string()),
        }
    }

    /// Set the description.
    pub fn description<S: Into<String>>(mut self, description: S) -> Self {
        self.template.description = description.into();
        self
    }

    /// Set the version.
    pub fn version<S: Into<String>>(mut self, version: S) -> Self {
        self.template.version = version.into();
        self
    }

    /// Set the author.
    pub fn author<S: Into<String>>(mut self, author: S) -> Self {
        self.template.author = author.into();
        self
    }

    /// Add a tag.
    pub fn tag<S: Into<String>>(mut self, tag: S) -> Self {
        self.template.tags.push(tag.into());
        self
    }

    /// Add a parameter.
    pub fn parameter(mut self, parameter: Parameter) -> Self {
        self.template.parameters.push(parameter);
        self
    }

    /// Set the template string.
    pub fn template<S: Into<String>>(mut self, template: S) -> Self {
        self.template.workflow_template = template.into();
        self
    }

    /// Set the category.
    pub fn category(mut self, category: TemplateCategory) -> Self {
        self.template.metadata.category = category;
        self
    }

    /// Set the complexity.
    pub fn complexity(mut self, complexity: u8) -> Self {
        self.template.metadata.complexity = complexity.min(5);
        self
    }

    /// Build the template.
    pub fn build(self) -> Result<WorkflowTemplate> {
        self.template.validate()?;
        Ok(self.template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_creation() {
        let template = WorkflowTemplate::new("test-template", "Test Template", "A test template");

        assert_eq!(template.id, "test-template");
        assert_eq!(template.name, "Test Template");
    }

    #[test]
    fn test_template_builder() {
        let template = TemplateBuilder::new("test", "Test")
            .description("Description")
            .version("1.0.0")
            .author("Test Author")
            .tag("test")
            .complexity(3)
            .build();

        // Will fail validation without proper template
        assert!(template.is_err());
    }

    #[test]
    fn test_parameter_filtering() {
        let mut template = WorkflowTemplate::new("test", "Test", "Description");

        let param1 = Parameter {
            name: "required_param".to_string(),
            param_type: ParameterType::String,
            description: "A required parameter".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        };

        let param2 = Parameter {
            name: "optional_param".to_string(),
            param_type: ParameterType::String,
            description: "An optional parameter".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("default".to_string())),
            constraints: None,
        };

        template.add_parameter(param1);
        template.add_parameter(param2);

        assert_eq!(template.get_required_parameters().len(), 1);
        assert_eq!(template.get_optional_parameters().len(), 1);
    }

    #[test]
    fn test_template_categories() {
        assert_eq!(
            TemplateCategory::SatelliteProcessing,
            TemplateCategory::SatelliteProcessing
        );
    }
}
